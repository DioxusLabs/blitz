use blitz_traits::node_id::NodeId;
use std::cmp::Ordering;

use style::dom::TNode as _;

use crate::{BaseDocument, Node};

impl Node {
    pub(crate) fn compare_document_order(&self, other: &Node) -> Ordering {
        if self.id == other.id {
            return Ordering::Equal;
        }
        let chain = |mut id| {
            let mut ids = vec![id];
            while let Some(parent) = self.with(id).parent {
                id = parent;
                ids.push(parent);
            }
            ids.reverse();
            ids
        };
        let a = chain(self.id);
        let b = chain(other.id);
        let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
        if common == a.len() {
            return Ordering::Less;
        }
        if common == b.len() {
            return Ordering::Greater;
        }
        let parent = self.with(a[common - 1]);
        let before = parent.before();
        let after = parent.after();
        let position = |id| {
            before
                .iter()
                .chain(&parent.children)
                .chain(after.iter())
                .position(|child| *child == id)
        };
        position(a[common]).cmp(&position(b[common]))
    }
}

macro_rules! iter_children {
    ($node_expr:expr, $cb:expr) => {{
        let node = &mut $node_expr;
        let children = core::mem::take(&mut node.children);
        for child_id in children.iter().copied() {
            $cb(child_id)
        }
        $node_expr.children = children;
    }};
}
pub(crate) use iter_children;

macro_rules! iter_children_and_pseudos {
    ($node_expr:expr, $cb:expr) => {{
        // Load node
        let node = &mut $node_expr;

        // Copy before, after, and take children
        let before = node.before();
        let after = node.after();
        let children = core::mem::take(&mut node.children);

        if let Some(before) = before {
            $cb(before)
        }
        for child_id in children.iter().copied() {
            $cb(child_id)
        }
        if let Some(after) = after {
            $cb(after)
        }

        // Reload node and put children back
        $node_expr.children = children;
    }};
}
pub(crate) use iter_children_and_pseudos;

#[derive(Clone)]
/// An pre-order tree traverser for a [BaseDocument](crate::document::BaseDocument).
pub struct TreeTraverser<'a> {
    doc: &'a BaseDocument,
    stack: Vec<NodeId>,
}

impl<'a> TreeTraverser<'a> {
    /// Creates a new tree traverser for the given document which starts at the root node.
    pub fn new(doc: &'a BaseDocument) -> Self {
        Self::new_with_root(doc, doc.root_node().id)
    }

    /// Creates a new tree traverser for the given document which starts at the specified node.
    pub fn new_with_root(doc: &'a BaseDocument, root: NodeId) -> Self {
        let mut stack = Vec::with_capacity(32);
        stack.push(root);
        TreeTraverser { doc, stack }
    }
}
impl Iterator for TreeTraverser<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let node = self.doc.get_node(id)?;
        self.stack.extend(node.children.iter().rev());
        Some(id)
    }
}

#[derive(Clone)]
/// An ancestor traverser for a [BaseDocument](crate::document::BaseDocument).
pub struct AncestorTraverser<'a> {
    doc: &'a BaseDocument,
    current: NodeId,
}
impl<'a> AncestorTraverser<'a> {
    /// Creates a new ancestor traverser for the given document and node ID.
    pub fn new(doc: &'a BaseDocument, node_id: NodeId) -> Self {
        AncestorTraverser {
            doc,
            current: node_id,
        }
    }
}
impl Iterator for AncestorTraverser<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let current_node = self.doc.get_node(self.current)?;
        self.current = current_node.parent?;
        Some(self.current)
    }
}

impl BaseDocument {
    /// Collect the nodes into a chain by traversing upwards
    pub fn node_chain(&self, node_id: NodeId) -> Vec<NodeId> {
        let mut chain = Vec::with_capacity(16);
        chain.push(node_id);
        chain.extend(
            AncestorTraverser::new(self, node_id).filter(|id| self.nodes[*id].is_element()),
        );
        chain
    }

    pub fn visit<F>(&self, mut visit: F)
    where
        F: FnMut(NodeId, &Node),
    {
        TreeTraverser::new(self).for_each(|node_id| visit(node_id, &self.nodes[node_id]));
    }

    /// If the node is non-anonymous then returns the node's id
    /// Else find's the first non-anonymous ancester of the node
    pub fn non_anon_ancestor_if_anon(&self, mut node_id: NodeId) -> NodeId {
        loop {
            let node = &self.nodes[node_id];

            if !node.is_anonymous() {
                return node.id;
            }

            let Some(parent_id) = node.layout_parent.get() else {
                // Shouldn't be reachable unless invalid node_id is passed
                // as root node is always non-anonymous
                panic!("Node does not exist or does not have a non-anonymous parent");
            };

            node_id = parent_id;
        }
    }

    pub fn iter_children_mut(
        &mut self,
        node_id: NodeId,
        mut cb: impl FnMut(NodeId, &mut BaseDocument),
    ) {
        let children = std::mem::take(&mut self.nodes[node_id].children);
        for child_id in children.iter().cloned() {
            cb(child_id, self);
        }
        self.nodes[node_id].children = children;
    }

