/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use serde_json::{Map, Value};

use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry};
use crate::protocol::ClientRequest;
use crate::{ActorMsg, EmptyReplyMsg, StreamId};

pub struct NetworkParentActor {}

impl Actor for NetworkParentActor {
    const BASE_NAME: &str = "network-parent";

    /// The network parent actor can handle the following messages:
    ///
    /// - `setSaveRequestAndResponseBodies`: Doesn't do anything yet
    fn handle_message(
        &self,
        name: String,
        request: ClientRequest,
        _registry: &ActorRegistry,
        msg_type: &str,
        _msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "setSaveRequestAndResponseBodies" => {
                let msg = EmptyReplyMsg { from: name };
                request.reply_final(&msg)?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl ActorEncode<ActorMsg> for NetworkParentActor {
    fn encode(&self, name: String, _: &ActorRegistry) -> ActorMsg {
        ActorMsg { actor: name }
    }
}
