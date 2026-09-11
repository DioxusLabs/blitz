//! `id_map` construction and reference-chain resolution shared by `<use>`, gradient `href`
//! inheritance, and paint-server/clip/mask/filter `url(#id)` lookups.

use std::collections::HashMap;

use blitz_traits::node_id::NodeId;
use style::Atom;

use crate::BaseDocument;

use super::attrs::raw_attr;

/// Maximum reference-chain depth for `<use>`, gradient `href`, and clip/mask/filter nesting.
/// Applied uniformly as a single backstop against hand-authored (or hostile) cyclic SVG
/// rather than one constant per feature.
pub const MAX_REF_DEPTH: u32 = 16;

/// Total number of instanced nodes a `<use>` expansion may produce across the whole fragment
/// before construction gives up and renders the prefix.
pub const MAX_INSTANCED_NODES: usize = 100_000;

/// Build the `id` -> `NodeId` map for every element in the fragment, by walking full
/// DOM children, before the render walk.
pub fn build_id_map(doc: &BaseDocument, root: NodeId) -> HashMap<Atom, NodeId> {
    let mut map = HashMap::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let node = &doc.nodes[id];
        if let Some(elem) = node.data.downcast_element() {
            if let Some(id_atom) = elem.id.as_ref() {
                map.entry(id_atom.clone()).or_insert(id);
            }
        }
        stack.extend(node.children.iter().copied());
    }
    map
}

/// Resolve an SVG `href`/`xlink:href` attribute (read off `attrs`) to a target `NodeId` within this
/// fragment. Prefers the namespace-less `href` SVG2), falling back to `xlink:href`.
/// Only local fragment references (`#id`) are resolved, external-document references
/// (`other.svg#id`) are resolve to `None`.
pub fn resolve_href(
    id_map: &HashMap<Atom, NodeId>,
    attrs: &[crate::node::Attribute],
) -> Option<NodeId> {
    let href = raw_attr(attrs, "href").or_else(|| raw_attr(attrs, "xlink:href"))?;
    let id = href.trim().strip_prefix('#')?;
    if id.is_empty() {
        return None;
    }
    id_map.get(&Atom::from(id)).copied()
}

/// Walk a same-kind reference chain starting at `start`, following `next` until it returns `None`,a
/// node repeats, or [`MAX_REF_DEPTH`] links have been followed. Returns the chain in traversal order.
pub fn resolve_ref_chain(
    start: NodeId,
    mut next: impl FnMut(NodeId) -> Option<NodeId>,
) -> Vec<NodeId> {
    let mut chain = vec![start];
    let mut cur = start;
    for _ in 0..MAX_REF_DEPTH {
        let Some(nxt) = next(cur) else { break };
        if chain.contains(&nxt) {
            break;
        }
        chain.push(nxt);
        cur = nxt;
    }
    chain
}

/// Whether `candidate` is `target` or a DOM ancestor of `target`, used by the `<use>` cycle guard.
/// Walks up the DOM `parent` chain, capped at a generous hop count as a backstop against
/// malformed/cyclic parent chains (which should not occur, but must never hang here).
pub fn is_ancestor_or_self(doc: &BaseDocument, candidate: NodeId, target: NodeId) -> bool {
    let mut cur = Some(target);
    for _ in 0..4096 {
        match cur {
            Some(id) if id == candidate => return true,
            Some(id) => cur = doc.nodes[id].parent,
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_chain_stops_at_cycle() {
        // 0 -> 1 -> 2 -> 0 (cycle)
        let next = |n: NodeId| -> Option<NodeId> {
            match n.as_u64() {
                0 => Some(NodeId::from_u64(1)),
                1 => Some(NodeId::from_u64(2)),
                2 => Some(NodeId::from_u64(0)),
                _ => None,
            }
        };
        let chain = resolve_ref_chain(NodeId::from_u64(0), next);
        assert_eq!(chain.len(), 3);
    }

    #[test]
    fn ref_chain_stops_at_max_depth() {
        let next = |n: NodeId| -> Option<NodeId> { Some(NodeId::from_u64(n.as_u64() + 1)) };
        let chain = resolve_ref_chain(NodeId::from_u64(0), next);
        assert_eq!(chain.len() as u32, MAX_REF_DEPTH + 1);
    }

    #[test]
    fn ref_chain_stops_when_exhausted() {
        let next = |n: NodeId| -> Option<NodeId> {
            if n.as_u64() == 0 {
                Some(NodeId::from_u64(1))
            } else {
                None
            }
        };
        let chain = resolve_ref_chain(NodeId::from_u64(0), next);
        assert_eq!(chain.len(), 2);
    }
}
