mod node;
mod tree_builder;

use crate::{BaseDocument, Node, accessibility::tree_builder::TreeBuilder};
use accesskit::TreeUpdate;
use blitz_traits::NodeId;
use markup5ever::local_name;

impl BaseDocument {
    /// Builds the AccessKit tree of this document.
    ///
    /// The root is a window node with the ID `u64::MAX`, whose only child is the document node.
    /// The other node IDs are the raw IDs of the DOM nodes. Nodes hidden with CSS, by
    /// `display: none` or `visibility: hidden`, are left out along with their descendants. Nodes
    /// with `aria-hidden="true"` are included and flagged as hidden.
    ///
    /// The label and description of a node are its accessible name and description as computed by
    /// the W3C specification, except that `title` only provides the description and never the
    /// name.
    ///
    /// <https://www.w3.org/TR/accname-1.2/>
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        TreeBuilder::new(self).build()
    }

    /// The elements whose IDs appear in `nodes_reference`, a space-separated list, in that order.
    /// IDs which match no element are skipped.
    fn referenced_dom_ids_to_node_ids<'a>(
        &'a self,
        nodes_reference: &'a str,
    ) -> impl Iterator<Item = NodeId> + 'a {
        nodes_reference
            .split_ascii_whitespace()
            .filter_map(|dom_id| self.get_element_by_id(dom_id))
    }

    /// The element labelled by the given `<label>` element, if any. This is the element which the
    /// `for` attribute refers to or, without that attribute, the first labelable descendant.
    ///
    /// <https://html.spec.whatwg.org/multipage/forms.html#dom-label-control>
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

/// Text which is collected piece by piece, with each run of ASCII whitespace collapsed to a single
/// space and no space at either end.
#[derive(Default)]
struct FlatText {
    text: String,
    pending_space: bool,
}

impl FlatText {
    /// Appends `text`. A space separates it from the earlier text if there is whitespace at the
    /// boundary or [FlatText::break_word] was called since.
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

    /// Separates the next text pushed from the previous text by a space, even without whitespace
    /// at the boundary.
    fn break_word(&mut self) {
        self.pending_space = true;
    }
}

/// `text` with leading and trailing whitespace removed and each run of ASCII whitespace
/// collapsed to a single space, or `None` if nothing is left.
fn normalize(text: &str) -> Option<String> {
    let mut normalizer = FlatText::default();
    normalizer.push(text);
    (!normalizer.text.is_empty()).then_some(normalizer.text)
}

/// The AccessKit ID of the DOM node with the given ID.
fn to_accesskit_node_id(node_id: NodeId) -> accesskit::NodeId {
    accesskit::NodeId(node_id.as_u64())
}

/// The AccessKit IDs of the DOM nodes with the given IDs, in the same order.
fn to_accesskit_node_id_vec<'a>(
    node_ids: impl IntoIterator<Item = &'a NodeId>,
) -> Vec<accesskit::NodeId> {
    node_ids
        .into_iter()
        .map(|n| to_accesskit_node_id(*n))
        .collect::<Vec<_>>()
}
