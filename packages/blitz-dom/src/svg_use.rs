//! Expansion of SVG `<use>` references.
//!
//! An inline `<svg>` is handed to usvg as a standalone document, so a `<use>` whose target lives
//! in another `<svg>` (an icon sprite) resolves to nothing. Browsers render a `<use>` from a
//! shadow tree: a clone of the target that inherits style from the `<use>` element. This pass
//! builds that tree in the DOM, as the `<use>` element's child, so the cascade styles the clone
//! in context (`color`, custom properties, inherited `fill`) and the serializer can write it
//! out in place of the reference.
//!
//! [`DocumentMutator`](crate::DocumentMutator) runs the pass when it flushes a batch that added an
//! SVG `<use>` element or an SVG element with an `id`. Clones are not updated when their target
//! changes later.

use blitz_traits::node_id::NodeId;
use markup5ever::{LocalName, QualName, local_name, ns};

use crate::BaseDocument;
use crate::node::Attribute;

/// Marks the root of a clone created for a `<use>` element; its value is the target's id.
pub(crate) const USE_CLONE_ATTR: &str = "data-blitz-use-clone";

/// References inside cloned content are expanded in further passes, up to this depth.
const MAX_DEPTH: usize = 8;

/// Upper bound on clones per document, against reference fan-out.
const MAX_CLONES: usize = 4096;

thread_local! {
    /// Set while [`BaseDocument::expand_svg_use_elements`] runs on this thread.
    static EXPANDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl BaseDocument {
    /// Expand every same-document SVG `<use>` reference into a clone of its target, appended as
    /// the `<use>` element's child. Idempotent: an expanded `<use>` is left alone.
    pub fn expand_svg_use_elements(&mut self) {
        // The clones are inserted through a `DocumentMutator`, whose flush would start this pass
        // again for every `<use>` they contain; the outer call's depth limit covers those.
        if EXPANDING.replace(true) {
            return;
        }
        self.expand_svg_use_elements_to_depth();
        EXPANDING.set(false);
    }

    fn expand_svg_use_elements_to_depth(&mut self) {
        let mut clones = 0;
        for _ in 0..MAX_DEPTH {
            let pending = self.unexpanded_svg_uses();
            if pending.is_empty() {
                return;
            }
            for (use_id, target_id) in pending {
                if clones == MAX_CLONES {
                    return;
                }
                self.expand_svg_use(use_id, target_id);
                clones += 1;
            }
        }
    }

    /// `<use>` elements in tree order that have not been expanded, with their targets.
    fn unexpanded_svg_uses(&self) -> Vec<(NodeId, NodeId)> {
        let mut found = Vec::new();
        let mut stack = vec![self.root_node().id];
        while let Some(node_id) = stack.pop() {
            let node = &self.nodes[node_id];
            stack.extend(node.children.iter().rev().copied());
            let Some(elem) = node.element_data() else {
                continue;
            };
            if elem.name.ns != ns!(svg) || elem.name.local != local_name!("use") {
                continue;
            }
            if node.children.iter().any(|&child| self.is_use_clone(child)) {
                continue;
            }
            let Some(target) = use_href(elem.attrs()).and_then(|href| href.strip_prefix('#'))
            else {
                continue;
            };
            let Some(target_id) = self.get_element_by_id(target) else {
                continue;
            };
            // A circular reference renders nothing, as in browsers.
            if !self.is_ancestor_or_self(target_id, node_id)
                && !self.is_inside_clone_of(target, node_id)
            {
                found.push((node_id, target_id));
            }
        }
        found
    }

    fn is_use_clone(&self, node_id: NodeId) -> bool {
        self.nodes[node_id].element_data().is_some_and(|elem| {
            elem.attrs()
                .iter()
                .any(|attr| *attr.name.local == *USE_CLONE_ATTR)
        })
    }

    /// Whether `node_id` sits inside a clone made for a `<use>` of `target`.
    fn is_inside_clone_of(&self, target: &str, node_id: NodeId) -> bool {
        let mut current = self.nodes[node_id].parent;
        while let Some(id) = current {
            let source = self.nodes[id].element_data().and_then(|elem| {
                elem.attrs()
                    .iter()
                    .find(|attr| *attr.name.local == *USE_CLONE_ATTR)
            });
            if source.is_some_and(|attr| attr.value == target) {
                return true;
            }
            current = self.nodes[id].parent;
        }
        false
    }

    fn is_ancestor_or_self(&self, ancestor_id: NodeId, node_id: NodeId) -> bool {
        let mut current = Some(node_id);
        while let Some(id) = current {
            if id == ancestor_id {
                return true;
            }
            current = self.nodes[id].parent;
        }
        false
    }

    fn expand_svg_use(&mut self, use_id: NodeId, target_id: NodeId) {
        let use_size: Vec<(LocalName, String)> = [local_name!("width"), local_name!("height")]
            .into_iter()
            .filter_map(|name| {
                let value = self.nodes[use_id].attr(name.clone())?.to_string();
                Some((name, value))
            })
            .collect();
        let Some(target) = self.nodes[target_id].element_data() else {
            return;
        };
        let is_symbol = target.name.local == local_name!("symbol");
        let source = target.id.as_deref().unwrap_or_default().to_string();
        // Without its id, the clone cannot be found in place of the original.
        let mut attrs: Vec<Attribute> = target
            .attrs()
            .iter()
            .filter(|attr| !(attr.name.ns == ns!() && attr.name.local == local_name!("id")))
            .cloned()
            .collect();
        attrs.push(Attribute {
            name: QualName::new(None, ns!(), LocalName::from(USE_CLONE_ATTR)),
            value: source.clone(),
        });

        let mut mutator = self.mutate();
        let clone_id = mutator.deep_clone_node(target_id);
        let root_id = if is_symbol {
            // A referenced `<symbol>` renders as an `<svg>` sized by the `<use>` element.
            attrs.retain(|attr| {
                attr.name.local != local_name!("width") && attr.name.local != local_name!("height")
            });
            for name in [local_name!("width"), local_name!("height")] {
                let value = use_size
                    .iter()
                    .find(|(dim, _)| *dim == name)
                    .map_or("100%", |(_, value)| value.as_str());
                attrs.push(Attribute {
                    name: QualName::new(None, ns!(), name),
                    value: value.to_string(),
                });
            }
            let svg_id =
                mutator.create_element(QualName::new(None, ns!(svg), local_name!("svg")), attrs);
            mutator.reparent_children(clone_id, svg_id);
            mutator.remove_and_drop_node(clone_id);
            svg_id
        } else {
            mutator.clear_attribute(clone_id, QualName::new(None, ns!(), local_name!("id")));
            mutator.set_attribute(
                clone_id,
                QualName::new(None, ns!(), LocalName::from(USE_CLONE_ATTR)),
                &source,
            );
            clone_id
        };
        mutator.append_children(use_id, &[root_id]);
    }
}

/// The `href` of a `<use>` element, preferring the SVG 2 attribute over `xlink:href`.
fn use_href(attrs: &[Attribute]) -> Option<&str> {
    let href = |namespace| {
        attrs
            .iter()
            .find(|attr| attr.name.ns == namespace && attr.name.local == local_name!("href"))
            .map(|attr| attr.value.as_str())
    };
    href(ns!()).or_else(|| href(ns!(xlink)))
}
