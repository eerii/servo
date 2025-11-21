/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Descriptor actor that represents a web view. It can link a tab to the corresponding watcher
//! actor to enable inspection.
//!
//! Liberally derived from the [Firefox JS implementation].
//!
//! [Firefox JS implementation]: https://searchfox.org/mozilla-central/source/devtools/server/actors/descriptors/tab.js

use devtools_traits::DevtoolScriptControlMsg;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::actor::{Actor, ActorEncodable, ActorError, ActorRegistry};
use crate::actors::browsing_context::{BrowsingContextActor, BrowsingContextActorMsg};
use crate::actors::root::{DescriptorTraits, RootActor};
use crate::actors::watcher::{WatcherActor, WatcherActorMsg};
use crate::protocol::ClientRequest;
use crate::{EmptyReplyMsg, StreamId};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabDescriptorActorMsg {
    actor: String,
    /// This correspond to webview_id
    #[serde(rename = "browserId")]
    browser_id: u32,
    #[serde(rename = "browsingContextID")]
    browsing_context_id: u32,
    is_zombie_tab: bool,
    #[serde(rename = "outerWindowID")]
    outer_window_id: u32,
    selected: bool,
    title: String,
    traits: DescriptorTraits,
    url: String,
}

impl TabDescriptorActorMsg {
    pub fn browser_id(&self) -> u32 {
        self.browser_id
    }

    pub fn actor(&self) -> String {
        self.actor.clone()
    }
}

#[derive(Serialize)]
struct GetTargetReply {
    from: String,
    frame: BrowsingContextActorMsg,
}

#[derive(Serialize)]
struct GetFaviconReply {
    from: String,
    favicon: String,
}

#[derive(Serialize)]
struct GetWatcherReply {
    from: String,
    #[serde(flatten)]
    watcher: WatcherActorMsg,
}

pub struct TabDescriptorActor {
    browsing_context_actor: String,
    is_top_level_global: bool,
}

impl Actor for TabDescriptorActor {
    const BASE_NAME: &str = "tab";

    /// The tab actor can handle the following messages:
    ///
    /// - `getTarget`: Returns the surrounding `BrowsingContextActor`.
    ///
    /// - `getFavicon`: Should return the tab favicon, but it is not yet supported.
    ///
    /// - `getWatcher`: Returns a `WatcherActor` linked to the tab's `BrowsingContext`. It is used
    ///   to describe the debugging capabilities of this tab.
    ///
    /// - `reloadDescriptor`: Causes the page to reload.
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
            "getTarget" => {
                let frame = registry
                    .find::<BrowsingContextActor>(&self.browsing_context_actor)
                    .encodable();
                request.reply_final(&GetTargetReply {
                    from: name,
                    frame,
                })?
            },
            "getFavicon" => {
                // TODO: Return a favicon when available
                request.reply_final(&GetFaviconReply {
                    from: name,
                    favicon: String::new(),
                })?
            },
            "getWatcher" => {
                let ctx_actor = registry.find::<BrowsingContextActor>(&self.browsing_context_actor);
                let watcher = registry.find::<WatcherActor>(&ctx_actor.watcher);
                request.reply_final(&GetWatcherReply {
                    from: name,
                    watcher: watcher.encodable(),
                })?
            },
            "reloadDescriptor" => {
                // There is an extra bypassCache parameter that we don't currently use.
                let ctx_actor = registry.find::<BrowsingContextActor>(&self.browsing_context_actor);
                let pipeline = ctx_actor.active_pipeline_id.get();
                ctx_actor
                    .script_chan
                    .send(DevtoolScriptControlMsg::Reload(pipeline))
                    .map_err(|_| ActorError::Internal)?;

                request.reply_final(&EmptyReplyMsg { from: name })?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl ActorEncodable<TabDescriptorActorMsg> for TabDescriptorActor {
    fn encode(&self, name: String) -> TabDescriptorActorMsg {
        let target = actors.find::<BrowsingContextActor>(&self.browsing_context_actor);
        let title = target.title.borrow().clone();
        let url = target.url.borrow().clone();

        TabDescriptorActorMsg {
            actor: name,
            browser_id: target.browser_id.value(),
            browsing_context_id: target.browsing_context_id.value(),
            is_zombie_tab: false,
            outer_window_id: target.active_outer_window_id.get().value(),
            selected,
            title,
            traits: DescriptorTraits {
                watcher: true,
                supports_reload_descriptor: true,
            },
            url,
        }
    }
}

impl TabDescriptorActor {
    pub(crate) fn register_new(
        actors: &mut ActorRegistry,
        browsing_context_actor: String,
        is_top_level_global: bool,
    ) -> String {
        let tab = TabDescriptorActor {
            browsing_context_actor,
            is_top_level_global,
        };
        let name = actors.register(tab);
        let root = actors.find_mut::<RootActor>("root");
        root.tabs.push(name.clone());
        name
    }

    pub(crate) fn is_top_level_global(&self) -> bool {
        self.is_top_level_global
    }

    pub fn browsing_context(&self) -> String {
        self.browsing_context_actor.clone()
    }
}