    pub fn iter_subtree_mut(
        &mut self,
        node_id: NodeId,
        mut cb: impl FnMut(NodeId, &mut BaseDocument),
    ) {
        cb(node_id, self);
        iter_subtree_mut_inner(self, node_id, &mut cb);
        fn iter_subtree_mut_inner(
            doc: &mut BaseDocument,
            node_id: NodeId,
            cb: &mut impl FnMut(NodeId, &mut BaseDocument),
        ) {
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().cloned() {
                cb(child_id, doc);
                iter_subtree_mut_inner(doc, child_id, cb);
            }
            doc.nodes[node_id].children = children;
        }
    }

    pub fn iter_children_and_pseudos_mut(
        &mut self,
        node_id: NodeId,
        mut cb: impl FnMut(NodeId, &mut BaseDocument),
    ) {
        let before = self.nodes[node_id].before();
        self.nodes[node_id].set_pe_by_index(1, None);
        if let Some(before_node_id) = before {
            cb(before_node_id, self)
        }
        self.nodes[node_id].set_pe_by_index(1, before);

        self.iter_children_mut(node_id, &mut cb);

        let after = self.nodes[node_id].after();
        self.nodes[node_id].set_pe_by_index(0, None);
        if let Some(after_node_id) = after {
            cb(after_node_id, self)
        }
        self.nodes[node_id].set_pe_by_index(0, after);
    }

    pub fn next_node(&self, start: &Node, mut filter: impl FnMut(&Node) -> bool) -> Option<NodeId> {
        let start_id = start.id;
        let mut node = start;
        let mut look_in_children = true;
        loop {
            // Next is first child
            let next = if look_in_children && !node.children.is_empty() {
                let node_id = node.children[0];
                &self.nodes[node_id]
            }
            // Next is next sibling or parent
            else if let Some(parent) = node.parent_node() {
                let self_idx = parent
                    .children
                    .iter()
                    .position(|id| *id == node.id)
                    .unwrap();
                // Next is next sibling
                if let Some(sibling_id) = parent.children.get(self_idx + 1) {
                    look_in_children = true;
                    &self.nodes[*sibling_id]
                }
                // Next is parent
                else {
                    look_in_children = false;
                    node = parent;
                    continue;
                }
            }
            // Continue search from the root
            else {
                look_in_children = true;
                self.root_node()
            };

            if filter(next) {
                return Some(next.id);
            } else if next.id == start_id {
                return None;
            }

            node = next;
        }
    }

    /// The node that comes last within `node`'s subtree in document order,
    /// which is what precedes `node`'s successor in reverse order.
    fn deepest_last_descendant<'a>(&'a self, mut node: &'a Node) -> &'a Node {
        while let Some(last_child_id) = node.children.last() {
            node = &self.nodes[*last_child_id];
        }
        node
    }

    /// Mirror of [`Self::next_node`]: walks the tree in reverse document
    /// order, wrapping around to the end of the document.
    pub fn prev_node(&self, start: &Node, mut filter: impl FnMut(&Node) -> bool) -> Option<NodeId> {
        let start_id = start.id;
        let mut node = start;
        loop {
            let prev = if let Some(parent) = node.parent_node() {
                let self_idx = parent
                    .children
                    .iter()
                    .position(|id| *id == node.id)
                    .unwrap();
                // Previous is the deepest last descendant of the previous
                // sibling, or the parent when there is no previous sibling
                if self_idx > 0 {
                    self.deepest_last_descendant(&self.nodes[parent.children[self_idx - 1]])
                } else {
                    parent
                }
            }
            // Continue the search from the end of the document
            else {
                self.deepest_last_descendant(self.root_node())
            };

            if filter(prev) {
                return Some(prev.id);
            } else if prev.id == start_id {
                return None;
            }

            node = prev;
        }
    }

    /// `node_id` and its ancestor elements in the DOM, root first. These are
    /// the elements that match `:hover`/`:active` when `node_id` does.
    pub fn node_element_ancestors(&self, node_id: NodeId) -> Vec<NodeId> {
        let mut ancestors = Vec::with_capacity(12);
        let mut maybe_id = Some(node_id);
        while let Some(id) = maybe_id {
            let node = &self.nodes[id];
            if !node.is_element() {
                break;
            }
            ancestors.push(id);
            maybe_id = node.parent;
        }
        ancestors.reverse();
        ancestors
    }

    pub fn maybe_node_element_ancestors(&self, node_id: Option<NodeId>) -> Vec<NodeId> {
        node_id
            .map(|id| self.node_element_ancestors(id))
            .unwrap_or_default()
    }

    /// Compare two nodes in DOM tree order.
    pub fn compare_document_order(&self, node_a: NodeId, node_b: NodeId) -> Ordering {
        self.nodes[node_a].compare_document_order(&self.nodes[node_b])
    }
}
