use style::{dom::TNode as _, values::specified::box_::DisplayInside};

use crate::{BaseDocument, Node};

#[derive(Clone)]
/// An pre-order tree traverser for a [BaseDocument](crate::document::BaseDocument).
pub struct TreeTraverser<'a> {
    doc: &'a BaseDocument,
    stack: Vec<usize>,
}

impl<'a> TreeTraverser<'a> {
    /// Creates a new tree traverser for the given document which starts at the root node.
    pub fn new(doc: &'a BaseDocument) -> Self {
        Self::new_with_root(doc, 0)
    }

    /// Creates a new tree traverser for the given document which starts at the specified node.
    pub fn new_with_root(doc: &'a BaseDocument, root: usize) -> Self {
        let mut stack = Vec::with_capacity(32);
        stack.push(root);
        TreeTraverser { doc, stack }
    }
}
impl Iterator for TreeTraverser<'_> {
    type Item = usize;

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
    current: usize,
}
impl<'a> AncestorTraverser<'a> {
    /// Creates a new ancestor traverser for the given document and node ID.
    pub fn new(doc: &'a BaseDocument, node_id: usize) -> Self {
        AncestorTraverser {
            doc,
            current: node_id,
        }
    }
}
impl Iterator for AncestorTraverser<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let current_node = self.doc.get_node(self.current)?;
        self.current = current_node.parent?;
        Some(self.current)
    }
}

impl Node {
    #[allow(dead_code)]
    pub(crate) fn should_traverse_layout_children(&mut self) -> bool {
        let prefer_layout_children = match self.display_constructed_as.inside() {
            DisplayInside::None => return false,
            DisplayInside::Contents => false,
            DisplayInside::Flow | DisplayInside::FlowRoot | DisplayInside::TableCell => {
                // Prefer layout children for "block" but not "inline" contexts
                self.element_data()
                    .is_none_or(|el| el.inline_layout_data.is_none())
            }
            DisplayInside::Flex | DisplayInside::Grid => true,
            DisplayInside::Table => false,
            DisplayInside::TableRowGroup => false,
            DisplayInside::TableColumn => false,
            DisplayInside::TableColumnGroup => false,
            DisplayInside::TableHeaderGroup => false,
            DisplayInside::TableFooterGroup => false,
            DisplayInside::TableRow => false,
        };
        let has_layout_children = self.layout_children.get_mut().is_some();
        prefer_layout_children & has_layout_children
    }
}

impl BaseDocument {
    /// Collect the nodes into a chain by traversing upwards
    pub fn node_chain(&self, node_id: usize) -> Vec<usize> {
        let mut chain = Vec::with_capacity(16);
        chain.push(node_id);
        chain.extend(
            AncestorTraverser::new(self, node_id).filter(|id| self.nodes[*id].is_element()),
        );
        chain
    }

    pub fn visit<F>(&self, mut visit: F)
    where
        F: FnMut(usize, &Node),
    {
        TreeTraverser::new(self).for_each(|node_id| visit(node_id, &self.nodes[node_id]));
    }

    /// If the node is non-anonymous then returns the node's id
    /// Else find's the first non-anonymous ancester of the node
    pub fn non_anon_ancestor_if_anon(&self, mut node_id: usize) -> usize {
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
        node_id: usize,
        mut cb: impl FnMut(usize, &mut BaseDocument),
    ) {
        let children = std::mem::take(&mut self.nodes[node_id].children);
        for child_id in children.iter().cloned() {
            cb(child_id, self);
        }
        self.nodes[node_id].children = children;
    }

