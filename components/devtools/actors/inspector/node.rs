/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! This actor represents one DOM node. It is created by the Walker actor when it is traversing the
//! document tree.

use std::cell::RefCell;
use std::collections::HashMap;

use devtools_traits::DevtoolScriptControlMsg::{
    GetChildren, GetDocumentElement, GetXPath, ModifyAttribute,
};
use devtools_traits::{NodeInfo, ShadowRootMode};
use serde::Serialize;
use serde_json::{self, Map, Value};

use crate::actor::{Actor, ActorError, ActorRegistry};
use crate::actors::browsing_context::BrowsingContextActor;
use crate::actors::inspector::walker::WalkerActor;
use crate::protocol::ClientRequest;
use crate::{EmptyReplyMsg, StreamId};

/// Text node type constant. This is defined again to avoid depending on `script`, where it is defined originally.
/// See `script::dom::bindings::codegen::Bindings::NodeBinding::NodeConstants`.
const TEXT_NODE: u16 = 3;

/// The maximum length of a text node for it to appear as an inline child in the inspector.
const MAX_INLINE_LENGTH: usize = 50;

#[derive(Serialize)]
struct GetUniqueSelectorReply {
    from: String,
    value: String,
}

#[derive(Serialize)]
struct GetXPathReply {
    from: String,
    value: String,
}

#[derive(Clone, Serialize)]
struct AttrMsg {
    name: String,
    value: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeActorMsg {
    pub actor: String,

    /// The ID of the shadow host of this node, if it is
    /// a shadow root
    host: Option<String>,
    #[serde(rename = "baseURI")]
    base_uri: String,
    causes_overflow: bool,
    container_type: Option<()>,
    pub display_name: String,
    display_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline_text_child: Option<Box<NodeActorMsg>>,
    is_after_pseudo_element: bool,
    is_anonymous: bool,
    is_before_pseudo_element: bool,
    is_direct_shadow_host_child: Option<bool>,
    /// Whether or not this node is displayed.
    ///
    /// Setting this value to `false` will cause the devtools to render the node name in gray.
    is_displayed: bool,
    #[serde(rename = "isInHTMLDocument")]
    is_in_html_document: Option<bool>,
    is_marker_pseudo_element: bool,
    is_native_anonymous: bool,
    is_scrollable: bool,
    is_shadow_host: bool,
    is_shadow_root: bool,
    is_top_level_document: bool,
    node_name: String,
    node_type: u16,
    node_value: Option<String>,
    pub num_children: usize,
    #[serde(skip_serializing_if = "String::is_empty")]
    parent: String,
    shadow_root_mode: Option<String>,
    traits: HashMap<String, ()>,
    attrs: Vec<AttrMsg>,

    /// The `DOCTYPE` name if this is a `DocumentType` node, `None` otherwise
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    /// The `DOCTYPE` public identifier if this is a `DocumentType` node, `None` otherwise
    #[serde(skip_serializing_if = "Option::is_none")]
    public_id: Option<String>,

    /// The `DOCTYPE` system identifier if this is a `DocumentType` node, `None` otherwise
    #[serde(skip_serializing_if = "Option::is_none")]
    system_id: Option<String>,
}

pub struct NodeActor {
    name: String,
    pub walker: String,
    pub style_rules: RefCell<HashMap<(String, usize), String>>,
}

impl Actor for NodeActor {
    fn name(&self) -> String {
        self.name.clone()
    }

