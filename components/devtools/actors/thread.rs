/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use serde::Serialize;
use serde_json::{Map, Value};

use super::source::{SourceManager, SourcesReply};
use crate::actor::{Actor, ActorError, ActorRegistry};
use crate::actors::pause::PauseActor;
use crate::protocol::{ClientRequest, JsonPacketStream};
use crate::{EmptyReplyMsg, StreamId};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadAttached {
    from: String,
    #[serde(rename = "type")]
    type_: String,
    actor: String,
    frame: u32,
    error: u32,
    recording_endpoint: u32,
    execution_point: u32,
    popped_frames: Vec<PoppedFrameMsg>,
    why: WhyMsg,
}

#[derive(Serialize)]
enum PoppedFrameMsg {}

#[derive(Serialize)]
struct WhyMsg {
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Serialize)]
struct ThreadResumedReply {
    from: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Serialize)]
struct ThreadInterruptedReply {
    from: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Default)]
pub struct ThreadActor {
    pub source_manager: SourceManager,
}

impl Actor for ThreadActor {
    const BASE_NAME: &str = "thread";

    fn handle_message(
        &self,
        name: String,
        mut request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        _msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "attach" => {
                let actor = registry.register_later(PauseActor {});
                let msg = ThreadAttached {
                    from: name.clone(),
                    type_: "paused".to_owned(),
                    actor,
                    frame: 0,
                    error: 0,
                    recording_endpoint: 0,
                    execution_point: 0,
                    popped_frames: vec![],
                    why: WhyMsg {
                        type_: "attached".to_owned(),
                    },
                };
                request.write_json_packet(&msg)?;
                request.reply_final(&EmptyReplyMsg { from: name })?
            },

            "resume" => {
                let msg = ThreadResumedReply {
                    from: name.clone(),
                    type_: "resumed".to_owned(),
                };
                request.write_json_packet(&msg)?;
                request.reply_final(&EmptyReplyMsg { from: name })?
            },

            "interrupt" => {
                let msg = ThreadInterruptedReply {
                    from: name.clone(),
                    type_: "interrupted".to_owned(),
                };
                request.write_json_packet(&msg)?;
                request.reply_final(&EmptyReplyMsg { from: name })?
            },

            "reconfigure" => request.reply_final(&EmptyReplyMsg { from: name })?,

            // Client has attached to the thread and wants to load script sources.
            // <https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html#loading-script-sources>
            "sources" => {
                let msg = SourcesReply {
                    from: name,
                    sources: self.source_manager.encode(registry),
                };
                request.reply_final(&msg)?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}
