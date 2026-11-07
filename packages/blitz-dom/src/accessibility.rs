use crate::{
    BaseDocument, ElementData, Node as BlitzDomNode, NodeData, NodeId as BlitzDomNodeId,
    local_name, util::Point,
};
use accesskit::{Affine, Node as AccessKitNode, NodeId, Rect, Role, TreeId, TreeInfo, TreeUpdate};
use std::collections::HashSet;
use style::properties::longhands::visibility;
use stylo_taffy::CopyStyleGetters;

/// The id of the AccessKit window node, which is the root of the tree
const WINDOW_ID: NodeId = NodeId(u64::MAX);

impl BaseDocument {
    /// Build the complete accessibility tree.
    ///
    /// This also starts recording which nodes change, so that later updates can be built
    /// with [`Self::build_accessibility_tree_update`] until [`Self::stop_accessibility_tracking`].
    pub fn build_accessibility_tree(&mut self) -> TreeUpdate {
        let mut changes = std::mem::take(&mut self.accessibility);
        changes.clear();
        changes.tracking = true;

        let mut excluded = HashSet::new();
        let mut nodes = Vec::new();
        let mut geometry = GeometryCache::default();
        self.visit(|node_id, node| {
            let is_hidden = node.is_hidden_from_accessibility_tree();
            if is_hidden {
                changes.hidden.insert(node_id);
            }
            if is_hidden || node.parent.is_some_and(|p| excluded.contains(&p)) {
                excluded.insert(node_id);
                return;
            }
            if node.has_accessibility_relations() {
                changes.relations.insert(node_id);
            }
            nodes.push((
                NodeId(node_id.as_u64()),
                self.build_accessibility_node(node, &mut geometry),
            ));
        });

        changes.viewport = Some(self.accessibility_viewport());
        nodes.push((WINDOW_ID, self.build_accessibility_window()));

        self.accessibility = changes;
        TreeUpdate {
            tree_id: TreeId::ROOT,
            nodes,
            tree: Some(TreeInfo::new(WINDOW_ID)),
            focus: self.accessibility_focus(),
        }
    }

    /// Build an update containing only the nodes that changed since the last
    /// [`Self::build_accessibility_tree`] or [`Self::build_accessibility_tree_update`].
    ///
    /// Must only be called while changes are being tracked
    /// (see [`Self::is_tracking_accessibility_changes`]).
    pub fn build_accessibility_tree_update(&mut self) -> TreeUpdate {
        let mut changes = std::mem::take(&mut self.accessibility);
        let subtrees = std::mem::take(&mut changes.subtrees);
        let mut dirty = std::mem::take(&mut changes.nodes);
        let layout = std::mem::take(&mut changes.layout);
        let scrolled = std::mem::take(&mut changes.scrolled);
        self.collect_accessibility_geometry_changes(layout, scrolled, &mut dirty);

        // Labels and descriptions from aria-labelledby/aria-describedby are computed from
        // other nodes, so they are rebuilt whenever anything changes.
        if !subtrees.is_empty() || !dirty.is_empty() {
            changes.relations.retain(|&id| {
                self.get_node(id)
                    .is_some_and(|n| n.has_accessibility_relations())
            });
            dirty.extend(changes.relations.iter().copied());
        }

        let mut nodes = Vec::new();
        let mut geometry = GeometryCache::default();
        let viewport = self.accessibility_viewport();
        if changes.viewport != Some(viewport) {
            changes.viewport = Some(viewport);
            nodes.push((WINDOW_ID, self.build_accessibility_window()));
        }

        let mut stack = Vec::new();
        for &root_id in &subtrees {
            // Skip subtrees inside other subtrees, and nodes not in the tree
            if self.accessibility_tree_position(root_id, &subtrees, false) != Some(false) {
                continue;
            }
            stack.push(root_id);
            while let Some(node_id) = stack.pop() {
                let node = &self.nodes[node_id];
                if node.is_hidden_from_accessibility_tree() {
                    changes.hidden.insert(node_id);
                    continue;
                }
                if node.has_accessibility_relations() {
                    changes.relations.insert(node_id);
                }
                nodes.push((
                    NodeId(node_id.as_u64()),
                    self.build_accessibility_node(node, &mut geometry),
                ));
                stack.extend(node.children.iter().copied());
            }
        }

        for node_id in dirty {
            // Skip nodes already sent with a subtree, and nodes not in the tree
            if self.accessibility_tree_position(node_id, &subtrees, true) != Some(false) {
                continue;
            }
            let node = &self.nodes[node_id];
            if node.has_accessibility_relations() {
                changes.relations.insert(node_id);
            }
            nodes.push((
                NodeId(node_id.as_u64()),
                self.build_accessibility_node(node, &mut geometry),
            ));
        }

        self.accessibility = changes;
        TreeUpdate {
            tree_id: TreeId::ROOT,
            nodes,
            tree: None,
            focus: self.accessibility_focus(),
        }
    }

    /// Record a change to whether an element is hidden from the accessibility tree
    /// because of its style (`display: none` or `visibility: hidden`).
    pub(crate) fn note_accessibility_style_change(&mut self, node_id: BlitzDomNodeId) {
        let node = &self.nodes[node_id];
        let is_hidden = node.is_hidden_from_accessibility_tree();
        let was_hidden = self.accessibility.hidden.contains(&node_id);
        if is_hidden == was_hidden {
            return;
        }
        if is_hidden {
            self.accessibility.hidden.insert(node_id);
        } else {
            // The subtree was not in the tree, so all of it needs sending
            self.accessibility.hidden.remove(&node_id);
            self.accessibility.subtrees.insert(node_id);
        }
        if let Some(parent_id) = node.parent {
            self.accessibility.nodes.insert(parent_id);
        }
        self.has_changes = true;
    }

