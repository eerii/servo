/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use serde::Serialize;
use serde_json::{Map, Value};

use super::source::{SourceManager, SourcesReply};
use crate::actor::{Actor,  ActorError, ActorRegistry};
use crate::actors::frame::{FrameActor, FrameActorMsg};
use crate::actors::pause::PauseActor;
use crate::protocol::{ClientRequest, JsonPacketStream};
use crate::{EmptyReplyMsg, StreamId};

/// Explains why a thread is in a certain state.
/// <https://searchfox.org/firefox-main/rev/main/devtools/server/actors/thread.js#156>
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
enum ThreadWhyReason {
    /// A client has attached to the thread.
    #[default]
    Attached,
    /// The thread has stopped because it received an `interrupt` packet from the client.
    Interrupted,
    // Not implemented in Servo
    _AlreadyPaused,
    _DebuggerStatement,
    _Exception,
    _EventBreakpoint,
    _MutationBreakpoint,
    _ResumeLimit,
    _XHR,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadWhy {
    /// Reason why the thread is in the state.
    #[serde(rename = "type")]
    type_: ThreadWhyReason,
    /// Only for `WhyReason::EventBreakpoint`.
    /// List of breakpoint actors.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    actors: Vec<String>,
    /// Only for `WhyReason::Interrupted`.
    /// Indicates if the execution should pause immediately.
    #[serde(skip_serializing_if = "Option::is_none")]
    on_next: Option<bool>,
}

/// Thread actor possible states.
/// <https://searchfox.org/firefox-main/source/devtools/server/actors/thread.js#141>
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum ThreadState {
    /// The client is attached and the thread is paused because of a breakpoint or an interrupt.
    Paused,
    // Not implemented in Servo
    _Dettached,
    _Exited,
    _Resumed,
    _Running,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadAttachReply {
    from: String,
    #[serde(rename = "type")]
    type_: ThreadState,
    actor: String,
    error: u32,
    execution_point: u32,
    frame: u32,
    recording_endpoint: u32,
    why: ThreadWhy,
}

#[derive(Serialize)]
struct ThreadInterruptReply {
    from: String,
    #[serde(rename = "type")]
    type_: ThreadState,
    actor: String,
    frame: FrameActorMsg,
    why: ThreadWhy,
}

#[derive(Serialize)]
struct ThreadResumeReply {
    from: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum EventBreakpointType {
    _Event,
    _Simple,
}

#[derive(Serialize)]
struct EventBreakpointEvent {
    id: String,
    name: String,
    #[serde(rename = "type")]
    type_: EventBreakpointType,
}

#[derive(Serialize)]
struct EventBreakpoint {
    name: String,
    events: Vec<EventBreakpointEvent>,
}

#[derive(Serialize)]
struct GetAvailableEventBreakpointsReply {
    from: String,
    value: Vec<EventBreakpoint>,
}

pub struct ThreadActor {
    pub name: String,
    pub source_manager: SourceManager,
}

impl ThreadActor {
    pub fn new(name: String) -> ThreadActor {
        ThreadActor {
            name: name.clone(),
            source_manager: Default::default(),
        }
    }
}

impl Actor for ThreadActor {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn handle_message(
        &self,
        mut request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        _msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "attach" => {
                let pause = registry.new_name::<PauseActor>();
                registry.register(PauseActor {
                    name: pause.clone(),
                });
                let msg = ThreadAttachReply {
                    from: self.name(),
                    type_: ThreadState::Paused,
                    actor: pause,
                    error: 0,
                    execution_point: 0,
                    frame: 0,
                    recording_endpoint: 0,
                    why: ThreadWhy {
                        type_: ThreadWhyReason::Attached,
                        ..Default::default()
                    },
                };
                request.write_json_packet(&msg)?;
                request.reply_final(&EmptyReplyMsg { from: self.name() })?
            },

            "getAvailableEventBreakpoints" => {
                // TODO: Send list of available event breakpoints (animation, clipboard, load...)
                let msg = GetAvailableEventBreakpointsReply {
                    from: self.name(),
                    value: vec![],
                };
                request.reply_final(&msg)?
            },

            "interrupt" => {
                let pause = registry.new_name::<PauseActor>();
                registry.register(PauseActor {
                    name: pause.clone(),
                });

                let frame = registry.new_name::<FrameActor>();
                registry.register(FrameActor {
                    name: frame.clone(),
                    // TODO: Get the source and object actors here
                    source_actor: "".into(),
                    object_actor: "".into(),
                });

                let msg = ThreadInterruptReply {
                    from: self.name(),
                    type_: ThreadState::Paused,
                    actor: pause,
                    frame: registry.encode::<FrameActor, _>(&frame),
                    why: ThreadWhy {
                        type_: ThreadWhyReason::Interrupted,
                        on_next: Some(true),
                        ..Default::default()
                    },
                };
                request.write_json_packet(&msg)?;
                request.reply_final(&EmptyReplyMsg { from: self.name() })?
            },

            "reconfigure" => request.reply_final(&EmptyReplyMsg { from: self.name() })?,

            // "resume" => {},

            // Client has attached to the thread and wants to load script sources.
            // <https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html#loading-script-sources>
            "sources" => {
                let msg = SourcesReply {
                    from: self.name(),
                    sources: self.source_manager.encode(registry),
                };
                request.reply_final(&msg)?
            },

            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}
