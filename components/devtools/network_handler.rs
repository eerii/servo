/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use devtools_traits::NetworkEvent;
use serde::Serialize;

use crate::actor::ActorRegistry;
use crate::actors::browsing_context::BrowsingContextActor;
use crate::actors::network_event::NetworkEventActor;
use crate::actors::watcher::WatcherActor;
use crate::resource::{ResourceArrayType, ResourceAvailable};

#[derive(Clone, Serialize)]
pub struct Cause {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "loadingDocumentUri")]
    pub loading_document_uri: Option<String>,
}

pub(crate) fn handle_network_event(
    actors: Arc<Mutex<ActorRegistry>>,
    netevent_actor_name: String,
    mut connections: Vec<TcpStream>,
    network_event: NetworkEvent,
) {
    let mut actors = actors.lock().unwrap();
    let actor = actors.find_mut::<NetworkEventActor>(&netevent_actor_name);
    let watcher_name = actor.watcher_name.clone();

    match network_event {
        NetworkEvent::HttpRequest(httprequest) => {
            actor.add_request(httprequest);

            let event_actor = actor.event_actor();
            let actor = actors.find::<NetworkEventActor>(&netevent_actor_name);
            let resource = actor.resource_updates(&actors);
            let watcher = actors.find::<WatcherActor>(&watcher_name);

            for stream in &mut connections {
                watcher.resource_array(
                    event_actor.clone(),
                    "network-event".to_string(),
                    ResourceArrayType::Available,
                    stream,
                );

                // Also push initial resource update (request headers, cookies)
                watcher.resource_array(
                    resource.clone(),
                    "network-event".to_string(),
                    ResourceArrayType::Updated,
                    stream,
                );
            }
        },

        NetworkEvent::HttpRequestUpdate(httprequest) => {
            actor.add_request(httprequest);

            let resource = actor.resource_updates(&actors);
            let watcher = actors.find::<WatcherActor>(&watcher_name);

            for stream in &mut connections {
                watcher.resource_array(
                    resource.clone(),
                    "network-event".to_string(),
                    ResourceArrayType::Updated,
                    stream,
                );
            }
        },

        NetworkEvent::HttpResponse(httpresponse) => {
            actor.add_response(httpresponse);

            let resource = actor.resource_updates(&actors);
            let watcher = actors.find::<WatcherActor>(&watcher_name);

            for stream in &mut connections {
                watcher.resource_array(
                    resource.clone(),
                    "network-event".to_string(),
                    ResourceArrayType::Updated,
                    stream,
                );
            }
        },

        NetworkEvent::SecurityInfo(update) => {
            actor.update_security_info(update.security_info);

            let resource = actor.resource_updates(&actors);
            let watcher = actors.find::<WatcherActor>(&watcher_name);

            for stream in &mut connections {
                watcher.resource_array(
                    resource.clone(),
                    "network-event".to_string(),
                    ResourceArrayType::Updated,
                    stream,
                );
            }
        },
    }
}
