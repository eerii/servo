/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use serde::Serialize;
use serde_json::{Map, Value};
use servo_config::pref;

use crate::StreamId;
use crate::actor::{Actor, ActorError, ActorRegistry};
use crate::protocol::ClientRequest;

pub struct PreferenceActor {}

impl Actor for PreferenceActor {
    const BASE_NAME: &str = "preference";

    fn handle_message(
        &self,
        name: String,
        request: ClientRequest,
        _registry: &ActorRegistry,
        msg_type: &str,
        msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        let key = msg
            .get("value")
            .ok_or(ActorError::MissingParameter)?
            .as_str()
            .ok_or(ActorError::BadParameterType)?;

        // TODO: Map more preferences onto their Servo values.
        match key {
            "dom.serviceWorkers.enabled" => {
                self.write_bool(name, request, pref!(dom_serviceworker_enabled))
            },
            _ => self.handle_missing_preference(name, request, msg_type),
        }
    }
}

impl PreferenceActor {
    fn handle_missing_preference(
        &self,
        name: String,
        request: ClientRequest,
        msg_type: &str,
    ) -> Result<(), ActorError> {
        match msg_type {
            "getBoolPref" => self.write_bool(name, request, false),
            "getCharPref" => self.write_char(name, request, "".into()),
            "getIntPref" => self.write_int(name, request, 0),
            "getFloatPref" => self.write_float(name, request, 0.),
            _ => Err(ActorError::UnrecognizedPacketType),
        }
    }

    fn write_bool(&self, name: String, request: ClientRequest, pref_value: bool) -> Result<(), ActorError> {
        #[derive(Serialize)]
        struct BoolReply {
            from: String,
            value: bool,
        }

        let reply = BoolReply {
            from: name,
            value: pref_value,
        };
        request.reply_final(&reply)
    }

    fn write_char(&self, name: String, request: ClientRequest, pref_value: String) -> Result<(), ActorError> {
        #[derive(Serialize)]
        struct CharReply {
            from: String,
            value: String,
        }

        let reply = CharReply {
            from: name,
            value: pref_value,
        };
        request.reply_final(&reply)
    }

    fn write_int(&self, name: String, request: ClientRequest, pref_value: i64) -> Result<(), ActorError> {
        #[derive(Serialize)]
        struct IntReply {
            from: String,
            value: i64,
        }

        let reply = IntReply {
            from: name,
            value: pref_value,
        };
        request.reply_final(&reply)
    }

    fn write_float(&self, name: String, request: ClientRequest, pref_value: f64) -> Result<(), ActorError> {
        #[derive(Serialize)]
        struct FloatReply {
            from: String,
            value: f64,
        }

        let reply = FloatReply {
            from: name,
            value: pref_value,
        };
        request.reply_final(&reply)
    }
}
