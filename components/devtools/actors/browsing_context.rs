/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Liberally derived from the [Firefox JS implementation](https://searchfox.org/mozilla-central/source/devtools/server/actors/webbrowser.js).
//! Connection point for remote devtools that wish to investigate a particular Browsing Context's contents.
//! Supports dynamic attaching and detaching which control notifications of navigation, etc.

use std::collections::HashMap;
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

use atomic_refcell::AtomicRefCell;
use base::generic_channel::{self, GenericSender};
use base::id::PipelineId;
use devtools_traits::DevtoolScriptControlMsg::{
    self, GetCssDatabase, SimulateColorScheme, WantsLiveNotifications,
};
use devtools_traits::{DevtoolsPageInfo, NavigationState};
use embedder_traits::Theme;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry};
use crate::actors::inspector::InspectorActor;
use crate::actors::inspector::accessibility::AccessibilityActor;
use crate::actors::inspector::css_properties::CssPropertiesActor;
use crate::actors::reflow::ReflowActor;
use crate::actors::stylesheets::StyleSheetsActor;
use crate::actors::tab::TabDescriptorActor;
use crate::actors::thread::ThreadActor;
use crate::actors::watcher::{SessionContext, SessionContextType, WatcherActor};
use crate::id::{DevtoolsBrowserId, DevtoolsBrowsingContextId, DevtoolsOuterWindowId, IdMap};
use crate::protocol::{ClientRequest, JsonPacketStream};
use crate::resource::{ResourceArrayType, ResourceAvailable};
use crate::{EmptyReplyMsg, StreamId};

#[derive(Serialize)]
struct ListWorkersReply {
    from: String,
    workers: Vec<()>,
}

#[derive(Serialize)]
struct FrameUpdateReply {
    from: String,
    #[serde(rename = "type")]
    type_: String,
    frames: Vec<FrameUpdateMsg>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FrameUpdateMsg {
    id: u32,
    is_top_level: bool,
    url: String,
    title: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowsingContextTraits {
    frames: bool,
    is_browsing_context: bool,
    log_in_page: bool,
    navigation: bool,
    supports_top_level_target_flag: bool,
    watchpoints: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum TargetType {
    Frame,
    // Other target types not implemented yet.
}

#[derive(Default, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DocumentEventName {
    #[default]
    WillNavigate,
    DomLoading,
    DomInteractive,
    DomComplete,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "browsingContextID")]
    browsing_context_id: Option<u32>,
    #[serde(rename = "hasNativeConsoleAPI")]
    has_native_console_api: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inner_window_id: Option<u32>,
    name: DocumentEventName,
    #[serde(rename = "newURI")]
    new_uri: Option<String>,
    time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

impl DocumentEvent {
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn will_navigate(url: String, browsing_context_id: u32, inner_window_id: u32) -> Self {
        Self {
            browsing_context_id: Some(browsing_context_id),
            inner_window_id: Some(inner_window_id),
            name: DocumentEventName::WillNavigate,
            new_uri: Some(url),
            time: Self::now(),
            ..Default::default()
        }
    }

    fn dom_loading(url: String) -> Self {
        Self {
            name: DocumentEventName::DomLoading,
            time: Self::now(),
            url: Some(url),
            ..Default::default()
        }
    }

    fn dom_interactive(title: String, url: String) -> Self {
        Self {
            name: DocumentEventName::DomInteractive,
            time: Self::now(),
            title: Some(title),
            url: Some(url),
            ..Default::default()
        }
    }

