/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The walker actor is responsible for traversing the DOM tree in various ways to create new nodes

use std::cell::RefCell;

use base::generic_channel::{self, GenericSender};
use base::id::PipelineId;
use devtools_traits::DevtoolScriptControlMsg::{GetChildren, GetDocumentElement};
use devtools_traits::{AttrModification, DevtoolScriptControlMsg};
use serde::Serialize;
use serde_json::{self, Map, Value};

use crate::actor::{Actor, ActorEncode, ActorError, ActorRegistry};
use crate::actors::inspector::layout::LayoutInspectorActor;
use crate::actors::inspector::node::{NodeActorMsg, NodeInfoToProtocol};
use crate::protocol::{ClientRequest, JsonPacketStream};
use crate::{ActorMsg, EmptyReplyMsg, StreamId};

#[derive(Serialize)]
pub struct WalkerMsg {
    pub actor: String,
    pub root: NodeActorMsg,
}

pub struct WalkerActor {
    pub script_chan: GenericSender<DevtoolScriptControlMsg>,
    pub pipeline: PipelineId,
    pub root_node: NodeActorMsg,
    pub mutations: RefCell<Vec<(AttrModification, String)>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuerySelectorReply {
    from: String,
    node: NodeActorMsg,
    new_parents: Vec<NodeActorMsg>,
}

#[derive(Serialize)]
struct DocumentElementReply {
    from: String,
    node: NodeActorMsg,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChildrenReply {
    has_first: bool,
    has_last: bool,
    nodes: Vec<NodeActorMsg>,
    from: String,
}

#[derive(Serialize)]
struct GetLayoutInspectorReply {
    from: String,
    #[serde(flatten)]
    actor: Option<ActorMsg>,
}

#[derive(Serialize)]
struct WatchRootNodeNotification {
    #[serde(rename = "type")]
    type_: String,
    from: String,
    node: NodeActorMsg,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MutationMsg {
    attribute_name: String,
    new_value: Option<String>,
    target: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Serialize)]
struct GetMutationsReply {
    from: String,
    mutations: Vec<MutationMsg>,
}

#[derive(Serialize)]
struct GetOffsetParentReply {
    from: String,
    node: Option<()>,
}

#[derive(Serialize)]
struct NewMutationsNotification {
    from: String,
    #[serde(rename = "type")]
    type_: String,
}

impl Actor for WalkerActor {
    const BASE_NAME: &str = "walker";

