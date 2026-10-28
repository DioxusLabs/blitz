use crate::{
    BaseDocument, Node,
    accessibility::{FlatText, to_accesskit_node_id, to_accesskit_node_id_vec},
};
use accesskit::{Role, TreeId, TreeInfo, TreeUpdate};
use blitz_traits::NodeId;
use markup5ever::local_name;
use std::collections::{HashMap, HashSet};

const DEFAULT_ACCESSKIT_NODE_ID: accesskit::NodeId = accesskit::NodeId(u64::MAX);

pub(super) struct TreeBuilder<'doc> {
    document: &'doc BaseDocument,
    window: accesskit::Node,
    nodes: HashMap<NodeId, accesskit::Node>,
    relations: HashMap<NodeId, Relations>,
    excluded_nodes: HashSet<NodeId>,
}

impl<'doc> TreeBuilder<'doc> {
    pub fn new(document: &'doc BaseDocument) -> Self {
        Self {
            document,
            window: accesskit::Node::new(Role::Window),
            nodes: Default::default(),
            relations: Default::default(),
            excluded_nodes: HashSet::new(),
        }
    }

    pub fn add_nodes(&mut self) {
        self.document.visit(|node_id, node| {
            if node.is_hidden_from_accessibility_tree()
                || node
                    .parent
                    .is_some_and(|parent_id| self.excluded_nodes.contains(&parent_id))
            {
                self.excluded_nodes.insert(node_id);
                return;
            }
            let parent = node
                .parent
                .and_then(|parent_id| self.nodes.get_mut(&parent_id))
                .unwrap_or(&mut self.window);
            let mut accesskit_node = node.build_accesskit_node(parent);
            if accesskit_node.role() == Role::Label {
                self.set_value_of_label(node, &mut accesskit_node);
            }
            self.collect_relations(node_id, node);
            self.nodes.insert(node_id, accesskit_node);
        });
    }

    fn collect_relations(&mut self, node_id: NodeId, node: &Node) {
        let mut relations = self.relations.remove(&node_id).unwrap_or_default();
        if let Some(id_refs) = node.attr(local_name!("aria-labelledby")) {
            let labelled_by =
                NameSource::AriaLabelledBy(self.referenced_dom_ids_to_node_ids(id_refs).collect());
            relations.labelled_by = Some(labelled_by);
        } else if node.is_element_named(local_name!("label"))
                && let Some(target) = self.label_control(node)
                // aria-label on the target takes precedence over <label>.
                && self.document.nodes[target]
                    .attr(local_name!("aria-label"))
                    .is_none_or(|aria_label| aria_label.trim_ascii().is_empty())
        {
            match self.relations.entry(target).or_default().labelled_by {
                // The target has no label yet.
                ref mut labelled_by @ None => {
                    *labelled_by = Some(NameSource::LabelElements(vec![node_id]));
                }

                // The target has one label and this element adds another.
                Some(NameSource::LabelElements(ref mut e)) => {
                    e.push(node_id);
                }

                // The target already uses "aria-labelledby", which takes precedence over the
                // <label> element.
                Some(NameSource::AriaLabelledBy(_)) => {}
            }
        }

        if let Some(aria_describedby) = node.attr(local_name!("aria-describedby")) {
            let described_by = self
                .referenced_dom_ids_to_node_ids(aria_describedby)
                .collect();
            relations.described_by = Some(described_by);
        }
        self.relations.insert(node_id, relations);
    }

    fn referenced_dom_ids_to_node_ids<'a>(
        &'a self,
        nodes_reference: &'a str,
    ) -> impl Iterator<Item = NodeId> + 'a {
        nodes_reference
            .split_ascii_whitespace()
            .filter_map(|dom_id| self.document.get_element_by_id(dom_id))
    }

    fn set_value_of_label(&self, node: &Node, accesskit_node: &mut accesskit::Node) {
        // A control wrapped by a <label> is not part of the text of the label.
        let skip = node
            .is_element_named(local_name!("label"))
            .then(|| self.label_control(node))
            .flatten();
        let mut text = FlatText::default();
        node.write_text_alternative(&mut text, skip);
        if !text.text.is_empty() {
            accesskit_node.set_value(text.text);
        }
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
                .document
                .get_element_by_id(for_id)
                .filter(|&target| self.document.nodes[target].is_labelable()),
            None => label.first_labelable_descendant(),
        }
    }

    pub fn resolve_names(&mut self) {
        for (target_node_id, relation) in &self.relations {
            let Some(target_builder) = self.nodes.get_mut(target_node_id) else {
                continue;
            };
            let mut label = FlatText::default();
            if let Some(ref labelled_by) = relation.labelled_by {
                match labelled_by {
                    NameSource::AriaLabelledBy(node_ids) => {
                        target_builder.set_labelled_by(to_accesskit_node_id_vec(
                            node_ids
                                .iter()
                                .filter(|id| !self.excluded_nodes.contains(id)),
                        ));
                        for label_node_id in node_ids {
                            self.document.nodes[*label_node_id]
                                .write_text_alternative(&mut label, None);
                            label.break_word();
                        }
                    }
                    NameSource::LabelElements(node_ids) => {
                        target_builder.set_labelled_by(to_accesskit_node_id_vec(node_ids));
                        for label_node_id in node_ids {
                            self.document.nodes[*label_node_id].write_text_alternative(
                                &mut label,
                                // A <label> does not contribute the content of the element it labels.
                                Some(*target_node_id),
                            );
                            label.break_word();
                        }
                    }
                }
            }
            if !label.text.is_empty() {
                target_builder.set_label(label.text.as_str());
            } else {
                self.document.nodes[*target_node_id].set_label_from_node(target_builder);
            }
        }
    }

    pub fn resolve_descriptions(&mut self) {
        for (target_node_id, relation) in &self.relations {
            let Some(target_builder) = self.nodes.get_mut(&target_node_id) else {
                continue;
            };
            let mut description = FlatText::default();
            if let Some(ref described_by) = relation.described_by {
                for description_node_id in described_by {
                    let description_node = &self.document.nodes[*description_node_id];
                    if !self.excluded_nodes.contains(description_node_id) {
                        target_builder
                            .push_described_by(to_accesskit_node_id(*description_node_id));
                    }
                    description_node.write_text_alternative(&mut description, None);
                    description.break_word();
                }
            }
            if !description.text.is_empty() {
                target_builder.set_description(description.text.as_str());
            } else {
                self.document.nodes[*target_node_id].set_description_from_node(target_builder);
            }
        }
    }

    pub fn build(self) -> TreeUpdate {
        TreeUpdate {
            tree_id: TreeId::ROOT,
            nodes: self
                .nodes
                .into_iter()
                .map(|(id, node)| (to_accesskit_node_id(id), node))
                .chain([(DEFAULT_ACCESSKIT_NODE_ID, self.window)])
                .collect(),
            tree: Some(TreeInfo::new(DEFAULT_ACCESSKIT_NODE_ID)),
            focus: self
                .document
                .focus_node_id
                .map(to_accesskit_node_id)
                .unwrap_or(DEFAULT_ACCESSKIT_NODE_ID),
        }
    }
}

#[derive(Default, Debug)]
struct Relations {
    labelled_by: Option<NameSource>,
    described_by: Option<Vec<NodeId>>,
}

#[derive(Debug)]
enum NameSource {
    AriaLabelledBy(Vec<NodeId>),
    LabelElements(Vec<NodeId>),
}
