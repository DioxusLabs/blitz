use crate::Node;
use crate::net::ResourceHandler;
use crate::node::NodeFlags;
use crate::{
    BaseDocument, net::ImageHandler, node::ImageResourceData, node::Status, util::ImageLayerKind,
};
use blitz_traits::node_id::NodeId;
use style::properties::ComputedValues;
use style::selector_parser::RestyleDamage;
use style::url::ComputedUrl;
use style::values::computed::Float;
use style::values::generics::image::Image as StyloImage;
use style::values::specified::align::AlignFlags;
use style::values::specified::box_::DisplayInside;
use style::values::specified::box_::DisplayOutside;
use thin_vec::ThinVec;

pub(crate) const CONSTRUCT_BOX: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0001_0000);
pub(crate) const CONSTRUCT_FC: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0010_0000);
pub(crate) const CONSTRUCT_DESCENDENT: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0100_0000);

pub(crate) const ONLY_RELAYOUT: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0000_1000);

pub(crate) const ALL_DAMAGE: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0111_1111);

impl BaseDocument {
    pub(crate) fn propagate_damage_flags(
        &mut self,
        node_id: NodeId,
        damage_from_parent: RestyleDamage,
    ) -> RestyleDamage {
        let mut damage = if let Some(data) = self.nodes[node_id]
            .try_stylo_element_data_mut()
            .and_then(|s| s.get_mut())
        {
            data.damage
        } else {
            return RestyleDamage::empty();
        };
        damage |= damage_from_parent;
        if damage.contains(RestyleDamage::REBUILD_STACKING_CONTEXT)
            || damage.intersects(CONSTRUCT_BOX | CONSTRUCT_FC | CONSTRUCT_DESCENDENT)
        {
            self.nodes[node_id].stacking_dirty_self.set(true);
        }

        // Skip subtrees which contain no damage. Anonymous nodes are never
        // skipped themselves because damage marking walks the DOM parent
        // chain, which bypasses anonymous boxes: a damaged node's flagged
        // ancestors may reach it only through an unflagged anonymous wrapper.
        {
            let node = &self.nodes[node_id];
            if damage.is_empty() && !node.has_damaged_descendants() && !node.is_anonymous() {
                return RestyleDamage::empty();
            }
        }

        let damage_for_children = RestyleDamage::empty();
        let children = std::mem::take(&mut self.nodes[node_id].children);
        let layout_children = std::mem::take(self.nodes[node_id].layout_children.get_mut());
        let use_layout_children = self.nodes[node_id].should_traverse_layout_children();
        if use_layout_children {
            let layout_children = layout_children.as_ref().unwrap();
            for child in layout_children.iter() {
                damage |= self.propagate_damage_flags(*child, damage_for_children);
            }
        } else {
            for child in children.iter() {
                damage |= self.propagate_damage_flags(*child, damage_for_children);
            }
            if let Some(before_id) = self.nodes[node_id].before() {
                damage |= self.propagate_damage_flags(before_id, damage_for_children);
            }
            if let Some(after_id) = self.nodes[node_id].after() {
                damage |= self.propagate_damage_flags(after_id, damage_for_children);
            }
        }

        let node = &mut self.nodes[node_id];

        // Put children back
        node.children = children;
        *node.layout_children.get_mut() = layout_children;

        if damage.contains(CONSTRUCT_BOX) {
            damage.insert(RestyleDamage::RELAYOUT);
        }

        // Compute damage to propagate to parent
        let damage_for_parent = damage; // & RestyleDamage::RELAYOUT;

        // If the node or any of it's children have been mutated or their layout styles
        // have changed, then we should clear it's layout cache.
        if damage.intersects(ONLY_RELAYOUT | CONSTRUCT_BOX) {
            node.clear_layout_cache();
            if let Some(inline_layout) = node
                .data
                .downcast_element_mut()
                .and_then(|el| el.inline_layout_data.as_mut())
            {
                inline_layout.content_widths = None;
            }
            damage.remove(ONLY_RELAYOUT);
        }

        // Store damage for current node
        node.set_damage(damage);

        // let _is_fc_root = node
        //     .primary_styles()
        //     .map(|s| is_fc_root(&s))
        //     .unwrap_or(false);

        // if damage.contains(CONSTRUCT_BOX) {
        //     // damage_for_parent.insert(CONSTRUCT_FC | CONSTRUCT_DESCENDENT);
        //     damage_for_parent.insert(CONSTRUCT_BOX);
        // }

        // if damage.contains(CONSTRUCT_FC) {
        //     damage_for_parent.insert(CONSTRUCT_DESCENDENT);
        //     // if !is_fc_root {
        //     damage_for_parent.insert(CONSTRUCT_FC);
        //     // }
        // }

        // Propagate damage to parent
        damage_for_parent
    }

