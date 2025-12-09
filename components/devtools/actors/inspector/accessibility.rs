/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The Accessibility actor is responsible for the Accessibility tab in the DevTools page. Right
//! now it is a placeholder for future functionality.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::StreamId;
use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry};
use crate::protocol::ClientRequest;

#[derive(Serialize)]
struct BootstrapState {
    enabled: bool,
}

#[derive(Serialize)]
struct BootstrapReply {
    from: String,
    state: BootstrapState,
}

#[derive(Serialize)]
struct GetSimulatorReply {
    from: String,
    #[serde(flatten)]
    simulator: Option<ActorMsg>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccessibilityTraits {
    tabbing_order: bool,
}

#[derive(Serialize)]
struct GetTraitsReply {
    from: String,
    traits: AccessibilityTraits,
}

#[derive(Serialize)]
struct ActorMsg {
    actor: String,
}

#[derive(Serialize)]
struct GetWalkerReply {
    from: String,
    #[serde(flatten)]
    walker: Option<ActorMsg>,
}

pub struct AccessibilityActor {}

impl Actor for AccessibilityActor {
    const BASE_NAME: &str = "accessibility";

    /// The accesibility actor can handle the following messages:
    ///
    /// - `bootstrap`: It is required but it doesn't do anything yet
    ///
    /// - `getSimulator`: Returns a new Simulator actor
    ///
    /// - `getTraits`: Informs the DevTools client about the configuration of the accessibility actor
    ///
    /// - `getWalker`: Returns a new AccessibleWalker actor (not to be confused with the general
    ///   inspector Walker actor)
    fn handle_message(
        &self,
        name: String,
        request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        _msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "bootstrap" => {
                let msg = BootstrapReply {
                    from: name,
                    state: BootstrapState { enabled: false },
                };
                request.reply_final(&msg)?
            },
            "getSimulator" => {
                let mut msg = GetSimulatorReply {
                    from: name,
                    simulator: None,
                };
                registry.register_with(|name| {
                    let actor = SimulatorActor {};
                    msg.simulator = Some(actor.encode(name.into(), registry));
                    actor
                });
                request.reply_final(&msg)?
            },
            "getTraits" => {
                let msg = GetTraitsReply {
                    from: name,
                    traits: AccessibilityTraits {
                        tabbing_order: true,
                    },
                };
                request.reply_final(&msg)?
            },
            "getWalker" => {
                let mut msg = GetWalkerReply {
                    from: name,
                    walker: None,
                };
                registry.register_with(|name| {
                    let actor = AccessibleWalkerActor {};
                    msg.walker = Some(actor.encode(name.into(), registry));
                    actor
                });
                request.reply_final(&msg)?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

/// Placeholder for the simulator actor
struct SimulatorActor {}

impl Actor for SimulatorActor {
    const BASE_NAME: &str = "simulator";
}

impl ActorEncode<ActorMsg> for SimulatorActor {
    fn encode(&self, name: String, _: &ActorRegistry) -> ActorMsg {
        ActorMsg { actor: name }
    }
}

/// Placeholder for the accessible walker actor
struct AccessibleWalkerActor {}

impl Actor for AccessibleWalkerActor {
    const BASE_NAME: &str = "accessible-walker";
}

impl ActorEncode<ActorMsg> for AccessibleWalkerActor {
    fn encode(&self, name: String, _: &ActorRegistry) -> ActorMsg {
        ActorMsg { actor: name }
    }
}
