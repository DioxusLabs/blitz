use std::ops::Range;

use crate::net::ResourceHandler;
use crate::node::NodeFlags;
use crate::{BaseDocument, net::ImageHandler, node::BackgroundImageData, util::ImageType};
use crate::{NON_INCREMENTAL, Node};
use blitz_traits::net::Request;
use style::properties::ComputedValues;
use style::properties::generated::longhands::position::computed_value::T as Position;
use style::selector_parser::RestyleDamage;
use style::servo::url::ComputedUrl;
use style::values::computed::Float;
use style::values::generics::image::Image as StyloImage;
use style::values::specified::align::AlignFlags;
use style::values::specified::box_::DisplayInside;
use style::values::specified::box_::DisplayOutside;
use taffy::Rect;

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
    #[cfg(feature = "incremental")]
    pub(crate) fn propagate_damage_flags(
        &mut self,
        node_id: usize,
        damage_from_parent: RestyleDamage,
    ) -> RestyleDamage {
        let Some(mut damage) = self.nodes[node_id].damage() else {
            return RestyleDamage::empty();
        };
        damage |= damage_from_parent;

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
            if let Some(before_id) = self.nodes[node_id].before {
                damage |= self.propagate_damage_flags(before_id, damage_for_children);
            }
            if let Some(after_id) = self.nodes[node_id].after {
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
            node.cache.clear();
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
    pub node_id: usize,
    pub z_index: i32,
    pub position: taffy::Point<f32>,
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
            let left = child.position.x + node.final_layout.location.x;
            let top = child.position.y + node.final_layout.location.y;
            let right = left + node.final_layout.size.width;
            let bottom = top + node.final_layout.size.height;

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
    pub(crate) fn invalidate_inline_contexts(&mut self) {
        let scale = self.viewport.scale();

        let font_ctx = &self.font_ctx;
        let layout_ctx = &mut self.layout_ctx;

        for (_, node) in self.nodes.iter_mut() {
            if !(node.flags.contains(NodeFlags::IS_IN_DOCUMENT)) {
                continue;
            }
            let Some(element) = node.data.downcast_element_mut() else {
                continue;
            };

            if element.inline_layout_data.is_some() {
                node.insert_damage(ALL_DAMAGE);
            } else if let Some(input) = element.text_input_data_mut() {
                input.editor.set_scale(scale);
                let mut font_ctx = font_ctx.lock().unwrap();
                input.editor.refresh_layout(&mut font_ctx, layout_ctx);
                node.insert_damage(ONLY_RELAYOUT);
            }
        }
    }

    pub fn flush_styles_to_layout(&mut self, node_id: usize) {
        self.flush_styles_to_layout_impl(node_id, None);
    }

    /// Walk the whole tree, converting styles to layout
    fn flush_styles_to_layout_impl(
        &mut self,
        node_id: usize,
        parent_stacking_context: Option<&mut HoistedPaintChildren>,
    ) {
        let doc_id = self.id();

        let mut new_stacking_context: HoistedPaintChildren = HoistedPaintChildren::new();
        let stacking_context = &mut new_stacking_context;

        let display = {
            let node = self.nodes.get_mut(node_id).unwrap();
            let _damage = node.damage().unwrap_or(ALL_DAMAGE);
            let stylo_element_data = node.stylo_element_data.borrow();
            let primary_styles = stylo_element_data
                .as_ref()
                .and_then(|data| data.styles.get_primary());

            let Some(style) = primary_styles else {
                return;
            };

            // if damage.intersects(RestyleDamage::RELAYOUT | CONSTRUCT_BOX) {
            node.style = stylo_taffy::to_taffy_style(style);
            node.display_constructed_as = style.clone_display();
            // }

            // Flush background image from style to dedicated storage on the node
            // TODO: handle multiple background images
            if let Some(elem) = node.data.downcast_element_mut() {
                let style_bgs = &style.get_background().background_image.0;
                let elem_bgs = &mut elem.background_images;

                let len = style_bgs.len();
                elem_bgs.resize_with(len, || None);

                for idx in 0..len {
                    let background_image = &style_bgs[idx];
                    let new_bg_image = match background_image {
                        StyloImage::Url(ComputedUrl::Valid(new_url)) => {
                            let old_bg_image = elem_bgs[idx].as_ref();
                            let old_bg_image_url = old_bg_image.map(|data| &data.url);
                            if old_bg_image_url.is_some_and(|old_url| **new_url == **old_url) {
                                break;
                            }

                            self.net_provider.fetch(
                                doc_id,
                                Request::get((**new_url).clone()),
                                ResourceHandler::boxed(
                                    self.tx.clone(),
                                    doc_id,
                                    Some(node_id),
                                    self.shell_provider.clone(),
                                    ImageHandler::new(ImageType::Background(idx)),
                                ),
                            );

                            let bg_image_data = BackgroundImageData::new(new_url.clone());
                            Some(bg_image_data)
                        }
                        _ => None,
                    };

                    // Element will always exist due to resize_with above
                    elem_bgs[idx] = new_bg_image;
                }
            }

            // In non-incremental mode we unconditionally clear the Taffy cache.
            // In incremental mode this is handled as part of damage propagation.
            if NON_INCREMENTAL {
                node.cache.clear();
                if let Some(inline_layout) = node
                    .data
                    .downcast_element_mut()
                    .and_then(|el| el.inline_layout_data.as_mut())
                {
                    inline_layout.content_widths = None;
                }
            }

            node.style.display
        };

        // If the node has children, then take those children and...
        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        if let Some(mut children) = children {
            let is_flex_or_grid = matches!(display, taffy::Display::Flex | taffy::Display::Grid);

            // Recursively call flush_styles_to_layout on each child
            for &child in children.iter() {
                self.flush_styles_to_layout_impl(
                    child,
                    match self.nodes[child].is_stacking_context_root(is_flex_or_grid) {
                        true => None,
                        false => Some(stacking_context),
                    },
                );
            }

            // Sort layout_children
            if is_flex_or_grid {
                children.sort_by(|left, right| {
                    let left_node = self.nodes.get(*left).unwrap();
                    let right_node = self.nodes.get(*right).unwrap();
                    left_node.order().cmp(&right_node.order())
                });
            }

            // Reserve space for paint_children
            let mut paint_children = self.nodes[node_id].paint_children.borrow_mut();
            if paint_children.is_none() {
                *paint_children = Some(Vec::new());
            }
            let paint_children = paint_children.as_mut().unwrap();
            paint_children.clear();
            paint_children.reserve(children.len());

            // Push children to either paint_children or layout_children depending on
            for &child_id in children.iter() {
                let child = &self.nodes[child_id];

                let Some(style) = child.primary_styles() else {
                    paint_children.push(child_id);
                    continue;
                };

                let position = style.clone_position();
                let z_index = style.clone_z_index().integer_or(0);

                // TODO: more complete hoisting detection
                if position != Position::Static && z_index != 0 {
                    stacking_context.children.push(HoistedPaintChild {
                        node_id: child_id,
                        z_index,
                        position: taffy::Point::ZERO,
                    })
                } else {
                    paint_children.push(child_id);
                }
            }

            // Sort paint_children
            paint_children.sort_by(|left, right| {
                let left_node = self.nodes.get(*left).unwrap();
                let right_node = self.nodes.get(*right).unwrap();
                node_to_paint_order(left_node, is_flex_or_grid)
                    .cmp(&node_to_paint_order(right_node, is_flex_or_grid))
            });

            // Put children back
            *self.nodes[node_id].layout_children.borrow_mut() = Some(children);
        }

        if let Some(parent_stacking_context) = parent_stacking_context {
            let position = self.nodes[node_id].final_layout.location;
            let scroll_offset = self.nodes[node_id].scroll_offset;
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

#[inline(always)]
fn position_to_order(pos: Position) -> i32 {
    match pos {
        Position::Static | Position::Relative | Position::Sticky => 0,
        Position::Absolute | Position::Fixed => 2,
    }
}
#[inline(always)]
fn float_to_order(pos: Float) -> i32 {
    match pos {
        Float::None => 0,
        _ => 1,
    }
}

#[inline(always)]
fn node_to_paint_order(node: &Node, is_flex_or_grid: bool) -> i32 {
    let Some(style) = node.primary_styles() else {
        return 0;
    };
    if is_flex_or_grid {
        match style.clone_position() {
            Position::Static | Position::Relative | Position::Sticky => style.clone_order(),
            Position::Absolute | Position::Fixed => 0,
        }
    } else {
        position_to_order(style.clone_position()) + float_to_order(style.clone_float())
    }
}
