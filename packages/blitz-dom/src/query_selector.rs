use blitz_traits::node_id::NodeId;
use selectors::SelectorList;
use smallvec::SmallVec;
use style::dom::{TDocument, TNode};
use style::dom_apis::{
    MayUseInvalidation, QueryAll, QueryFirst, element_closest, element_matches, query_selector,
};
use style::selector_parser::{SelectorImpl, SelectorParser};
use style_traits::ParseError;

use crate::{BaseDocument, Node};

impl BaseDocument {
    /// Find the node with the specified id attribute (if one exists).
    /// If multiple nodes have the same id, the first in tree order is returned.
    pub fn get_element_by_id(&self, id: &str) -> Option<NodeId> {
        match self.nodes_to_id.get(id)?.as_slice() {
            [] => None,
            [node_id] => Some(*node_id),
            candidates => self.first_in_tree_order(candidates),
        }
    }

    /// Find the first of `candidates` in tree order
    fn first_in_tree_order(&self, candidates: &[NodeId]) -> Option<NodeId> {
        let mut stack = vec![self.root_node_id];
        while let Some(node_id) = stack.pop() {
            if candidates.contains(&node_id) {
                return Some(node_id);
            }
            stack.extend(self.nodes[node_id].children.iter().rev().copied());
        }
        None
    }

    /// Add a node to the id-to-node map
    pub(crate) fn add_to_id_map(&mut self, id: &str, node_id: NodeId) {
        if id.is_empty() {
            return;
        }
        let node_ids = self.nodes_to_id.entry(id.to_string()).or_default();
        if !node_ids.contains(&node_id) {
            node_ids.push(node_id);
        }
    }

    /// Remove a node from the id-to-node map
    pub(crate) fn remove_from_id_map(&mut self, id: &str, node_id: NodeId) {
        if let Some(node_ids) = self.nodes_to_id.get_mut(id) {
            node_ids.retain(|nid| *nid != node_id);
            if node_ids.is_empty() {
                self.nodes_to_id.remove(id);
            }
        }
    }

    /// Find the first node that matches the selector specified as a string
    /// Returns:
    ///   - Err(_) if parsing the selector fails
    ///   - Ok(None) if nothing matches
    ///   - Ok(Some(node_id)) with the first node ID that matches if one is found
    pub fn query_selector<'input>(
        &self,
        selector: &'input str,
    ) -> Result<Option<NodeId>, ParseError<'input>> {
        self.query_selector_in(self.root_node_id, selector)
    }

    /// Find the first descendant of `scope` that matches the selector specified
    /// as a string.
    ///
    /// The scope node itself is never matched. Selector parts may match nodes
    /// outside the scope while evaluating relationships between descendants.
    ///
    /// Returns:
    ///   - `Err(_)` if parsing the selector fails
    ///   - `Ok(None)` if nothing matches
    ///   - `Ok(Some(node_id))` with the first matching descendant ID otherwise
    pub fn query_selector_in<'input>(
        &self,
        scope: NodeId,
        selector: &'input str,
    ) -> Result<Option<NodeId>, ParseError<'input>> {
        let selector_list = self.try_parse_selector_list(selector)?;
        Ok(self.query_selector_in_raw(scope, &selector_list))
    }

    /// Find the first descendant of the document root that matches the
    /// selector(s) specified in `selector_list`.
    ///
    /// The document root itself is never matched. Selector parts may match
    /// nodes outside the scope while evaluating relationships between
    /// descendants.
    pub fn query_selector_raw(&self, selector_list: &SelectorList<SelectorImpl>) -> Option<NodeId> {
        self.query_selector_in_raw(self.root_node_id, selector_list)
    }

    /// Find the first descendant of `scope` that matches the selector(s)
    /// specified in `selector_list`.
    ///
    /// The scope node itself is never matched. Selector parts may match nodes
    /// outside the scope while evaluating relationships between descendants.
    pub fn query_selector_in_raw(
        &self,
        scope: NodeId,
        selector_list: &SelectorList<SelectorImpl>,
    ) -> Option<NodeId> {
        let root_node = &self.nodes[scope];
        let mut result = None;
        query_selector::<&Node, QueryFirst>(
            root_node,
            selector_list,
            &mut result,
            self.may_use_invalidation_for(scope),
        );

        result.map(|node| node.id)
    }

    /// Find all nodes that match the selector specified as a string
    /// Returns:
    ///   - `Err(_)` if parsing the selector fails
    ///   - `Ok(SmallVec<usize>)` with all matching nodes otherwise
    pub fn query_selector_all<'input>(
        &self,
        selector: &'input str,
    ) -> Result<SmallVec<[NodeId; 32]>, ParseError<'input>> {
        self.query_selector_all_in(self.root_node_id, selector)
    }

    /// Find all descendants of `scope` that match the selector specified as a
    /// string, in tree order.
    ///
    /// The scope node itself is never matched. Selector parts may match nodes
    /// outside the scope while evaluating relationships between descendants.
    ///
    /// Returns:
    ///   - `Err(_)` if parsing the selector fails
    ///   - `Ok(_)` with all matching descendant IDs otherwise
    pub fn query_selector_all_in<'input>(
        &self,
        scope: NodeId,
        selector: &'input str,
    ) -> Result<SmallVec<[NodeId; 32]>, ParseError<'input>> {
        let selector_list = self.try_parse_selector_list(selector)?;
        Ok(self.query_selector_all_in_raw(scope, &selector_list))
    }

    /// Find all descendants of the document root that match the selector(s)
    /// specified in `selector_list`, in tree order.
    ///
    /// The document root itself is never matched. Selector parts may match
    /// nodes outside the scope while evaluating relationships between
    /// descendants.
    pub fn query_selector_all_raw(
        &self,
        selector_list: &SelectorList<SelectorImpl>,
    ) -> SmallVec<[NodeId; 32]> {
        self.query_selector_all_in_raw(self.root_node_id, selector_list)
    }

    /// Find all descendants of `scope` that match the selector(s) specified in
    /// `selector_list`, in tree order.
    ///
    /// The scope node itself is never matched. Selector parts may match nodes
    /// outside the scope while evaluating relationships between descendants.
    pub fn query_selector_all_in_raw(
        &self,
        scope: NodeId,
        selector_list: &SelectorList<SelectorImpl>,
    ) -> SmallVec<[NodeId; 32]> {
        let root_node = &self.nodes[scope];
        let mut results = SmallVec::new();
        query_selector::<&Node, QueryAll>(
            root_node,
            selector_list,
            &mut results,
            self.may_use_invalidation_for(scope),
        );

        results.iter().map(|node| node.id).collect()
    }

    fn may_use_invalidation_for(&self, scope: NodeId) -> MayUseInvalidation {
        if scope == self.root_node_id {
            MayUseInvalidation::Yes
        } else {
            MayUseInvalidation::No
        }
    }

    /// Test whether the node identified by `node_id` matches the selector
    /// specified as a string.
    ///
    /// Non-element nodes never match.
    pub fn matches_selector<'input>(
        &self,
        node_id: NodeId,
        selector: &'input str,
    ) -> Result<bool, ParseError<'input>> {
        let selector_list = self.try_parse_selector_list(selector)?;
        Ok(self.nodes[node_id].matches_selector_raw(&selector_list))
    }

    /// Find the closest matching element at or above the node identified by
    /// `node_id`.
    ///
    /// Non-element nodes never match and return `None`.
    pub fn closest<'input>(
        &self,
        node_id: NodeId,
        selector: &'input str,
    ) -> Result<Option<NodeId>, ParseError<'input>> {
        let selector_list = self.try_parse_selector_list(selector)?;
        Ok(self.nodes[node_id].closest_raw(&selector_list))
    }

    pub fn try_parse_selector_list<'input>(
        &self,
        input: &'input str,
    ) -> Result<SelectorList<SelectorImpl>, ParseError<'input>> {
        let url_extra_data = self.url.url_extra_data();
        SelectorParser::parse_author_origin_no_namespace(input, &url_extra_data)
    }
}