    fn dom_complete() -> Self {
        Self {
            name: DocumentEventName::DomComplete,
            time: Self::now(),
            ..Default::default()
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowsingContextActorMsg {
    actor: String,
    title: String,
    url: String,
    /// This correspond to webview_id
    #[serde(rename = "browserId")]
    browser_id: u32,
    #[serde(rename = "outerWindowID")]
    outer_window_id: u32,
    #[serde(rename = "browsingContextID")]
    browsing_context_id: u32,
    is_top_level_target: bool,
    traits: BrowsingContextTraits,
    // Implemented actors
    accessibility_actor: String,
    console_actor: String,
    css_properties_actor: String,
    inspector_actor: String,
    reflow_actor: String,
    style_sheets_actor: String,
    thread_actor: String,
    target_type: TargetType,
    // Part of the official protocol, but not yet implemented.
    // animations_actor: String,
    // changes_actor: String,
    // framerate_actor: String,
    // manifest_actor: String,
    // memory_actor: String,
    // network_content_actor: String,
    // objects_manager: String,
    // performance_actor: String,
    // resonsive_actor: String,
    // storage_actor: String,
    // tracer_actor: String,
    // web_extension_inspected_window_actor: String,
    // web_socket_actor: String,
}

/// The browsing context actor encompasses all of the other supporting actors when debugging a web
/// view. To this extent, it contains a watcher actor that helps when communicating with the host,
/// as well as resource actors that each perform one debugging function.
pub(crate) struct BrowsingContextActor {
    name: String,
    pub title: AtomicRefCell<String>,
    pub url: AtomicRefCell<String>,
    /// This corresponds to webview_id
    pub browser_id: DevtoolsBrowserId,
    // TODO: Should these ids be atomic?
    active_pipeline_id: AtomicRefCell<PipelineId>,
    active_outer_window_id: AtomicRefCell<DevtoolsOuterWindowId>,
    pub browsing_context_id: DevtoolsBrowsingContextId,
    accessibility: String,
    pub console: String,
    css_properties: String,
    inspector: String,
    reflow: String,
    style_sheets: String,
    pub thread: String,
    _tab: String,
    pub script_chan: GenericSender<DevtoolScriptControlMsg>,

    pub streams: AtomicRefCell<HashMap<StreamId, TcpStream>>,
    pub watcher: String,
}

impl ResourceAvailable for BrowsingContextActor {
    fn actor_name(&self) -> String {
        self.name.clone()
    }
}

impl Actor for BrowsingContextActor {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn handle_message(
        &self,
        request: ClientRequest,
        _registry: &ActorRegistry,
        msg_type: &str,
        _msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "listFrames" => {
                // TODO: Find out what needs to be listed here
                let msg = EmptyReplyMsg { from: self.name() };
                request.reply_final(&msg)?
            },
            "listWorkers" => {
                request.reply_final(&ListWorkersReply {
                    from: self.name(),
                    // TODO: Find out what needs to be listed here
                    workers: vec![],
                })?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }

    fn cleanup(&self, id: StreamId) {
        self.streams.borrow_mut().remove(&id);
        if self.streams.borrow().is_empty() {
            self.script_chan
                .send(WantsLiveNotifications(self.pipeline_id(), false))
                .unwrap();
        }
    }
}

impl BrowsingContextActor {
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn new(
        console: String,
        browser_id: DevtoolsBrowserId,
        browsing_context_id: DevtoolsBrowsingContextId,
        page_info: DevtoolsPageInfo,
        pipeline_id: PipelineId,
        outer_window_id: DevtoolsOuterWindowId,
        script_sender: GenericSender<DevtoolScriptControlMsg>,
        actors: &ActorRegistry,
    ) -> BrowsingContextActor {
        let name = actors.new_name::<BrowsingContextActor>();
        let DevtoolsPageInfo {
            title,
            url,
            is_top_level_global,
        } = page_info;

        let accessibility = AccessibilityActor::new(actors.new_name::<AccessibilityActor>());

        let properties = (|| {
            let (properties_sender, properties_receiver) = generic_channel::channel()?;
            script_sender.send(GetCssDatabase(properties_sender)).ok()?;
            properties_receiver.recv().ok()
        })()
        .unwrap_or_default();
        let css_properties =
            CssPropertiesActor::new(actors.new_name::<CssPropertiesActor>(), properties);

        let inspector = InspectorActor::register(actors, pipeline_id, script_sender.clone());

        let reflow = ReflowActor::new(actors.new_name::<ReflowActor>());

        let style_sheets = StyleSheetsActor::new(actors.new_name::<StyleSheetsActor>());

        let tabdesc = TabDescriptorActor::new(actors, name.clone(), is_top_level_global);

        let thread = ThreadActor::new(actors.new_name::<ThreadActor>());

        let watcher = WatcherActor::new(
            actors,
            name.clone(),
            SessionContext::new(SessionContextType::BrowserElement),
        );

        let target = BrowsingContextActor {
            name,
            script_chan: script_sender,
            title: AtomicRefCell::new(title),
            url: AtomicRefCell::new(url.into_string()),
            active_pipeline_id: AtomicRefCell::new(pipeline_id),
            active_outer_window_id: AtomicRefCell::new(outer_window_id),
            browser_id,
            browsing_context_id,
            accessibility: accessibility.name(),
            console,
            css_properties: css_properties.name(),
            inspector,
            reflow: reflow.name(),
            streams: AtomicRefCell::new(HashMap::new()),
            style_sheets: style_sheets.name(),
            _tab: tabdesc.name(),
            thread: thread.name(),
            watcher: watcher.name(),
        };

        actors.register(accessibility);
        actors.register(css_properties);
        actors.register(reflow);
        actors.register(style_sheets);
        actors.register(tabdesc);
        actors.register(thread);
        actors.register(watcher);

        target
    }

    pub(crate) fn navigate(
        &self,
        registry: &ActorRegistry,
        state: NavigationState,
        id_map: Option<&mut IdMap>,
    ) {
        match state {
            NavigationState::Start(url) => {
                let watcher = registry.find::<WatcherActor>(&self.watcher);

                for stream in self.streams.borrow_mut().values_mut() {
                    // will-navigate
                    if id_map.is_some() {
                        watcher.resource_array(
                            DocumentEvent::will_navigate(
                                url.clone().into_string(),
                                self.browsing_context_id.value(),
                                0, // TODO: Send correct inner window id
                            ),
                            "document-event".into(),
                            ResourceArrayType::Available,
                            stream,
                        );
                    }
                    // dom-loading
                    self.resource_array(
                        DocumentEvent::dom_loading(url.clone().into_string()),
                        "document-event".into(),
                        ResourceArrayType::Available,
                        stream,
                    );
                }

                *self.url.borrow_mut() = url.into_string();
            },
            NavigationState::Stop(pipeline_id, info) => {
                for stream in self.streams.borrow_mut().values_mut() {
                    // dom-interactive
                    self.resources_array(
                        vec![
                            DocumentEvent::dom_interactive(
                                info.title.clone(),
                                info.url.clone().into_string(),
                            ),
                            DocumentEvent::dom_complete(),
                        ],
                        "document-event".into(),
                        ResourceArrayType::Available,
                        stream,
                    );
                }

                if let Some(id_map) = id_map {
                    *self.active_outer_window_id.borrow_mut() = id_map.outer_window_id(pipeline_id);
                    *self.active_pipeline_id.borrow_mut() = pipeline_id;
                    *self.url.borrow_mut() = info.url.into_string();
                    *self.title.borrow_mut() = info.title;
                }
            },
        }
    }

    pub(crate) fn title_changed(&self, pipeline_id: PipelineId, title: String) {
        if pipeline_id != self.pipeline_id() {
            return;
        }
        *self.title.borrow_mut() = title;
    }

    pub(crate) fn frame_update<T: JsonPacketStream>(&self, request: &mut T) {
        let _ = request.write_json_packet(&FrameUpdateReply {
            from: self.name(),
            type_: "frameUpdate".into(),
            frames: vec![FrameUpdateMsg {
                id: self.outer_window_id().value(),
                is_top_level: true,
                title: self.title.borrow().clone(),
                url: self.url.borrow().clone(),
            }],
        });
    }

    pub fn simulate_color_scheme(&self, theme: Theme) -> Result<(), ()> {
        self.script_chan
            .send(SimulateColorScheme(self.pipeline_id(), theme))
            .map_err(|_| ())
    }

    pub(crate) fn pipeline_id(&self) -> PipelineId {
        *self.active_pipeline_id.borrow()
    }

    pub(crate) fn outer_window_id(&self) -> DevtoolsOuterWindowId {
        *self.active_outer_window_id.borrow()
    }
}

impl ActorEncode<BrowsingContextActorMsg> for BrowsingContextActor {
    fn encode(&self, _: &ActorRegistry) -> BrowsingContextActorMsg {
        BrowsingContextActorMsg {
            actor: self.name(),
            traits: BrowsingContextTraits {
                is_browsing_context: true,
                frames: true,
                log_in_page: false,
                navigation: true,
                supports_top_level_target_flag: true,
                watchpoints: true,
            },
            title: self.title.borrow().clone(),
            url: self.url.borrow().clone(),
            browser_id: self.browser_id.value(),
            browsing_context_id: self.browsing_context_id.value(),
            outer_window_id: self.outer_window_id().value(),
            is_top_level_target: true,
            accessibility_actor: self.accessibility.clone(),
            console_actor: self.console.clone(),
            css_properties_actor: self.css_properties.clone(),
            inspector_actor: self.inspector.clone(),
            reflow_actor: self.reflow.clone(),
            style_sheets_actor: self.style_sheets.clone(),
            thread_actor: self.thread.clone(),
            target_type: TargetType::Frame,
        }
    }
}
