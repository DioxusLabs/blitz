use crate::{
    BaseDocument, Node,
    accessibility::{FlatText, to_accesskit_node_id, to_accesskit_node_id_vec},
};
use accesskit::{Role, TreeId, TreeInfo, TreeUpdate};
use blitz_traits::NodeId;
use markup5ever::local_name;
use rustc_hash::{FxHashMap, FxHashSet};

const DEFAULT_ACCESSKIT_NODE_ID: accesskit::NodeId = accesskit::NodeId(u64::MAX);

pub(super) struct TreeBuilder<'doc> {
    document: &'doc BaseDocument,
    relations: FxHashMap<NodeId, Relations>,
    excluded_nodes: FxHashSet<NodeId>,
}

impl<'doc> TreeBuilder<'doc> {
    pub fn new(document: &'doc BaseDocument) -> Self {
        let mut excluded_nodes = FxHashSet::default();
        let mut relations = FxHashMap::default();
        document.visit(|node_id, node| {
            if node.is_hidden_from_accessibility_tree()
                || node
                    .parent
                    .is_some_and(|parent_id| excluded_nodes.contains(&parent_id))
            {
                excluded_nodes.insert(node_id);
                return;
            }
            Self::collect_relations(&mut relations, node_id, node, document);
        });
        Self {
            document,
            relations,
            excluded_nodes,
        }
    }

    fn collect_relations(
        relations_map: &mut FxHashMap<NodeId, Relations>,
        node_id: NodeId,
        node: &Node,
        document: &BaseDocument,
    ) {
        if let Some(id_refs) = node.attr(local_name!("aria-labelledby")) {
            let labelled_by = NameSource::AriaLabelledBy(
                document.referenced_dom_ids_to_node_ids(id_refs).collect(),
            );
            relations_map.entry(node_id).or_default().labelled_by = Some(labelled_by);
        } else if node.is_element_named(local_name!("label"))
                && let Some(target) = document.label_control(node)
                // aria-label on the target takes precedence over <label>.
                && document.nodes[target]
                    .attr(local_name!("aria-label"))
                    .is_none_or(|aria_label| aria_label.trim_ascii().is_empty())
        {
            match relations_map.entry(target).or_default().labelled_by {
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
            let described_by = document
                .referenced_dom_ids_to_node_ids(aria_describedby)
                .collect();
            relations_map.entry(node_id).or_default().described_by = Some(described_by);
        }
    }

    pub fn build(self) -> TreeUpdate {
        TreeUpdate {
            tree_id: TreeId::ROOT,
            nodes: self.create_nodes(),
            tree: Some(TreeInfo::new(DEFAULT_ACCESSKIT_NODE_ID)),
            focus: self
                .document
                .focus_node_id
                .map(to_accesskit_node_id)
                .unwrap_or(DEFAULT_ACCESSKIT_NODE_ID),
        }
    }

    fn create_nodes(&self) -> Vec<(accesskit::NodeId, accesskit::Node)> {
        let mut window = accesskit::Node::new(Role::Window);
        let mut nodes = FxHashMap::default();
        self.document.visit(|node_id, node| {
            if self.excluded_nodes.contains(&node_id) {
                return;
            }
            let parent = node
                .parent
                .and_then(|parent_id| nodes.get_mut(&parent_id))
                .unwrap_or(&mut window);
            parent.push_child(to_accesskit_node_id(node_id));
            let mut accesskit_node = node.build_accesskit_node();
            if accesskit_node.role() == Role::Label {
                self.set_value_of_label(node, &mut accesskit_node);
            }
            self.resolve_name(node_id, &mut accesskit_node);
            self.resolve_description(node_id, &mut accesskit_node);
            nodes.insert(node_id, accesskit_node);
        });
        nodes
            .into_iter()
            .map(|(id, node)| (to_accesskit_node_id(id), node))
            .chain([(DEFAULT_ACCESSKIT_NODE_ID, window)])
            .collect()
    }

    fn set_value_of_label(&self, node: &Node, accesskit_node: &mut accesskit::Node) {
        // A control wrapped by a <label> is not part of the text of the label.
        let skip = node
            .is_element_named(local_name!("label"))
            .then(|| self.document.label_control(node))
            .flatten();
        let mut text = FlatText::default();
        node.write_text_alternative(&mut text, skip);
        if !text.text.is_empty() {
            accesskit_node.set_value(text.text);
        }
    }

    fn resolve_name(&self, node_id: NodeId, accesskit_node: &mut accesskit::Node) {
        let mut label = FlatText::default();
        if let Some(labelled_by) = self
            .relations
            .get(&node_id)
            .and_then(|r| r.labelled_by.as_ref())
        {
            match labelled_by {
                NameSource::AriaLabelledBy(node_ids) => {
                    accesskit_node.set_labelled_by(to_accesskit_node_id_vec(
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
                    accesskit_node.set_labelled_by(to_accesskit_node_id_vec(node_ids));
                    for label_node_id in node_ids {
                        self.document.nodes[*label_node_id].write_text_alternative(
                            &mut label,
                            // A <label> does not contribute the content of the element it labels.
                            Some(node_id),
                        );
                        label.break_word();
                    }
                }
            }
        }
        if !label.text.is_empty() {
            accesskit_node.set_label(label.text.as_str());
        } else {
            self.document.nodes[node_id].set_label_from_node(accesskit_node);
        }
    }

    fn resolve_description(&self, node_id: NodeId, accesskit_node: &mut accesskit::Node) {
        let mut description = FlatText::default();
        if let Some(described_by) = self
            .relations
            .get(&node_id)
            .and_then(|r| r.described_by.as_ref())
        {
            for description_node_id in described_by {
                let description_node = &self.document.nodes[*description_node_id];
                if !self.excluded_nodes.contains(description_node_id) {
                    accesskit_node.push_described_by(to_accesskit_node_id(*description_node_id));
                }
                description_node.write_text_alternative(&mut description, None);
                description.break_word();
            }
        }
        if !description.text.is_empty() {
            accesskit_node.set_description(description.text.as_str());
        } else {
            self.document.nodes[node_id].set_description_from_node(accesskit_node);
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
