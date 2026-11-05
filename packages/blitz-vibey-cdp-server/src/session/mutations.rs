//! Translation of buffered DOM mutation records into `DOM` domain events

use std::collections::{HashMap, HashSet};

use blitz_dom::{BaseDocument, NodeData};
use blitz_traits::node_id::NodeId;
use serde_json::json;

use super::{
    Session, cdp_node_id, emit_child_count_updated, emit_child_inserted, forget_subtree,
    is_known_to_frontend,
};
use crate::MessageWriter;
use blitz_traits::devtools::MutationRecord;

impl Session {
    /// Emit the events describing a batch of mutations to this session's
    /// document. Events are derived from the document's current state, so
    /// a batch must contain every mutation since the previous one.
    pub(crate) fn emit_mutations<'a>(
        &mut self,
        writer: &mut MessageWriter,
        doc: &BaseDocument,
        records: impl Iterator<Item = &'a MutationRecord> + Clone,
    ) {
        // The frontend has no tree until it has requested the document
        if self.children_sent.is_empty() {
            return;
        }
        let children_sent = &mut self.children_sent;

        // An insertion undone by a later removal in the same batch cancels out
        let mut cancelled = HashSet::new();
        let mut pending_inserts = HashMap::new();
        for (index, record) in records.clone().enumerate() {
            match record {
                MutationRecord::Inserted { parent_id, node_id } => {
                    pending_inserts.insert((*parent_id, *node_id), index);
                }
                MutationRecord::Removed { parent_id, node_id } => {
                    if let Some(insert_index) = pending_inserts.remove(&(*parent_id, *node_id)) {
                        cancelled.insert(insert_index);
                        cancelled.insert(index);
                    }
                }
                _ => {}
            }
        }

        // Removals first: inserted nodes are placed relative to their current
        // siblings, which removed nodes no longer are
        let mut count_updates = Vec::new();
        for (index, record) in records.clone().enumerate() {
            let MutationRecord::Removed { parent_id, node_id } = *record else {
                continue;
            };
            if cancelled.contains(&index) {
                continue;
            }
            // The frontend discards a removed subtree's nodes
            forget_subtree(doc, node_id, children_sent);
            children_sent.remove(&node_id);
            if children_sent.contains(&parent_id) {
                writer.event(
                    "DOM.childNodeRemoved",
                    json!({ "parentNodeId": cdp_node_id(parent_id), "nodeId": cdp_node_id(node_id) }),
                );
            } else {
                count_updates.push(parent_id);
            }
        }

        // Insertions of nodes still in the parent they were inserted into
        let mut inserted: Vec<(NodeId, usize, NodeId)> = Vec::new();
        for (index, record) in records.clone().enumerate() {
            let MutationRecord::Inserted { parent_id, node_id } = *record else {
                continue;
            };
            if cancelled.contains(&index) {
                continue;
            }
            let Some(parent) = doc.get_node(parent_id) else {
                continue;
            };
            if !parent.flags.is_in_document() {
                continue;
            }
            let Some(child_index) = parent.index_of_child(node_id) else {
                continue;
            };
            inserted.push((parent_id, child_index, node_id));
        }
        // In tree order, so that each node's previous sibling is already known
        inserted.sort_unstable();
        inserted.dedup();
        // An inserted node is sent with its current subtree, so changes within
        // inserted subtrees are already reflected
        let inserted_ids: HashSet<NodeId> = inserted.iter().map(|&(_, _, id)| id).collect();
        let in_inserted_subtree = |node_id: NodeId| {
            let mut current = Some(node_id);
            while let Some(id) = current {
                if inserted_ids.contains(&id) {
                    return true;
                }
                current = doc.get_node(id).and_then(|node| node.parent);
            }
            false
        };
        inserted.retain(|&(parent_id, _, _)| !in_inserted_subtree(parent_id));
        // An inserted node is new to the frontend: none of its children have
        // been sent (its id may have been reused from a dropped node)
        for &(_, _, node_id) in &inserted {
            forget_subtree(doc, node_id, children_sent);
        }
        for &(parent_id, _, node_id) in &inserted {
            if children_sent.contains(&parent_id) {
                emit_child_inserted(writer, doc, node_id, children_sent);
            } else {
                count_updates.push(parent_id);
            }
        }

        // Attribute and text changes, reported with their current values
        let mut changed = HashSet::new();
        for record in records {
            match record {
                MutationRecord::AttributeChanged { node_id, name } => {
                    let node_id = *node_id;
                    if !changed.insert((node_id, Some(name.as_str())))
                        || in_inserted_subtree(node_id)
                        || !is_known_to_frontend(doc, node_id, children_sent)
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
                MutationRecord::CharacterDataChanged { node_id } => {
                    let node_id = *node_id;
                    if !changed.insert((node_id, None))
                        || in_inserted_subtree(node_id)
                        || !is_known_to_frontend(doc, node_id, children_sent)
                    {
                        continue;
                    }
                    let Some(NodeData::Text(text)) = doc.get_node(node_id).map(|n| &n.data) else {
                        continue;
                    };
                    writer.event(
                        "DOM.characterDataModified",
                        json!({ "nodeId": cdp_node_id(node_id), "characterData": text.content }),
                    );
                }
                _ => {}
            }
        }

        // Child counts of known nodes whose children the frontend hasn't loaded
        count_updates.sort_unstable();
        count_updates.dedup();
        for parent_id in count_updates {
            if !in_inserted_subtree(parent_id) {
                emit_child_count_updated(writer, doc, parent_id, children_sent);
            }
        }
    }
}
