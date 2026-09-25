use blitz_traits::node_id::NodeId;

use crate::net::ResourceHandler;
use crate::node::NodeFlags;
use crate::tree::NodeTree;
use crate::{
    BaseDocument, net::ImageHandler, node::ImageResourceData, node::Status, util::ImageLayerKind,
};
use style::properties::ComputedValues;
use style::properties::generated::longhands::position::computed_value::T as Position;
use style::selector_parser::RestyleDamage;
use style::url::ComputedUrl;
use style::values::generics::image::Image as StyloImage;
use style::values::specified::align::AlignFlags;
use style::values::specified::box_::DisplayInside;
use style::values::specified::box_::DisplayOutside;
use thin_vec::ThinVec;

// Blitz-specific damage bits, in increasing order of severity above Servo's
// `RestyleDamage::RELAYOUT` (0b1111).

pub(crate) const ONLY_RELAYOUT: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0000_1000);

/// The node's `order` changed: its flex/grid container must re-collect its
/// `layout_children` (which sorts them). Consumed by the container in
/// `propagate_damage_flags`, which marks itself `CONSTRUCT_BOX` without
/// forwarding that further up. Deliberately not part of `ALL_DAMAGE`.
pub(crate) const REORDER_CHILDREN: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0001_0000);

pub(crate) const CONSTRUCT_BOX: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0010_0000);
pub(crate) const CONSTRUCT_FC: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_0100_0000);
pub(crate) const CONSTRUCT_DESCENDENT: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_1000_0000);

pub(crate) const ALL_DAMAGE: RestyleDamage =
    RestyleDamage::from_bits_retain(0b_0000_0000_1110_1111);

