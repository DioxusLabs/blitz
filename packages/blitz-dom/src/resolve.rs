//! Resolve style and layout

use blitz_traits::node_id::NodeId;
use std::{
    cell::RefCell,
    time::{SystemTime, UNIX_EPOCH},
};

use debug_timer::debug_timer;
use kurbo::{Affine, Rect};
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

use style::selector_parser::RestyleDamage;
use taffy::AvailableSpace;

use crate::{
    BaseDocument,
    events::ScrollAnimationState,
    layout::{
        construct::{
            ConstructionTask, ConstructionTaskData, ConstructionTaskResult,
            ConstructionTaskResultData, LayoutChildren, build_inline_layout_into,
            collect_layout_children,
        },
        damage::{ALL_DAMAGE, CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC},
    },
    node::TextBrush,
};

impl BaseDocument {
    /// Restyle the tree and then relayout it
    pub fn resolve(&mut self, current_time_for_animations: f64) {
        if TDocument::as_node(&self.root_node())
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

        // Drop scrollbar-activity entries whose fade-out has finished (also
        // sheds entries for removed nodes).
        {
            use crate::node::scrollbar::{FADE_DELAY, FADE_DURATION};
            self.scrollbar_activity
                .retain(|_, last| last.elapsed() < FADE_DELAY + FADE_DURATION);
        }

        let root_node_id = self.root_element().id;
        debug_timer!(timer, feature = "log-phase-times");

        // we need to resolve stylist first since it will need to drive our layout bits
        self.resolve_stylist(current_time_for_animations);
        timer.record_time("style");

        // Propagate damage flags (from mutation and restyles) up and down the tree
        if self.incremental_layout {
            self.propagate_damage_flags(root_node_id, RestyleDamage::empty());
            timer.record_time("damage");
        }

        // Fix up tree for layout (insert anonymous blocks as necessary, etc)
        self.resolve_layout_children();
        timer.record_time("construct");

        self.resolve_deferred_tasks();
        timer.record_time("pconstruct");

        // Merge stylo into taffy
        self.flush_styles_to_layout(root_node_id);
        timer.record_time("flush");

        // Next we resolve layout with the data resolved by stlist
        self.resolve_layout();
        timer.record_time("layout");

        // Refresh hoisted paint child positions now that layout is final
        self.refresh_hoisted_paint_positions();

        // Resolve transforms
        self.resolve_transforms(root_node_id);
        timer.record_time("transform");

        // Clear all damage and dirty flags
        if self.incremental_layout {
            for (_, node) in self.nodes.iter_mut() {
                node.clear_damage_mut();
                node.unset_dirty_descendants();
            }
            timer.record_time("c_damage");
        }

        // Re-resolve the hover node from the pointer position against the fresh
        // layout. This must run *after* the damage/dirty flags are cleared
        // above, so that the restyle hint and ancestor `dirty_descendants`
        // flags set by any resulting hover change survive into the next resolve
        // pass (the clearing loop would otherwise wipe them). Any resulting
        // restyle is picked up on the next resolve pass; a redraw is requested
        // if the hovered node actually changes.
        self.refresh_hover();

        let mut subdoc_is_animating = false;
        for &node_id in &self.sub_document_nodes {
            let node = &mut self.nodes[node_id];
            let size = node.final_layout().size;
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

    fn resolve_transforms(&mut self, node_id: NodeId) -> Rect {
        if !self.nodes.contains_key(node_id) {
            return Rect::ZERO;
        }

        if !self.nodes[node_id]
            .damage()
            .map(|d| d.contains(style::selector_parser::RestyleDamage::RECALCULATE_OVERFLOW))
            .unwrap_or(false)
        {
            return *self.nodes[node_id].scrollable_overflow();
        }

        let scale = self.viewport.scale_f64();

        let transform = self.nodes[node_id].set_transform(scale as f32);

        let w = self.nodes[node_id].final_layout().size.width as f64 * scale;
        let h = self.nodes[node_id].final_layout().size.height as f64 * scale;
        let mut overflow = Rect::new(0.0, 0.0, w, h);

        let layout_children = std::mem::take(self.nodes[node_id].layout_children.get_mut());

        if let Some(ref children) = layout_children {
            for &child_id in children {
                let child_rect_in_self = self.resolve_transforms(child_id);
                // Fixed position children do not contribute to their ancestors'
                // scrollable overflow
                let child_is_fixed =
                    self.nodes[child_id].style().position == taffy::Position::Fixed;
                if !child_is_fixed {
                    overflow = overflow.union(child_rect_in_self);
                }
            }
        }
        if let Some(before) = self.nodes[node_id].before() {
            let child_rect_in_self = self.resolve_transforms(before);
            overflow = overflow.union(child_rect_in_self);
        }
        if let Some(after) = self.nodes[node_id].after() {
            let child_rect_in_self = self.resolve_transforms(after);
            overflow = overflow.union(child_rect_in_self);
        }

        *self.nodes[node_id].scrollable_overflow_mut() = overflow;
        *self.nodes[node_id].layout_children.get_mut() = layout_children;

        let scaled_x = self.nodes[node_id].final_layout().location.x as f64 * scale;
        let scaled_y = self.nodes[node_id].final_layout().location.y as f64 * scale;

        let full = if let Some(t) = transform {
            Affine::translate((scaled_x, scaled_y)) * t
        } else {
            Affine::translate((scaled_x, scaled_y))
        };

        full.transform_rect_bbox(overflow)
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

        fn resolve_layout_children_recursive(doc: &mut BaseDocument, node_id: NodeId) {
            // Anonymous blocks and pseudo-elements can be removed from the slab
            // between render passes. Bail out rather than panicking on a stale key.
            if doc.nodes.get(node_id).is_none() {
                return;
            }

            let mut damage = doc.nodes[node_id].damage().unwrap_or(ALL_DAMAGE);
            let _flags = doc.nodes[node_id].flags;

            if !doc.incremental_layout || damage.intersects(CONSTRUCT_FC | CONSTRUCT_BOX) {
                //} || flags.contains(NodeFlags::IS_INLINE_ROOT) {

                // Deallocate the anonymous blocks created for this node in the
                // previous construction round. They live only in the slab, so
                // reconstructing without freeing them would leak a slab entry per
                // anonymous block per reconstruction.
                let old_anonymous_blocks = std::mem::take(&mut doc.nodes[node_id].anonymous_blocks);
                for anon_id in old_anonymous_blocks {
                    doc.deallocate_anonymous_block(anon_id);
                }

                let mut collected = LayoutChildren::default();
                collect_layout_children(doc, node_id, &mut collected);
                let layout_children = collected.children;
                doc.nodes[node_id].anonymous_blocks = collected.anonymous_blocks;

                // Recurse into newly collected layout children
                for child_id in layout_children.iter().copied() {
                    resolve_layout_children_recursive(doc, child_id);
                    doc.nodes[child_id].layout_parent.set(Some(node_id));
                    if let Some(mut data) = doc.nodes[child_id]
                        .stylo_element_data_opt_mut()
                        .and_then(|s| s.get_mut())
                    {
                        data.damage
                            .remove(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
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
                    for child_id in layout_children.iter().copied() {
                        // Anonymous blocks and pseudo-elements can be removed from the
                        // slab between render passes; skip stale IDs.
                        if !doc.nodes.contains_key(child_id) {
                            continue;
                        }
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
                    self.nodes[result.node_id].cache_mut().clear();
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
    pub fn resolve_layout(&mut self) {
        let size = self.stylist.device().au_viewport_size();

        let available_space = taffy::Size {
            width: AvailableSpace::Definite(size.width.to_f32_px()),
            height: AvailableSpace::Definite(size.height.to_f32_px()),
        };

        let root_element_id = crate::taffy_node_id(self.root_element().id);

        // println!("\n\nRESOLVE LAYOUT\n===========\n");

        taffy::compute_root_layout(self, root_element_id, available_space);
        self.position_deferred_children();
        taffy::round_layout(self, root_element_id);

        // println!("\n\n");
        // taffy::print_tree(self, root_node_id)
    }

    /// Lay out out-of-flow children which were deferred during the main layout pass (via
    /// [`taffy::LayoutPartialTree::defer_absolute_child`]) against their actual containing block:
    ///
    /// - `position: absolute` children of non-positioned containers are positioned against
    ///   their nearest positioned (or transformed) ancestor, falling back to the initial
    ///   containing block (the viewport).
    /// - `position: fixed` children are positioned against their nearest transformed ancestor,
    ///   falling back to the initial containing block (the viewport).
    ///
    /// This runs after `compute_root_layout` (so all in-flow layout and static positions are
    /// resolved) and before `round_layout` (so the final positions get rounded normally).
    fn position_deferred_children(&mut self) {
        use taffy::{Point, Position};

        /// A containing block for out-of-flow boxes, described relative to the border box of
        /// the node establishing it.
        #[derive(Copy, Clone)]
        struct Cb {
            node_id: NodeId,
            area_size: taffy::Size<f32>,
            area_offset: taffy::Point<f32>,
        }

        fn cb_from_node(doc: &BaseDocument, node_id: NodeId) -> Cb {
            let layout = *doc.nodes[node_id].unrounded_layout();
            let is_rtl = doc.nodes[node_id].style().direction.is_rtl();
            let area_size = taffy::Size {
                width: layout.size.width
                    - layout.border.left
                    - layout.border.right
                    - layout.scrollbar_size.width,
                height: layout.size.height
                    - layout.border.top
                    - layout.border.bottom
                    - layout.scrollbar_size.height,
            };
            let area_offset = taffy::Point {
                x: if is_rtl {
                    layout.border.left + layout.scrollbar_size.width
                } else {
                    layout.border.left
                },
                y: layout.border.top,
            };
            Cb {
                node_id,
                area_size,
                area_offset,
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn recurse(
            doc: &mut BaseDocument,
            node_id: NodeId,
            fixed_cb: Cb,
            abs_cb: Cb,
            offset_from_fixed_cb: taffy::Point<f32>,
            offset_from_abs_cb: taffy::Point<f32>,
        ) {
            let node_position = doc.nodes[node_id].style().position;

            let layout_children = doc.nodes[node_id].layout_children.borrow().clone();
            let mut child_ids: Vec<NodeId> = layout_children.map(|c| c.to_vec()).unwrap_or_default();
            if let Some(before) = doc.nodes[node_id].before() {
                child_ids.push(before);
            }
            if let Some(after) = doc.nodes[node_id].after() {
                child_ids.push(after);
            }

            for child_id in child_ids {
                if !doc.nodes.contains_key(child_id) {
                    continue;
                }
                let child_position = doc.nodes[child_id].style().position;

                let is_deferred = child_position == Position::Fixed
                    || (child_position == Position::Absolute && node_position == Position::Static);

                if is_deferred {
                    if let Some((order, static_position)) = *doc.nodes[child_id].deferred_position()
                    {
                        let (cb, parent_offset) = if child_position == Position::Fixed {
                            (fixed_cb, offset_from_fixed_cb)
                        } else {
                            (abs_cb, offset_from_abs_cb)
                        };
                        let direction = doc.nodes[cb.node_id].style().direction;
                        let static_position_in_cb = Point {
                            x: static_position.x + parent_offset.x,
                            y: static_position.y + parent_offset.y,
                        };
                        let result = taffy::compute_absolute_child_layout(
                            doc,
                            crate::taffy_node_id(child_id),
                            order,
                            cb.area_size,
                            cb.area_offset,
                            static_position_in_cb,
                            direction,
                        );
                        // `compute_absolute_child_layout` writes a location relative to the
                        // containing block's border box. Convert it to be relative to the layout
                        // parent's border box (the coordinate space layouts are stored in).
                        let mut layout = result.layout;
                        layout.location.x -= parent_offset.x;
                        layout.location.y -= parent_offset.y;
                        *doc.nodes[child_id].unrounded_layout_mut() = layout;
                    }
                }

                // Compute the containing blocks and offsets that apply to the child's descendants
                let child_layout = *doc.nodes[child_id].unrounded_layout();
                let child_establishes_fixed_cb =
                    doc.nodes[child_id].establishes_fixed_containing_block();
                let child_is_positioned =
                    child_position != Position::Static || child_establishes_fixed_cb;

                let child_fixed_cb;
                let child_offset_from_fixed_cb;
                if child_establishes_fixed_cb {
                    child_fixed_cb = cb_from_node(doc, child_id);
                    child_offset_from_fixed_cb = Point::ZERO;
                } else {
                    child_fixed_cb = fixed_cb;
                    child_offset_from_fixed_cb = Point {
                        x: offset_from_fixed_cb.x + child_layout.location.x,
                        y: offset_from_fixed_cb.y + child_layout.location.y,
                    };
                }

                let child_abs_cb;
                let child_offset_from_abs_cb;
                if child_is_positioned {
                    child_abs_cb = cb_from_node(doc, child_id);
                    child_offset_from_abs_cb = Point::ZERO;
                } else {
                    child_abs_cb = abs_cb;
                    child_offset_from_abs_cb = Point {
                        x: offset_from_abs_cb.x + child_layout.location.x,
                        y: offset_from_abs_cb.y + child_layout.location.y,
                    };
                }

                recurse(
                    doc,
                    child_id,
                    child_fixed_cb,
                    child_abs_cb,
                    child_offset_from_fixed_cb,
                    child_offset_from_abs_cb,
                );
            }
        }

        let viewport_size = self.stylist.device().au_viewport_size();
        let root_id = self.root_element().id;

        // The initial containing block: the viewport. Offsets are tracked relative to the root
        // element's border box, which is positioned at the viewport origin.
        let viewport_cb = Cb {
            node_id: root_id,
            area_size: taffy::Size {
                width: viewport_size.width.to_f32_px(),
                height: viewport_size.height.to_f32_px(),
            },
            area_offset: taffy::Point::ZERO,
        };

        recurse(
            self,
            root_id,
            viewport_cb,
            viewport_cb,
            Point::ZERO,
            Point::ZERO,
        );
    }
}