    fn accessibility_focus(&self) -> NodeId {
        self.focus_node_id
            .map(|id| NodeId(id.as_u64()))
            .unwrap_or(WINDOW_ID)
    }

    /// `None` if the node is not in the accessibility tree, otherwise whether it (if
    /// `include_self`) or one of its ancestors is in `subtrees`
    fn accessibility_tree_position(
        &self,
        node_id: BlitzDomNodeId,
        subtrees: &HashSet<BlitzDomNodeId>,
        include_self: bool,
    ) -> Option<bool> {
        let mut in_subtree = include_self && subtrees.contains(&node_id);
        let mut node = self.get_node(node_id)?;
        loop {
            if node.is_hidden_from_accessibility_tree() {
                return None;
            }
            let Some(parent_id) = node.parent else {
                return (node.id == self.root_node_id).then_some(in_subtree);
            };
            in_subtree |= subtrees.contains(&parent_id);
            node = self.get_node(parent_id)?;
        }
    }

    /// Whether the node is in the accessibility tree: it is in the document, and neither it
    /// nor any of its ancestors are hidden.
    fn is_in_accessibility_tree(&self, node_id: BlitzDomNodeId) -> bool {
        self.accessibility_tree_position(node_id, &HashSet::new(), false)
            .is_some()
    }