    /// Clear damage and the `damaged_descendants`/`dirty_descendants` flags
    /// on all nodes which may carry them, using the `damaged_descendants`
    /// flags to skip clean subtrees (mirroring `propagate_damage_flags`).
    pub(crate) fn clear_damage_and_dirty_flags(&mut self, node_id: NodeId) {
        {
            let node = &self.nodes[node_id];
            let has_damage = node.damage().is_some_and(|d| !d.is_empty());
            if !has_damage && !node.has_damaged_descendants() && !node.is_anonymous() {
                return;
            }
        }

        let children = std::mem::take(&mut self.nodes[node_id].children);
        let layout_children = std::mem::take(self.nodes[node_id].layout_children.get_mut());
        for child in children.iter() {
            self.clear_damage_and_dirty_flags(*child);
        }
        if let Some(layout_children) = layout_children.as_ref() {
            for child in layout_children.iter() {
                self.clear_damage_and_dirty_flags(*child);
            }
        }
        if let Some(before_id) = self.nodes[node_id].before() {
            self.clear_damage_and_dirty_flags(before_id);
        }
        if let Some(after_id) = self.nodes[node_id].after() {
            self.clear_damage_and_dirty_flags(after_id);
        }

        let node = &mut self.nodes[node_id];
        node.children = children;
        *node.layout_children.get_mut() = layout_children;
        node.clear_damage_mut();
        node.unset_damaged_descendants();
        node.unset_dirty_descendants();
        node.stacking_dirty_self.set(false);
        node.spatial_dirty_self.set(false);
    }
}

// fn is_fc_root(style: &ComputedValues) -> bool {
//     let display = style.clone_display();
//     let display_inside = display.inside();

//     match display_inside {
//         DisplayInside::Flow => {
//             // Depends on parent context
//             false
//         }

//         DisplayInside::None => true,
//         DisplayInside::FlowRoot => true,
//         DisplayInside::Flex => true,
//         DisplayInside::Grid => true,
//         DisplayInside::Table => true,
//         DisplayInside::TableCell => true,

//         DisplayInside::Contents => false,
//         DisplayInside::TableRowGroup => false,
//         DisplayInside::TableColumn => false,
//         DisplayInside::TableColumnGroup => false,
//         DisplayInside::TableHeaderGroup => false,
//         DisplayInside::TableFooterGroup => false,
//         DisplayInside::TableRow => false,
//     }
// }

pub(crate) fn compute_layout_damage(old: &ComputedValues, new: &ComputedValues) -> RestyleDamage {
    let box_tree_needs_rebuild = || {
        let old_box = old.get_box();
        let new_box = new.get_box();

        if old_box.display != new_box.display
            || old_box.float != new_box.float
            || old_box.position != new_box.position
            || old_box.contain != new_box.contain
            || old.clone_visibility() != new.clone_visibility()
        {
            return true;
        }

        if old.get_font() != new.get_font() {
            return true;
        }

        if new_box.display.outside() == DisplayOutside::Block
            && new_box.display.inside() == DisplayInside::Flow
        {
            let alignment_establishes_new_block_formatting_context = |style: &ComputedValues| {
                style.get_position().align_content.primary() != AlignFlags::NORMAL
            };

            let old_column = old.get_column();
            let new_column = new.get_column();
            if old_box.overflow_x.is_scrollable() != new_box.overflow_x.is_scrollable()
                || old_column.is_multicol() != new_column.is_multicol()
                || old_column.column_span != new_column.column_span
                || alignment_establishes_new_block_formatting_context(old)
                    != alignment_establishes_new_block_formatting_context(new)
            {
                return true;
            }
        }

        if old_box.display.is_list_item() {
            let old_list = old.get_list();
            let new_list = new.get_list();
            if old_list.list_style_position != new_list.list_style_position
                || old_list.list_style_image != new_list.list_style_image
                || (new_list.list_style_image == StyloImage::None
                    && old_list.list_style_type != new_list.list_style_type)
            {
                return true;
            }
        }

        if new.is_pseudo_style() && old.get_counters().content != new.get_counters().content {
            return true;
        }

        false
    };

    let text_shaping_needs_recollect = || {
        if old.clone_direction() != new.clone_direction()
            || old.clone_unicode_bidi() != new.clone_unicode_bidi()
        {
            return true;
        }

        let old_text = old.get_inherited_text();
        let new_text = new.get_inherited_text();
        if !std::ptr::eq(old_text, new_text)
            && (old_text.white_space_collapse != new_text.white_space_collapse
                || old_text.text_transform != new_text.text_transform
                || old_text.word_break != new_text.word_break
                || old_text.overflow_wrap != new_text.overflow_wrap
                || old_text.letter_spacing != new_text.letter_spacing
                || old_text.word_spacing != new_text.word_spacing
                || old_text.text_rendering != new_text.text_rendering)
        {
            return true;
        }

        false
    };

    #[allow(
        clippy::if_same_then_else,
        reason = "these branches will soon be different"
    )]
    if box_tree_needs_rebuild() {
        ALL_DAMAGE
    } else if text_shaping_needs_recollect() {
        ALL_DAMAGE
    } else {
        // This element needs to be laid out again, but does not have any damage to
        // its box. In the future, we will distinguish between types of damage to the
        // fragment as well.
        RestyleDamage::RELAYOUT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackingLevel {
    Negative(i32),
    Auto,
    Zero,
    Positive(i32),
}

/// An atomic stacking context or positioned `z-index:auto` container in a
/// real stacking context's paint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackingEntry {
    pub node_id: NodeId,
    pub level: StackingLevel,
}

