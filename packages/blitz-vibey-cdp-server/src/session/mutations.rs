//! Translation of buffered DOM mutation records into `DOM` domain events

use std::collections::{HashMap, HashSet};

use blitz_dom::{BaseDocument, NodeData};
use blitz_traits::devtools::{
    AttributeChanged, CharacterDataChanged, MutationRecord, NodeInserted, NodeRemoved,
};
use blitz_traits::node_id::NodeId;
use serde_json::json;

use super::{Session, cdp_node_id, emit_child_count_updated, forget_subtree, is_known_to_frontend};
use crate::MessageWriter;

impl Session {
    /// Emit the events for a batch of mutations to this session's document,
    /// in the order they were made. Inserted nodes are serialized from the
    /// document's current state, so later records within a subtree sent in
    /// this batch are already reflected and skipped.
    pub(crate) fn emit_mutations<'a>(
        &mut self,
        writer: &mut MessageWriter,
        doc: &BaseDocument,
        records: impl Iterator<Item = &'a MutationRecord>,
    ) {
        // The frontend has no tree until it has requested the document
        if self.children_sent.is_empty() {
            return;
        }
        let children_sent = &mut self.children_sent;
        // Roots of the subtrees sent in this batch
        let mut sent_roots = HashSet::new();
        // Inserted nodes that were not sent, with their previous sibling
        let mut unsent: HashMap<NodeId, Option<NodeId>> = HashMap::new();
        // Known nodes whose unloaded children changed
        let mut count_updates = Vec::new();
        // Attributes/text already reported (with their current value)
        let mut changed = HashSet::new();

        let in_sent_subtree = |sent_roots: &HashSet<NodeId>, node_id: NodeId| {
            let mut current = Some(node_id);
            while let Some(id) = current {
                if sent_roots.contains(&id) {
                    return true;
                }
                current = doc.get_node(id).and_then(|node| node.parent);
            }
            false
        };
        let is_hidden = |node_id: NodeId| {
            doc.get_node(node_id)
                .is_some_and(|node| node.is_whitespace_node() || node.is_anonymous())
        };

        for record in records {
            match record {
                MutationRecord::NodeInserted(NodeInserted {
                    parent_id,
                    node_id,
                    previous_sibling_id,
                }) => {
                    let (parent_id, node_id) = (*parent_id, *node_id);
                    if in_sent_subtree(&sent_roots, parent_id) {
                        continue;
                    }
                    // The node is new to the frontend (its id may be reused)
                    forget_subtree(doc, node_id, children_sent);
                    if !children_sent.contains(&parent_id) {
                        unsent.insert(node_id, *previous_sibling_id);
                        count_updates.push(parent_id);
                        continue;
                    }
                    let node = (!is_hidden(node_id))
                        .then(|| crate::dom::node_json(doc, node_id, 0, children_sent))
                        .flatten();
                    let Some(node) = node else {
                        unsent.insert(node_id, *previous_sibling_id);
                        continue;
                    };
                    let mut previous = *previous_sibling_id;
                    while let Some(&before) = previous.and_then(|id| unsent.get(&id)) {
                        previous = before;
                    }
                    unsent.remove(&node_id);
                    sent_roots.insert(node_id);
                    writer.event(
                        "DOM.childNodeInserted",
                        json!({
                            "parentNodeId": cdp_node_id(parent_id),
                            "previousNodeId": previous.map(cdp_node_id).unwrap_or(0),
                            "node": node,
                        }),
                    );
                }
                MutationRecord::NodeRemoved(NodeRemoved { parent_id, node_id }) => {
                    let (parent_id, node_id) = (*parent_id, *node_id);
                    if in_sent_subtree(&sent_roots, parent_id) {
                        continue;
                    }
                    forget_subtree(doc, node_id, children_sent);
                    sent_roots.remove(&node_id);
                    if !children_sent.contains(&parent_id) {
                        count_updates.push(parent_id);
                    } else if !unsent.contains_key(&node_id) && !is_hidden(node_id) {
                        writer.event(
                            "DOM.childNodeRemoved",
                            json!({ "parentNodeId": cdp_node_id(parent_id), "nodeId": cdp_node_id(node_id) }),
                        );
                    }
                }
                MutationRecord::AttributeChanged(AttributeChanged { node_id, name }) => {
                    let node_id = *node_id;
                    if in_sent_subtree(&sent_roots, node_id)
                        || unsent.contains_key(&node_id)
                        || !is_known_to_frontend(doc, node_id, children_sent)
                        || !changed.insert((node_id, Some(name.as_str())))
                    {
                        continue;
                    }
                    let Some(element) = doc.get_node(node_id).and_then(|n| n.element_data()) else {
                        continue;
                    };
                    let value = element
                        .attrs()
                        .iter()
                        .find(|attr| &*attr.name.local == name.as_str())
                        .map(|attr| attr.value.to_string());
                    match value {
                        Some(value) => writer.event(
                            "DOM.attributeModified",
                            json!({ "nodeId": cdp_node_id(node_id), "name": name.as_str(), "value": value }),
                        ),
                        None => writer.event(
                            "DOM.attributeRemoved",
                            json!({ "nodeId": cdp_node_id(node_id), "name": name.as_str() }),
                        ),
                    }
                }
                MutationRecord::CharacterDataChanged(CharacterDataChanged { node_id }) => {
                    let node_id = *node_id;
                    if in_sent_subtree(&sent_roots, node_id)
                        || unsent.contains_key(&node_id)
                        || !is_known_to_frontend(doc, node_id, children_sent)
                        || !changed.insert((node_id, None))
                    {
                        continue;
                    }
                    let data = match doc.get_node(node_id).map(|n| &n.data) {
                        Some(NodeData::Text(text)) => &text.content,
                        Some(NodeData::Comment { contents }) => contents,
                        _ => continue,
                    };
                    writer.event(
                        "DOM.characterDataModified",
                        json!({ "nodeId": cdp_node_id(node_id), "characterData": data }),
                    );
                }
            }
        }

        // Sent last, once the counted nodes themselves are known to the frontend
        count_updates.sort_unstable();
        count_updates.dedup();
        for parent_id in count_updates {
            if !unsent.contains_key(&parent_id) && !in_sent_subtree(&sent_roots, parent_id) {
                emit_child_count_updated(writer, doc, parent_id, children_sent);
            }
        }
    }
}
