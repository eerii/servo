/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use serde::Serialize;

use crate::actor::{Actor, ActorEncode, ActorRegistry};

#[derive(Serialize)]
pub struct ObjectPreview {
    kind: String,
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectActorMsg {
    actor: String,
    #[serde(rename = "type")]
    type_: String,
    class: String,
    own_property_length: i32,
    extensible: bool,
    frozen: bool,
    sealed: bool,
    is_error: bool,
    preview: ObjectPreview,
}

pub struct ObjectActor {
    pub _uuid: String,
}

impl Actor for ObjectActor {
    const BASE_NAME: &str = "object";

    // TODO: Handle messages
    // https://searchfox.org/firefox-main/source/devtools/shared/specs/object.js
}

impl ObjectActor {
    pub fn register(registry: &ActorRegistry, uuid: String) -> String {
        if !registry.script_actor_registered(&uuid) {
            let object = registry.register_later(ObjectActor {
                _uuid: uuid.clone(),
            });
            registry.register_script_actor(uuid, object.clone());
            object
        } else {
            registry.script_to_actor(uuid)
        }
    }
}

impl ActorEncode<ObjectActorMsg> for ObjectActor {
    fn encode(&self, name: String, _: &ActorRegistry) -> ObjectActorMsg {
        // TODO: Review hardcoded values here
        ObjectActorMsg {
            actor: name,
            type_: "object".into(),
            class: "Window".into(),
            own_property_length: 0,
            extensible: true,
            frozen: false,
            sealed: false,
            is_error: false,
            preview: ObjectPreview {
                kind: "ObjectWithURL".into(),
                url: "".into(),
            },
        }
    }
}