#[derive(Debug, Default)]
pub struct StackingContext {
    pub negative: Vec<StackingEntry>,
    pub auto_and_zero: Vec<StackingEntry>,
    pub positive: Vec<StackingEntry>,
}

impl StackingContext {
    pub fn push(&mut self, entry: StackingEntry) {
        match entry.level {
            StackingLevel::Negative(_) => self.negative.push(entry),
            StackingLevel::Auto | StackingLevel::Zero => self.auto_and_zero.push(entry),
            StackingLevel::Positive(_) => self.positive.push(entry),
        }
    }

    pub fn sort(&mut self) {
        self.negative.sort_by_key(|entry| match entry.level {
            StackingLevel::Negative(z) => z,
            _ => unreachable!(),
        });
        self.positive.sort_by_key(|entry| match entry.level {
            StackingLevel::Positive(z) => z,
            _ => unreachable!(),
        });
    }
}

impl BaseDocument {
    pub(crate) fn invalidate_inline_contexts(&mut self) {
        let scale = self.viewport.scale();

        let font_ctx = &self.font_ctx;
        let layout_ctx = &mut self.layout_ctx;

        let mut anon_nodes = Vec::new();

        for (_, node) in self.nodes.iter_mut() {
            if !(node.flags.contains(NodeFlags::IS_IN_DOCUMENT)) {
                continue;
            }

            let Some(element) = node.data.downcast_element_mut() else {
                continue;
            };

            if element.inline_layout_data.is_some() {
                if node.is_anonymous() {
                    anon_nodes.push(node.id);
                } else {
                    node.insert_damage(ALL_DAMAGE);
                }
            } else if let Some(input) = element.text_input_data_mut() {
                input.editor.set_scale(scale);
                let mut font_ctx = font_ctx.lock().unwrap();
                input.editor.refresh_layout(&mut font_ctx, layout_ctx);
                node.insert_damage(ONLY_RELAYOUT);
            }
        }

        for node_id in anon_nodes {
            if let Some(parent_id) = *(self.nodes[node_id].layout_parent.get_mut()) {
                self.nodes[parent_id].insert_damage(ALL_DAMAGE);
            }
        }
    }

    pub fn clear_layout_caches(&mut self, node_id: NodeId) {
        if !self.nodes.contains_key(node_id) {
            return;
        }
        let children = self.nodes[node_id].layout_children.borrow().clone();
        let node = &mut self.nodes[node_id];
        node.clear_layout_cache();
        if let Some(inline_layout) = node
            .data
            .downcast_element_mut()
            .and_then(|element| element.inline_layout_data.as_mut())
        {
            inline_layout.content_widths = None;
        }
        if let Some(children) = children {
            for child_id in children {
                self.clear_layout_caches(child_id);
            }
        }
    }

    /// Flush the image layers of nodes whose style changed during the last
    /// style traversal (or whose pseudo-element boxes were (re)constructed).
    pub(crate) fn flush_pending_style_images(&mut self) {
        let mut pending = std::mem::take(&mut self.pending_style_image_nodes);
        pending.sort_unstable();
        pending.dedup();
        for node_id in pending {
            // Anonymous boxes (including pseudo-elements) can be removed from
            // the slab between queueing and flushing; skip stale IDs.
            if !self.nodes.contains_key(node_id) {
                continue;
            }
            self.flush_image_layers_from_style(node_id, ImageLayerKind::Background);
            self.flush_image_layers_from_style(node_id, ImageLayerKind::Mask);
        }
    }

