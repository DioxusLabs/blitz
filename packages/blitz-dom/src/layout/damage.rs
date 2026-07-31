use blitz_traits::node_id::NodeId;
use std::ops::Range;

use crate::Node;
use crate::net::ResourceHandler;
use crate::node::NodeFlags;
use crate::{
    BaseDocument, net::ImageHandler, node::ImageResourceData, node::Status, util::ImageLayerKind,
};
use style::properties::ComputedValues;
use style::properties::generated::longhands::position::computed_value::T as Position;
use style::selector_parser::RestyleDamage;
use style::url::ComputedUrl;
use style::values::computed::Float;
use style::values::generics::image::Image as StyloImage;
use style::values::specified::align::AlignFlags;
use style::values::specified::box_::DisplayInside;
use style::values::specified::box_::DisplayOutside;
use taffy::Rect;
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
            .stylo_element_data_opt_mut()
            .and_then(|s| s.get_mut())
        {
            data.damage
        } else {
            return RestyleDamage::empty();
        };
        damage |= damage_from_parent;

        // Flush updated pseudo-element styles to their anonymous nodes so that
        // style changes which don't trigger box construction still take effect.
        //
        // TODO: see if this can be made more efficient (/run less often)
        self.sync_pseudo_element_styles(node_id);

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
            node.cache_mut().clear();
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

    /// Flush updated pseudo-element (`::before`/`::after`) styles from the owning
    /// element's stylo data to the pseudo-element's anonymous node.
    ///
    /// Pseudo-element styles are normally flushed to the pseudo-element's node
    /// during box construction (see `flush_pseudo_elements`), but in incremental
    /// mode box construction only runs for nodes with construction damage.
    /// Pseudo-element style changes which don't require reconstruction (e.g.
    /// animations/transitions of repaint- or relayout-only properties) must still
    /// be flushed to the pseudo-element's node - along with the damage they imply -
    /// so that layout and paint see the new style.
    fn sync_pseudo_element_styles(&mut self, node_id: NodeId) {
        let node = &self.nodes[node_id];

        let before_node_id = node.before();
        let after_node_id = node.after();
        if before_node_id.is_none() && after_node_id.is_none() {
            return;
        }

        let (before_style, after_style) = {
            let style_data = node.stylo_element_data_opt().and_then(|s| s.get());
            let Some(style_data) = style_data.as_ref() else {
                return;
            };
            // Note: yes these are kinda backwards (see `flush_pseudo_elements`)
            let pseudos = style_data.styles.pseudos.as_array();
            (pseudos[1].clone(), pseudos[0].clone())
        };

        // Creation and removal of pseudo-elements is handled during box construction
        // (Stylo generates construction damage for those cases), so only the case
        // where the pseudo-element both was and remains present is handled here.
        for (pe_node_id, pe_style) in [(before_node_id, before_style), (after_node_id, after_style)]
        {
            let (Some(pe_node_id), Some(pe_style)) = (pe_node_id, pe_style) else {
                continue;
            };
            let mut pe_data = self.nodes[pe_node_id]
                .stylo_element_data_opt_mut()
                .and_then(|s| s.get_mut());
            let Some(pe_data) = pe_data.as_mut() else {
                continue;
            };
            let Some(old_style) = pe_data.styles.primary.clone() else {
                continue;
            };
            if std::ptr::eq(&*old_style, &*pe_style) {
                continue;
            }

            let diff = RestyleDamage::compute_style_difference::<&Node>(&old_style, &pe_style);
            pe_data.damage.insert(diff.damage);
            pe_data.styles.primary = Some(pe_style);
            pe_data.set_restyled();
        }
    }
}

// #[cfg(feature = "incremental")]
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

