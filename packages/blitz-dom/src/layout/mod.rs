//! Enable the dom to lay itself out using taffy
//!
//! In servo, style and layout happen together during traversal
//! However, in Blitz, we do a style pass then a layout pass.
//! This is slower, yes, but happens fast enough that it's not a huge issue.

use crate::node::{ImageData, NodeData, SpecialElementData};
use crate::{document::BaseDocument, node::Node};
use markup5ever::local_name;
use std::cell::Ref;
use std::sync::Arc;
use style::Atom;
use style::values::computed::CSSPixelLength;
use style::values::computed::length_percentage::CalcLengthPercentage;
use taffy::{
    BlockContext, CollapsibleMarginSet, FlexDirection, LayoutPartialTree, NodeId, ResolveOrZero,
    RoundTree, Style, TraversePartialTree, TraverseTree, compute_block_layout,
    compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_leaf_layout,
    prelude::*,
};

pub(crate) mod construct;
pub(crate) mod damage;
pub(crate) mod inline;
pub(crate) mod list;
pub(crate) mod replaced;
pub(crate) mod table;

use self::replaced::{ReplacedContext, replaced_measure_function};
use self::table::TableTreeWrapper;

pub(crate) fn resolve_calc_value(calc_ptr: *const (), parent_size: f32) -> f32 {
    let calc = unsafe { &*(calc_ptr as *const CalcLengthPercentage) };
    let result = calc.resolve(CSSPixelLength::new(parent_size));
    result.px()
}

impl BaseDocument {
    fn node_from_id(&self, node_id: taffy::prelude::NodeId) -> &Node {
        &self.nodes[node_id.into()]
    }
    fn node_from_id_mut(&mut self, node_id: taffy::prelude::NodeId) -> &mut Node {
        &mut self.nodes[node_id.into()]
    }
}