    /// Flush a CSS image layer list (`background-image` or `mask-image`) from style
    /// to dedicated storage on the node, fetching any images which are not yet loaded.
    fn flush_image_layers_from_style(&mut self, node_id: NodeId, kind: ImageLayerKind) {
        let doc_id = self.id();
        let node = self.nodes.get_mut(node_id).unwrap();
        // Clone the primary style `Arc` into an owned value so the immutable
        // borrow of `node` (held by the stylo element data guard) is released
        // before we take a mutable borrow of `node.data` below.
        let style = {
            let stylo_element_data = node.try_stylo_element_data().and_then(|s| s.get());
            let primary_styles = stylo_element_data
                .as_ref()
                .and_then(|data| data.styles.get_primary());
            let Some(style) = primary_styles else {
                return;
            };
            style.clone()
        };
        let Some(elem) = node.data.downcast_element_mut() else {
            return;
        };

        let (style_images, elem_images) = match kind {
            ImageLayerKind::Background => (
                &style.get_background().background_image.0,
                &mut elem.background_images,
            ),
            ImageLayerKind::Mask => (&style.get_svg().mask_image.0, &mut elem.mask_images),
        };

        let len = style_images.len();
        elem_images.resize(len, None);

        for idx in 0..len {
            let style_image = &style_images[idx];
            let new_image = match style_image {
                StyloImage::Url(ComputedUrl::Valid(new_url)) => {
                    let old_image = elem_images[idx].as_ref();
                    let old_image_url = old_image.map(|data| &data.url);
                    if old_image_url.is_some_and(|old_url| **new_url == **old_url) {
                        continue;
                    }

                    // Check cache first
                    let url_str = new_url.as_str();
                    if let Some(cached_image) = self.image_cache.get(url_str) {
                        #[cfg(feature = "tracing")]
                        tracing::info!("Loading image {url_str} from cache");
                        Some(ImageResourceData {
                            url: new_url.clone(),
                            status: Status::Ok,
                            image: cached_image.clone(),
                        })
                    } else if let Some(waiting_list) = self.pending_images.get_mut(url_str) {
                        // Image is already being fetched, queue this node
                        #[cfg(feature = "tracing")]
                        tracing::info!("Image {url_str} already pending, queueing node {node_id}");
                        waiting_list.push((node_id, kind.image_type(idx)));
                        Some(ImageResourceData::new(new_url.clone()))
                    } else {
                        // Start fetch and track as pending
                        #[cfg(feature = "tracing")]
                        tracing::info!("Fetching image {url_str}");
                        self.pending_images
                            .insert(url_str.to_string(), vec![(node_id, kind.image_type(idx))]);

                        self.net_provider.fetch(
                            doc_id,
                            crate::net::stamped_request(
                                (**new_url).clone(),
                                self.abort_signal.as_ref(),
                            ),
                            ResourceHandler::boxed(
                                self.tx.clone(),
                                doc_id,
                                None, // Don't pass node_id, we'll handle via pending_images
                                self.shell_provider.clone(),
                                ImageHandler::new(kind.image_type(idx)),
                            ),
                        );

                        Some(ImageResourceData::new(new_url.clone()))
                    }
                }
                _ => None,
            };

            // Element will always exist due to resize_with above
            elem_images[idx] = new_image;
        }
    }

    pub fn rebuild_stacking_contexts(&mut self, root_id: NodeId) {
        if !self.incremental_layout || self.nodes[root_id].stacking_context.is_none() {
            self.rebuild_stacking_context(root_id);
            return;
        }

        let mut dirty_contexts = Vec::new();
        self.collect_dirty_stacking_contexts(root_id, root_id, false, &mut dirty_contexts);
        dirty_contexts.sort_unstable();
        dirty_contexts.dedup();
        for context_id in dirty_contexts {
            if self.nodes.contains_key(context_id) {
                self.rebuild_stacking_context(context_id);
            }
        }
    }