    /// The node actor can handle the following messages:
    ///
    /// - `modifyAttributes`: Asks the script to change a value in the attribute of the
    ///   corresponding node
    ///
    /// - `getUniqueSelector`: Returns the display name of this node
    fn handle_message(
        &self,
        mut request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        let walker = registry.find::<WalkerActor>(&self.walker);
        let browsing_context = registry.find::<BrowsingContextActor>(&walker.browsing_context);

        match msg_type {
            "modifyAttributes" => {
                let mods = msg
                    .get("modifications")
                    .ok_or(ActorError::MissingParameter)?
                    .as_array()
                    .ok_or(ActorError::BadParameterType)?;
                let modifications: Vec<_> = mods
                    .iter()
                    .filter_map(|json_mod| {
                        serde_json::from_str(&serde_json::to_string(json_mod).ok()?).ok()
                    })
                    .collect();

                let walker = registry.find::<WalkerActor>(&self.walker);
                walker.new_mutations(&mut request, &self.name, &modifications);

                browsing_context.send(|pipeline| {
                    ModifyAttribute(
                        pipeline,
                        registry.actor_to_script(self.name()),
                        modifications,
                    )
                })?;

                let reply = EmptyReplyMsg { from: self.name() };
                request.reply_final(&reply)?
            },

            "getUniqueSelector" => {
                let doc_elem_info = browsing_context
                    .send_rx(|pipeline, tx| GetDocumentElement(pipeline, tx))?
                    .ok_or(ActorError::Internal)?;

                let node = doc_elem_info.encode(registry, self.walker.clone(), &walker.browsing_context);

                let msg = GetUniqueSelectorReply {
                    from: self.name(),
                    value: node.display_name,
                };
                request.reply_final(&msg)?
            },
            "getXPath" => {
                let target = msg
                    .get("to")
                    .ok_or(ActorError::MissingParameter)?
                    .as_str()
                    .ok_or(ActorError::BadParameterType)?;
                let target_id = registry.actor_to_script(target.to_owned());

                let xpath_selector =
                    browsing_context.send_rx(|pipeline, tx| GetXPath(pipeline, target_id, tx))?;

                let msg = GetXPathReply {
                    from: self.name(),
                    value: xpath_selector,
                };
                request.reply_final(&msg)?
            },

            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

pub trait NodeInfoToProtocol {
    fn encode(self, registry: &ActorRegistry, walker: String, browsing_context: &str) -> NodeActorMsg;
}

impl NodeInfoToProtocol for NodeInfo {
    fn encode(self, registry: &ActorRegistry, walker: String, browsing_context: &str) -> NodeActorMsg {
        let browsing_context_actor =
            registry.find::<BrowsingContextActor>(&browsing_context);

        let get_or_register_node_actor = |id: &str| {
            if !registry.script_actor_registered(id.to_string()) {
                let name = registry.new_name("node");
                registry.register_script_actor(id.to_string(), name.clone());

                let node_actor = NodeActor {
                    name: name.clone(),
                    walker: walker.clone(),
                    style_rules: RefCell::new(HashMap::new()),
                };
                registry.register_later(node_actor);
                name
            } else {
                registry.script_to_actor(id.to_string())
            }
        };

        let actor = get_or_register_node_actor(&self.unique_id);
        let host = self
            .host
            .as_ref()
            .map(|host_id| get_or_register_node_actor(host_id));

        let name = registry.actor_to_script(actor.clone());

        // If a node only has a single text node as a child whith a small enough text,
        // return it with this node as an `inlineTextChild`.
        let inline_text_child = (|| {
            // TODO: Also return if this node is a flex element.
            if self.num_children != 1 || self.node_name == "SLOT" {
                return None;
            }

            let mut children = browsing_context_actor
                .send_rx(|pipeline, tx| GetChildren(pipeline, name.clone(), tx))
                .ok()??;

            let child = children.pop()?;
            let msg = child.encode(registry, walker, browsing_context);

            // If the node child is not a text node, do not represent it inline.
            if msg.node_type != TEXT_NODE {
                return None;
            }

            // If the text node child is too big, do not represent it inline.
            if msg.node_value.clone().unwrap_or_default().len() > MAX_INLINE_LENGTH {
                return None;
            }

            Some(Box::new(msg))
        })();

        NodeActorMsg {
            actor,
            host,
            base_uri: self.base_uri,
            causes_overflow: false,
            container_type: None,
            display_name: self.node_name.clone().to_lowercase(),
            display_type: self.display,
            inline_text_child,
            is_after_pseudo_element: false,
            is_anonymous: false,
            is_before_pseudo_element: false,
            is_direct_shadow_host_child: None,
            is_displayed: self.is_displayed,
            is_in_html_document: Some(true),
            is_marker_pseudo_element: false,
            is_native_anonymous: false,
            is_scrollable: false,
            is_shadow_host: self.is_shadow_host,
            is_shadow_root: self.shadow_root_mode.is_some(),
            is_top_level_document: self.is_top_level_document,
            node_name: self.node_name,
            node_type: self.node_type,
            node_value: self.node_value,
            num_children: self.num_children,
            parent: registry.script_to_actor(self.parent.clone()),
            shadow_root_mode: self
                .shadow_root_mode
                .as_ref()
                .map(ShadowRootMode::to_string),
            traits: HashMap::new(),
            attrs: self
                .attrs
                .into_iter()
                .map(|attr| AttrMsg {
                    name: attr.name,
                    value: attr.value,
                })
                .collect(),
            name: self.doctype_name,
            public_id: self.doctype_public_identifier,
            system_id: self.doctype_system_identifier,
        }
    }
}
