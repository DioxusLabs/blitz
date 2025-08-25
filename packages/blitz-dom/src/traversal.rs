use std::mem;

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
    pub(crate) fn should_traverse_layout_children(&mut self) -> bool {
        let prefer_layout_children = match self.display_constructed_as.inside() {
            DisplayInside::None => return false,
            DisplayInside::Contents => false,
            DisplayInside::Flow | DisplayInside::FlowRoot | DisplayInside::TableCell => {
                // Prefer layout children for "block" but not "inline" contexts
                !self
                    .element_data()
                    .is_some_and(|el| el.inline_layout_data.is_some())
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

    pub fn iter_layout_subtree_mut(
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
            let children = doc.nodes[node_id].layout_children.get_mut().take();
            if let Some(children) = children {
                for child_id in children.iter().cloned() {
                    cb(child_id, doc);
                    iter_subtree_mut_inner(doc, child_id, cb);
                }
                *doc.nodes[node_id].layout_children.get_mut() = Some(children);
            }
        }
    }

    pub fn iter_subtree_incl_anon_mut(
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
            let children = mem::take(&mut doc.nodes[node_id].children);
            let layout_children = doc.nodes[node_id].layout_children.get_mut().take();

            let use_layout_children = doc.nodes[node_id].should_traverse_layout_children();
            let child_ids = if use_layout_children {
                layout_children.as_ref().unwrap()
            } else {
                &children
            };

            for child_id in child_ids.iter().cloned() {
                cb(child_id, doc);
                iter_subtree_mut_inner(doc, child_id, cb);
            }

            doc.nodes[node_id].children = children;
            *doc.nodes[node_id].layout_children.get_mut() = layout_children;
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
}