    fn collect_dirty_stacking_contexts(
        &mut self,
        node_id: NodeId,
        containing_context: NodeId,
        is_flex_or_grid_item: bool,
        dirty_contexts: &mut Vec<NodeId>,
    ) {
        if !self.nodes.contains_key(node_id) {
            return;
        }
        let node = &self.nodes[node_id];
        if node_id != containing_context
            && !node.stacking_dirty_self.get()
            && !node.has_damaged_descendants()
            && !node.is_anonymous()
        {
            return;
        }

        let is_context =
            node_id == containing_context || node.is_stacking_context_root(is_flex_or_grid_item);
        if node.stacking_dirty_self.get() {
            if let Some(old_owner) = node.stacking_context_owner.get() {
                dirty_contexts.push(old_owner);
            }
            dirty_contexts.push(containing_context);
            let rebuild_owned_context = node.stacking_context.is_none()
                || node
                    .damage()
                    .is_some_and(|damage| damage.contains(RestyleDamage::RECALCULATE_OVERFLOW));
            if is_context && node_id != containing_context && rebuild_owned_context {
                dirty_contexts.push(node_id);
            }
        }

        let child_context = if is_context {
            node_id
        } else {
            containing_context
        };
        let is_flex_or_grid = node.display_style().is_some_and(|display| {
            matches!(display.inside(), DisplayInside::Flex | DisplayInside::Grid)
        });
        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        for child_id in children.as_deref().unwrap_or(&[]).iter().copied() {
            self.collect_dirty_stacking_contexts(
                child_id,
                child_context,
                is_flex_or_grid,
                dirty_contexts,
            );
        }
        *self.nodes[node_id].layout_children.borrow_mut() = children;
    }

    fn rebuild_stacking_context(&mut self, context_root: NodeId) {
        let mut context = StackingContext::default();
        self.collect_stacking_children(context_root, context_root, &mut context);
        context.sort();
        self.nodes[context_root].stacking_context = Some(Box::new(context));
    }

    fn collect_stacking_children(
        &mut self,
        context_root: NodeId,
        node_id: NodeId,
        context: &mut StackingContext,
    ) {
        let is_flex_or_grid = self.nodes[node_id].display_style().is_some_and(|display| {
            matches!(display.inside(), DisplayInside::Flex | DisplayInside::Grid)
        });
        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        let mut paint_children = ThinVec::with_capacity(children.as_ref().map_or(0, ThinVec::len));

        for child_id in children.as_deref().unwrap_or(&[]).iter().copied() {
            if !self.nodes.contains_key(child_id) {
                continue;
            }
            self.nodes[child_id]
                .stacking_context_owner
                .set(Some(context_root));
            let child_is_context = self.nodes[child_id].is_stacking_context_root(is_flex_or_grid);
            if child_is_context {
                if self.nodes[child_id].stacking_context.is_none() {
                    self.rebuild_stacking_context(child_id);
                }
                context.push(StackingEntry {
                    node_id: child_id,
                    level: stacking_level(&self.nodes[child_id], is_flex_or_grid),
                });
                continue;
            }

            if self.nodes[child_id].stacking_context.is_some() {
                self.nodes[child_id].stacking_context = None;
            }

            if self.nodes[child_id].is_positioned_stacking_container() {
                context.push(StackingEntry {
                    node_id: child_id,
                    level: StackingLevel::Auto,
                });
            } else {
                paint_children.push(child_id);
            }
            self.collect_stacking_children(context_root, child_id, context);
        }

        paint_children.sort_by_key(|child_id| {
            match self.nodes[*child_id]
                .primary_styles()
                .map(|style| style.clone_float())
            {
                Some(Float::None) | None => 0,
                Some(_) => 1,
            }
        });
        *self.nodes[node_id].layout_children.borrow_mut() = children;
        *self.nodes[node_id].paint_children.borrow_mut() = Some(paint_children);
    }

    pub(crate) fn sort_layout_children(&mut self, node_id: NodeId) {
        let is_flex_or_grid = self.nodes[node_id].display_style().is_some_and(|display| {
            matches!(display.inside(), DisplayInside::Flex | DisplayInside::Grid)
        });
        if !is_flex_or_grid {
            return;
        }

        if let Some(children) = self.nodes[node_id].layout_children.borrow_mut().as_mut() {
            children.sort_by_key(|child_id| {
                let child = &self.nodes[*child_id];
                if child.taffy_position().is_out_of_flow() {
                    0
                } else {
                    child.order()
                }
            });
        }
    }
}

fn stacking_level(node: &Node, is_flex_or_grid_item: bool) -> StackingLevel {
    let Some(style) = node.primary_styles() else {
        return StackingLevel::Zero;
    };
    if node.taffy_position() == taffy::Position::Static && !is_flex_or_grid_item {
        return StackingLevel::Zero;
    }
    if style.clone_z_index().is_auto() {
        return StackingLevel::Zero;
    }
    match style.clone_z_index().integer_or(0) {
        z if z < 0 => StackingLevel::Negative(z),
        0 => StackingLevel::Zero,
        z => StackingLevel::Positive(z),
    }
}
