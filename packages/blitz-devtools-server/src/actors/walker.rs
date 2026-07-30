use std::collections::HashMap;

use blitz_dom::BaseDocument;
use blitz_dom::node::{Node, NodeData};
use blitz_traits::node_id::NodeId;
use serde_json::json;

use crate::actors::layout::LayoutActor;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};
use crate::{GenericClientMessage, JsonValue};

const ELEMENT_NODE: u32 = 1;
const TEXT_NODE: u32 = 3;
const COMMENT_NODE: u32 = 8;
const DOCUMENT_NODE: u32 = 9;

const MAX_INLINE_TEXT_LENGTH: usize = 50;

/// The walker actor implements devtools' view of the DOM tree. It assigns
/// stable actor names to Blitz `NodeId`s and serializes "node forms" for the
/// markup view.
pub(crate) struct WalkerActor {
    name: String,
    pub(crate) doc_id: usize,
    layout_actor_name: Option<String>,
    node_to_actor: HashMap<NodeId, String>,
    actor_to_node: HashMap<String, NodeId>,
}

impl WalkerActor {
    pub(crate) fn new(doc_id: usize) -> Self {
        Self {
            name: generate_name("walker"),
            doc_id,
            layout_actor_name: None,
            node_to_actor: HashMap::new(),
            actor_to_node: HashMap::new(),
        }
    }

    /// Get (assigning if necessary) the actor name for a node
    pub(crate) fn actor_for_node(&mut self, node_id: NodeId) -> String {
        if let Some(name) = self.node_to_actor.get(&node_id) {
            return name.clone();
        }
        let name = generate_name("node");
        self.node_to_actor.insert(node_id, name.clone());
        self.actor_to_node.insert(name.clone(), node_id);
        name
    }

    /// Resolve a node actor name back to a `NodeId`
    pub(crate) fn node_for_actor(&self, actor: &str) -> Option<NodeId> {
        self.actor_to_node.get(actor).copied()
    }

    /// The DOM children of a node (excluding anonymous boxes, which live in
    /// the layout tree rather than the DOM tree, and whitespace-only text
    /// nodes, which are just noise in the inspector)
    fn dom_children<'doc>(doc: &'doc BaseDocument, node: &Node) -> Vec<&'doc Node> {
        node.children
            .iter()
            .filter_map(|child_id| doc.get_node(*child_id))
            .filter(|child| !child.is_anonymous() && !child.is_whitespace_node())
            .collect()
    }

    /// Serialize a node to its devtools "form"
    pub(crate) fn node_form(&mut self, doc: &BaseDocument, node_id: NodeId) -> Option<JsonValue> {
        let node = doc.get_node(node_id)?;
        let actor = self.actor_for_node(node_id);

        let (node_type, node_name, node_value) = match &node.data {
            NodeData::Document(_) => (DOCUMENT_NODE, "#document".to_string(), None),
            NodeData::Element(el) | NodeData::AnonymousBlock(el) => {
                (ELEMENT_NODE, el.name.local.to_uppercase(), None)
            }
            NodeData::Text(_) => (TEXT_NODE, "#text".to_string(), Some(node.text_content())),
            NodeData::Comment => (COMMENT_NODE, "#comment".to_string(), None),
        };

        let attrs: Vec<JsonValue> = node
            .attrs()
            .unwrap_or_default()
            .iter()
            .map(|attr| json!({ "name": attr.name.local.to_string(), "value": attr.value }))
            .collect();

        let display_type = node
            .primary_styles()
            .map(|styles| display_string(&styles))
            .unwrap_or_else(|| "block".to_string());
        let is_displayed = display_type != "none";

        let children = Self::dom_children(doc, node);
        let num_children = children.len();

        // If the node has a single small text child, represent it inline
        let inline_text_child = if num_children == 1 && children[0].is_text_node() {
            let text = children[0].text_content();
            let child_id = children[0].id;
            if text.len() <= MAX_INLINE_TEXT_LENGTH {
                self.node_form(doc, child_id)
            } else {
                None
            }
        } else {
            None
        };

        let parent = node
            .parent
            .map(|parent_id| self.actor_for_node(parent_id))
            .unwrap_or_default();

        let is_top_level_document = node.parent.is_none();

        Some(json!({
            "actor": actor,
            "attrs": attrs,
            "baseURI": doc.url().to_string(),
            "causesOverflow": false,
            "containerType": null,
            "displayName": node_name.to_lowercase(),
            "displayType": display_type,
            "host": null,
            "inlineTextChild": inline_text_child,
            "isAfterPseudoElement": false,
            "isAnonymous": node.is_anonymous(),
            "isBeforePseudoElement": false,
            "isDirectShadowHostChild": null,
            "isDisplayed": is_displayed,
            "isInHTMLDocument": true,
            "isMarkerPseudoElement": false,
            "isNativeAnonymous": false,
            "isScrollable": false,
            "isShadowHost": false,
            "isShadowRoot": false,
            "isTopLevelDocument": is_top_level_document,
            "nodeName": node_name,
            "nodeType": node_type,
            "nodeValue": node_value,
            "numChildren": num_children,
            "parent": parent,
            "shadowRootMode": null,
            "traits": {
                "supportsIsUsedInCustomProperty": false,
            },
        }))
    }

    /// The form of the document root node
    pub(crate) fn root_form(&mut self, ctx: &mut DevtoolContext<'_>) -> Option<JsonValue> {
        let result = ctx.with_doc(self.doc_id, |doc| {
            let root_id = doc.root_node().id;
            self.node_form(doc, root_id)
        });
        result.flatten()
    }

    /// Build a "unique" selector for a node (best effort)
    fn unique_selector(doc: &BaseDocument, node: &Node) -> String {
        let Some(element) = node.element_data() else {
            return node.node_debug_str();
        };
        if let Some(id) = &element.id {
            return format!("#{id}");
        }
        let tag = element.name.local.to_string();
        // Disambiguate with :nth-child if the node has same-tag siblings
        let nth = node
            .parent
            .and_then(|parent_id| doc.get_node(parent_id))
            .and_then(|parent| {
                let siblings = Self::dom_children(doc, parent);
                if siblings
                    .iter()
                    .filter(|sibling| sibling.is_element())
                    .count()
                    > 1
                {
                    siblings
                        .iter()
                        .position(|sibling| sibling.id == node.id)
                        .map(|idx| idx + 1)
                } else {
                    None
                }
            });
        match nth {
            Some(nth) => format!("{tag}:nth-child({nth})"),
            None => tag,
        }
    }
}