impl BaseDocument {
    /// Union `RestyleDamage` up the tree and clear the layout caches of every
    /// node whose subtree needs relayout.
    ///
    /// Damage is stored on (styled) DOM nodes, so this pass walks the DOM tree
    /// (`children` + `::before`/`::after`): `display:contents` nodes and the
    /// inline descendants of inline roots carry damage but are not layout
    /// children. Anonymous boxes only exist in the layout tree and are
    /// therefore never visited; their caches are invalidated by walking
    /// `layout_parent` upward from each damaged node instead.
    pub(crate) fn propagate_damage_flags(&mut self, node_id: NodeId) -> RestyleDamage {
        let mut damage = if let Some(data) = self.nodes[node_id]
            .try_stylo_element_data_mut()
            .and_then(|s| s.get_mut())
        {
            data.damage
        } else {
            return RestyleDamage::empty();
        };

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

        let mut damage_from_children = RestyleDamage::empty();
        let children = std::mem::take(&mut self.nodes[node_id].children);
        for child in children.iter() {
            damage_from_children |= self.propagate_damage_flags(*child);
        }
        if let Some(before_id) = self.nodes[node_id].before() {
            damage_from_children |= self.propagate_damage_flags(before_id);
        }
        if let Some(after_id) = self.nodes[node_id].after() {
            damage_from_children |= self.propagate_damage_flags(after_id);
        }

        // A child's `order` changed: this container re-collects (and so re-sorts)
        // its layout children. Only the direct parent consumes the flag, and the
        // resulting `CONSTRUCT_BOX` is not forwarded further up: the container's
        // own box is unchanged, so ancestors need only `RELAYOUT` (always set
        // alongside `REORDER_CHILDREN`) plus `CONSTRUCT_DESCENDENT` so that a
        // traversal gated on it still reaches this container.
        let reorder = damage_from_children.contains(REORDER_CHILDREN)
            && matches!(
                self.nodes[node_id].display_constructed_as().inside(),
                DisplayInside::Flex | DisplayInside::Grid
            );
        damage_from_children.remove(REORDER_CHILDREN);
        damage |= damage_from_children;

        let node = &mut self.nodes[node_id];

        // Put children back
        node.children = children;

        if damage.contains(CONSTRUCT_BOX) {
            damage.insert(RestyleDamage::RELAYOUT);
        }

        // Compute damage to propagate to parent
        let mut damage_for_parent = damage; // & RestyleDamage::RELAYOUT;

        if reorder {
            damage.insert(CONSTRUCT_BOX);
            damage_for_parent.insert(CONSTRUCT_DESCENDENT);
        }

        // If the node or any of it's children have been mutated or their layout styles
        // have changed, then we should clear it's layout cache.
        let mut invalidate_anonymous_ancestors = false;
        if damage.intersects(ONLY_RELAYOUT | CONSTRUCT_BOX) {
            node.invalidate_layout_cache();
            invalidate_anonymous_ancestors = true;
            damage.remove(ONLY_RELAYOUT);
        }

        // Store damage for current node
        node.set_damage(damage);

        // Anonymous boxes between this node and its nearest element ancestor
        // are not visited by the DOM walk, so invalidate them here by walking
        // `layout_parent`. Stop at the first non-anonymous node: elements are
        // DOM ancestors and clear themselves when RELAYOUT reaches them.
        //
        // The chain is always usable: anonymous blocks are only freed by the
        // reconstruct branch of `resolve_layout_children` (which immediately
        // re-collects and resets `layout_parent` for every child it visits,
        // recursing through the fresh anonymous blocks which are born with
        // `ALL_DAMAGE`) and by node removal (the subtree leaves the document).
        // So when this pass runs, a node's `layout_parent` chain is either
        // current or owned by a container which reconstructs this frame. A
        // stale key for a freed block yields `None` (slab keys are versioned).
        if invalidate_anonymous_ancestors {
            let mut current = self.nodes[node_id].layout_parent.get();
            while let Some(ancestor) = current.and_then(|id| self.nodes.get_mut(id)) {
                if !ancestor.is_anonymous() {
                    break;
                }
                ancestor.invalidate_layout_cache();
                current = ancestor.layout_parent.get();
            }
        }

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

    /// Mark every node in the document with `ALL_DAMAGE` (non-incremental
    /// mode). Iterates the node slotmap directly rather than walking the tree,
    /// which is cheaper and also reaches pseudo-elements and anonymous boxes
    /// without special-casing them. Detached nodes get marked too, which is
    /// harmless: they are reconstructed on re-attachment anyway.
    pub(crate) fn mark_all_damaged(&mut self) {
        for (_, node) in self.nodes.iter_mut() {
            node.insert_damage(ALL_DAMAGE);
        }
    }

    /// Clear damage and the `damaged_descendants`/`dirty_descendants` flags
    /// on all nodes which may carry them, using the `damaged_descendants`
    /// flags to skip clean subtrees (mirroring `propagate_damage_flags`).
    ///
    /// Every node is visited exactly once: DOM nodes through their parent's
    /// `children`, pseudo-elements through their owner's `before`/`after`,
    /// anonymous boxes through their owner's `anonymous_blocks`. An anonymous
    /// box's `children` are DOM nodes already reached through its owner, so
    /// they are not recursed into.
    pub(crate) fn clear_damage_and_dirty_flags(&mut self, node_id: NodeId) {
        // Anonymous boxes can be freed during construction while still
        // listed in their owner's `anonymous_blocks`.
        let Some(node) = self.nodes.get_mut(node_id) else {
            return;
        };
        let is_anonymous = node.is_anonymous();
        let has_damage = node.damage().is_some_and(|d| !d.is_empty());
        if !has_damage && !node.has_damaged_descendants() && !is_anonymous {
            return;
        }
        let anonymous_blocks = std::mem::take(&mut node.anonymous_blocks);
        let children = std::mem::take(&mut node.children);
        let (before, after) = (node.before(), node.after());

        for anon_id in anonymous_blocks.iter() {
            self.clear_damage_and_dirty_flags(*anon_id);
        }
        if !is_anonymous {
            for child in children.iter() {
                self.clear_damage_and_dirty_flags(*child);
            }
        }
        if let Some(before_id) = before {
            self.clear_damage_and_dirty_flags(before_id);
        }
        if let Some(after_id) = after {
            self.clear_damage_and_dirty_flags(after_id);
        }

        let node = &mut self.nodes[node_id];
        node.anonymous_blocks = anonymous_blocks;
        node.children = children;
        node.clear_damage_mut();
        node.unset_damaged_descendants();
        node.unset_dirty_descendants();
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
    } else if old.get_position().order != new.get_position().order
        // `position` is unchanged here (else the box tree would be rebuilt), so
        // checking the new style suffices. Out-of-flow children are not flex/grid
        // items: `Node::order()` keys them as 0 regardless of `order`.
        && !matches!(
            new.get_box().position,
            Position::Absolute | Position::Fixed
        )
    {
        RestyleDamage::RELAYOUT | REORDER_CHILDREN
    } else {
        // This element needs to be laid out again, but does not have any damage to
        // its box. In the future, we will distinguish between types of damage to the
        // fragment as well.
        RestyleDamage::RELAYOUT
    }
}

/// Stable-sort a flex/grid container's freshly constructed `layout_children`
/// (in document order) by `order`. Skips the sort when every child has the
/// default `order` (0).
pub(crate) fn sort_layout_children_by_order(
    nodes: &NodeTree,
    layout_children: &mut ThinVec<NodeId>,
) {
    if layout_children.iter().all(|id| nodes[*id].order() == 0) {
        return;
    }
    layout_children.sort_by_cached_key(|id| nodes[*id].order());
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
}