impl BaseDocument {
    fn compute_child_layout_internal(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::tree::LayoutOutput {
        let node = &mut self.nodes[node_id.into()];

        let font_styles = node.primary_styles().map(|style| {
            use style::values::computed::font::LineHeight;

            let font_size = style.clone_font_size().used_size().px();
            let line_height = match style.clone_line_height() {
                LineHeight::Normal => font_size * 1.2,
                LineHeight::Number(num) => font_size * num.0,
                LineHeight::Length(value) => value.0.px(),
            };

            (font_size, line_height)
        });
        let font_size = font_styles.map(|s| s.0);
        let resolved_line_height = font_styles.map(|s| s.1);

        match &mut node.data {
            NodeData::Text(data) => {
                // With the new "inline context" architecture all text nodes should be wrapped in an "inline layout context"
                // and should therefore never be measured individually.
                println!(
                    "ERROR: Tried to lay out text node individually ({})",
                    usize::from(node_id)
                );
                dbg!(data);
                taffy::LayoutOutput::HIDDEN
                // unreachable!();

                // compute_leaf_layout(inputs, &node.style, |known_dimensions, available_space| {
                //     let context = TextContext {
                //         text_content: &data.content.trim(),
                //         writing_mode: WritingMode::Horizontal,
                //     };
                //     let font_metrics = FontMetrics {
                //         char_width: 8.0,
                //         char_height: 16.0,
                //     };
                //     text_measure_function(
                //         known_dimensions,
                //         available_space,
                //         &context,
                //         &font_metrics,
                //     )
                // })
            }
            NodeData::Element(element_data) | NodeData::AnonymousBlock(element_data) => {
                // TODO: deduplicate with single-line text input
                if *element_data.name.local == *"textarea" {
                    let rows = element_data
                        .attr(local_name!("rows"))
                        .and_then(|val| val.parse::<f32>().ok())
                        .unwrap_or(2.0);

                    let cols = element_data
                        .attr(local_name!("cols"))
                        .and_then(|val| val.parse::<f32>().ok());

                    return compute_leaf_layout(
                        inputs,
                        &node.style,
                        resolve_calc_value,
                        |_known_size, _available_space| taffy::Size {
                            width: cols
                                .map(|cols| cols * font_size.unwrap_or(16.0) * 0.6)
                                .unwrap_or(300.0),
                            height: resolved_line_height.unwrap_or(16.0) * rows,
                        },
                    );
                }

                if *element_data.name.local == *"input" {
                    match element_data.attr(local_name!("type")) {
                        // if the input type is hidden, hide it
                        Some("hidden") => {
                            node.style.display = Display::None;
                            return taffy::LayoutOutput::HIDDEN;
                        }
                        Some("checkbox") => {
                            return compute_leaf_layout(
                                inputs,
                                &node.style,
                                resolve_calc_value,
                                |_known_size, _available_space| {
                                    let width = node.style.size.width.resolve_or_zero(
                                        inputs.parent_size.width,
                                        resolve_calc_value,
                                    );
                                    let height = node.style.size.height.resolve_or_zero(
                                        inputs.parent_size.height,
                                        resolve_calc_value,
                                    );
                                    let min_size = width.min(height);
                                    taffy::Size {
                                        width: min_size,
                                        height: min_size,
                                    }
                                },
                            );
                        }
                        None | Some("text" | "password" | "email" | "tel" | "url" | "search") => {
                            return compute_leaf_layout(
                                inputs,
                                &node.style,
                                resolve_calc_value,
                                |_known_size, _available_space| taffy::Size {
                                    width: match inputs.available_space.width {
                                        AvailableSpace::Definite(limit) => limit.min(300.0),
                                        AvailableSpace::MinContent => 0.0,
                                        AvailableSpace::MaxContent => 300.0,
                                    },
                                    height: resolved_line_height.unwrap_or(16.0),
                                },
                            );
                        }
                        _ => {}
                    }
                }

                if *element_data.name.local == *"img"
                    || *element_data.name.local == *"canvas"
                    || (cfg!(feature = "svg") && *element_data.name.local == *"svg")
                {
                    // Get width and height attributes on image element
                    //
                    // TODO: smarter sizing using these (depending on object-fit, they shouldn't
                    // necessarily just override the native size)
                    let attr_size = taffy::Size {
                        width: element_data
                            .attr(local_name!("width"))
                            .and_then(|val| val.parse::<f32>().ok()),
                        height: element_data
                            .attr(local_name!("height"))
                            .and_then(|val| val.parse::<f32>().ok()),
                    };

                    // Get image's native sizespecial_data
                    let inherent_size = match &element_data.special_data {
                        SpecialElementData::Image(image_data) => match &**image_data {
                            ImageData::Raster(image) => taffy::Size {
                                width: image.width as f32,
                                height: image.height as f32,
                            },
                            #[cfg(feature = "svg")]
                            ImageData::Svg(svg) => {
                                let size = svg.size();
                                taffy::Size {
                                    width: size.width(),
                                    height: size.height(),
                                }
                            }
                            ImageData::None => taffy::Size::ZERO,
                        },
                        SpecialElementData::Canvas(_) => taffy::Size::ZERO,
                        SpecialElementData::None => taffy::Size::ZERO,
                        _ => unreachable!(),
                    };

                    let replaced_context = ReplacedContext {
                        inherent_size,
                        attr_size,
                    };

                    let computed = replaced_measure_function(
                        inputs.known_dimensions,
                        inputs.parent_size,
                        inputs.available_space,
                        &replaced_context,
                        &node.style,
                        false,
                    );

                    return taffy::LayoutOutput {
                        size: computed,
                        content_size: computed,
                        first_baselines: taffy::Point::NONE,
                        top_margin: CollapsibleMarginSet::ZERO,
                        bottom_margin: CollapsibleMarginSet::ZERO,
                        margins_can_collapse_through: false,
                    };
                }

                if node.flags.is_table_root() {
                    let SpecialElementData::TableRoot(context) = &self.nodes[node_id.into()]
                        .data
                        .downcast_element()
                        .unwrap()
                        .special_data
                    else {
                        panic!("Node marked as table root but doesn't have TableContext");
                    };
                    let context = Arc::clone(context);

                    let mut table_wrapper = TableTreeWrapper {
                        doc: self,
                        ctx: context,
                    };
                    let mut output = compute_grid_layout(&mut table_wrapper, node_id, inputs);

                    // HACK: Cap content size at node size to prevent scrolling
                    output.content_size.width = output.content_size.width.min(output.size.width);
                    output.content_size.height = output.content_size.height.min(output.size.height);

                    return output;
                }

                if node.flags.is_inline_root() {
                    return self.compute_inline_layout(usize::from(node_id), inputs, block_ctx);
                }

                // The default CSS file will set
                match node.style.display {
                    Display::Block => compute_block_layout(self, node_id, inputs, block_ctx),
                    Display::Flex => compute_flexbox_layout(self, node_id, inputs),
                    Display::Grid => compute_grid_layout(self, node_id, inputs),
                    Display::None => taffy::LayoutOutput::HIDDEN,
                }
            }
            NodeData::Document => compute_block_layout(self, node_id, inputs, None),

            _ => taffy::LayoutOutput::HIDDEN,
        }
    }
}

impl TraversePartialTree for BaseDocument {
    type ChildIter<'a> = RefCellChildIter<'a>;

    fn child_ids(&self, node_id: NodeId) -> Self::ChildIter<'_> {
        let layout_children = self.node_from_id(node_id).layout_children.borrow(); //.unwrap().as_ref();
        RefCellChildIter::new(Ref::map(layout_children, |children| {
            children.as_ref().map(|c| c.as_slice()).unwrap_or(&[])
        }))
    }

    fn child_count(&self, node_id: NodeId) -> usize {
        self.node_from_id(node_id)
            .layout_children
            .borrow()
            .as_ref()
            .map(|c| c.len())
            .unwrap_or(0)
    }

