/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Liberally derived from <https://searchfox.org/mozilla-central/source/devtools/server/actors/thread-configuration.js>
//! This actor manages the configuration flags that the devtools host can apply to threads.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry};
use crate::protocol::ClientRequest;
use crate::{ActorMsg, EmptyReplyMsg, StreamId};

#[derive(Default)]
pub struct ThreadConfigurationActor {
    _configuration: HashMap<&'static str, bool>,
}

impl Actor for ThreadConfigurationActor {
    const BASE_NAME: &str = "thread-configuration";

    /// The thread configuration actor can handle the following messages:
    ///
    /// - `updateConfiguration`: Receives new configuration flags from the devtools host.
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
            "updateConfiguration" => {
                // TODO: Actually update configuration
                let msg = EmptyReplyMsg { from: name };
                request.reply_final(&msg)?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl ActorEncode<ActorMsg> for ThreadConfigurationActor {
    fn encode(&self, name: String, _: &ActorRegistry) -> ActorMsg {
        ActorMsg { actor: name }
    }
}