impl Actor for WalkerActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        let doc_id = self.doc_id;
        match &*message.type_ {
            "children" => {
                let msg = message.data.json()?;
                let node_actor = msg
                    .get("node")
                    .and_then(|v| v.as_str())
                    .ok_or(ActorMessageErr::MissingParameter)?;
                let node_id = self
                    .node_for_actor(node_actor)
                    .ok_or(ActorMessageErr::NoSuchNode)?;

                let nodes = ctx.with_doc(doc_id, |doc| {
                    let Some(node) = doc.get_node(node_id) else {
                        return Vec::new();
                    };
                    let child_ids: Vec<NodeId> =
                        Self::dom_children(doc, node).iter().map(|c| c.id).collect();
                    child_ids
                        .into_iter()
                        .filter_map(|child_id| self.node_form(doc, child_id))
                        .collect::<Vec<_>>()
                });
                let nodes = nodes.ok_or(ActorMessageErr::NoSuchDocument)?;

                ctx.write_msg(
                    self.name(),
                    json!({
                        "hasFirst": true,
                        "hasLast": true,
                        "nodes": nodes,
                    }),
                );
                Ok(())
            }
            "querySelector" => {
                let msg = message.data.json()?;
                let node_actor = msg
                    .get("node")
                    .and_then(|v| v.as_str())
                    .ok_or(ActorMessageErr::MissingParameter)?;
                let selector = msg
                    .get("selector")
                    .and_then(|v| v.as_str())
                    .ok_or(ActorMessageErr::MissingParameter)?
                    .to_string();
                let _context_node = self
                    .node_for_actor(node_actor)
                    .ok_or(ActorMessageErr::NoSuchNode)?;

                let known_nodes: Vec<NodeId> = self.node_to_actor.keys().copied().collect();
                let result = ctx.with_doc(doc_id, |doc| {
                    let node_id = doc.query_selector(&selector).ok().flatten()?;
                    let node_form = self.node_form(doc, node_id)?;

                    // Send forms for any ancestors that the client doesn't
                    // know about yet, so that it can connect the node to
                    // its existing tree
                    let mut new_parents = Vec::new();
                    let mut current = doc.get_node(node_id).and_then(|n| n.parent);
                    while let Some(ancestor_id) = current {
                        let already_known = known_nodes.contains(&ancestor_id);
                        if let Some(form) = self.node_form(doc, ancestor_id) {
                            new_parents.push(form);
                        }
                        if already_known {
                            break;
                        }
                        current = doc.get_node(ancestor_id).and_then(|n| n.parent);
                    }

                    Some((node_form, new_parents))
                });

                match result.ok_or(ActorMessageErr::NoSuchDocument)? {
                    Some((node, new_parents)) => {
                        ctx.write_msg(
                            self.name(),
                            json!({ "node": node, "newParents": new_parents }),
                        );
                    }
                    None => {
                        ctx.write_msg(self.name(), json!({}));
                    }
                }
                Ok(())
            }
            "watchRootNode" => {
                // Send a "root-available" notification followed by an empty reply
                let root = self.root_form(ctx).ok_or(ActorMessageErr::NoSuchDocument)?;
                ctx.write_msg(
                    self.name(),
                    json!({ "type": "root-available", "node": root }),
                );
                ctx.write_msg(self.name(), json!({}));
                Ok(())
            }
            "documentElement" => {
                let form = ctx.with_doc(doc_id, |doc| {
                    let root = doc.root_node();
                    let html_id = Self::dom_children(doc, root)
                        .iter()
                        .find(|child| child.is_element())
                        .map(|child| child.id)?;
                    self.node_form(doc, html_id)
                });
                let form = form.flatten().ok_or(ActorMessageErr::NoSuchDocument)?;
                ctx.write_msg(self.name(), json!({ "node": form }));
                Ok(())
            }
            "getUniqueSelector" => {
                let msg = message.data.json()?;
                let node_actor = msg
                    .get("node")
                    .and_then(|v| v.as_str())
                    .ok_or(ActorMessageErr::MissingParameter)?;
                let node_id = self
                    .node_for_actor(node_actor)
                    .ok_or(ActorMessageErr::NoSuchNode)?;
                let selector = ctx
                    .with_doc(doc_id, |doc| {
                        doc.get_node(node_id)
                            .map(|node| Self::unique_selector(doc, node))
                    })
                    .flatten()
                    .unwrap_or_default();
                ctx.write_msg(self.name(), json!({ "value": selector }));
                Ok(())
            }
            // Firefox resolves flex-item/grid actors to DOM nodes via this
            // request
            "getNodeFromActor" => {
                let msg = message.data.json()?;
                let actor_id = msg
                    .get("actorID")
                    .and_then(|v| v.as_str())
                    .ok_or(ActorMessageErr::MissingParameter)?;
                let node_id = ctx
                    .actors
                    .get(actor_id)
                    .and_then(|actor| {
                        let any: &dyn std::any::Any = actor.as_ref();
                        any.downcast_ref::<crate::actors::layout::FlexItemActor>()
                            .map(|a| a.node_id)
                    })
                    .ok_or(ActorMessageErr::NoSuchNode)?;
                let form = ctx
                    .with_doc(doc_id, |doc| self.node_form(doc, node_id))
                    .flatten()
                    .ok_or(ActorMessageErr::NoSuchNode)?;
                ctx.write_msg(
                    self.name(),
                    json!({ "node": { "node": form, "newParents": [] } }),
                );
                Ok(())
            }
            "getOffsetParent" => {
                ctx.write_msg(self.name(), json!({ "node": null }));
                Ok(())
            }
            "getLayoutInspector" => {
                let layout_name = match &self.layout_actor_name {
                    Some(name) => name.clone(),
                    None => {
                        let layout = LayoutActor::new(self.doc_id, self.name());
                        let name = layout.name();
                        self.layout_actor_name = Some(name.clone());
                        ctx.push_actor(Box::new(layout));
                        name
                    }
                };
                ctx.write_msg(self.name(), json!({ "actor": { "actor": layout_name } }));
                Ok(())
            }
            "getMutations" => {
                ctx.write_msg(self.name(), json!({ "mutations": [] }));
                Ok(())
            }
            "isInDOMTree" => {
                ctx.write_msg(self.name(), json!({ "attached": true }));
                Ok(())
            }
            "retainNode"
            | "unretainNode"
            | "releaseNode"
            | "clearPseudoClassLocks"
            | "removeNode"
            | "watchSeekableNodes" => {
                ctx.write_msg(self.name(), json!({}));
                Ok(())
            }
            _ => Err(ActorMessageErr::UnrecognizedPacketType),
        }
    }
}

/// Serialize a node's computed `display` value
pub(crate) fn display_string(styles: &style::properties::ComputedValues) -> String {
    use style::properties::{LonghandId, PropertyDeclarationId};
    styles.computed_value_to_string(PropertyDeclarationId::Longhand(LonghandId::Display))
}
