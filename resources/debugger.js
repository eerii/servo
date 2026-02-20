if ("dbg" in this) {
    throw new Error("Debugger script must not run more than once!");
}

const dbg = new Debugger;
const debuggeesToPipelineIds = new Map;
const debuggeesToWorkerIds = new Map;
const sourceIdsToScripts = new Map;

// Find script by scriptId within a script tree
function findScriptById(script, scriptId) {
    if (script.sourceStart === scriptId) {
        return script;
    }
    for (const child of script.getChildScripts()) {
        const found = findScriptById(child, scriptId);
        if (found) return found;
    }
    return null;
}

// Find a key by a value in a map
function findKeyByValue(map, search) {
    for (const [key, value] of map) {
        if (value === search) return key;
    }
    return undefined;
}

// Walk script tree and call callback for each script
function walkScriptTree(script, callback) {
    callback(script);
    for (const child of script.getChildScripts()) {
        walkScriptTree(child, callback);
    }
}

// Parses a completion value into a result value object
// <https://firefox-source-docs.mozilla.org/js/Debugger/Conventions.html#completion-values>
function completionValueToResult(completionValue) {
    if (completionValue === null) {
        return { completionType: "terminated", valueType: "undefined" };
    }

    // Get the debuggee value
    // <https://firefox-source-docs.mozilla.org/js/Debugger/Conventions.html#debuggee-values>
    let value, completionType;
    if ("throw" in completionValue) {
        value = completionValue.throw;
        completionType = "throw";
    } else if ("return" in completionValue) {
        value = completionValue.return;
        completionType = "return";
    } else {
        console.error("Invalid completion value:", completionValue);
        return;
    }

    // Adopt the value to ensure proper Debugger ownership
    // <https://firefox-source-docs.mozilla.org/js/Debugger/Debugger.html#adoptdebuggeevalue-value>
    // <https://searchfox.org/firefox-main/source/devtools/server/actors/webconsole/eval-with-debugger.js#312>
    value = dbg.adoptDebuggeeValue(value);

    // Parse the value into a result object
    // Type detection follows Firefox's createValueGrip pattern:
    // <https://searchfox.org/mozilla-central/source/devtools/server/actors/object/utils.js#116>
    const valueToResult = (value) => {
        switch (typeof value) {
            case "undefined":
                return { valueType: "undefined" };
            case "boolean":
                return { valueType: "boolean", booleanValue: value };
            case "number":
                return { valueType: "number", numberValue: value };
            case "string":
                return { valueType: "string", stringValue: value };
            case "object":
                if (value === null) {
                    return { valueType: "null" };
                }
                // Debugger.Object - use the `class` accessor property
                // <https://firefox-source-docs.mozilla.org/js/Debugger/Debugger.Object.html>
                return { valueType: "object", objectClass: value.class };
            default:
                return { valueType: "string", stringValue: String(value) };
        }
    }

    return { completionType, ...valueToResult(value) };
}

// Print exceptions when running the debugger
// <https://firefox-source-docs.mozilla.org/js/Debugger/Debugger.html#uncaughtexceptionhook>
dbg.uncaughtExceptionHook = function(error) {
    console.error(`[debugger] Uncaught exception at ${error.fileName}:${error.lineNumber}:${error.columnNumber}: ${error.name}: ${error.message}`);
};

// A new script has been loaded for the debuggees
// <https://firefox-source-docs.mozilla.org/js/Debugger/Debugger.html#onnewscript-script-global>
dbg.onNewScript = function(script) {
    // TODO: handle wasm (`script.source.introductionType == wasm`)
    sourceIdsToScripts.set(script.source.id, script);
    notifyNewSource({
        pipelineId: debuggeesToPipelineIds.get(script.global),
        workerId: debuggeesToWorkerIds.get(script.global),
        spidermonkeyId: script.source.id,
        url: script.source.url,
        urlOverride: script.source.displayURL,
        text: script.source.text,
        introductionType: script.source.introductionType ?? null,
    });
};

// Track a new debuggee global
addEventListener("addDebuggee", event => {
    const {global, pipelineId, workerId} = event;
    const debuggerObject = dbg.addDebuggee(global);
    debuggeesToPipelineIds.set(debuggerObject, pipelineId);
    if (workerId !== undefined) {
        debuggeesToWorkerIds.set(debuggerObject, workerId);
    }
});

