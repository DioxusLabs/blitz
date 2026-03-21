//! Resolve style and layout

use std::{
    cell::RefCell,
    time::{SystemTime, UNIX_EPOCH},
};

use debug_timer::debug_timer;
use parley::LayoutContext;
use selectors::Element as _;
use style::dom::TDocument;

#[cfg(feature = "parallel-construct")]
use rayon::prelude::*;

// FIXME: static thread_local FontCtx isn't necessarily correct in multi-document context.
// Should use thread_local crate with ThreadLocal value store in the Document.
thread_local! {
    pub(crate) static LAYOUT_CTX: RefCell<Option<Box<LayoutContext<TextBrush>>>> = const { RefCell::new(None) };
}

#[cfg(feature = "incremental")]
use style::selector_parser::RestyleDamage;
use taffy::AvailableSpace;

use crate::{
    BaseDocument, NON_INCREMENTAL,
    events::ScrollAnimationState,
    layout::{
        construct::{
            ConstructionTask, ConstructionTaskData, ConstructionTaskResult,
            ConstructionTaskResultData, build_inline_layout_into, collect_layout_children,
        },
        damage::{ALL_DAMAGE, CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC},
    },
    node::TextBrush,
};

impl BaseDocument {
    /// Restyle the tree and then relayout it
    pub fn resolve(&mut self, current_time_for_animations: f64) {
        if TDocument::as_node(&&self.nodes[0])
            .first_element_child()
            .is_none()
        {
            #[cfg(feature = "tracing")]
            tracing::warn!("No DOM - not resolving");
            return;
        }

        // Process messages that have been sent to our message channel (e.g. loaded resource)
        self.handle_messages();

        self.resolve_scroll_animation();

        let root_node_id = self.root_element().id;
        debug_timer!(timer, feature = "log_phase_times");

        // we need to resolve stylist first since it will need to drive our layout bits
        self.resolve_stylist(current_time_for_animations);
        timer.record_time("style");

        // Propagate damage flags (from mutation and restyles) up and down the tree
        #[cfg(feature = "incremental")]
        self.propagate_damage_flags(root_node_id, RestyleDamage::empty());
        #[cfg(feature = "incremental")]
        timer.record_time("damage");

        // Fix up tree for layout (insert anonymous blocks as necessary, etc)
        self.resolve_layout_children();
        timer.record_time("construct");

        // Reparent absolutely/fixed-positioned children to their correct containing block
        // ancestor so Taffy resolves insets and percentages against the right dimensions.
        self.reparent_out_of_flow_children();
        timer.record_time("reparent");

        self.resolve_deferred_tasks();
        timer.record_time("pconstruct");

        // Merge stylo into taffy
        self.flush_styles_to_layout(root_node_id);
        self.collect_sticky_nodes();
        timer.record_time("flush");

        // Next we resolve layout with the data resolved by stlist
        self.resolve_layout();
        self.recompute_sticky_offsets();
        timer.record_time("layout");

        // Clear all damage and dirty flags
        #[cfg(feature = "incremental")]
        {
            for (_, node) in self.nodes.iter_mut() {
                node.clear_damage_mut();
                node.unset_dirty_descendants();
            }
            timer.record_time("c_damage");
        }

        let mut subdoc_is_animating = false;
        for &node_id in &self.sub_document_nodes {
            let node = &mut self.nodes[node_id];
            let size = node.final_layout.size;
            if let Some(mut sub_doc) = node.subdoc_mut().map(|doc| doc.inner_mut()) {
                // Set viewport
                // viewport_mut handles change detection. So we just unconditionally set the values;
                let mut sub_viewport = sub_doc.viewport_mut();
                sub_viewport.hidpi_scale = self.viewport.hidpi_scale;
                sub_viewport.zoom = self.viewport.zoom;
                sub_viewport.color_scheme = self.viewport.color_scheme;

                let viewport_scale = self.viewport.scale();
                sub_viewport.window_size = (
                    (size.width * viewport_scale) as u32,
                    (size.height * viewport_scale) as u32,
                );
                drop(sub_viewport);

                sub_doc.resolve(current_time_for_animations);

                subdoc_is_animating |= sub_doc.is_animating();
            }
        }
        self.subdoc_is_animating = subdoc_is_animating;
        timer.record_time("subdocs");

        timer.print_times(&format!("Resolve({}): ", self.id()));
    }

