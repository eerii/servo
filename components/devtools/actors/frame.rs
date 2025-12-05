/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cell::RefCell;
use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::StreamId;
use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry};
use crate::actors::environment::{EnvironmentActor, EnvironmentActorMsg};
use crate::actors::object::{ObjectActor, ObjectActorMsg};
use crate::protocol::ClientRequest;

#[derive(Serialize)]
struct FrameEnvironmentReply {
    from: String,
    #[serde(flatten)]
    environment: EnvironmentActorMsg,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrameState {
    OnStack,
    // Not implemented
    _Suspended,
    _Dead,
}

#[derive(Serialize)]
pub struct FrameWhere {
    actor: String,
    line: u32,
    column: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameActorMsg {
    actor: String,
    #[serde(rename = "type")]
    type_: String,
    arguments: Vec<Value>,
    async_cause: Option<String>,
    display_name: String,
    oldest: bool,
    state: FrameState,
    #[serde(rename = "this")]
    this_: ObjectActorMsg,
    #[serde(rename = "where")]
    where_: FrameWhere,
}

#[derive(Serialize)]
pub struct FramesReply {
    pub from: String,
    pub frames: Vec<FrameActorMsg>,
}

#[derive(Default)]
pub struct FrameManager {
    frame_actor_names: RefCell<BTreeSet<String>>,
}

impl FrameManager {
    pub fn add_frame(&self, actor_name: &str) {
        self.frame_actor_names
            .borrow_mut()
            .insert(actor_name.to_owned());
    }

    pub fn encoded_frames(&self, registry: &ActorRegistry) -> Vec<FrameActorMsg> {
        self.frame_actor_names
            .borrow()
            .iter()
            .map(|name| registry.find::<FrameActor>(name).encode(registry))
            .collect()
    }
}

/// Represents an stack frame. Used by `ThreadActor` when replying to interrupt messages.
/// <https://searchfox.org/firefox-main/source/devtools/server/actors/frame.js>
pub struct FrameActor {
    pub name: String,
    pub source_actor: String,
    pub object_actor: String,
}

impl Actor for FrameActor {
    fn name(&self) -> String {
        self.name.clone()
    }

    // https://searchfox.org/firefox-main/source/devtools/shared/specs/frame.js
    fn handle_message(
        &self,
        request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        _msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "getEnvironment" => {
                let environment = EnvironmentActor {
                    name: registry.new_name("environment"),
                    parent: None,
                };
                let msg = FrameEnvironmentReply {
                    from: self.name(),
                    environment: environment.encode(registry),
                };
                registry.register_later(environment);
                request.reply_final(&msg)?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl ActorEncode<FrameActorMsg> for FrameActor {
    fn encode(&self, registry: &ActorRegistry) -> FrameActorMsg {
        // TODO: Handle other states
        let state = FrameState::OnStack;
        let async_cause = if let FrameState::OnStack = state {
            None
        } else {
            Some("await".into())
        };
        FrameActorMsg {
            actor: self.name(),
            type_: "call".into(),
            arguments: vec![],
            async_cause,
            display_name: "".into(), // TODO: get display name
            oldest: true,
            state,
            this_: registry.encode::<ObjectActor, _>(&self.object_actor),
            where_: FrameWhere {
                actor: self.source_actor.clone(),
                line: 1, // TODO: get from breakpoint?
                column: 1,
            },
        }
    }
}