/// A child with a z_index that is hoisted up to it's containing Stacking Context for paint purposes
#[derive(Debug, Clone)]
pub struct HoistedPaintChild {
    pub node_id: NodeId,
    pub z_index: i32,
    pub position: taffy::Point<f32>,
    /// The overflow clip applied by ancestors between this box and its stacking context
    /// root, in the coordinate space of the stacking context root. `None` means unclipped.
    /// Out-of-flow boxes are only clipped by ancestors from their containing block outwards.
    pub clip: Option<taffy::Rect<f32>>,
}

#[derive(Debug)]
pub struct HoistedPaintChildren {
    pub children: Vec<HoistedPaintChild>,
    /// The number of hoisted point children with negative z_index
    pub negative_z_count: u32,

    pub content_area: taffy::Rect<f32>,
}

impl HoistedPaintChildren {
    fn new() -> Self {
        Self {
            children: Vec::new(),
            negative_z_count: 0,
            content_area: taffy::Rect::ZERO,
        }
    }

    pub fn reset(&mut self) {
        self.children.clear();
        self.negative_z_count = 0;
    }

    pub fn compute_content_size(&mut self, doc: &BaseDocument) {
        fn child_pos(child: &HoistedPaintChild, doc: &BaseDocument) -> Rect<f32> {
            let node = &doc.nodes[child.node_id];
            let left = child.position.x + node.final_layout().location.x;
            let top = child.position.y + node.final_layout().location.y;
            let right = left + node.final_layout().size.width;
            let bottom = top + node.final_layout().size.height;

            taffy::Rect {
                top,
                left,
                bottom,
                right,
            }
        }

        if self.children.is_empty() {
            self.content_area = taffy::Rect::ZERO;
        } else {
            self.content_area = child_pos(&self.children[0], doc);
            for child in self.children[1..].iter() {
                let pos = child_pos(child, doc);
                self.content_area.left = self.content_area.left.min(pos.left);
                self.content_area.top = self.content_area.top.min(pos.top);
                self.content_area.right = self.content_area.right.max(pos.right);
                self.content_area.bottom = self.content_area.bottom.max(pos.bottom);
            }
        }
    }

    pub fn sort(&mut self) {
        self.children.sort_by_key(|c| c.z_index);
        self.negative_z_count = self.children.iter().take_while(|c| c.z_index < 0).count() as u32;
    }

    pub fn neg_z_range(&self) -> Range<usize> {
        0..(self.negative_z_count as usize)
    }

    pub fn pos_z_range(&self) -> Range<usize> {
        (self.negative_z_count as usize)..self.children.len()
    }

    pub fn neg_z_hoisted_children(
        &self,
    ) -> impl ExactSizeIterator<Item = &HoistedPaintChild> + DoubleEndedIterator {
        self.children[self.neg_z_range()].iter()
    }

    pub fn pos_z_hoisted_children(
        &self,
    ) -> impl ExactSizeIterator<Item = &HoistedPaintChild> + DoubleEndedIterator {
        self.children[self.pos_z_range()].iter()
    }
}

impl BaseDocument {
    /// Recompute the offsets of hoisted paint children relative to their stacking context
    /// root, using the final (post-layout) positions of the intervening nodes.
    ///
    /// The offsets initially recorded by `flush_styles_to_layout` are computed *before*
    /// layout runs and may therefore be stale.
    pub(crate) fn refresh_hoisted_paint_positions(&mut self) {
        let root_ids: Vec<NodeId> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.stacking_context.is_some())
            .map(|(id, _)| id)
            .collect();