impl Node {
    /// Find the first descendant of this node that matches the selector(s)
    /// specified in `selector_list`.
    ///
    /// The scope node itself is never matched. Selector parts may match nodes
    /// outside the scope while evaluating relationships between descendants.
    ///
    /// Text and comment scope nodes return no matches.
    pub fn query_selector_raw(&self, selector_list: &SelectorList<SelectorImpl>) -> Option<NodeId> {
        let mut result = None;
        query_selector::<&Node, QueryFirst>(
            self,
            selector_list,
            &mut result,
            MayUseInvalidation::No,
        );
        result.map(|node| node.id)
    }

    /// Find all descendants of this node that match the selector(s) specified
    /// in `selector_list`, in tree order.
    ///
    /// The scope node itself is never matched. Selector parts may match nodes
    /// outside the scope while evaluating relationships between descendants.
    ///
    /// Text and comment scope nodes return no matches.
    pub fn query_selector_all_raw(
        &self,
        selector_list: &SelectorList<SelectorImpl>,
    ) -> SmallVec<[NodeId; 32]> {
        let mut results = SmallVec::new();
        query_selector::<&Node, QueryAll>(
            self,
            selector_list,
            &mut results,
            MayUseInvalidation::No,
        );
        results.iter().map(|node| node.id).collect()
    }

    /// Test whether this element matches the selector(s) specified in
    /// `selector_list`.
    ///
    /// Non-element nodes never match.
    pub fn matches_selector_raw(&self, selector_list: &SelectorList<SelectorImpl>) -> bool {
        if !self.is_element() {
            return false;
        }

        element_matches(&self, selector_list, self.owner_doc().quirks_mode())
    }

    /// Find the closest matching element at or above this element.
    ///
    /// Non-element nodes never match and return `None`.
    pub fn closest_raw(&self, selector_list: &SelectorList<SelectorImpl>) -> Option<NodeId> {
        if !self.is_element() {
            return None;
        }

        element_closest(self, selector_list, self.owner_doc().quirks_mode()).map(|node| node.id)
    }
}