    /// The walker actor can handle the following messages:
    ///
    /// - `children`: Returns a list of children nodes of the specified node
    ///
    /// - `clearPseudoClassLocks`: Placeholder
    ///
    /// - `documentElement`: Returns the base document element node
    ///
    /// - `getLayoutInspector`: Returns the Layout inspector actor, placeholder
    ///
    /// - `getMutations`: Returns the list of attribute changes since it was last called
    ///
    /// - `getOffsetParent`: Placeholder
    ///
    /// - `querySelector`: Recursively looks for the specified selector in the tree, reutrning the
    ///   node and its ascendents
    fn handle_message(
        &self,
        name: String,
        mut request: ClientRequest,
        registry: &ActorRegistry,
        msg_type: &str,
        msg: &Map<String, Value>,
        _id: StreamId,
    ) -> Result<(), ActorError> {
        match msg_type {
            "children" => {
                let target = msg
                    .get("node")
                    .ok_or(ActorError::MissingParameter)?
                    .as_str()
                    .ok_or(ActorError::BadParameterType)?;
                let Some((tx, rx)) = generic_channel::channel() else {
                    return Err(ActorError::Internal);
                };
                self.script_chan
                    .send(GetChildren(
                        self.pipeline,
                        registry.actor_to_script(target.into()),
                        tx,
                    ))
                    .map_err(|_| ActorError::Internal)?;
                let children = rx
                    .recv()
                    .map_err(|_| ActorError::Internal)?
                    .ok_or(ActorError::Internal)?;

                let msg = ChildrenReply {
                    has_first: true,
                    has_last: true,
                    nodes: children
                        .into_iter()
                        .map(|child| {
                            child.encode(
                                registry,
                                self.script_chan.clone(),
                                self.pipeline,
                                name.clone(),
                            )
                        })
                        .collect(),
                    from: name,
                };
                request.reply_final(&msg)?
            },
            "clearPseudoClassLocks" => {
                let msg = EmptyReplyMsg { from: name };
                request.reply_final(&msg)?
            },
            "documentElement" => {
                let Some((tx, rx)) = generic_channel::channel() else {
                    return Err(ActorError::Internal);
                };
                self.script_chan
                    .send(GetDocumentElement(self.pipeline, tx))
                    .map_err(|_| ActorError::Internal)?;
                let doc_elem_info = rx
                    .recv()
                    .map_err(|_| ActorError::Internal)?
                    .ok_or(ActorError::Internal)?;
                let node = doc_elem_info.encode(
                    registry,
                    self.script_chan.clone(),
                    self.pipeline,
                    name.clone(),
                );

                let msg = DocumentElementReply {
                    from: name,
                    node,
                };
                request.reply_final(&msg)?
            },
            "getLayoutInspector" => {
                let mut msg = GetLayoutInspectorReply {
                    from: name,
                    actor: None,
                };
                registry.register_with(|name| {
                    let actor = LayoutInspectorActor {};
                    msg.actor = Some(actor.encode(name.into(), registry));
                    actor
                });
                request.reply_final(&msg)?
            },
            "getMutations" => {
                let msg = GetMutationsReply {
                    from: name,
                    mutations: self
                        .mutations
                        .borrow_mut()
                        .drain(..)
                        .map(|(mutation, target)| MutationMsg {
                            attribute_name: mutation.attribute_name,
                            new_value: mutation.new_value,
                            target,
                            type_: "attributes".into(),
                        })
                        .collect(),
                };
                request.reply_final(&msg)?
            },
            "getOffsetParent" => {
                let msg = GetOffsetParentReply {
                    from: name,
                    node: None,
                };
                request.reply_final(&msg)?
            },
            "querySelector" => {
                let selector = msg
                    .get("selector")
                    .ok_or(ActorError::MissingParameter)?
                    .as_str()
                    .ok_or(ActorError::BadParameterType)?;
                let node = msg
                    .get("node")
                    .ok_or(ActorError::MissingParameter)?
                    .as_str()
                    .ok_or(ActorError::BadParameterType)?;
                let mut hierarchy = find_child(
                    &self.script_chan,
                    self.pipeline,
                    &name,
                    registry,
                    node,
                    vec![],
                    |msg| msg.display_name == selector,
                )
                .map_err(|_| ActorError::Internal)?;
                hierarchy.reverse();
                let node = hierarchy.pop().ok_or(ActorError::Internal)?;

                let msg = QuerySelectorReply {
                    from: name,
                    node,
                    new_parents: hierarchy,
                };
                request.reply_final(&msg)?
            },
            "watchRootNode" => {
                let msg = WatchRootNodeNotification {
                    type_: "root-available".into(),
                    from: name.clone(),
                    node: self.root_node.clone(),
                };
                let _ = request.write_json_packet(&msg);

                let msg = EmptyReplyMsg { from: name };
                request.reply_final(&msg)?
            },
            _ => return Err(ActorError::UnrecognizedPacketType),
        };
        Ok(())
    }
}

impl WalkerActor {
    pub(crate) fn new_mutations(
        &self,
        name: String,
        request: &mut ClientRequest,
        target: &str,
        modifications: &[AttrModification],
    ) {
        {
            let mut mutations = self.mutations.borrow_mut();
            mutations.extend(modifications.iter().cloned().map(|m| (m, target.into())));
        }
        let _ = request.write_json_packet(&NewMutationsNotification {
            from: name,
            type_: "newMutations".into(),
        });
    }
}

/// Recursively searches for a child with the specified selector
/// If it is found, returns a list with the child and all of its ancestors.
/// TODO: Investigate how to cache this to some extent.
pub fn find_child(
    script_chan: &GenericSender<DevtoolScriptControlMsg>,
    pipeline: PipelineId,
    name: &str,
    registry: &ActorRegistry,
    node: &str,
    mut hierarchy: Vec<NodeActorMsg>,
    compare_fn: impl Fn(&NodeActorMsg) -> bool + Clone,
) -> Result<Vec<NodeActorMsg>, Vec<NodeActorMsg>> {
    let (tx, rx) = generic_channel::channel().unwrap();
    script_chan
        .send(GetChildren(
            pipeline,
            registry.actor_to_script(node.into()),
            tx,
        ))
        .unwrap();
    let children = rx.recv().unwrap().ok_or(vec![])?;

    for child in children {
        let msg = child.encode(registry, script_chan.clone(), pipeline, name.into());
        if compare_fn(&msg) {
            hierarchy.push(msg);
            return Ok(hierarchy);
        };

        if msg.num_children == 0 {
            continue;
        }

        match find_child(
            script_chan,
            pipeline,
            name,
            registry,
            &msg.actor,
            hierarchy,
            compare_fn.clone(),
        ) {
            Ok(mut hierarchy) => {
                hierarchy.push(msg);
                return Ok(hierarchy);
            },
            Err(e) => {
                hierarchy = e;
            },
        }
    }
    Err(hierarchy)
}
