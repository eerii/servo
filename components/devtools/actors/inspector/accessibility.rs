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
    simulator: ActorMsg,
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
    walker: ActorMsg,
}

pub struct AccessibilityActor {
    name: String,
}

impl Actor for AccessibilityActor {
    const BASE_NAME: &str = "accessibility";

    fn name(&self) -> String {
        self.name.clone()
    }

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
        request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        _msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "bootstrap" => {
                let msg = BootstrapReply {
                    from: self.name(),
                    state: BootstrapState { enabled: false },
                };
                request.reply_final(&msg)?
            },
            "getSimulator" => {
                let simulator = SimulatorActor {
                    name: registry.new_name::<SimulatorActor>(),
                };
                let msg = GetSimulatorReply {
                    from: self.name(),
                    simulator: simulator.encode(registry),
                };
                registry.register_later(simulator);
                request.reply_final(&msg)?
            },
            "getTraits" => {
                let msg = GetTraitsReply {
                    from: self.name(),
                    traits: AccessibilityTraits {
                        tabbing_order: true,
                    },
                };
                request.reply_final(&msg)?
            },
            "getWalker" => {
                let walker = AccessibleWalkerActor {
                    name: registry.new_name::<AccessibleWalkerActor>(),
                };
                let msg = GetWalkerReply {
                    from: self.name(),
                    walker: walker.encode(registry),
                };
                registry.register_later(walker);
                request.reply_final(&msg)?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl AccessibilityActor {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

/// Placeholder for the simulator actor
struct SimulatorActor {
    name: String,
}

impl Actor for SimulatorActor {
    const BASE_NAME: &str = "simulator";

    fn name(&self) -> String {
        self.name.clone()
    }
}

impl ActorEncode<ActorMsg> for SimulatorActor {
    fn encode(&self, _: &ActorRegistry) -> ActorMsg {
        ActorMsg { actor: self.name() }
    }
}

/// Placeholder for the accessible walker actor
struct AccessibleWalkerActor {
    name: String,
}

impl Actor for AccessibleWalkerActor {
    const BASE_NAME: &str = "accessible-walker";

    fn name(&self) -> String {
        self.name.clone()
    }
}

impl ActorEncode<ActorMsg> for AccessibleWalkerActor {
    fn encode(&self, _: &ActorRegistry) -> ActorMsg {
        ActorMsg { actor: self.name() }
    }
}