    fn referenced_dom_ids_to_node_ids<'a>(
        &'a self,
        nodes_reference: &'a str,
    ) -> impl Iterator<Item = BlitzDomNodeId> + 'a {
        nodes_reference
            .split_ascii_whitespace()
            .filter_map(|dom_id| self.nodes_to_id.get(dom_id)?.first().copied())
    }

    fn build_accessibility_node(
        &self,
        node: &BlitzDomNode,
        cache: &mut GeometryCache,
    ) -> AccessKitNode {
        let mut builder = AccessKitNode::default();
        let geometry = self.accessibility_geometry(node, cache);
        if let Some(size) = geometry.size {
            builder.set_bounds(Rect::new(0.0, 0.0, size.width as f64, size.height as f64));
        }
        if let Some(parent_id) = node.parent {
            let parent = self.accessibility_geometry(&self.nodes[parent_id], cache);
            let anchor_offset = self.layout_origin_offset(geometry.anchor, parent.anchor);
            let x = geometry.offset.x - parent.offset.x + anchor_offset.x;
            let y = geometry.offset.y - parent.offset.y + anchor_offset.y;
            if x != 0.0 || y != 0.0 {
                builder.set_transform(Affine::translate((x as f64, y as f64)));
            }
        }
        if node.parent.is_none() {
            builder.set_role(Role::Window)
        } else if let Some(element_data) = node.element_data() {
            let name = element_data.name.local.to_string();
            let role_attr = element_data.attr(local_name!("role"));

            // TODO: The roles of elements with strong native semantics cannot be overridden; see
            // https://www.w3.org/TR/wai-aria-1.2/#host_general_conflict.
            let role = role_attr
                .and_then(role_from_name)
                .or_else(|| role_from_element_data(element_data))
                .unwrap_or(Role::Unknown);

            builder.set_role(role);

            // https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion
            if element_data.attr(local_name!("aria-hidden")) == Some("true") {
                builder.set_hidden();
            }

            if let Some(aria_expanded) = element_data.attr(local_name!("aria-expanded"))
                && let Ok(aria_expanded) = aria_expanded.parse()
            {
                builder.set_expanded(aria_expanded);
            }

            if element_data.attr(local_name!("aria-disabled")) == Some("true") {
                builder.set_disabled();
            }

            if let Some(aria_selected) = element_data.attr(local_name!("aria-selected"))
                && let Ok(aria_selected) = aria_selected.parse()
            {
                builder.set_selected(aria_selected);
            }

            if let Some(aria_label) = element_data.attr(local_name!("aria-label")) {
                builder.set_label(aria_label);
            }
            if let Some(aria_description) = element_data.attr(local_name!("aria-description")) {
                builder.set_description(aria_description);
            }
            if let Some(aria_level) = element_data.attr(local_name!("aria-level"))
                && let Ok(aria_level) = aria_level.parse::<usize>()
                // Accesskit levels are 0-based, while ARIA levels are 1-based
                // See https://docs.rs/accesskit/latest/accesskit/struct.Node.html#method.level
                && let Some(aria_level) = aria_level.checked_sub(1)
            {
                builder.set_level(aria_level);
            } else if let Some(default_level) = default_level_for_tag(&name) {
                builder.set_level(default_level);
            } else if let Some(default_level) = default_level_for_role(role) {
                builder.set_level(default_level);
            }

            builder.set_html_tag(name);
        } else if node.is_text_node() {
            builder.set_role(Role::TextRun);
            builder.set_value(node.text_content());
        }

        for &child_id in &node.children {
            let child = &self.nodes[child_id];
            if child.is_hidden_from_accessibility_tree() {
                continue;
            }
            let id = NodeId(child_id.as_u64());
            if child.is_text_node() {
                builder.push_labelled_by(id);
            }
            builder.push_child(id);
        }

        if let Some(id_refs) = node.attr(local_name!("aria-labelledby")) {
            let mut label = FlatText::default();
            let mut has_target = false;
            for target in self.referenced_dom_ids_to_node_ids(id_refs) {
                if !has_target {
                    builder.clear_labelled_by();
                    has_target = true;
                }
                if target == node.id || self.is_in_accessibility_tree(target) {
                    builder.push_labelled_by(NodeId(target.as_u64()));
                }
                self.nodes[target].write_text_alternative(&mut label);
                label.break_word();
            }
            if !label.text.is_empty() {
                builder.set_label(label.text.as_str());
            }
        }

        if let Some(id_refs) = node.attr(local_name!("aria-describedby")) {
            for target in self.referenced_dom_ids_to_node_ids(id_refs) {
                if target == node.id || self.is_in_accessibility_tree(target) {
                    builder.push_described_by(NodeId(target.as_u64()));
                }
            }
        }

        builder
    }

    /// The AccessKit window: the root of the tree, which maps CSS pixels in the document to
    /// physical pixels in the window.
    fn build_accessibility_window(&self) -> AccessKitNode {
        let mut window = AccessKitNode::new(Role::Window);
        window.push_child(NodeId(self.root_node_id.as_u64()));
        let (scale, scroll_x, scroll_y) = self.accessibility_viewport();
        let transform = Affine::scale(scale) * Affine::translate((-scroll_x, -scroll_y));
        if transform != Affine::IDENTITY {
            window.set_transform(transform);
        }
        window
    }

    fn accessibility_viewport(&self) -> (f64, f64, f64) {
        let scroll = self.viewport_scroll();
        (self.viewport().scale_f64(), scroll.x, scroll.y)
    }

    /// Where the node's AccessKit bounds are. Each node's bounds are relative to its parent
    /// (through its `transform`), so a layout change only needs the nodes that moved relative
    /// to their parent resent, not everything inside them.
    fn accessibility_geometry(
        &self,
        node: &BlitzDomNode,
        cache: &mut GeometryCache,
    ) -> AccessibilityGeometry {
        if let Some(geometry) = cache.get(node.id) {
            return geometry;
        }
        let geometry = self.compute_accessibility_geometry(node, cache);
        cache.insert(node.id, geometry);
        geometry
    }

    fn compute_accessibility_geometry(
        &self,
        node: &BlitzDomNode,
        cache: &mut GeometryCache,
    ) -> AccessibilityGeometry {
        let mut inherit = |with_size: bool| match node.parent {
            Some(parent_id) => {
                let mut geometry = self.accessibility_geometry(&self.nodes[parent_id], cache);
                if !with_size {
                    geometry.size = None;
                }
                geometry
            }
            None => AccessibilityGeometry::own_box(node, None),
        };

        // Text nodes have no box of their own, so they use their parent's bounds
        if node.is_text_node() {
            return inherit(true);
        }
        if node.parent.is_none() {
            return AccessibilityGeometry::own_box(node, Some(self.physical_box_size(node)));
        }
        if !node.is_element() {
            return inherit(false);
        }

        // Non-atomic inline elements have no layout box: use the union of their fragments
        if let Some(fragments) = node.inline_fragment_boxes() {
            let union = fragments.reduce(|a, b| taffy::Rect {
                left: a.left.min(b.left),
                right: a.right.max(b.right),
                top: a.top.min(b.top),
                bottom: a.bottom.max(b.bottom),
            });
            let (Some(rect), Some(inline_root)) = (union, node.inline_root_ancestor()) else {
                return inherit(false);
            };
            // Fragments are relative to the inline root's border box, and scroll with it
            let scroll = inline_root.scroll_offset();
            return AccessibilityGeometry {
                anchor: inline_root.id,
                offset: Point {
                    x: rect.left - scroll.x as f32,
                    y: rect.top - scroll.y as f32,
                },
                size: Some(taffy::Size {
                    width: rect.right - rect.left,
                    height: rect.bottom - rect.top,
                }),
            };
        }

        if node.has_no_box() {
            return inherit(false);
        }
        AccessibilityGeometry::own_box(node, Some(self.physical_box_size(node)))
    }

    fn physical_box_size(&self, node: &BlitzDomNode) -> taffy::Size<f32> {
        #[cfg(feature = "writing-mode")]
        if self.is_vertical_box(node.id)
            || node
                .containing_block()
                .is_some_and(|cb| self.is_vertical_box(cb))
        {
            return self.physical_unrounded_geometry(node.id).1.size;
        }
        node.unrounded_layout().size
    }

    #[cfg(feature = "writing-mode")]
    fn is_vertical_box(&self, node_id: BlitzDomNodeId) -> bool {
        self.nodes[node_id].layout_data().writing_mode.is_vertical()
    }

    /// The position of `from`'s border box relative to `to`'s border box. Usually `to` is
    /// a containing block of `from`, which only needs a walk up from `from` to `to`.
    /// Otherwise both are measured from their closest common containing block.
    fn layout_origin_offset(&self, from: BlitzDomNodeId, to: BlitzDomNodeId) -> Point<f32> {
        #[cfg(feature = "writing-mode")]
        if self.is_vertical_box(from) || self.is_vertical_box(to) {
            return point_sub(self.layout_origin(from), self.layout_origin(to));
        }
        let mut pos = Point { x: 0.0, y: 0.0 };
        let mut current = from;
        while current != to {
            let Some((cb, offset)) = self.offset_in_containing_block(current) else {
                return self.offset_via_common_block(from, to);
            };
            #[cfg(feature = "writing-mode")]
            if self.is_vertical_box(cb) {
                return point_sub(self.layout_origin(from), self.layout_origin(to));
            }
            pos = Point {
                x: pos.x + offset.x,
                y: pos.y + offset.y,
            };
            current = cb;
        }
        pos
    }

    /// `layout_origin_offset` when `to` is not a containing block of `from`: walk up from `to`
    /// until reaching a node in `from`'s containing block chain.
    fn offset_via_common_block(&self, from: BlitzDomNodeId, to: BlitzDomNodeId) -> Point<f32> {
        let mut from_chain = smallvec::SmallVec::<[_; 32]>::new();
        let mut from_root_pos = Point { x: 0.0, y: 0.0 };
        let mut from_root = from;
        while let Some((cb, offset)) = self.offset_in_containing_block(from_root) {
            from_chain.push((from_root, from_root_pos));
            from_root_pos = Point {
                x: from_root_pos.x + offset.x,
                y: from_root_pos.y + offset.y,
            };
            from_root = cb;
        }

        let mut pos = Point { x: 0.0, y: 0.0 };
        let mut current = to;
        loop {
            if current == from_root {
                return point_sub(from_root_pos, pos);
            }
            if let Some(&(_, from_pos)) = from_chain.iter().find(|(id, _)| *id == current) {
                return point_sub(from_pos, pos);
            }
            let Some((cb, offset)) = self.offset_in_containing_block(current) else {
                // Not in the same tree of boxes
                return point_sub(
                    point_sub(self.layout_origin(from_root), self.layout_origin(current)),
                    point_sub(pos, from_root_pos),
                );
            };
            pos = Point {
                x: pos.x + offset.x,
                y: pos.y + offset.y,
            };
            current = cb;
        }
    }

    /// The node's containing block, and the position of the node's border box relative to
    /// the containing block's border box
    fn offset_in_containing_block(
        &self,
        node_id: BlitzDomNodeId,
    ) -> Option<(BlitzDomNodeId, Point<f32>)> {
        let node = &self.nodes[node_id];
        let cb = node.containing_block()?;
        let scroll = self.nodes[cb].scroll_offset();
        let location = node.unrounded_layout().location;
        Some((
            cb,
            Point {
                x: location.x - scroll.x as f32,
                y: location.y - scroll.y as f32,
            },
        ))
    }

    /// The document-relative position of the node's border box
    fn layout_origin(&self, node_id: BlitzDomNodeId) -> Point<f32> {
        let (pos, _) = self.physical_unrounded_geometry(node_id);
        let scroll = self.nodes[node_id].scroll_offset();
        Point {
            x: pos.x + scroll.x as f32,
            y: pos.y + scroll.y as f32,
        }
    }

    /// Mark the nodes whose position relative to their parent, or whose size, changed with
    /// the layout or scroll changes.
    fn collect_accessibility_geometry_changes(
        &self,
        layout: HashSet<BlitzDomNodeId>,
        scrolled: HashSet<BlitzDomNodeId>,
        dirty: &mut HashSet<BlitzDomNodeId>,
    ) {
        // Nodes that moved, and whether their size may have changed too
        let mut moved: Vec<_> = layout.into_iter().map(|id| (id, true)).collect();
        for node_id in scrolled {
            let Some(node) = self.get_node(node_id) else {
                continue;
            };
            // Everything positioned against the scrolled node's content moved
            let children = node.layout_children_or_children().into_iter();
            let hoisted = node.hoisted_children.borrow().to_vec();
            moved.extend(children.chain(hoisted).map(|id| (id, false)));
            if node.flags.is_inline_root() {
                self.collect_inline_content(node_id, dirty);
            }
        }

        let mut children = Vec::new();
        while let Some((node_id, resized)) = moved.pop() {
            let Some(node) = self.get_node(node_id) else {
                continue;
            };
            // Anonymous boxes are not in the tree, but the boxes inside them moved
            if node.is_anonymous() {
                let children = node.layout_children_or_children().into_iter();
                moved.extend(children.map(|id| (id, false)));
                continue;
            }
            dirty.insert(node_id);

            // Text takes its parent's size, and children that are not positioned against
            // this node do not move with it.
            children.extend(node.children.iter().copied());
            while let Some(child_id) = children.pop() {
                let child = &self.nodes[child_id];
                if child.is_text_node() {
                    if resized {
                        dirty.insert(child_id);
                    }
                } else if !child.is_element() || child.has_no_box() {
                    children.extend(child.children.iter().copied());
                } else if !child.is_non_atomic_inline() && !self.is_positioned_by(child, node_id) {
                    dirty.insert(child_id);
                }
            }
        }
    }

    /// Whether `node` is laid out inside `ancestor`'s box, possibly via anonymous boxes
    fn is_positioned_by(&self, node: &BlitzDomNode, ancestor: BlitzDomNodeId) -> bool {
        let mut cb = node.containing_block();
        while let Some(cb_id) = cb {
            if cb_id == ancestor {
                return true;
            }
            // Nodes that were not laid out again can point at removed anonymous boxes
            let Some(cb_node) = self.get_node(cb_id) else {
                return false;
            };
            if !cb_node.is_anonymous() {
                return false;
            }
            cb = cb_node.containing_block();
        }
        false
    }

    /// Record that an inline root was laid out again: its text and inline elements may have
    /// moved without any layout box changing.
    pub(crate) fn mark_accessibility_inline_content(&mut self, inline_root: BlitzDomNodeId) {
        let mut nodes = std::mem::take(&mut self.accessibility.nodes);
        self.collect_inline_content(inline_root, &mut nodes);
        self.accessibility.nodes = nodes;
        self.has_changes = true;
    }

    fn collect_inline_content(
        &self,
        inline_root: BlitzDomNodeId,
        dirty: &mut HashSet<BlitzDomNodeId>,
    ) {
        let mut stack: Vec<_> = self.nodes[inline_root].children.to_vec();
        while let Some(node_id) = stack.pop() {
            let node = &self.nodes[node_id];
            if node.is_text_node() {
                dirty.insert(node_id);
            } else if node.is_non_atomic_inline() || (node.is_element() && node.has_no_box()) {
                dirty.insert(node_id);
                stack.extend(node.children.iter().copied());
            } else if node.is_element() {
                // Atomic inlines are placed by the inline layout, not `set_unrounded_layout`
                dirty.insert(node_id);
            }
        }
    }
}