// Evaluate some javascript code in the global context of the debuggee
// <https://firefox-source-docs.mozilla.org/js/Debugger/Debugger.Object.html#executeinglobal-code-options>
addEventListener("eval", event => {
    const {code, pipelineId, workerId} = event;
    const object = workerId !== undefined ?
        findKeyByValue(debuggeesToWorkerIds, workerId) :
        findKeyByValue(debuggeesToPipelineIds, pipelineId);

    const completionValue = object.executeInGlobal(code);
    const resultValue = completionValueToResult(completionValue);

    evalResult(event, resultValue);
});

// Get a list of the possible breakpoint locations in a script
// <https://firefox-source-docs.mozilla.org/js/Debugger/Debugger.Script.html#getpossiblebreakpoints-query>
addEventListener("getPossibleBreakpoints", event => {
    const {spidermonkeyId} = event;
    const script = sourceIdsToScripts.get(spidermonkeyId);
    const result = [];
    walkScriptTree(script, (currentScript) => {
        for (const location of currentScript.getPossibleBreakpoints()) {
            location["scriptId"] = currentScript.sourceStart;
            result.push(location);
        }
    });
    getPossibleBreakpointsResult(event, result);
});

// Set a breakpoint in a script
// When execution reaches the given instruction, the hit method is called 
// <https://firefox-source-docs.mozilla.org/js/Debugger/Debugger.Script.html#setbreakpoint-offset-handler>
addEventListener("setBreakpoint", event => {
    const {spidermonkeyId, scriptId, offset} = event;
    const script = sourceIdsToScripts.get(spidermonkeyId);
    const target = findScriptById(script, scriptId);
    if (target) {
        target.setBreakpoint(offset, {
            // The hit handler receives a Debugger.Frame instance representing the currently executing stack frame.
            hit: (frame) => {
                // Get the pipeline ID for this debuggee
                const pipelineId = debuggeesToPipelineIds.get(frame.script.global);
                if (!pipelineId) {
                    console.error("[debugger] No pipeline ID for frame's global");
                    return undefined;
                }

                const result = {
                    column: frame.script.startColumn,
                    displayName: frame.script.displayName,
                    line: frame.script.startLine,
                    onStack: frame.onStack,
                    oldest: frame.older == null,
                    terminated: frame.terminated,
                    type_: frame.type,
                    url: frame.script.url,
                };

                // Notify devtools and enter pause loop. This blocks until Resume.
                notifyBreakpointHit(pipelineId, result);
                // <https://firefox-source-docs.mozilla.org/js/Debugger/Conventions.html#resumption-values>
                // Return undefined to continue execution normally after resume.
                return undefined;
            }
        });
    }
});

// Handle a protocol request to pause the debuggee
// <https://searchfox.org/firefox-main/source/devtools/server/actors/thread.js#1644>
addEventListener("pause", event => {
    dbg.onEnterFrame = function(frame) {
        dbg.onEnterFrame = undefined;
        // TODO: Some properties throw if terminated is true
        // TODO: Check if start line / column is correct or we need the proper breakpoint
        const result = {
            // TODO: arguments: frame.arguments,
            column: frame.script.startColumn,
            displayName: frame.script.displayName,
            line: frame.script.startLine,
            onStack: frame.onStack,
            oldest: frame.older == null,
            terminated: frame.terminated,
            type_: frame.type,
            url: frame.script.url,
        };
        getFrameResult(event, result);
    };
});

// Remove a breakpoint in a script
// <https://firefox-source-docs.mozilla.org/js/Debugger/Debugger.Script.html#clearallbreakpoints-offset>
// If the instance refers to a JSScript, remove all breakpoints set in this script at that offset.
addEventListener("clearBreakpoint", event => {
    const {spidermonkeyId, scriptId, offset} = event;
    const script = sourceIdsToScripts.get(spidermonkeyId);
    const target = findScriptById(script, scriptId);
    if (target) {
        // There may be more than one breakpoint at the same offset with different handlers, but we don’t handle that case for now.
        target.clearAllBreakpoints(offset);
    }
});
