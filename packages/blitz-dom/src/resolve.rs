//! Resolve style and layout

use blitz_traits::node_id::NodeId;
use std::cell::RefCell;

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

        // While render-blocking resources (e.g. stylesheets linked from the `<head>`) are
        // still loading, don't resolve styles or layout (matching how browsers block
        // rendering). Resolving styles before the document's stylesheets have loaded would
        // give elements computed styles based on an incomplete cascade, and a later restyle
        // (once the stylesheet loads) would treat those as genuine "before-change styles",
        // spuriously starting CSS transitions from unstyled values. See issue #689.
        //
        // `handle_messages` above must still run so that loaded resources are ingested and
        // this state can clear.
        if self.has_pending_critical_resources() {
            return;
        }

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

        // Apply any device changes (viewport resize, zoom, color-scheme, etc)
        // accumulated since the last resolve as a single device rebuild.
        self.flush_pending_device_changes();

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
        // Flush background/mask images from style to dedicated storage on the
        // nodes whose style changed (queued by the style traversal and by
        // pseudo-element box construction), fetching any not-yet-loaded images.
        self.flush_pending_style_images();
        timer.record_time("pconstruct");

        // Merge stylo into taffy
        self.flush_styles_to_layout(root_node_id);
        timer.record_time("flush");

        // Next we resolve layout with the data resolved by stlist
        self.resolve_layout();
        timer.record_time("layout");

        // Attach out-of-flow boxes to their containing block for painting and
        // hit-testing, and repoint their layout_parent at the containing block
        self.attach_hoisted_children();

        // Resolve transforms
        self.resolve_transforms(root_node_id);
        timer.record_time("transform");

        // Clear all damage and dirty flags, walking only subtrees which are
        // marked as (potentially) containing damage.
        if self.incremental_layout {
            let doc_node_id = self.root_node().id;
            self.clear_damage_and_dirty_flags(doc_node_id);
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

        let scale = self.viewport.scale_f64();

        if !self.nodes[node_id]
            .damage()
            .map(|d| d.contains(style::selector_parser::RestyleDamage::RECALCULATE_OVERFLOW))
            .unwrap_or(false)
        {
            let node = &self.nodes[node_id];
            let location = node.final_layout().location.map(|v| v as f64 * scale);

            let mut transform = Affine::translate((location.x, location.y));
            if let Some(t) = node.transform() {
                transform *= *t
            }

            let overflow = *node.scrollable_overflow();
            return transform.transform_rect_bbox(overflow);
        }

        let transform = self.nodes[node_id].set_transform(scale as f32);

        let w = self.nodes[node_id].final_layout().size.width as f64 * scale;
        let h = self.nodes[node_id].final_layout().size.height as f64 * scale;
        let mut overflow = Rect::new(0.0, 0.0, w, h);

        let layout_children = std::mem::take(self.nodes[node_id].layout_children.get_mut());

        if let Some(ref children) = layout_children {
            for &child_id in children {
                // Out-of-flow children are laid out relative to their containing
                // block, not their DOM parent: their overflow contribution is
                // accounted for at the containing block (below) instead.
                let is_out_of_flow = self.nodes[child_id].style().position.is_out_of_flow();
                let child_rect_in_self = self.resolve_transforms(child_id);
                if !is_out_of_flow {
                    overflow = overflow.union(child_rect_in_self);
                }
            }
        }
        let hoisted_children =
            std::mem::take(&mut *self.nodes[node_id].hoisted_children.borrow_mut());
        for &child_id in &hoisted_children {
            if !self.nodes.contains_key(child_id) {
                continue;
            }
            let child_rect_in_self = self.resolve_transforms(child_id);
            overflow = overflow.union(child_rect_in_self);
        }
        *self.nodes[node_id].hoisted_children.borrow_mut() = hoisted_children;
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

    /// Attach out-of-flow (absolutely/fixed positioned) boxes to their containing
    /// block, as recorded by Taffy's out-of-flow positioning pass in each node's
    /// `hoisted_children` list:
    ///
    /// - repoints each hoisted box's `layout_parent` at its containing block so
    ///   that coordinate accumulation (e.g. `absolute_position`) follows the
    ///   containing block chain that its `Layout.location` is relative to; and
    /// - appends each hoisted box to its containing block's `paint_children` so
    ///   that paint and hit-testing visit it with the correct coordinates
    ///   (out-of-flow boxes are skipped in their DOM parent's paint list).
    fn attach_hoisted_children(&mut self) {
        let mut modified_stacking_roots: Vec<NodeId> = Vec::new();
        let mut pairs: Vec<(NodeId, Vec<NodeId>)> = Vec::new();
        for (cb_id, node) in self.nodes.iter() {
            let hoisted = node.hoisted_children.borrow();
            if !hoisted.is_empty() {
                pairs.push((cb_id, hoisted.iter().copied().collect()));
            }
        }

        for (cb_id, hoisted) in pairs {
            // Nodes can be removed from the slab between layout passes; drop stale ids
            let mut valid: Vec<NodeId> = hoisted
                .into_iter()
                .filter(|id| self.nodes.contains_key(*id))
                .collect();
            // Sort by z-index (stable sort preserves document order within a z-index)
            valid.sort_by_key(|id| self.nodes[*id].z_index());

            for &child_id in &valid {
                self.nodes[child_id].layout_parent.set(Some(cb_id));
            }

            // Boxes whose containing block is their direct layout parent were kept
            // in its paint tree by `flush_styles_to_layout` (paint_children or the
            // stacking context, depending on z-index) in tree order, so only append
            // the ones hoisted past their DOM parent.
            let direct_layout_children: Vec<NodeId> = self.nodes[cb_id]
                .layout_children
                .borrow()
                .as_ref()
                .map(|c| c.to_vec())
                .unwrap_or_default();

            let mut z_indexed: Vec<NodeId> = Vec::new();
            {
                let mut paint_children = self.nodes[cb_id].paint_children.borrow_mut();
                let paint_children = paint_children.get_or_insert_with(thin_vec::ThinVec::new);
                for &child_id in &valid {
                    if self.nodes[child_id].z_index() != 0 {
                        z_indexed.push(child_id);
                        continue;
                    }
                    if direct_layout_children.contains(&child_id) {
                        continue;
                    }
                    if !paint_children.contains(&child_id) {
                        paint_children.push(child_id);
                    }
                }
            }

            // Children with a z-index belong to the nearest stacking context at
            // or above the containing block: hoist them there (matching
            // `flush_styles_to_layout`'s z-index hoisting), with their position
            // recorded relative to the stacking context root.
            for child_id in z_indexed {
                let mut position = taffy::Point::<f32>::ZERO;
                let mut sc_root = cb_id;
                while self.nodes[sc_root].stacking_context.is_none() {
                    let node = &self.nodes[sc_root];
                    let location = node.final_layout().location;
                    let scroll = *node.scroll_offset();
                    position.x += location.x - scroll.x as f32;
                    position.y += location.y - scroll.y as f32;
                    let Some(parent) = node.layout_parent.get() else {
                        break;
                    };
                    sc_root = parent;
                }

                let z_index = self.nodes[child_id].z_index();
                if let Some(sc) = self.nodes[sc_root].stacking_context.as_mut() {
                    if !sc.children.iter().any(|c| c.node_id == child_id) {
                        sc.children.push(crate::layout::damage::HoistedPaintChild {
                            node_id: child_id,
                            z_index,
                            position,
                        });
                        modified_stacking_roots.push(sc_root);
                    }
                }
            }
        }

        modified_stacking_roots.sort_unstable();
        modified_stacking_roots.dedup();
        for sc_root in modified_stacking_roots {
            let mut sc = self.nodes[sc_root].stacking_context.take();
            if let Some(sc) = sc.as_mut() {
                sc.sort();
                sc.compute_content_size(self);
            }
            self.nodes[sc_root].stacking_context = sc;
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
                        .try_stylo_element_data_mut()
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
                    self.nodes[result.node_id].clear_layout_cache();
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
        taffy::round_layout(self, root_element_id);

        // println!("\n\n");
        // taffy::print_tree(self, root_node_id)
    }
}