    fn get_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        NodeId::from(
            self.node_from_id(node_id)
                .layout_children
                .borrow()
                .as_ref()
                .unwrap()[index],
        )
    }
}
impl TraverseTree for BaseDocument {}

impl LayoutPartialTree for BaseDocument {
    type CoreContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type CustomIdent = Atom;

    fn get_core_container_style(&self, node_id: NodeId) -> &Style<Atom> {
        &self.node_from_id(node_id).style
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_from_id_mut(node_id).unrounded_layout = *layout;
    }

    fn resolve_calc_value(&self, calc_ptr: *const (), parent_size: f32) -> f32 {
        resolve_calc_value(calc_ptr, parent_size)
    }

    #[inline(always)]
    fn compute_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::LayoutInput,
    ) -> taffy::LayoutOutput {
        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            tree.compute_child_layout_internal(node_id, inputs, None)
        })
    }
}

impl taffy::CacheTree for BaseDocument {
    #[inline]
    fn cache_get(
        &self,
        node_id: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: taffy::RunMode,
    ) -> Option<taffy::LayoutOutput> {
        self.node_from_id(node_id)
            .cache
            .get(known_dimensions, available_space, run_mode)
    }

    #[inline]
    fn cache_store(
        &mut self,
        node_id: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: taffy::RunMode,
        layout_output: taffy::LayoutOutput,
    ) {
        self.node_from_id_mut(node_id).cache.store(
            known_dimensions,
            available_space,
            run_mode,
            layout_output,
        );
    }

    #[inline]
    fn cache_clear(&mut self, node_id: NodeId) {
        self.node_from_id_mut(node_id).cache.clear();
    }
}

impl taffy::LayoutBlockContainer for BaseDocument {
    type BlockContainerStyle<'a>
        = &'a Style<Atom>
    where
        Self: 'a;

    type BlockItemStyle<'a>
        = &'a Style<Atom>
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }

    #[inline(always)]
    fn compute_block_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::LayoutOutput {
        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            tree.compute_child_layout_internal(node_id, inputs, block_ctx)
        })
    }
}

impl taffy::LayoutFlexboxContainer for BaseDocument {
    type FlexboxContainerStyle<'a>
        = &'a Style<Atom>
    where
        Self: 'a;

    type FlexboxItemStyle<'a>
        = &'a Style<Atom>
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }
}

impl taffy::LayoutGridContainer for BaseDocument {
    type GridContainerStyle<'a>
        = &'a Style<Atom>
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a Style<Atom>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }
}

impl RoundTree for BaseDocument {
    fn get_unrounded_layout(&self, node_id: NodeId) -> Layout {
        self.node_from_id(node_id).unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.node_from_id_mut(node_id).final_layout = *layout;
    }
}

impl PrintTree for BaseDocument {
    fn get_debug_label(&self, node_id: NodeId) -> &'static str {
        let node = &self.node_from_id(node_id);
        let style = &node.style;

        match node.data {
            NodeData::Document => "DOCUMENT",
            // NodeData::Doctype { .. } => return "DOCTYPE",
            NodeData::Text { .. } => node.node_debug_str().leak(),
            NodeData::Comment => "COMMENT",
            NodeData::AnonymousBlock(_) => "ANONYMOUS BLOCK",
            NodeData::Element(_) => {
                let display = match style.display {
                    Display::Flex => match style.flex_direction {
                        FlexDirection::Row | FlexDirection::RowReverse => "FLEX ROW",
                        FlexDirection::Column | FlexDirection::ColumnReverse => "FLEX COL",
                    },
                    Display::Grid => "GRID",
                    Display::Block => "BLOCK",
                    Display::None => "NONE",
                };
                format!("{} ({})", node.node_debug_str(), display).leak()
            } // NodeData::ProcessingInstruction { .. } => return "PROCESSING INSTRUCTION",
        }
    }

    fn get_final_layout(&self, node_id: NodeId) -> Layout {
        self.node_from_id(node_id).final_layout
    }
}

// pub struct ChildIter<'a>(std::slice::Iter<'a, usize>);
// impl<'a> Iterator for ChildIter<'a> {
//     type Item = NodeId;
//     fn next(&mut self) -> Option<Self::Item> {
//         self.0.next().copied().map(NodeId::from)
//     }
// }

pub struct RefCellChildIter<'a> {
    items: Ref<'a, [usize]>,
    idx: usize,
}
impl<'a> RefCellChildIter<'a> {
    fn new(items: Ref<'a, [usize]>) -> RefCellChildIter<'a> {
        RefCellChildIter { items, idx: 0 }
    }
}

impl Iterator for RefCellChildIter<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<Self::Item> {
        self.items.get(self.idx).map(|id| {
            self.idx += 1;
            NodeId::from(*id)
        })
    }
}