        for root_id in root_ids {
            let Some(mut stacking_context) = self.nodes[root_id].stacking_context.take() else {
                continue;
            };

            for hoisted in stacking_context.children.iter_mut() {
                if !self.nodes.contains_key(hoisted.node_id) {
                    continue;
                }
                // Out-of-flow boxes do not scroll with the ancestors between them and
                // their containing block: only apply scroll offsets from the containing
                // block outwards.
                let hoisted_position = self.nodes[hoisted.node_id].style().position;

                // Sticky boxes are laid out at their flow position; their sticky offset
                // is a paint-time shift computed from live scroll offsets.
                let is_sticky = self.nodes[hoisted.node_id]
                    .primary_styles()
                    .is_some_and(|s| s.clone_position() == Position::Sticky);
                let sticky_offset = if is_sticky {
                    self.compute_sticky_offset(hoisted.node_id)
                } else {
                    taffy::Point::ZERO
                };
                *self.nodes[hoisted.node_id].sticky_offset_mut() = sticky_offset;

                // Overflow clips from clipping ancestors, recorded as (offset of the box
                // relative to the ancestor, clip rect relative to the ancestor's border box)
                let mut clips: Vec<(taffy::Point<f32>, taffy::Rect<f32>)> = Vec::new();

                let mut past_cb = false;
                let mut position = taffy::Point::ZERO;
                let mut current = self.nodes[hoisted.node_id].layout_parent.get();
                while let Some(ancestor_id) = current {
                    if ancestor_id == root_id || !self.nodes.contains_key(ancestor_id) {
                        break;
                    }
                    let ancestor = &self.nodes[ancestor_id];
                    let is_cb = match hoisted_position {
                        taffy::Position::Fixed => ancestor.establishes_fixed_containing_block(),
                        taffy::Position::Absolute => {
                            ancestor.style().position != taffy::Position::Static
                                || ancestor.establishes_fixed_containing_block()
                        }
                        // z-index hoisted in-flow boxes scroll with all their ancestors
                        _ => true,
                    };
                    // Ancestors clip this box only from its containing block outwards
                    let mut pushed_clip = false;
                    if is_cb || past_cb {
                        let style = ancestor.style();
                        if style.overflow.x != taffy::Overflow::Visible
                            || style.overflow.y != taffy::Overflow::Visible
                        {
                            let layout = ancestor.final_layout();
                            clips.push((
                                position,
                                taffy::Rect {
                                    left: layout.border.left,
                                    right: layout.size.width
                                        - layout.border.right
                                        - layout.scrollbar_size.width,
                                    top: layout.border.top,
                                    bottom: layout.size.height
                                        - layout.border.bottom
                                        - layout.scrollbar_size.height,
                                },
                            ));
                            pushed_clip = true;
                        }
                    }
                    let location = ancestor.final_layout().location;
                    // In-flow ancestors carry their own sticky offset (nested sticky)
                    let ancestor_sticky = *ancestor.sticky_offset();
                    position.x += location.x + ancestor_sticky.x;
                    position.y += location.y + ancestor_sticky.y;
                    if is_cb || past_cb {
                        let scroll_offset = *ancestor.scroll_offset();
                        position.x -= scroll_offset.x as f32;
                        position.y -= scroll_offset.y as f32;
                        // An ancestor's scroll offset moves its content (including this box)
                        // but not its own clip box: cancel it out of the clip's offset.
                        if pushed_clip {
                            let (offset, _) = clips.last_mut().unwrap();
                            offset.x -= scroll_offset.x as f32;
                            offset.y -= scroll_offset.y as f32;
                        }
                    }
                    past_cb |= is_cb;
                    current = ancestor.layout_parent.get();
                }
                hoisted.position = taffy::Point {
                    x: position.x + sticky_offset.x,
                    y: position.y + sticky_offset.y,
                };

                // Convert the recorded clips into the stacking context root's coordinate
                // space and intersect them
                hoisted.clip = None;
                for (offset, rect) in clips {
                    let clip = taffy::Rect {
                        left: position.x - offset.x + rect.left,
                        right: position.x - offset.x + rect.right,
                        top: position.y - offset.y + rect.top,
                        bottom: position.y - offset.y + rect.bottom,
                    };
                    hoisted.clip = Some(match hoisted.clip {
                        Some(existing) => taffy::Rect {
                            left: existing.left.max(clip.left),
                            right: existing.right.min(clip.right),
                            top: existing.top.max(clip.top),
                            bottom: existing.bottom.min(clip.bottom),
                        },
                        None => clip,
                    });
                }
            }

            stacking_context.compute_content_size(self);
            self.nodes[root_id].stacking_context = Some(stacking_context);
        }
    }

    /// Compute the paint-time offset for a `position: sticky` box.
    ///
    /// The box is shifted from its flow position so that it satisfies its inset
    /// constraints against the nearest scrollport (the padding box of the nearest
    /// scroll container, or the viewport), clamped so that its margin box never
    /// escapes its containing block.
    fn compute_sticky_offset(&self, node_id: NodeId) -> taffy::Point<f32> {
        use style::values::computed::CSSPixelLength;
        use style::values::generics::length::GenericMargin;
        use style::values::generics::position::Inset as GenericInset;

        let node = &self.nodes[node_id];
        let Some(style) = node.primary_styles() else {
            return taffy::Point::ZERO;
        };

        let layout = *node.final_layout();

        // The sticky box's position box is its margin box, except that auto margins
        // (used for alignment) do not restrict sticky movement.
        let margin_style = style.get_margin();
        let used_margin = |margin: &GenericMargin<style::values::computed::LengthPercentage>,
                           resolved: f32| match margin {
            GenericMargin::Auto => 0.0,
            _ => resolved,
        };
        let margin = taffy::Rect {
            left: used_margin(&margin_style.margin_left, layout.margin.left),
            right: used_margin(&margin_style.margin_right, layout.margin.right),
            top: used_margin(&margin_style.margin_top, layout.margin.top),
            bottom: used_margin(&margin_style.margin_bottom, layout.margin.bottom),
        };

        // Border box position of the sticky box, translated up to the scrollport's
        // border box coordinate space (unscrolled flow position).
        let mut box_pos = taffy::Point {
            x: layout.location.x,
            y: layout.location.y,
        };
        // Containing block rect (the layout parent's content box), in the same
        // coordinate space, shrunk by the sticky box's margins.
        let mut cb_rect: Option<taffy::Rect<f32>> = None;

        let mut scrollport: Option<NodeId> = None;
        let mut current = node.layout_parent.get();
        while let Some(ancestor_id) = current {
            if !self.nodes.contains_key(ancestor_id) {
                break;
            }
            let ancestor = &self.nodes[ancestor_id];
            let a_layout = ancestor.final_layout();
            if cb_rect.is_none() {
                // When the containing block is itself a scroll container, its content
                // extends over the whole scrollable area, not just the visible part.
                let content_right = (a_layout.size.width
                    - a_layout.border.right
                    - a_layout.padding.right
                    - a_layout.scrollbar_size.width)
                    .max(a_layout.border.left + a_layout.content_size.width);
                let content_bottom = (a_layout.size.height
                    - a_layout.border.bottom
                    - a_layout.padding.bottom
                    - a_layout.scrollbar_size.height)
                    .max(a_layout.border.top + a_layout.content_size.height);
                cb_rect = Some(taffy::Rect {
                    left: a_layout.border.left + a_layout.padding.left + margin.left,
                    right: content_right - margin.right,
                    top: a_layout.border.top + a_layout.padding.top + margin.top,
                    bottom: content_bottom - margin.bottom,
                });
            }
            let a_style = ancestor.style();
            if a_style.overflow.x != taffy::Overflow::Visible
                || a_style.overflow.y != taffy::Overflow::Visible
            {
                scrollport = Some(ancestor_id);
                break;
            }
            let location = a_layout.location;
            let ancestor_sticky = *ancestor.sticky_offset();
            let dx = location.x + ancestor_sticky.x;
            let dy = location.y + ancestor_sticky.y;
            box_pos.x += dx;
            box_pos.y += dy;
            if let Some(rect) = cb_rect.as_mut() {
                rect.left += dx;
                rect.right += dx;
                rect.top += dy;
                rect.bottom += dy;
            }
            current = ancestor.layout_parent.get();
        }

        // The visible window of the scrollport, in the same (unscrolled) coordinate
        // space as `box_pos`: the scrollport's padding box shifted by its scroll offset.
        let view_rect = match scrollport {
            Some(scrollport_id) => {
                let scrollport = &self.nodes[scrollport_id];
                let sp_layout = scrollport.final_layout();
                let scroll = *scrollport.scroll_offset();
                taffy::Rect {
                    left: sp_layout.border.left + scroll.x as f32,
                    right: sp_layout.size.width
                        - sp_layout.border.right
                        - sp_layout.scrollbar_size.width
                        + scroll.x as f32,
                    top: sp_layout.border.top + scroll.y as f32,
                    bottom: sp_layout.size.height
                        - sp_layout.border.bottom
                        - sp_layout.scrollbar_size.height
                        + scroll.y as f32,
                }
            }
            // No scroll container ancestor: stick relative to the viewport.
            None => {
                let scale = self.viewport.scale();
                let scroll = self.viewport_scroll();
                taffy::Rect {
                    left: scroll.x as f32,
                    right: scroll.x as f32 + self.viewport.window_size.0 as f32 / scale,
                    top: scroll.y as f32,
                    bottom: scroll.y as f32 + self.viewport.window_size.1 as f32 / scale,
                }
            }
        };

        let cb_rect = cb_rect.unwrap_or(view_rect);

        // Sticky inset percentages resolve against the containing block
        fn resolve_inset(
            inset: &GenericInset<
                style::values::computed::Percentage,
                style::values::computed::LengthPercentage,
            >,
            basis: f32,
        ) -> Option<f32> {
            match inset {
                GenericInset::LengthPercentage(lp) => {
                    Some(lp.resolve(CSSPixelLength::new(basis)).px())
                }
                _ => None,
            }
        }
        let pos_style = style.get_position();
        let cb_width = cb_rect.right - cb_rect.left;
        let cb_height = cb_rect.bottom - cb_rect.top;
        let left = resolve_inset(&pos_style.left, cb_width);
        let right = resolve_inset(&pos_style.right, cb_width);
        let top = resolve_inset(&pos_style.top, cb_height);
        let bottom = resolve_inset(&pos_style.bottom, cb_height);

        taffy::Point {
            x: sticky_axis_offset(
                box_pos.x,
                box_pos.x + layout.size.width,
                cb_rect.left,
                cb_rect.right,
                view_rect.left,
                view_rect.right,
                left,
                right,
            ),
            y: sticky_axis_offset(
                box_pos.y,
                box_pos.y + layout.size.height,
                cb_rect.top,
                cb_rect.bottom,
                view_rect.top,
                view_rect.bottom,
                top,
                bottom,
            ),
        }
    }

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

    pub fn flush_styles_to_layout(&mut self, node_id: NodeId) {
        self.flush_styles_to_layout_impl(node_id, None);
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
            let stylo_element_data = node.stylo_element_data_opt().and_then(|s| s.get());
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
        elem_images.resize_with(len, || None);

        for idx in 0..len {
            let style_image = &style_images[idx];
            let new_image = match style_image {
                StyloImage::Url(ComputedUrl::Valid(new_url)) => {
                    let old_image = elem_images[idx].as_ref();
                    let old_image_url = old_image.map(|data| &data.url);
                    if old_image_url.is_some_and(|old_url| **new_url == **old_url) {
                        break;
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

    /// Walk the whole tree, converting styles to layout
    fn flush_styles_to_layout_impl(
        &mut self,
        node_id: NodeId,
        parent_stacking_context: Option<&mut HoistedPaintChildren>,
    ) {
        let mut new_stacking_context: HoistedPaintChildren = HoistedPaintChildren::new();
        let stacking_context = &mut new_stacking_context;

        // Flush background/mask images from style to dedicated storage on the node
        self.flush_image_layers_from_style(node_id, ImageLayerKind::Background);
        self.flush_image_layers_from_style(node_id, ImageLayerKind::Mask);

        let incremental = self.incremental_layout;
        let display = {
            let node = self.nodes.get_mut(node_id).unwrap();
            let _damage = node.damage().unwrap_or(ALL_DAMAGE);

            // Compute the owned taffy style and display in an inner scope so the
            // immutable borrow of `node` (held by the stylo element data guard)
            // is released before we mutably access `node` below.
            let (mut taffy_style, display_constructed_as) = {
                let stylo_element_data = node.stylo_element_data_opt().and_then(|s| s.get());
                let primary_styles = stylo_element_data
                    .as_ref()
                    .and_then(|data| data.styles.get_primary());

                let Some(style) = primary_styles else {
                    return;
                };

                (stylo_taffy::to_taffy_style(style), style.clone_display())
            };
            taffy_style.item_is_replaced = node
                .data
                .downcast_element()
                .is_some_and(|el| matches!(&*el.name.local, "img" | "canvas"));

            // Sticky offsets are recomputed each resolve (in `refresh_hoisted_paint_positions`)
            // for boxes that are still sticky; clear here so boxes that are no longer sticky
            // don't retain a stale offset.
            *node.sticky_offset_mut() = taffy::Point::ZERO;

            // if damage.intersects(RestyleDamage::RELAYOUT | CONSTRUCT_BOX) {
            *node.style_mut() = taffy_style;
            *node.display_constructed_as_mut() = display_constructed_as;
            // }

            // In non-incremental mode we unconditionally clear the Taffy cache.
            // In incremental mode this is handled as part of damage propagation.
            if !incremental {
                node.cache_mut().clear();
                if let Some(inline_layout) = node
                    .data
                    .downcast_element_mut()
                    .and_then(|el| el.inline_layout_data.as_mut())
                {
                    inline_layout.content_widths = None;
                }
            }

            node.style().display
        };

        // If the node has children, then take those children and...
        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        if let Some(mut children) = children {
            let is_flex_or_grid = matches!(display, taffy::Display::Flex | taffy::Display::Grid);

            // Sort layout_children
            if is_flex_or_grid {
                children.sort_by(|left, right| {
                    let left_node = self.nodes.get(*left).unwrap();
                    let right_node = self.nodes.get(*right).unwrap();
                    left_node.order().cmp(&right_node.order())
                });
            }

            let mut new_paint_children: ThinVec<NodeId> = ThinVec::with_capacity(children.len());

            // Push children to either paint_children or the stacking context, then recurse.
            // Hoisted entries are pushed before recursing into the child so that the hoisted
            // list is in tree (pre-)order: CSS 2.1 Appendix E step 8 paints positioned
            // descendants with z-index: auto at the same level, in tree order.
            for &child_id in children.iter() {
                let hoisted = 'hoisted: {
                    let child = &self.nodes[child_id];

                    let Some(style) = child.primary_styles() else {
                        break 'hoisted false;
                    };

                    let position = style.clone_position();
                    let z_index = style.clone_z_index().integer_or(0);

                    // Positioned descendants are painted at the level of their stacking
                    // context root rather than at the level of their layout parent
                    // (CSS 2.1 Appendix E steps 8-9): hoist them. This also ensures that
                    // out-of-flow boxes whose containing block is not their layout parent
                    // (fixed children, and absolute children of non-positioned parents)
                    // escape the scrolling and clipping of the ancestors between them and
                    // their containing block.
                    //
                    // z-index also applies to static flex/grid items
                    // (css-flexbox-1 §painting, css-grid-1 §z-order).
                    position != Position::Static || (z_index != 0 && is_flex_or_grid)
                };

                if hoisted {
                    let z_index = self.nodes[child_id]
                        .primary_styles()
                        .map(|style| style.clone_z_index().integer_or(0))
                        .unwrap_or(0);
                    stacking_context.children.push(HoistedPaintChild {
                        node_id: child_id,
                        z_index,
                        position: taffy::Point::ZERO,
                        clip: None,
                    })
                } else {
                    new_paint_children.push(child_id);
                }

                self.flush_styles_to_layout_impl(
                    child_id,
                    match self.nodes[child_id].is_stacking_context_root(is_flex_or_grid) {
                        true => None,
                        false => Some(stacking_context),
                    },
                );
            }

            // Sort paint_children
            new_paint_children.sort_by(|left, right| {
                let left_node = self.nodes.get(*left).unwrap();
                let right_node = self.nodes.get(*right).unwrap();
                node_to_paint_order(left_node, is_flex_or_grid)
                    .cmp(&node_to_paint_order(right_node, is_flex_or_grid))
            });
            *self.nodes[node_id].paint_children.borrow_mut() = Some(new_paint_children);

            // Put children back
            *self.nodes[node_id].layout_children.borrow_mut() = Some(children);
        }

        if let Some(parent_stacking_context) = parent_stacking_context {
            let position = self.nodes[node_id].final_layout().location;
            let scroll_offset = *self.nodes[node_id].scroll_offset();
            for hoisted in stacking_context.children.iter_mut() {
                hoisted.position.x += position.x - scroll_offset.x as f32;
                hoisted.position.y += position.y - scroll_offset.y as f32;
            }
            parent_stacking_context
                .children
                .extend(stacking_context.children.iter().cloned());
        } else {
            stacking_context.sort();
            stacking_context.compute_content_size(self);
            self.nodes[node_id].stacking_context = Some(Box::new(new_stacking_context));
        }
    }
}

/// Compute the sticky shift along one axis: shift the box (border box edges
/// `box_start..box_end`) so that it satisfies the `inset_start`/`inset_end`
/// constraints against the scrollport's visible window (`view_start..view_end`),
/// clamped so it never escapes its containing block (`cb_start..cb_end`, already
/// shrunk by the box's margins).
#[allow(clippy::too_many_arguments)]
fn sticky_axis_offset(
    box_start: f32,
    box_end: f32,
    cb_start: f32,
    cb_end: f32,
    view_start: f32,
    view_end: f32,
    inset_start: Option<f32>,
    inset_end: Option<f32>,
) -> f32 {
    let mut offset = 0.0f32;
    if let Some(inset) = inset_start {
        let max_offset = (cb_end - box_end).max(0.0);
        offset = (view_start + inset - box_start).clamp(0.0, max_offset);
    }
    if let Some(inset) = inset_end {
        let min_offset = (cb_start - box_start).min(0.0);
        let desired = view_end - inset - box_end;
        if desired < offset {
            offset = desired.max(min_offset);
        }
    }
    offset
}

#[inline(always)]
fn position_to_order(pos: Position) -> i32 {
    match pos {
        Position::Static => 0,
        // All positioned descendants with z-index: auto share one paint
        // level (CSS 2.1 Appendix E step 8); the stable sort keeps them in
        // tree order among themselves, above in-flow content and floats.
        Position::Relative | Position::Sticky | Position::Absolute | Position::Fixed => 2,
    }
}
#[inline(always)]
fn float_to_order(pos: Float) -> i32 {
    match pos {
        Float::None => 0,
        _ => 1,
    }
}

/// Paint sort key: (paint level, order-modified position). Positioned
/// (z-index: auto) descendants paint above in-flow content (CSS 2.1
/// Appendix E step 8); within a level the stable sort preserves
/// (order-modified) document order.
#[inline(always)]
fn node_to_paint_order(node: &Node, is_flex_or_grid: bool) -> (i32, i32) {
    let Some(style) = node.primary_styles() else {
        return (0, 0);
    };
    let position = style.clone_position();
    if is_flex_or_grid {
        match position {
            Position::Static => (0, style.clone_order()),
            Position::Relative | Position::Sticky => (2, style.clone_order()),
            // Out-of-flow children are not flex/grid items: `order` does
            // not apply; tree order does.
            Position::Absolute | Position::Fixed => (2, 0),
        }
    } else {
        (
            position_to_order(position) + float_to_order(style.clone_float()),
            0,
        )
    }
}
