use crate::{
    BaseDocument, Node,
    accessibility::{FlatText, to_accesskit_node_id, to_accesskit_node_id_vec},
};
use accesskit::{Role, TreeId, TreeInfo, TreeUpdate};
use blitz_traits::NodeId;
use markup5ever::local_name;
use rustc_hash::{FxHashMap, FxHashSet};

/// The ID of the window node, which is the root of the tree. It is also the focus when no node is
/// focused.
const DEFAULT_ACCESSKIT_NODE_ID: accesskit::NodeId = accesskit::NodeId(u64::MAX);

/// A utility to build accesskit trees out of [BaseDocument].
///
/// To use this, construct an instance with [TreeBuilder::new], then invoke [TreeBuilder::build] on
/// it.
pub(super) struct TreeBuilder<'doc> {
    /// The document whose tree is built.
    document: &'doc BaseDocument,
    /// How the name and the description of a node derive from other nodes, by the ID of the node.
    relations: FxHashMap<NodeId, Relations>,
    /// The nodes which are not in the tree: those hidden with CSS, and their descendants.
    excluded_nodes: FxHashSet<NodeId>,
}

impl<'doc> TreeBuilder<'doc> {
    /// Collects what building needs: the nodes which are excluded from the tree and the relations
    /// between nodes. This traverses the whole document.
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

    /// Records in `relations_map` the nodes which name or describe `node`. A `<label>` is recorded
    /// on the element which it labels, not on itself. How the sources take precedence over one
    /// another follows the specification.
    ///
    /// <https://www.w3.org/TR/accname-1.2/#computation-steps>
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

    /// Builds the tree update.
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

    /// The AccessKit nodes in document order, followed by the window node. The children of each
    /// node are its children in the DOM except for the excluded ones.
    fn create_nodes(&self) -> Vec<(accesskit::NodeId, accesskit::Node)> {
        let mut window = accesskit::Node::new(Role::Window);
        window.push_child(to_accesskit_node_id(self.document.root_node_id));
        let mut nodes = Vec::new();
        self.document.visit(|node_id, node| {
            if self.excluded_nodes.contains(&node_id) {
                return;
            }
            let mut accesskit_node = node.build_accesskit_node();
            if accesskit_node.role() == Role::Label {
                self.set_value_of_label(node, &mut accesskit_node);
            }
            let children: Vec<_> = node
                .children
                .iter()
                .filter(|n| !self.excluded_nodes.contains(n))
                .map(|n| to_accesskit_node_id(*n))
                .collect();
            accesskit_node.set_children(children);
            self.resolve_name(node_id, node, &mut accesskit_node);
            self.resolve_description(node_id, node, &mut accesskit_node);
            nodes.push((to_accesskit_node_id(node_id), accesskit_node));
        });
        nodes
            .into_iter()
            .chain([(DEFAULT_ACCESSKIT_NODE_ID, window)])
            .collect()
    }

    /// Sets the value of a node with the role [accesskit::Role::Label] to its text. AccessKit
    /// expects the text of such a node in its value, not its label, and `accesskit_consumer` reads
    /// it there when the node labels another node. A control wrapped by the `<label>` is left out.
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

    /// Sets the label of `accesskit_node`, and `labelled_by` if other nodes name it. The text of
    /// those nodes takes precedence. Without any, this falls back to
    /// [Node::set_accesskit_node_label].
    ///
    /// The `labelled_by` of an `aria-labelledby` leaves out the targets which are not in the tree,
    /// but their text still counts.
    ///
    /// <https://www.w3.org/TR/accname-1.2/#comp_hidden_not_referenced>
    fn resolve_name(&self, node_id: NodeId, node: &Node, accesskit_node: &mut accesskit::Node) {
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
            node.set_accesskit_node_label(accesskit_node);
        }
    }

    /// Sets the description of `accesskit_node`, and `described_by` if `aria-describedby` refers
    /// to other nodes. Their text takes precedence. Without any, this falls back to
    /// [Node::set_accesskit_node_description].
    ///
    /// The `described_by` leaves out the targets which are not in the tree, but their text still
    /// counts. This must come after [TreeBuilder::resolve_name], since the description of a node
    /// can depend on its label.
    ///
    /// <https://www.w3.org/TR/accname-1.2/#mapping_additional_nd_description>
    fn resolve_description(
        &self,
        node_id: NodeId,
        node: &Node,
        accesskit_node: &mut accesskit::Node,
    ) {
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
            node.set_accesskit_node_description(accesskit_node);
        }
    }
}

/// The nodes which name or describe a node, other than the node itself.
#[derive(Default, Debug)]
struct Relations {
    /// What provides the name, if anything.
    labelled_by: Option<NameSource>,
    /// The targets of `aria-describedby`, in order.
    described_by: Option<Vec<NodeId>>,
}

/// What provides the name of a node, other than the node itself.
#[derive(Debug)]
enum NameSource {
    /// The targets of `aria-labelledby`, in order. IDs which match no element are left out.
    AriaLabelledBy(Vec<NodeId>),
    /// The `<label>` elements which label the node, in document order.
    LabelElements(Vec<NodeId>),
}
