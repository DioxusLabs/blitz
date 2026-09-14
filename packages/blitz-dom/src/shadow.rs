//! Shadow DOM: encapsulated subtrees, slot assignment and scoped styles.
//!
//! `Node::children` is never rewritten: it stays the light DOM, so
//! index-based mutations, `childNodes`, `querySelector` and text content keep
//! their DOM semantics. The composed (flat) tree is a *view*, read through
//! [`crate::node::Node::composed_children`]: a shadow host yields its shadow
//! root's children, a `<slot>` its assigned nodes. Style traversal, box
//! construction, painting and hit testing walk that view; every new tree walk
//! must consciously pick light or composed.
//!
//! A shadow root is a node of element kind named `shadow-root`, flagged
//! `IS_SHADOW_ROOT`, whose `parent` is its host but which is not in the host's
//! `children`. Slot assignment is tree-side state (`assigned_nodes` on the
//! slot, `assigned_slot` on the light child), refreshed by
//! [`BaseDocument::assign_slots`] before style resolution.
//!
//! Not yet: `::slotted()`, closed-mode enforcement (mode is recorded only),
//! declarative shadow DOM.

use blitz_traits::node_id::NodeId;
use markup5ever::{LocalName, QualName, local_name, ns};

use style::author_styles::AuthorStyles;
use style::stylesheets::{CustomMediaMap, DocumentStyleSheet};

use crate::node::{Attribute, NodeData, NodeFlags};
use crate::{BaseDocument, DocumentMutator};

/// `attachShadow({ mode })`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowRootMode {
    Open,
    Closed,
}

impl DocumentMutator<'_> {
    /// Attaches a shadow root to `host` and returns its id. The host's current
    /// children become its light DOM. Calling it twice returns the existing root.
    pub fn attach_shadow(&mut self, host_id: NodeId, mode: ShadowRootMode) -> NodeId {
        if let Some(existing) = self.doc.nodes[host_id].shadow_root {
            return existing;
        }
        let name = QualName::new(None, ns!(html), LocalName::from("shadow-root"));
        let attrs = vec![Attribute {
            name: QualName::new(None, ns!(), LocalName::from("mode")),
            value: match mode {
                ShadowRootMode::Open => "open".into(),
                ShadowRootMode::Closed => "closed".into(),
            },
        }];
        let root_id = self.create_element(name, attrs);
        let in_document = self.doc.nodes[host_id].flags.is_in_document();
        self.doc.nodes[host_id].shadow_root = Some(root_id);
        {
            let root = &mut self.doc.nodes[root_id];
            root.parent = Some(host_id);
            root.flags.insert(NodeFlags::IS_SHADOW_ROOT);
            root.author_styles = Some(Box::new(AuthorStyles::new()));
            if in_document {
                root.flags.insert(NodeFlags::IS_IN_DOCUMENT);
            }
        }
        self.doc.shadow_hosts.push(host_id);
        self.doc.assign_slots_for(host_id);
        self.mark_dirty(host_id);
        root_id
    }

    fn mark_dirty(&mut self, node_id: NodeId) {
        // Re-run box construction for the host on the next resolve.
        self.doc.damage_box_owner(node_id);
        self.doc.nodes[node_id].mark_ancestors_dirty();
        self.doc.shell_provider.request_redraw();
    }
}

impl BaseDocument {
    /// Marks the node that owns boxes for `id` as needing box reconstruction:
    /// `id` itself, or its host when `id` is a shadow root (which has no style
    /// data to carry damage).
    pub(crate) fn damage_box_owner(&mut self, id: NodeId) {
        let target = if self.nodes[id].flags.contains(NodeFlags::IS_SHADOW_ROOT) {
            self.nodes[id].parent.unwrap_or(id)
        } else {
            id
        };
        self.nodes[target].insert_damage(crate::layout::damage::ALL_DAMAGE);
        self.nodes[target].mark_ancestors_dirty();
    }