fn point_sub(a: Point<f32>, b: Point<f32>) -> Point<f32> {
    Point {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}

/// The most recently computed geometries while building an update, as each node also
/// needs its parent's, and siblings share a parent
#[derive(Default)]
struct GeometryCache {
    entries: [Option<(BlitzDomNodeId, AccessibilityGeometry)>; 4],
    next: usize,
}

impl GeometryCache {
    fn get(&self, node_id: BlitzDomNodeId) -> Option<AccessibilityGeometry> {
        self.entries
            .iter()
            .flatten()
            .find(|(id, _)| *id == node_id)
            .map(|&(_, geometry)| geometry)
    }

    fn insert(&mut self, node_id: BlitzDomNodeId, geometry: AccessibilityGeometry) {
        self.entries[self.next] = Some((node_id, geometry));
        self.next = (self.next + 1) % self.entries.len();
    }
}

/// A node's accessibility bounds: an offset from the border box of a node with a layout
/// box (the anchor), and the size of the bounds if the node has any.
#[derive(Clone, Copy)]
struct AccessibilityGeometry {
    anchor: BlitzDomNodeId,
    offset: Point<f32>,
    size: Option<taffy::Size<f32>>,
}

impl AccessibilityGeometry {
    fn own_box(node: &BlitzDomNode, size: Option<taffy::Size<f32>>) -> Self {
        Self {
            anchor: node.id,
            offset: Point { x: 0.0, y: 0.0 },
            size,
        }
    }
}

impl BlitzDomNode {
    /// Whether the element generates no box (`display: contents`)
    fn has_no_box(&self) -> bool {
        self.primary_styles()
            .is_some_and(|styles| styles.clone_display().is_contents())
    }

    fn layout_children_or_children(&self) -> Vec<BlitzDomNodeId> {
        match &*self.layout_children.borrow() {
            Some(children) => children.to_vec(),
            None => self.children.to_vec(),
        }
    }

    /// Whether the node's accessibility node depends on other nodes
    fn has_accessibility_relations(&self) -> bool {
        self.attr(local_name!("aria-labelledby")).is_some()
            || self.attr(local_name!("aria-describedby")).is_some()
    }

    // https://www.w3.org/TR/wai-aria-1.2/#tree_exclusion
    fn is_hidden_from_accessibility_tree(&self) -> bool {
        self.try_stylo_element_data()
            .as_ref()
            .and_then(|s| s.get())
            .and_then(|s| {
                let style = s.styles.get_primary()?;
                Some(
                    style.get_box().get_display().is_none()
                        || style.clone_visibility() == visibility::computed_value::T::Hidden,
                )
            })
            .unwrap_or(false)
    }

    fn write_text_alternative(&self, out: &mut FlatText) {
        match self
            .attr(local_name!("aria-label"))
            .filter(|aria_label| !aria_label.trim_ascii().is_empty())
        {
            Some(aria_label) => out.push(aria_label),
            None => self.write_flat_text_content(out),
        }
    }

    fn write_flat_text_content(&self, out: &mut FlatText) {
        match &self.data {
            NodeData::Text(data) => out.push(&data.content),
            NodeData::Element(..) | NodeData::AnonymousBlock(..) => {
                for &child_id in &self.children {
                    self.with(child_id).write_flat_text_content(out);
                }
            }
            _ => {}
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

fn role_from_name(name: &str) -> Option<Role> {
    match name {
        "alert" => Some(Role::Alert),
        "alertdialog" => Some(Role::AlertDialog),
        "button" => Some(Role::Button),
        "checkbox" => Some(Role::CheckBox),
        "dialog" => Some(Role::Dialog),
        "gridcell" => Some(Role::GridCell),
        "link" => Some(Role::Link),
        "log" => Some(Role::Log),
        "marquee" => Some(Role::Marquee),
        "menuitem" => Some(Role::MenuItem),
        "menuitemcheckbox" => Some(Role::MenuItemCheckBox),
        "menuitemradio" => Some(Role::MenuItemRadio),
        "option" => Some(Role::ListBoxOption),
        "progressbar" => Some(Role::ProgressIndicator),
        "radio" => Some(Role::RadioButton),
        "scrollbar" => Some(Role::ScrollBar),
        "slider" => Some(Role::Slider),
        "spinbutton" => Some(Role::SpinButton),
        "status" => Some(Role::Status),
        "tab" => Some(Role::Tab),
        "tabpanel" => Some(Role::TabPanel),
        "textbox" => Some(Role::TextInput),
        "timer" => Some(Role::Timer),
        "tooltip" => Some(Role::Tooltip),
        "treeitem" => Some(Role::TreeItem),
        "combobox" => Some(Role::ComboBox),
        "grid" => Some(Role::Grid),
        "listbox" => Some(Role::ListBox),
        "menu" => Some(Role::Menu),
        "menubar" => Some(Role::MenuBar),
        "radiogroup" => Some(Role::RadioGroup),
        "tablist" => Some(Role::TabList),
        "tree" => Some(Role::Tree),
        "treegrid" => Some(Role::TreeGrid),
        "article" => Some(Role::Article),
        "columnheader" => Some(Role::ColumnHeader),
        "definition" => Some(Role::Definition),
        "document" => Some(Role::Document),
        "group" => Some(Role::Group),
        "heading" => Some(Role::Heading),
        "img" => Some(Role::Image),
        "list" => Some(Role::List),
        "listitem" => Some(Role::ListItem),
        "math" => Some(Role::Math),
        "note" => Some(Role::Note),
        "region" => Some(Role::Region),
        "row" => Some(Role::Row),
        "rowgroup" => Some(Role::RowGroup),
        "rowheader" => Some(Role::RowHeader),
        "toolbar" => Some(Role::Toolbar),
        "application" => Some(Role::Application),
        "banner" => Some(Role::Banner),
        "complementary" => Some(Role::Complementary),
        "contentinfo" => Some(Role::ContentInfo),
        "form" => Some(Role::Form),
        "main" => Some(Role::Main),
        "navigation" => Some(Role::Navigation),
        "search" => Some(Role::Search),
        _ => None,
    }
}

fn default_level_for_tag(name: &str) -> Option<usize> {
    // Accesskit levels are 0-based, while ARIA levels are 1-based
    // See https://docs.rs/accesskit/latest/accesskit/struct.Node.html#method.level
    match name {
        "h1" => Some(0),
        "h2" => Some(1),
        "h3" => Some(2),
        "h4" => Some(3),
        "h5" => Some(4),
        "h6" => Some(5),
        _ => None,
    }
}

fn default_level_for_role(role: Role) -> Option<usize> {
    match role {
        // ARIA specifies an implicit level of 2 for the heading role. Accesskit levels are 0-based,
        // so this turns into level 1
        Role::Heading => Some(1),
        _ => None,
    }
}

fn role_from_element_data(element_data: &ElementData) -> Option<Role> {
    // <https://www.w3.org/TR/html-aam-1.0/>
    match &*element_data.name.local {
        // Document structure
        "article" => Some(Role::Article),
        "aside" => Some(Role::Complementary),
        "footer" => Some(Role::Footer),
        "header" => Some(Role::Header),
        "main" => Some(Role::Main),
        "nav" => Some(Role::Navigation),
        "search" => Some(Role::Search),
        "section" => Some(Role::Section),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(Role::Heading),
        "p" => Some(Role::Paragraph),
        "blockquote" => Some(Role::Blockquote),
        "figure" => Some(Role::Figure),
        "figcaption" | "caption" => Some(Role::Caption),
        "hr" => Some(Role::Splitter),

        // Grouping
        "ul" | "ol" | "menu" => Some(Role::List),
        "li" => Some(Role::ListItem),
        "dl" => Some(Role::DescriptionList),
        "dt" => Some(Role::Term),
        "dd" => Some(Role::Definition),
        "dialog" => Some(Role::Dialog),
        "fieldset" => Some(Role::Group),
        "form" => Some(Role::Form),
        "div" => Some(Role::GenericContainer),

        // Tables
        "table" => Some(Role::Table),
        "thead" | "tbody" | "tfoot" => Some(Role::RowGroup),
        "tr" => Some(Role::Row),
        "td" => Some(Role::Cell),
        "th" => match element_data.attr(local_name!("scope")) {
            Some("row") | Some("rowgroup") => Some(Role::RowHeader),
            _ => Some(Role::ColumnHeader),
        },

        // Interactive
        // An <a> is only a link when it has an href.
        "a" => match element_data.attr(local_name!("href")) {
            Some(_) => Some(Role::Link),
            None => Some(Role::GenericContainer),
        },
        "button" => Some(Role::Button),
        "label" => Some(Role::Label),
        "legend" => Some(Role::Label),
        "select" => match element_data.attr(local_name!("multiple")) {
            Some(_) => Some(Role::ListBox),
            None => Some(Role::ComboBox),
        },
        "option" => Some(Role::ListBoxOption),
        "textarea" => Some(Role::MultilineTextInput),
        "progress" => Some(Role::ProgressIndicator),
        "meter" => Some(Role::Meter),
        "output" => Some(Role::Status),
        "summary" => Some(Role::DisclosureTriangle),

        // Inline semantics
        "code" => Some(Role::Code),
        "em" => Some(Role::Emphasis),
        "strong" => Some(Role::Strong),
        "mark" => Some(Role::Mark),
        "time" => Some(Role::Time),
        "img" => Some(Role::Image),
        "iframe" => Some(Role::Iframe),

        "input" => {
            let ty = element_data.attr(local_name!("type")).unwrap_or("text");
            match ty {
                "button" | "submit" | "reset" => Some(Role::Button),
                "checkbox" => Some(Role::CheckBox),
                "color" => Some(Role::ColorWell),
                "date" => Some(Role::DateInput),
                "datetime-local" => Some(Role::DateTimeInput),
                "email" => Some(Role::EmailInput),
                "number" => Some(Role::NumberInput),
                "password" => Some(Role::PasswordInput),
                "radio" => Some(Role::RadioButton),
                "range" => Some(Role::Slider),
                "search" => Some(Role::SearchInput),
                "tel" => Some(Role::PhoneNumberInput),
                "time" => Some(Role::TimeInput),
                _ => Some(Role::TextInput),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentConfig, qual_name};
    use blitz_traits::shell::{ColorScheme, Viewport};
    use std::collections::HashMap;

    type Tree = HashMap<NodeId, AccessKitNode>;

    /// Apply an update the way AccessKit does: replace the listed nodes, then drop the
    /// nodes that are no longer reachable from the root.
    fn apply(tree: &mut Tree, update: TreeUpdate) -> usize {
        let count = update.nodes.len();
        let updated: Vec<_> = update.nodes.iter().map(|(id, _)| *id).collect();
        for (id, node) in update.nodes {
            tree.insert(id, node);
        }
        let mut reachable = HashSet::new();
        let mut stack = vec![WINDOW_ID];
        while let Some(id) = stack.pop() {
            if reachable.insert(id) {
                let node = tree.get(&id).expect("child is missing from the tree");
                stack.extend(node.children().iter().copied());
            }
        }
        for id in updated {
            assert!(
                reachable.contains(&id),
                "update contains detached node {id:?}"
            );
        }
        tree.retain(|id, _| reachable.contains(id));
        count
    }

    /// The full tree, without changing what the document has recorded
    fn full_tree(doc: &mut BaseDocument) -> Tree {
        let changes = std::mem::take(&mut doc.accessibility);
        let tree = doc.build_accessibility_tree().nodes.into_iter().collect();
        doc.accessibility = changes;
        tree
    }

    /// Resolve, apply the incremental update, and check it matches the full tree
    fn update(doc: &mut BaseDocument, tree: &mut Tree) -> usize {
        doc.resolve(0.0);
        let update = doc.build_accessibility_tree_update();
        let focus = update.focus;
        let count = apply(tree, update);
        doc.clear_changes();
        let expected = full_tree(doc);
        assert_eq!(tree.len(), expected.len());
        for (id, node) in &expected {
            assert!(
                tree.get(id) == Some(node),
                "node {id:?} is out of date: {:?} != {node:?}",
                tree.get(id)
            );
        }
        assert_eq!(focus, doc.accessibility_focus());
        count
    }

    #[test]
    fn incremental_updates_match_full_tree() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(crate::UserAgentStylesheet::parse(
            "html, body, h1, ul, li, div, section, p { display: block; }",
        ));
        let root_id = doc.root_node().id;
        let mut m = doc.mutate();
        let html = m.create_element(qual_name!("html"), vec![]);
        let body = m.create_element(qual_name!("body"), vec![]);
        let h1 = m.create_element(qual_name!("h1"), vec![]);
        let h1_text = m.create_text_node("Title");
        m.append_children(h1, &[h1_text]);
        let list = m.create_element(qual_name!("ul"), vec![]);
        let mut items = Vec::new();
        for i in 0..3 {
            let li = m.create_element(qual_name!("li"), vec![]);
            let text = m.create_text_node(&format!("Item {i}"));
            m.append_children(li, &[text]);
            items.push((li, text));
        }
        let ids: Vec<_> = items.iter().map(|(li, _)| *li).collect();
        m.append_children(list, &ids);
        let label = m.create_element(qual_name!("div"), vec![]);
        m.set_attribute(label, qual_name!("id"), "label");
        let label_text = m.create_text_node("Name");
        m.append_children(label, &[label_text]);
        let button = m.create_element(qual_name!("button"), vec![]);
        m.set_attribute(button, qual_name!("aria-labelledby"), "label");
        let section = m.create_element(qual_name!("section"), vec![]);
        m.set_style_property(section, "display", "none");
        let p = m.create_element(qual_name!("p"), vec![]);
        let p_text = m.create_text_node("Hidden");
        m.append_children(p, &[p_text]);
        m.append_children(section, &[p]);
        m.append_children(body, &[h1, list, label, button, section]);
        m.append_children(html, &[body]);
        m.append_children(root_id, &[html]);
        drop(m);
        doc.resolve(0.0);

        let mut tree: Tree = doc.build_accessibility_tree().nodes.into_iter().collect();
        doc.clear_changes();
        assert!(!tree.contains_key(&NodeId(p.as_u64())));

        // Nothing changed
        assert_eq!(update(&mut doc, &mut tree), 0);

        // Text change: the text node, plus the button labelled by other nodes
        doc.mutate().set_node_text(items[0].1, "First");
        assert_eq!(update(&mut doc, &mut tree), 2);

        // Attribute change
        doc.mutate()
            .set_attribute(items[1].0, qual_name!("aria-label"), "Second");
        assert_eq!(update(&mut doc, &mut tree), 2);

        // Insertion: the list, the new item and its text
        let mut m = doc.mutate();
        let li = m.create_element(qual_name!("li"), vec![]);
        let text = m.create_text_node("Item 3");
        m.append_children(li, &[text]);
        m.append_children(list, &[li]);
        drop(m);
        update(&mut doc, &mut tree);

        // Removal
        doc.mutate().remove_and_drop_node(items[2].0);
        update(&mut doc, &mut tree);

        // Hiding with style removes the subtree via its parent
        doc.mutate().set_style_property(list, "display", "none");
        update(&mut doc, &mut tree);
        assert!(!tree.contains_key(&NodeId(list.as_u64())));

        // Showing with style sends the whole subtree
        doc.mutate().remove_style_property(section, "display");
        update(&mut doc, &mut tree);
        assert!(tree.contains_key(&NodeId(p_text.as_u64())));

        // A label computed from another node's text
        doc.mutate().set_node_text(label_text, "Full name");
        assert_eq!(update(&mut doc, &mut tree), 2);

        // Moving a node
        doc.mutate().append_children(section, &[h1]);
        update(&mut doc, &mut tree);

        // A node moved out of a hidden subtree
        doc.mutate().append_children(body, &[items[0].0]);
        update(&mut doc, &mut tree);
        assert!(tree.contains_key(&NodeId(items[0].1.as_u64())));

        // Focus only changes the update's focus
        assert!(doc.set_focus_to(button));
        assert_eq!(update(&mut doc, &mut tree), 0);
    }

    /// The node's bounds in window coordinates, applying the transforms of it and its ancestors
    fn absolute_bounds(tree: &Tree, id: NodeId) -> Option<Rect> {
        let mut parents = HashMap::new();
        for (&parent_id, node) in tree {
            for &child_id in node.children() {
                parents.insert(child_id, parent_id);
            }
        }
        let mut chain = vec![id];
        while let Some(&parent_id) = parents.get(chain.last().unwrap()) {
            chain.push(parent_id);
        }
        let transform = chain.iter().rev().fold(Affine::IDENTITY, |transform, id| {
            transform * tree[id].transform().copied().unwrap_or(Affine::IDENTITY)
        });
        tree[&id]
            .bounds()
            .map(|bounds| transform.transform_rect_bbox(bounds))
    }

    struct GeometryPage {
        doc: BaseDocument,
        header: BlitzDomNodeId,
        span_text: BlitzDomNodeId,
        scroller: BlitzDomNodeId,
        rows: Vec<BlitzDomNodeId>,
    }

    fn geometry_page() -> GeometryPage {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 2.0, ColorScheme::Light)),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(crate::UserAgentStylesheet::parse(
            "html, body, div, p { display: block; } body { margin: 0; } \
             div { margin-left: 10px; margin-top: 5px; } \
             .header { height: 20px; } \
             .scroller { height: 60px; overflow: scroll; } \
             .row { height: 30px; } \
             .positioned { position: relative; height: 40px; } \
             .abs { position: absolute; left: 7px; top: 9px; width: 20px; height: 10px; } \
             .ib { display: inline-block; width: 30px; height: 12px; }",
        ));
        let root_id = doc.root_node().id;
        let mut m = doc.mutate();
        let element = |m: &mut crate::DocumentMutator, class: &str| {
            let id = m.create_element(qual_name!("div"), vec![]);
            if !class.is_empty() {
                m.set_attribute(id, qual_name!("class"), class);
            }
            id
        };
        let html = m.create_element(qual_name!("html"), vec![]);
        let body = m.create_element(qual_name!("body"), vec![]);
        let header = element(&mut m, "header");
        let p = m.create_element(qual_name!("p"), vec![]);
        let before = m.create_text_node("Some text ");
        let span = m.create_element(qual_name!("span"), vec![]);
        let span_text = m.create_text_node("in a span");
        m.append_children(span, &[span_text]);
        let inline_block = m.create_element(qual_name!("span"), vec![]);
        m.set_attribute(inline_block, qual_name!("class"), "ib");
        m.append_children(p, &[before, span, inline_block]);
        let scroller = element(&mut m, "scroller");
        let rows: Vec<_> = (0..4)
            .map(|i| {
                let row = element(&mut m, "row");
                let text = m.create_text_node(&format!("Row {i}"));
                m.append_children(row, &[text]);
                row
            })
            .collect();
        m.append_children(scroller, &rows);
        let positioned = element(&mut m, "positioned");
        let wrapper = element(&mut m, "");
        let abs = element(&mut m, "abs");
        m.append_children(wrapper, &[abs]);
        m.append_children(positioned, &[wrapper]);
        // Inline content next to a block, which gets wrapped in an anonymous block
        let mixed = element(&mut m, "");
        let mixed_text = m.create_text_node("Anonymous");
        let mixed_block = element(&mut m, "row");
        m.append_children(mixed, &[mixed_text, mixed_block]);
        m.append_children(body, &[header, p, scroller, positioned, mixed]);
        m.append_children(html, &[body]);
        m.append_children(root_id, &[html]);
        drop(m);
        doc.resolve(0.0);
        GeometryPage {
            doc,
            header,
            span_text,
            scroller,
            rows,
        }
    }

    #[test]
    fn bounds_match_client_rects() {
        let GeometryPage {
            mut doc, scroller, ..
        } = geometry_page();
        doc.scroll_to(scroller, 0.0, 25.0, crate::ScrollBehavior::Instant);
        doc.set_viewport_scroll(crate::Point { x: 0.0, y: 12.0 });
        doc.resolve(0.0);

        let tree: Tree = doc.build_accessibility_tree().nodes.into_iter().collect();
        let scale = doc.viewport().scale_f64();
        let mut checked = 0;
        doc.visit(|node_id, node| {
            // getBoundingClientRect subtracts a scroll container's own scroll offset
            if node.is_hidden_from_accessibility_tree()
                || node_id == scroller
                || !(node.is_element() || node.is_text_node())
            {
                return;
            }
            let rect_id = if node.is_text_node() {
                node.parent.unwrap()
            } else {
                node_id
            };
            let Some(rect) = doc.get_client_bounding_rect(rect_id) else {
                return;
            };
            let bounds = absolute_bounds(&tree, NodeId(node_id.as_u64())).unwrap();
            let expected = Rect::new(
                rect.x * scale,
                rect.y * scale,
                (rect.x + rect.width) * scale,
                (rect.y + rect.height) * scale,
            );
            for (a, b) in [
                (bounds.x0, expected.x0),
                (bounds.y0, expected.y0),
                (bounds.x1, expected.x1),
                (bounds.y1, expected.y1),
            ] {
                assert!(
                    (a - b).abs() < 0.1,
                    "{node_id:?}: {bounds:?} != {expected:?}"
                );
            }
            checked += 1;
        });
        assert!(checked > 15);
    }

    #[test]
    fn geometry_updates_match_full_tree() {
        let GeometryPage {
            mut doc,
            header,
            span_text,
            scroller,
            rows,
        } = geometry_page();
        let mut tree: Tree = doc.build_accessibility_tree().nodes.into_iter().collect();
        doc.clear_changes();
        let total = tree.len();

        // Content after a resized box moves, but not the nodes inside it
        doc.mutate().set_style_property(header, "height", "50px");
        let count = update(&mut doc, &mut tree);
        assert!(count < total / 2, "{count} of {total} nodes resent");

        // Scrolling moves the scroller's children
        doc.scroll_to(scroller, 0.0, 20.0, crate::ScrollBehavior::Instant);
        assert_eq!(update(&mut doc, &mut tree), rows.len());

        // Scrolling the viewport or changing the scale only changes the window
        doc.set_viewport_scroll(crate::Point { x: 0.0, y: 30.0 });
        assert_eq!(update(&mut doc, &mut tree), 1);
        doc.set_viewport(Viewport::new(400, 300, 1.5, ColorScheme::Light));
        update(&mut doc, &mut tree);

        // Resizing the window
        doc.set_viewport(Viewport::new(200, 300, 1.5, ColorScheme::Light));
        update(&mut doc, &mut tree);

        // Text changes move the inline content after them
        doc.mutate()
            .set_node_text(span_text, "in a much longer span");
        update(&mut doc, &mut tree);
    }

    #[test]
    fn changes_are_not_recorded_while_not_tracking() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let root_id = doc.root_node().id;
        let mut m = doc.mutate();
        let html = m.create_element(qual_name!("html"), vec![]);
        m.append_children(root_id, &[html]);
        drop(m);
        doc.build_accessibility_tree();
        assert!(doc.is_tracking_accessibility_changes());

        doc.stop_accessibility_tracking();
        doc.mutate()
            .set_attribute(html, qual_name!("aria-label"), "Page");
        assert!(!doc.is_tracking_accessibility_changes());
        assert!(doc.accessibility.nodes.is_empty());
        assert!(doc.has_changes());
    }
}
