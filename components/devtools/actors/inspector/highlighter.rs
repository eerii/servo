/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Handles highlighting selected DOM nodes in the inspector. At the moment it only replies and
//! changes nothing on Servo's side.

use devtools_traits::DevtoolScriptControlMsg::HighlightDomNode;
use serde::Serialize;
use serde_json::{self, Map, Value};

use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry};
use crate::actors::browsing_context::BrowsingContextActor;
use crate::protocol::ClientRequest;
use crate::{ActorMsg, EmptyReplyMsg, StreamId};

#[derive(Serialize)]
struct ShowReply {
    from: String,
    value: bool,
}

pub struct HighlighterActor {
    pub name: String,
    pub browsing_context: String,
}

impl Actor for HighlighterActor {
    fn name(&self) -> String {
        self.name.clone()
    }

    /// The highligher actor can handle the following messages:
    ///
    /// - `show`: Enables highlighting for the selected node
    ///
    /// - `hide`: Disables highlighting for the selected node
    fn handle_message(
        &self,
        request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        let browsing_context = registry.find::<BrowsingContextActor>(&self.browsing_context);
        match msg_type {
            "show" => {
                let Some(node_actor) = msg.get("node") else {
                    return Err(ActorError::MissingParameter);
                };

                let Some(node_actor_name) = node_actor.as_str() else {
                    return Err(ActorError::BadParameterType);
                };

                if node_actor_name.starts_with("inspector") {
                    // TODO: For some reason, the client initially asks us to highlight
                    // the inspector? Investigate what this is supposed to mean.
                    let msg = ShowReply {
                        from: self.name(),
                        value: false,
                    };
                    return request.reply_final(&msg);
                }

                let node_id = registry.actor_to_script(node_actor_name.into());
                browsing_context.send(|pipeline| HighlightDomNode(pipeline, Some(node_id)))?;

                let msg = ShowReply {
                    from: self.name(),
                    value: true,
                };
                request.reply_final(&msg)?
            },

            "hide" => {
                browsing_context.send(|pipeline| HighlightDomNode(pipeline, None))?;

                let msg = EmptyReplyMsg { from: self.name() };
                request.reply_final(&msg)?
            },

            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl ActorEncode<ActorMsg> for HighlighterActor {
    fn encode(&self, _: &ActorRegistry) -> ActorMsg {
        ActorMsg { actor: self.name() }
    }
}