    pub fn iter_subtree_mut(
        &mut self,
        node_id: usize,
        mut cb: impl FnMut(usize, &mut BaseDocument),
    ) {
        cb(node_id, self);
        iter_subtree_mut_inner(self, node_id, &mut cb);
        fn iter_subtree_mut_inner(
            doc: &mut BaseDocument,
            node_id: usize,
            cb: &mut impl FnMut(usize, &mut BaseDocument),
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
        node_id: usize,
        mut cb: impl FnMut(usize, &mut BaseDocument),
    ) {
        let before = self.nodes[node_id].before.take();
        if let Some(before_node_id) = before {
            cb(before_node_id, self)
        }
        self.nodes[node_id].before = before;

        self.iter_children_mut(node_id, &mut cb);

        let after = self.nodes[node_id].after.take();
        if let Some(after_node_id) = after {
            cb(after_node_id, self)
        }
        self.nodes[node_id].after = after;
    }

    pub fn next_node(&self, start: &Node, mut filter: impl FnMut(&Node) -> bool) -> Option<usize> {
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

    pub fn node_layout_ancestors(&self, node_id: usize) -> Vec<usize> {
        let mut ancestors = Vec::with_capacity(12);
        let mut maybe_id = Some(node_id);
        while let Some(id) = maybe_id {
            ancestors.push(id);
            maybe_id = self.nodes[id].layout_parent.get();
        }
        ancestors.reverse();
        ancestors
    }

    pub fn maybe_node_layout_ancestors(&self, node_id: Option<usize>) -> Vec<usize> {
        node_id
            .map(|id| self.node_layout_ancestors(id))
            .unwrap_or_default()
    }

    /// Compare the document order of two nodes.
    /// Returns Ordering::Less if node_a comes before node_b in document order.
    /// Returns Ordering::Greater if node_a comes after node_b.
    /// Returns Ordering::Equal if they are the same node.
    pub fn compare_document_order(&self, node_a: usize, node_b: usize) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        if node_a == node_b {
            return Ordering::Equal;
        }

        // Build ancestor chains from root to node (inclusive)
        let chain_a = self.ancestor_chain_from_root(node_a);
        let chain_b = self.ancestor_chain_from_root(node_b);

        // Find where the chains diverge
        let mut common_depth = 0;
        for (a, b) in chain_a.iter().zip(chain_b.iter()) {
            if a != b {
                break;
            }
            common_depth += 1;
        }

        // If one is an ancestor of the other
        if common_depth == chain_a.len() {
            return Ordering::Less; // node_a is ancestor of node_b
        }
        if common_depth == chain_b.len() {
            return Ordering::Greater; // node_b is ancestor of node_a
        }

        // Safety: common_depth must be > 0 here because both chains start from the same
        // root node (node 0), so they share at least that node. If common_depth were 0,
        // chain_a[0] != chain_b[0], but both start from root, so this is impossible.
        debug_assert!(common_depth > 0, "nodes must share a common ancestor (the root)");

        // Compare position among siblings at the divergence point
        let divergent_a = chain_a[common_depth];
        let divergent_b = chain_b[common_depth];
        let parent_id = chain_a[common_depth - 1];
        let parent = &self.nodes[parent_id];

        for &child_id in &parent.children {
            if child_id == divergent_a {
                return Ordering::Less;
            }
            if child_id == divergent_b {
                return Ordering::Greater;
            }
        }

        // Should not reach here if tree is well-formed
        Ordering::Equal
    }

    /// Build ancestor chain from root to node (inclusive), ordered [root, ..., node].
    fn ancestor_chain_from_root(&self, node_id: usize) -> Vec<usize> {
        let mut ancestors = Vec::with_capacity(16);
        let mut current = Some(node_id);
        while let Some(id) = current {
            ancestors.push(id);
            current = self.nodes[id].parent;
        }
        ancestors.reverse();
        ancestors
    }

    /// Collect all inline root nodes between start_node and end_node in document order.
    /// Both start and end are assumed to be inline roots.
    /// Returns the nodes in document order (from first to last).
    pub fn collect_inline_roots_in_range(&self, start_node: usize, end_node: usize) -> Vec<usize> {
        use std::cmp::Ordering;

        // Ensure start comes before end in document order
        let (first, last) = match self.compare_document_order(start_node, end_node) {
            Ordering::Less | Ordering::Equal => (start_node, end_node),
            Ordering::Greater => (end_node, start_node),
        };

        let mut result = Vec::new();
        let mut found_first = false;

        // Traverse tree in document order
        for node_id in TreeTraverser::new(self) {
            if node_id == first {
                found_first = true;
            }

            if found_first {
                let node = &self.nodes[node_id];
                if node.flags.is_inline_root() {
                    result.push(node_id);
                }
            }

            if node_id == last {
                break;
            }
        }

        result
    }
}