    /// The `<slot>` name a light child asks for (`slot` attribute, "" = default).
    fn requested_slot(&self, id: NodeId) -> String {
        self.nodes[id]
            .data
            .attr(local_name!("slot"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }

    /// Re-assigns light children to the slots of every shadow tree.
    pub fn assign_slots(&mut self) {
        let hosts = self.shadow_hosts.clone();
        for host in hosts {
            self.assign_slots_for(host);
        }
    }

    pub(crate) fn assign_slots_for(&mut self, host_id: NodeId) {
        let Some(root_id) = self.nodes[host_id].shadow_root else { return };
        // Slots of this shadow tree in tree order, not descending into nested
        // shadow trees (their slots belong to their own host).
        let mut slots: Vec<NodeId> = Vec::new();
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id];
            if id != root_id && node.shadow_root.is_some() {
                continue;
            }
            if node.is_slot() {
                slots.push(id);
            }
            for child in node.children.iter().rev() {
                stack.push(*child);
            }
        }
        // First matching slot per name wins, as in the spec.
        let light: Vec<NodeId> = self.nodes[host_id].children.iter().copied().collect();
        let mut used = vec![false; light.len()];
        let mut changed = false;
        for slot_id in slots {
            let slot_name = self.nodes[slot_id]
                .data
                .attr(local_name!("name"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let mut assigned: Vec<NodeId> = Vec::new();
            for (i, child) in light.iter().enumerate() {
                if used[i] {
                    continue;
                }
                let wants = match &self.nodes[*child].data {
                    NodeData::Element(_) => self.requested_slot(*child),
                    NodeData::Text(_) => String::new(),
                    _ => continue,
                };
                if wants == slot_name {
                    assigned.push(*child);
                    used[i] = true;
                }
            }
            let slot = &mut self.nodes[slot_id];
            if slot.assigned_nodes.iter().copied().collect::<Vec<_>>() != assigned {
                slot.assigned_nodes.clear();
                slot.assigned_nodes.extend(assigned.iter().copied());
                changed = true;
                self.damage_box_owner(slot_id);
            }
            for child in assigned {
                self.nodes[child].assigned_slot = Some(slot_id);
            }
        }
        for (i, child) in light.iter().enumerate() {
            if !used[i] {
                self.nodes[*child].assigned_slot = None;
            }
        }
        if changed {
            self.damage_box_owner(host_id);
        }
    }

    /// The shadow root whose tree contains `id` (crossing light DOM of nested
    /// hosts correctly), or `None` in the document tree.
    pub fn containing_shadow_root(&self, id: NodeId) -> Option<NodeId> {
        let mut cur = self.nodes[id].parent;
        while let Some(p) = cur {
            let node = &self.nodes[p];
            if node.flags.contains(NodeFlags::IS_SHADOW_ROOT) {
                return Some(p);
            }
            cur = node.parent;
        }
        None
    }

    /// Registers a stylesheet with the shadow root that scopes `owner_id`.
    /// Returns false when the owner is in the document tree.
    pub(crate) fn add_shadow_stylesheet(&mut self, owner_id: NodeId, sheet: DocumentStyleSheet) -> bool {
        let Some(root) = self.containing_shadow_root(owner_id) else { return false };
        let guard = self.guard.read();
        let Self { nodes, stylist, .. } = self;
        let Some(styles) = nodes[root].author_styles.as_mut() else { return false };
        styles.stylesheets.append_stylesheet(Some(stylist.device()), &CustomMediaMap::default(), sheet.clone(), &guard);
        self.shadow_stylesheets.insert(owner_id, (root, sheet));
        true
    }

    /// Unregisters the stylesheet owned by `owner_id` from its shadow root, if any.
    pub(crate) fn remove_shadow_stylesheet(&mut self, owner_id: NodeId) -> bool {
        let Some((root, sheet)) = self.shadow_stylesheets.remove(&owner_id) else { return false };
        let guard = self.guard.read();
        let Self { nodes, stylist, .. } = self;
        if let Some(styles) = nodes[root].author_styles.as_mut() {
            styles.stylesheets.remove_stylesheet(Some(stylist.device()), &CustomMediaMap::default(), sheet, &guard);
        }
        true
    }

    /// After `subtree` is inserted: any stylesheet it owns that is registered
    /// with the document but now lives in a shadow tree moves to that shadow
    /// root's scope (a `<style>` built by script gets its text before it is
    /// appended, so it was first registered document-wide).
    pub(crate) fn rescope_stylesheets(&mut self, subtree: NodeId) {
        let mut stack = vec![subtree];
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.get(id) else { continue };
            stack.extend(node.children.iter().copied());
            let sheet = match node.element_data().map(|el| &el.special_data) {
                Some(crate::node::SpecialElementData::Stylesheet(sheet)) => sheet.clone(),
                _ => continue,
            };
            let in_shadow = self.containing_shadow_root(id).is_some();
            let registered_doc = self.nodes_to_stylesheet.contains_key(&id);
            if in_shadow && registered_doc {
                self.nodes_to_stylesheet.remove(&id);
                {
                    let guard = self.guard.read();
                    self.stylist.remove_stylesheet(sheet.clone(), &guard);
                    self.stylist.force_stylesheet_origins_dirty(style::stylesheets::OriginSet::all());
                }
                self.add_shadow_stylesheet(id, sheet);
                self.damage_box_owner(id);
            } else if !in_shadow && self.shadow_stylesheets.contains_key(&id) {
                self.remove_shadow_stylesheet(id);
                self.add_stylesheet_for_node(sheet, id);
            }
        }
    }

    /// Rebuilds the cascade data of every shadow root whose sheets changed.
    pub(crate) fn flush_shadow_styles(&mut self) {
        let hosts = self.shadow_hosts.clone();
        let guard = self.guard.read();
        let Self { nodes, stylist, .. } = self;
        for host in hosts {
            let Some(root) = nodes[host].shadow_root else { continue };
            let flushed = match nodes[root].author_styles.as_mut() {
                Some(styles) if styles.stylesheets.dirty() => {
                    let _invalidations = styles.flush(stylist, &guard);
                    true
                }
                _ => false,
            };
            if flushed {
                // Styles inside the shadow tree changed: restyle the host's subtree.
                if let Some(mut data) = nodes[host].try_stylo_element_data_mut().and_then(|s| s.get_mut()) {
                    data.hint |= style::invalidation::element::restyle_hints::RestyleHint::restyle_subtree();
                }
                nodes[host].mark_ancestors_dirty();
            }
        }
    }

    /// Shadow root attached to `host`, if any.
    pub fn shadow_root_of(&self, host: NodeId) -> Option<NodeId> {
        self.nodes[host].shadow_root
    }

    /// Whether `id` is a shadow root node.
    pub fn is_shadow_root(&self, id: NodeId) -> bool {
        self.nodes[id].flags.contains(NodeFlags::IS_SHADOW_ROOT)
    }

    /// Composed-tree children of `id` (see `Node::composed_children`).
    pub fn composed_children(&self, id: NodeId) -> &[NodeId] {
        self.nodes[id].composed_children()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BaseDocument, DocumentConfig};
    use markup5ever::{LocalName, QualName, ns};

    fn el(m: &mut DocumentMutator<'_>, tag: &str) -> NodeId {
        m.create_element(QualName::new(None, ns!(html), LocalName::from(tag)), Vec::new())
    }

    #[test]
    fn light_dom_is_untouched_and_slots_compose() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let (host, a, b, root, slot_default, slot_named, fallback) = {
            let mut m = doc.mutate();
            let host = el(&mut m, "div");
            let a = el(&mut m, "p");
            let b = el(&mut m, "span");
            m.set_attribute(b, QualName::new(None, ns!(), LocalName::from("slot")), "side");
            m.append_children(host, &[a, b]);
            let root = m.attach_shadow(host, ShadowRootMode::Open);
            let slot_default = el(&mut m, "slot");
            let fallback = m.create_text_node("fallback");
            m.append_children(slot_default, &[fallback]);
            let slot_named = el(&mut m, "slot");
            m.set_attribute(slot_named, QualName::new(None, ns!(), LocalName::from("name")), "side");
            m.append_children(root, &[slot_default, slot_named]);
            (host, a, b, root, slot_default, slot_named, fallback)
        };
        // Light DOM untouched; composed tree goes through the shadow root.
        assert_eq!(doc.nodes[host].children.as_slice(), &[a, b]);
        assert_eq!(doc.shadow_root_of(host), Some(root));
        assert_eq!(doc.composed_children(host), &[slot_default, slot_named]);
        doc.assign_slots();
        assert_eq!(doc.composed_children(slot_default), &[a]);
        assert_eq!(doc.composed_children(slot_named), &[b]);
        assert_eq!(doc.nodes[a].assigned_slot, Some(slot_default));
        // Slot children (fallback) are still its DOM children.
        assert_eq!(doc.nodes[slot_default].children.as_slice(), &[fallback]);
        // Removing the named child: its slot falls back to its own children (none here).
        doc.mutate().remove_node(b);
        doc.assign_slots();
        assert!(doc.composed_children(slot_named).is_empty());
        assert_eq!(doc.nodes[host].children.as_slice(), &[a]);
    }
}