    pub fn resolve_scroll_animation(&mut self) {
        match &mut self.scroll_animation {
            ScrollAnimationState::Fling(fling_state) => {
                let time_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64 as f64;

                let time_diff_ms = time_ms - fling_state.last_seen_time;

                // 0.95 @ 60fps normalized to actual frame times
                let deceleration = 1.0 - ((0.05 / 16.66666) * time_diff_ms);

                fling_state.x_velocity *= deceleration;
                fling_state.y_velocity *= deceleration;
                fling_state.last_seen_time = time_ms;
                let fling_state = fling_state.clone();

                let dx = fling_state.x_velocity * time_diff_ms;
                let dy = fling_state.y_velocity * time_diff_ms;

                self.scroll_by(Some(fling_state.target), dx, dy, &mut |_| {});
                if fling_state.x_velocity.abs() < 0.1 && fling_state.y_velocity.abs() < 0.1 {
                    self.scroll_animation = ScrollAnimationState::None;
                }
            }
            ScrollAnimationState::None => {
                // Do nothing
            }
        }
    }

    /// Ensure that the layout_children field is populated for all nodes
    pub fn resolve_layout_children(&mut self) {
        resolve_layout_children_recursive(self, self.root_node().id);

        fn resolve_layout_children_recursive(doc: &mut BaseDocument, node_id: usize) {
            let mut damage = doc.nodes[node_id].damage().unwrap_or(ALL_DAMAGE);
            let _flags = doc.nodes[node_id].flags;

            if NON_INCREMENTAL || damage.intersects(CONSTRUCT_FC | CONSTRUCT_BOX) {
                //} || flags.contains(NodeFlags::IS_INLINE_ROOT) {
                let mut layout_children = Vec::new();
                let mut anonymous_block: Option<usize> = None;
                collect_layout_children(doc, node_id, &mut layout_children, &mut anonymous_block);

                // Recurse into newly collected layout children
                for child_id in layout_children.iter().copied() {
                    resolve_layout_children_recursive(doc, child_id);
                    doc.nodes[child_id].layout_parent.set(Some(node_id));
                    if let Some(mut data) = doc.nodes[child_id].stylo_element_data.borrow_mut() {
                        data.damage.remove(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                    }
                }

                *doc.nodes[node_id].layout_children.borrow_mut() = Some(layout_children.clone());
                // *doc.nodes[node_id].paint_children.borrow_mut() = Some(layout_children);

                damage.remove(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                // damage.insert(RestyleDamage::RELAYOUT | RestyleDamage::REPAINT);
            } else {
                //if damage.contains(CONSTRUCT_DESCENDENT) {
                let layout_children = doc.nodes[node_id].layout_children.borrow_mut().take();
                if let Some(layout_children) = layout_children {
                    // Recurse into previously computed layout children
                    for child_id in layout_children.iter().copied() {
                        resolve_layout_children_recursive(doc, child_id);
                        doc.nodes[child_id].layout_parent.set(Some(node_id));
                    }

                    *doc.nodes[node_id].layout_children.borrow_mut() = Some(layout_children);
                }

                // damage.remove(CONSTRUCT_DESCENDENT);
                // damage.insert(RestyleDamage::RELAYOUT | RestyleDamage::REPAINT);
            }

            doc.nodes[node_id].set_damage(damage);
        }
    }

    /// Move absolutely/fixed-positioned children from their DOM parent's layout_children
    /// to their correct CSS containing block ancestor's layout_children.
    ///
    /// This ensures Taffy resolves insets and percentage sizes against the correct
    /// containing block dimensions (CSS2.1 §10.1).
    ///
    /// Sticky-positioned elements are intentionally NOT reparented — they participate
    /// in normal flow and their visual offset is computed at scroll-time.
    fn reparent_out_of_flow_children(&mut self) {
        use style::computed_values::position::T as CssPosition;

        // Collect reparenting operations: (child_id, old_parent_id, new_parent_id)
        let mut reparent_list: Vec<(usize, usize, usize)> = Vec::new();

        for (node_id, node) in self.nodes.iter() {
            let Some(style) = node.primary_styles() else {
                continue;
            };
            let position = style.clone_position();

            let is_abs = position == CssPosition::Absolute;
            let is_fixed = position == CssPosition::Fixed;
            if !is_abs && !is_fixed {
                continue;
            }

            let Some(current_parent) = node.layout_parent.get() else {
                continue;
            };

            let target = if is_fixed {
                self.find_fixed_containing_block(node_id)
            } else {
                self.find_absolute_containing_block(node_id)
            };

            if let Some(target) = target {
                if current_parent != target {
                    reparent_list.push((node_id, current_parent, target));
                }
            }
        }

        // Apply reparenting
        for (child_id, old_parent, new_parent) in reparent_list {
            if let Some(ref mut children) = *self.nodes[old_parent].layout_children.borrow_mut() {
                children.retain(|&id| id != child_id);
            }
            if let Some(ref mut children) = *self.nodes[new_parent].layout_children.borrow_mut() {
                children.push(child_id);
            }
            self.nodes[child_id].layout_parent.set(Some(new_parent));
        }
    }

    /// Find the containing block for an absolutely-positioned element.
    /// This is the nearest ancestor with position != static (CSS2.1 §10.1).
    fn find_absolute_containing_block(&self, node_id: usize) -> Option<usize> {
        let mut current = self.nodes[node_id].parent?;
        loop {
            if self.node_is_positioned(current) {
                return Some(current);
            }
            match self.nodes[current].parent {
                Some(p) => current = p,
                None => return Some(current), // root = initial containing block
            }
        }
    }

    /// Find the containing block for a fixed-position element.
    /// This is the nearest ancestor with transform/filter/perspective,
    /// or the root element (viewport) if none found.
    fn find_fixed_containing_block(&self, node_id: usize) -> Option<usize> {
        let mut current = self.nodes[node_id].parent?;
        loop {
            if self.node_creates_containing_block_for_fixed(current) {
                return Some(current);
            }
            match self.nodes[current].parent {
                Some(p) => current = p,
                None => return Some(current), // root = viewport
            }
        }
    }

    /// Returns true if the node has position != static (is "positioned").
    fn node_is_positioned(&self, node_id: usize) -> bool {
        use style::computed_values::position::T as CssPosition;
        self.nodes[node_id]
            .primary_styles()
            .map(|s| s.clone_position() != CssPosition::Static)
            .unwrap_or(false)
    }

    /// Returns true if the node creates a containing block for fixed-position descendants.
    /// Per CSS spec, this is triggered by transform, filter, or perspective.
    fn node_creates_containing_block_for_fixed(&self, node_id: usize) -> bool {
        let Some(style) = self.nodes[node_id].primary_styles() else {
            return false;
        };
        !style.get_box().transform.0.is_empty()
            || !style.get_effects().filter.0.is_empty()
        // TODO: perspective, will-change: transform/filter
    }

    pub fn resolve_deferred_tasks(&mut self) {
        let mut deferred_construction_nodes = std::mem::take(&mut self.deferred_construction_nodes);

        // Deduplicate deferred tasks by node_id to avoid redundant work
        deferred_construction_nodes.sort_unstable_by_key(|task| task.node_id);
        deferred_construction_nodes.dedup_by_key(|task| task.node_id);

        #[cfg(feature = "parallel-construct")]
        let iter = deferred_construction_nodes.into_par_iter();
        #[cfg(not(feature = "parallel-construct"))]
        let iter = deferred_construction_nodes.into_iter();

        let results: Vec<ConstructionTaskResult> = iter
            .map(|task: ConstructionTask| match task.data {
                ConstructionTaskData::InlineLayout(mut layout) => {
                    #[cfg(feature = "parallel-construct")]
                    let mut layout_ctx = LAYOUT_CTX
                        .take()
                        .unwrap_or_else(|| Box::new(LayoutContext::new()));
                    #[cfg(feature = "parallel-construct")]
                    let layout_ctx_mut = &mut layout_ctx;

                    #[cfg(feature = "parallel-construct")]
                    let mut font_ctx = self
                        .thread_font_contexts
                        .get_or(|| RefCell::new(Box::new(self.font_ctx.lock().unwrap().clone())))
                        .borrow_mut();
                    #[cfg(feature = "parallel-construct")]
                    let font_ctx_mut = &mut *font_ctx;

                    #[cfg(not(feature = "parallel-construct"))]
                    let layout_ctx_mut = &mut self.layout_ctx;
                    #[cfg(not(feature = "parallel-construct"))]
                    let font_ctx_mut = &mut *self.font_ctx.lock().unwrap();

                    layout.content_widths = None;
                    build_inline_layout_into(
                        &self.nodes,
                        layout_ctx_mut,
                        font_ctx_mut,
                        &mut layout,
                        self.viewport.scale(),
                        task.node_id,
                    );

                    #[cfg(feature = "parallel-construct")]
                    {
                        LAYOUT_CTX.set(Some(layout_ctx));
                    }

                    // If layout doesn't contain any inline boxes, then it is safe to populate the content_widths
                    // cache during this parallelized stage.
                    // if layout.layout.inline_boxes().is_empty() {
                    //     layout.content_widths();
                    // }

                    ConstructionTaskResult {
                        node_id: task.node_id,
                        data: ConstructionTaskResultData::InlineLayout(layout),
                    }
                }
            })
            .collect();

        for result in results {
            match result.data {
                ConstructionTaskResultData::InlineLayout(layout) => {
                    self.nodes[result.node_id].cache.clear();
                    self.nodes[result.node_id]
                        .element_data_mut()
                        .unwrap()
                        .inline_layout_data = Some(layout);
                }
            }
        }

        self.deferred_construction_nodes.clear();
    }

    /// Walk the nodes now that they're properly styled and transfer their styles to the taffy style system
    ///
    /// TODO: update taffy to use an associated type instead of slab key
    /// TODO: update taffy to support traited styles so we don't even need to rely on taffy for storage
    /// Collect all position:sticky nodes for efficient recomputation on scroll.
    fn collect_sticky_nodes(&mut self) {
        use style::computed_values::position::T as CssPosition;
        self.sticky_nodes.clear();
        for (node_id, node) in self.nodes.iter() {
            if node.css_position == CssPosition::Sticky {
                self.sticky_nodes.push(node_id);
            }
        }
    }

    /// Find the nearest scroll port ancestor (overflow != visible on either axis).
    /// Returns None if the viewport is the scroll port.
    /// Per CSS Overflow Level 3: overflow hidden/scroll/auto all establish scroll ports.
    fn find_scroll_port_ancestor(&self, node_id: usize) -> Option<usize> {
        use style::values::computed::Overflow;
        let mut current = self.nodes[node_id].parent?;
        loop {
            if let Some(style) = self.nodes[current].primary_styles() {
                let ox = style.clone_overflow_x();
                let oy = style.clone_overflow_y();
                if !matches!(ox, Overflow::Visible) || !matches!(oy, Overflow::Visible) {
                    return Some(current);
                }
            }
            match self.nodes[current].parent {
                Some(p) => current = p,
                None => return None,
            }
        }
    }

    /// Compute a node's position relative to an ancestor by walking the layout_parent chain.
    /// Accumulates final_layout.location and subtracts intermediate scroll_offsets.
    /// If ancestor_id is None, walks all the way to the root (for viewport case).
    fn position_relative_to_ancestor(
        &self,
        node_id: usize,
        ancestor_id: Option<usize>,
    ) -> crate::Point<f64> {
        let mut x = 0.0;
        let mut y = 0.0;
        let mut current = node_id;
        loop {
            let node = &self.nodes[current];
            x += node.final_layout.location.x as f64;
            y += node.final_layout.location.y as f64;
            match node.layout_parent.get() {
                Some(pid) if Some(pid) == ancestor_id => break,
                Some(pid) => {
                    x -= self.nodes[pid].scroll_offset.x;
                    y -= self.nodes[pid].scroll_offset.y;
                    current = pid;
                }
                None => break,
            }
        }
        crate::Point { x, y }
    }

    /// Compute the sticky offset for a single node.
    /// Implements CSS Positioned Layout Level 3 §3:
    /// - Element stays within its scroll port (viewport or overflow ancestor)
    /// - Clamped to its containing block (DOM parent) boundary
    fn compute_sticky_offset_for_node(&self, node_id: usize) -> crate::Point<f64> {
        let node = &self.nodes[node_id];
        let Some(styles) = node.primary_styles() else {
            return crate::Point::ZERO;
        };

        let pos_style = styles.get_position();
        let element_width = node.final_layout.size.width as f64;
        let element_height = node.final_layout.size.height as f64;

        // Find scroll port ancestor (or viewport)
        let scroll_port_id = self.find_scroll_port_ancestor(node_id);

        // Get scroll state and port dimensions
        let (scroll, port_width, port_height) = match scroll_port_id {
            Some(sp_id) => {
                let sp = &self.nodes[sp_id];
                (
                    sp.scroll_offset,
                    sp.final_layout.size.width as f64,
                    sp.final_layout.size.height as f64,
                )
            }
            None => {
                let w = self.viewport.window_size.0 as f64 / self.viewport.scale() as f64;
                let h = self.viewport.window_size.1 as f64 / self.viewport.scale() as f64;
                (self.viewport_scroll, w, h)
            }
        };

        // Compute element's normal-flow position relative to scroll port
        let node_pos = self.position_relative_to_ancestor(node_id, scroll_port_id);

        // Resolve sticky thresholds from Stylo computed styles
        let top = resolve_sticky_inset(&pos_style.top, port_height);
        let bottom = resolve_sticky_inset(&pos_style.bottom, port_height);
        let left = resolve_sticky_inset(&pos_style.left, port_width);
        let right = resolve_sticky_inset(&pos_style.right, port_width);

        // Y axis: compute visible position and clamp to thresholds
        let visible_y = node_pos.y - scroll.y;
        let mut min_y = f64::NEG_INFINITY;
        let mut max_y = f64::INFINITY;
        if let Some(t) = top {
            min_y = t;
        }
        if let Some(b) = bottom {
            max_y = port_height - b - element_height;
        }
        // Top wins over bottom per CSS spec when they conflict
        if min_y > max_y {
            max_y = min_y;
        }
        let clamped_y = visible_y.clamp(min_y, max_y);
        let mut offset_y = clamped_y - visible_y;

        // X axis: compute visible position and clamp to thresholds
        let visible_x = node_pos.x - scroll.x;
        let mut min_x = f64::NEG_INFINITY;
        let mut max_x = f64::INFINITY;
        if let Some(l) = left {
            min_x = l;
        }
        if let Some(r) = right {
            max_x = port_width - r - element_width;
        }
        // Left wins over right per CSS spec when they conflict
        if min_x > max_x {
            max_x = min_x;
        }
        let clamped_x = visible_x.clamp(min_x, max_x);
        let mut offset_x = clamped_x - visible_x;

        // Clamp to containing block (DOM parent): element must stay within parent's box
        if let Some(parent_id) = node.parent {
            let parent = &self.nodes[parent_id];
            let parent_pos = self.position_relative_to_ancestor(parent_id, scroll_port_id);
            let parent_height = parent.final_layout.scroll_height() as f64;
            let parent_width = parent.final_layout.scroll_width() as f64;

            let cb_min_y = parent_pos.y - node_pos.y;
            let cb_max_y = (parent_pos.y + parent_height - element_height - node_pos.y)
                .max(cb_min_y);
            offset_y = offset_y.clamp(cb_min_y, cb_max_y);

            let cb_min_x = parent_pos.x - node_pos.x;
            let cb_max_x = (parent_pos.x + parent_width - element_width - node_pos.x)
                .max(cb_min_x);
            offset_x = offset_x.clamp(cb_min_x, cb_max_x);
        }

        crate::Point {
            x: offset_x,
            y: offset_y,
        }
    }

    /// Recompute sticky offsets for all sticky nodes.
    /// Called after layout and after every scroll event.
    pub fn recompute_sticky_offsets(&mut self) {
        let sticky_nodes = std::mem::take(&mut self.sticky_nodes);
        for &node_id in &sticky_nodes {
            let offset = self.compute_sticky_offset_for_node(node_id);
            self.nodes[node_id].sticky_offset = offset;
        }
        self.sticky_nodes = sticky_nodes;
    }

    pub fn resolve_layout(&mut self) {
        let size = self.stylist.device().au_viewport_size();

        let available_space = taffy::Size {
            width: AvailableSpace::Definite(size.width.to_f32_px()),
            height: AvailableSpace::Definite(size.height.to_f32_px()),
        };

        let root_element_id = taffy::NodeId::from(self.root_element().id);

        // println!("\n\nRESOLVE LAYOUT\n===========\n");

        taffy::compute_root_layout(self, root_element_id, available_space);
        taffy::round_layout(self, root_element_id);

        // println!("\n\n");
        // taffy::print_tree(self, root_node_id)
    }
}

/// Resolve a sticky inset threshold (top/bottom/left/right) from Stylo computed values to pixels.
/// Uses LengthPercentage::resolve() which handles length, percentage, and calc uniformly.
/// Returns None for `auto` values.
fn resolve_sticky_inset(
    val: &style::values::generics::position::GenericInset<
        style::values::computed::Percentage,
        style::values::computed::LengthPercentage,
    >,
    reference_size: f64,
) -> Option<f64> {
    use style::values::computed::Length;
    use style::values::generics::position::GenericInset;
    match val {
        GenericInset::LengthPercentage(lp) => {
            Some(lp.resolve(Length::new(reference_size as f32)).px() as f64)
        }
        _ => None, // Auto or anchor positioning
    }
}
