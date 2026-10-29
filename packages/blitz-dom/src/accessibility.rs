mod node;
mod tree_builder;

use crate::{BaseDocument, Node, accessibility::tree_builder::TreeBuilder};
use accesskit::TreeUpdate;
use blitz_traits::NodeId;
use markup5ever::local_name;

impl BaseDocument {
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        TreeBuilder::new(self).build()
    }

    fn referenced_dom_ids_to_node_ids<'a>(
        &'a self,
        nodes_reference: &'a str,
    ) -> impl Iterator<Item = NodeId> + 'a {
        nodes_reference
            .split_ascii_whitespace()
            .filter_map(|dom_id| self.get_element_by_id(dom_id))
    }

    /// The element labelled by the given `<label>` element, if any.
    ///
    /// https://html.spec.whatwg.org/multipage/forms.html#dom-label-control
    fn label_control(&self, label: &Node) -> Option<NodeId> {
        match label.attr(local_name!("for")) {
            // The spec restricts which elements can be labelled. If the first element with the id
            // is not labelable, no later element with the same id is used and the descendants of
            // the label are not considered either.
            Some(for_id) => self
                .get_element_by_id(for_id)
                .filter(|&target| self.nodes[target].is_labelable()),
            None => label.first_labelable_descendant(),
        }
    }
}

#[derive(Default)]
struct FlatText {
    text: String,
    pending_space: bool,
}

impl FlatText {
    fn push(&mut self, text: &str) {
        for (i, word) in text.split(|c: char| c.is_ascii_whitespace()).enumerate() {
            self.pending_space |= i > 0;
            if word.is_empty() {
                continue;
            }
            if self.pending_space && !self.text.is_empty() {
                self.text.push(' ');
            }
            self.pending_space = false;
            self.text.push_str(word);
        }
    }

    fn break_word(&mut self) {
        self.pending_space = true;
    }
}

fn normalize(text: &str) -> Option<String> {
    let mut normalizer = FlatText::default();
    normalizer.push(text);
    (!normalizer.text.is_empty()).then_some(normalizer.text)
}

fn to_accesskit_node_id(node_id: NodeId) -> accesskit::NodeId {
    accesskit::NodeId(node_id.as_u64())
}

fn to_accesskit_node_id_vec<'a>(
    node_ids: impl IntoIterator<Item = &'a NodeId>,
) -> Vec<accesskit::NodeId> {
    node_ids
        .into_iter()
        .map(|n| to_accesskit_node_id(*n))
        .collect::<Vec<_>>()
}
