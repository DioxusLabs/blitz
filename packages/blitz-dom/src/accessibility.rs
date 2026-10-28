mod node;
mod tree_builder;

use crate::{BaseDocument, accessibility::tree_builder::TreeBuilder};
use accesskit::TreeUpdate;
use blitz_traits::NodeId;

impl BaseDocument {
    pub fn build_accessibility_tree(&self) -> TreeUpdate {
        let mut builder = TreeBuilder::new(self);
        builder.add_nodes();
        builder.resolve_names();
        builder.resolve_descriptions();
        builder.build()
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
