/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Liberally derived from <https://searchfox.org/mozilla-central/source/devtools/server/actors/thread-configuration.js>
//! This actor manages the configuration flags that the devtools host can apply to threads.

use std::collections::HashMap;
use std::sync::Arc;

use malloc_size_of_derive::MallocSizeOf;
use serde_json::{Map, Value};

use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry, new_actor_name};
use crate::actors::browsing_context::BrowsingContextActor;
use crate::actors::thread::ThreadActor;
use crate::protocol::ClientRequest;
use crate::{ActorMsg, EmptyReplyMsg, StreamId};

#[derive(MallocSizeOf)]
pub(crate) struct ThreadConfigurationActor {
    name: String,
    browsing_context_name: String,
    _configuration: HashMap<&'static str, bool>,
}

impl Actor for ThreadConfigurationActor {
    fn name(&self) -> &str {
        &self.name
    }

    /// The thread configuration actor can handle the following messages:
    ///
    /// - `updateConfiguration`: Receives new configuration flags from the devtools host.
    fn handle_message(
        &self,
        request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "updateConfiguration" => {
                if let Some(pause) = msg
                    .get("configuration")
                    .and_then(Value::as_object)
                    .and_then(|obj| obj.get("pauseOnExceptions"))
                    .and_then(Value::as_bool)
                {
                    let browsing_context_actor =
                        registry.find::<BrowsingContextActor>(&self.browsing_context_name);
                    let thread_actor =
                        registry.find::<ThreadActor>(&browsing_context_actor.thread_name);
                    thread_actor.set_pause_on_exceptions(pause);
                }
                let msg = EmptyReplyMsg {
                    from: self.name().into(),
                };
                request.reply_final(&msg)?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl ThreadConfigurationActor {
    pub fn register(registry: &ActorRegistry, browsing_context_name: String) -> Arc<Self> {
        let name = new_actor_name::<Self>();
        let actor = Self {
            name,
            browsing_context_name,
            _configuration: HashMap::new(),
        };
        registry.register::<Self>(actor)
    }
}

impl ActorEncode<ActorMsg> for ThreadConfigurationActor {
    fn encode(&self, _: &ActorRegistry) -> ActorMsg {
        ActorMsg {
            actor: self.name().into(),
        }
    }
}
