use selectors::SelectorList;
use smallvec::SmallVec;
use style::dom_apis::{MayUseInvalidation, QueryAll, QueryFirst, query_selector};
use style::selector_parser::{SelectorImpl, SelectorParser};
use style_traits::ParseError;

use crate::{BaseDocument, Node};

impl BaseDocument {
    /// Find the node with the specified id attribute (if one exists)
    pub fn get_element_by_id(&self, id: &str) -> Option<usize> {
        self.nodes_to_id.get(id).copied()
    }

    /// Find the first node that matches the selector specified as a string
    /// Returns:
    ///   - Err(_) if parsing the selector fails
    ///   - Ok(None) if nothing matches
    ///   - Ok(Some(node_id)) with the first node ID that matches if one is found
    pub fn query_selector<'input>(
        &self,
        selector: &'input str,
    ) -> Result<Option<usize>, ParseError<'input>> {
        let selector_list = self.try_parse_selector_list(selector)?;
        Ok(self.query_selector_raw(&selector_list))
    }

    /// Find the first node that matches the selector(s) specified in selector_list
    pub fn query_selector_raw(&self, selector_list: &SelectorList<SelectorImpl>) -> Option<usize> {
        let root_node = self.root_node();
        let mut result = None;
        query_selector::<&Node, QueryFirst>(
            root_node,
            selector_list,
            &mut result,
            MayUseInvalidation::Yes,
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
    ) -> Result<SmallVec<[usize; 32]>, ParseError<'input>> {
        let selector_list = self.try_parse_selector_list(selector)?;
        Ok(self.query_selector_all_raw(&selector_list))
    }

    /// Find all nodes that match the selector(s) specified in selector_list
    pub fn query_selector_all_raw(
        &self,
        selector_list: &SelectorList<SelectorImpl>,
    ) -> SmallVec<[usize; 32]> {
        let root_node = self.root_node();
        let mut results = SmallVec::new();
        query_selector::<&Node, QueryAll>(
            root_node,
            selector_list,
            &mut results,
            MayUseInvalidation::Yes,
        );

        results.iter().map(|node| node.id).collect()
    }

    pub fn try_parse_selector_list<'input>(
        &self,
        input: &'input str,
    ) -> Result<SelectorList<SelectorImpl>, ParseError<'input>> {
        let url_extra_data = self.url.url_extra_data();
        SelectorParser::parse_author_origin_no_namespace(input, &url_extra_data)
    }
}
