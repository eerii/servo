/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::RefCell;

use serde::Serialize;
use serde_json::{Map, Value};

use super::source::{SourceManager, SourcesReply};
use crate::actor::{Actor, ActorError, ActorRegistry};
use crate::actors::frame::{FrameActorMsg, FrameManager, FramesReply};
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
    // Not yet implemented
    // AlreadyPaused,
    // DebuggerStatement,
    // Exception,
    // EventBreakpoint,
    // MutationBreakpoint,
    // ResumeLimit,
    // XHR,
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
    // Not yet implemented
    // Dettached,
    // Exited,
    // Resumed,
    // Running,
}

// #[derive(Serialize)]
// #[serde(rename_all = "camelCase")]
// struct ThreadAttachReply {
//     from: String,
//     #[serde(rename = "type")]
//     type_: ThreadState,
//     actor: String,
//     error: u32,
//     execution_point: u32,
//     frame: u32,
//     recording_endpoint: u32,
//     why: ThreadWhy,
// }

#[derive(Serialize)]
struct ThreadInterruptReply {
    from: String,
    #[serde(rename = "type")]
    type_: ThreadState,
    actor: String,
    frame: FrameActorMsg,
    why: ThreadWhy,
}

// #[derive(Serialize)]
// struct ThreadResumeReply {
//     from: String,
//     #[serde(rename = "type")]
//     type_: ThreadState,
// }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum EventBreakpointType {
    _Simple,
    _Event,
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
    pub frame_manager: FrameManager,
    pub pause: RefCell<Option<String>>,
}

impl ThreadActor {
    pub fn new(name: String) -> ThreadActor {
        ThreadActor {
            name: name.clone(),
            source_manager: SourceManager::new(),
            frame_manager: FrameManager::default(),
            pause: RefCell::default(),
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
            // "attach" => {
            //     let actor = self.pause.borrow().clone().ok_or(ActorError::Internal)?;
            //     let msg = ThreadAttachReply {
            //         from: self.name(),
            //         type_: ThreadState::Paused,
            //         actor,
            //         error: 0,
            //         execution_point: 0,
            //         frame: 0,
            //         recording_endpoint: 0,
            //         why: ThreadWhy {
            //             type_: ThreadWhyReason::Attached,
            //             ..Default::default()
            //         },
            //     };
            //     stream.write_json_packet(&msg)?;
            // },

            "frames" => {
                let msg = FramesReply {
                    from: self.name(),
                    frames: self.frame_manager.encoded_frames(registry),
                };
                request.reply_final(&msg)?
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
                // TODO: Check if we should send `thread-state` true in the watcher actor.

                // let source_forms = self.source_manager.source_forms(registry);
                // let source_actor = source_forms[0].actor.clone();

                let mut encoded_frames = self.frame_manager.encoded_frames(registry);
                let frame = encoded_frames.pop().expect("It should have a frame");

                let pause = PauseActor {
                    name: registry.new_name("pause"),
                };
                let msg = ThreadInterruptReply {
                    from: self.name(),
                    type_: ThreadState::Paused,
                    actor: pause.name.clone(),
                    frame,
                    why: ThreadWhy {
                        type_: ThreadWhyReason::Interrupted,
                        on_next: Some(true),
                        ..Default::default()
                    },
                };
                request.write_json_packet(&msg)?;

                self.pause.replace(Some(pause.name.clone()));
                registry.register_later(pause);

                let msg = EmptyReplyMsg { from: self.name() };
                request.reply_final(&msg)?
            },

            "reconfigure" => request.reply_final(&EmptyReplyMsg { from: self.name() })?,

            // Client has attached to the thread and wants to load script sources.
            // <https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html#loading-script-sources>
            "sources" => {
                let msg = SourcesReply {
                    from: self.name(),
                    sources: self.source_manager.source_forms(registry),
                };
                request.reply_final(&msg)?
            },

            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}
