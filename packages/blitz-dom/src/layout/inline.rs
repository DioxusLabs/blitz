use blitz_traits::node_id::NodeId;
use parley::{AlignmentOptions, BreakReason, IndentOptions};
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::values::specified::text::TextOverflowSide;
use style::values::{
    computed::{CSSPixelLength, Contain},
    generics::text::GenericTextIndent,
};
use taffy::{
    AvailableSpace, AxisStaticPosition, BlockContainerStyle, BlockContext, BlockFormattingContext,
    BoxSizing, CollapsibleMarginSet, CompactLength, CoreStyle as _, Direction, LayoutInput,
    LayoutOutput, LayoutPartialTree as _, MaybeMath as _, MaybeResolve as _, OofCandidate,
    OofCandidates, OofItemStyle, OofPositioningArea, Overflow, Point, RequestedAxis,
    ResolveOrZero as _, RunMode, Size, SizingMode,
};

#[cfg(feature = "floats")]
use parley::YieldData;
#[cfg(feature = "floats")]
use taffy::{BlockItemStyle as _, Clear, Float, prelude::TaffyMaxContent};

use stylo_taffy::StyleFlags;

use super::resolve_calc_value;
use super::text_overflow::{self, Marker, TextOverflowLayout};
use crate::BaseDocument;
use crate::layout::LayoutPassState;
use crate::node::TextBrush;
use crate::stylo_to_parley;

/// Subtract a child's margins from the definite axes of the available space it is laid out in.
/// Taffy's convention is that the parent subtracts a child's margins from the available space
/// before passing it to the child, and that the child does not subtract them again.
fn subtract_margins(mut child_inputs: LayoutInput, margin: taffy::Rect<f32>) -> LayoutInput {
    child_inputs.available_space = child_inputs
        .available_space
        .maybe_sub(margin.sum_axes())
        .maybe_max(Size::ZERO);
    child_inputs
}

/// Layout inputs for an atomic inline box, with any sizing keyword on its `width` style
/// (`min-content`, `max-content`, `fit-content`, `fit-content(...)`, `stretch`) resolved
/// into the available space or known width the box is measured with.
fn inline_box_inputs(
    width_style: taffy::Dimension,
    margin: taffy::Rect<f32>,
    child_inputs: LayoutInput,
) -> LayoutInput {
    let mut inputs = subtract_margins(child_inputs, margin);
    let stretch_width = inputs.available_space.width.into_option();
    let percent_basis = child_inputs.parent_size.width;

    match width_style.tag() {
        CompactLength::MIN_CONTENT_TAG => inputs.available_space.width = AvailableSpace::MinContent,
        CompactLength::MAX_CONTENT_TAG => inputs.available_space.width = AvailableSpace::MaxContent,
        CompactLength::FIT_CONTENT_PX_TAG => {
            inputs.available_space.width = AvailableSpace::Definite(width_style.value())
        }
        CompactLength::FIT_CONTENT_PERCENT_TAG => {
            if let Some(basis) = percent_basis {
                inputs.available_space.width =
                    AvailableSpace::Definite(basis * width_style.value());
            }
        }
        CompactLength::FIT_CONTENT_KEYWORD_TAG => {
            if let Some(width) = stretch_width {
                inputs.available_space.width = AvailableSpace::Definite(width);
            }
        }
        _ if width_style.is_fit_content_calc() => {
            if let Some(basis) = percent_basis {
                inputs.available_space.width =
                    AvailableSpace::Definite(resolve_calc_value(width_style.calc_value(), basis));
            }
        }
        CompactLength::STRETCH_TAG => {
            if let Some(width) = stretch_width {
                inputs.known_dimensions.width = Some(width);
                inputs.available_space.width = AvailableSpace::Definite(width);
            }
        }
        _ => {}
    }
    inputs
}

impl LayoutPassState<'_> {
    pub(crate) fn compute_inline_layout(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> taffy::LayoutOutput {
        let LayoutInput {
            known_dimensions,
            parent_size,
            run_mode,
            ..
        } = inputs;
        let style = self.nodes[node_id].layout_style();

        // Pull these out earlier to avoid borrowing issues
        let is_scroll_container =
            style.overflow().x.is_scroll_container() || style.overflow().y.is_scroll_container();
        let padding = style
            .padding()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let border = style
            .border()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let padding_border_size = (padding + border).sum_axes();
        let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
            padding_border_size
        } else {
            Size::ZERO
        };

        // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
        // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
        let (clamped_style_size, min_size, max_size, _aspect_ratio) = match inputs.sizing_mode {
            SizingMode::ContentSize => {
                let node_size = known_dimensions;
                let node_min_size = Size::NONE;
                let node_max_size = Size::NONE;
                (node_size, node_min_size, node_max_size, None)
            }
            SizingMode::InherentSize => {
                let aspect_ratio = style.aspect_ratio();
                let style_size = style
                    .size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_apply_aspect_ratio(aspect_ratio)
                    .maybe_add(box_sizing_adjustment);
                let style_min_size = style
                    .min_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_apply_aspect_ratio(aspect_ratio)
                    .maybe_add(box_sizing_adjustment);
                let style_max_size = style
                    .max_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);

                let node_size =
                    known_dimensions.or(style_size.maybe_clamp(style_min_size, style_max_size));
                (node_size, style_min_size, style_max_size, aspect_ratio)
            }
        };

        // If both min and max in a given axis are set and max <= min then this determines the size in that axis
        let min_max_definite_size = min_size.zip_map(max_size, |min, max| match (min, max) {
            (Some(min), Some(max)) if max <= min => Some(min),
            _ => None,
        });

        let styled_based_known_dimensions = known_dimensions
            .or(min_max_definite_size)
            .or(clamped_style_size)
            .maybe_max(padding_border_size);

        // Short-circuit layout if the container's size is fully determined by the container's size and the run mode
        // is ComputeSize (and thus the container's size is all that we're interested in)
        if run_mode == RunMode::ComputeSize {
            if let Size {
                width: Some(width),
                height: Some(height),
            } = styled_based_known_dimensions
            {
                return LayoutOutput::from_outer_size(Size { width, height });
            }
        }

        drop(style);

        // Unwrap the block formatting context if one was passed, or else create a new one
        match block_ctx {
            Some(inherited_bfc) if !is_scroll_container => self.compute_inline_layout_inner(
                node_id,
                LayoutInput {
                    known_dimensions: styled_based_known_dimensions,
                    ..inputs
                },
                inherited_bfc,
            ),
            _ => {
                let mut root_bfc = BlockFormattingContext::new();
                let mut root_ctx = root_bfc.root_block_context();
                self.compute_inline_layout_inner(
                    node_id,
                    LayoutInput {
                        known_dimensions: styled_based_known_dimensions,
                        ..inputs
                    },
                    &mut root_ctx,
                )
            }
        }
    }

    /// The out-of-flow positions for which an inline span between the inline box `box_id` and its
    /// inline root `root_id` is a containing block (as `SPAN_*_CB` flags)
    fn inline_span_cb_flags(&self, root_id: NodeId, box_id: NodeId) -> StyleFlags {
        let root = &self.nodes[root_id];
        // The spans of an anonymous inline root are descendants of its parent
        let root_parent = root.is_anonymous().then_some(root.parent).flatten();

        let mut flags = StyleFlags::empty();
        let mut current = self.nodes[box_id].parent;
        while let Some(id) = current {
            if id == root_id || Some(id) == root_parent {
                return flags;
            }
            let ancestor = &self.nodes[id];
            if let Some(style) = ancestor.primary_styles() {
                let display = style.clone_display();
                if display.outside() == DisplayOutside::Inline
                    && display.inside() == DisplayInside::Flow
                {
                    let claims = stylo_taffy::convert::inline_containing_block_claims(&style);
                    if claims.absolute {
                        flags |= StyleFlags::SPAN_ABSOLUTE_CB;
                    }
                    if claims.fixed {
                        flags |= StyleFlags::SPAN_FIXED_CB;
                    }
                }
            }
            current = ancestor.parent;
        }
        StyleFlags::empty()
    }

    fn compute_inline_layout_inner(
        &mut self,
        node_id: NodeId,
        inputs: taffy::tree::LayoutInput,
        block_ctx: &mut BlockContext<'_>,
    ) -> taffy::LayoutOutput {
        let scale = self.viewport.scale();
        let LayoutInput {
            known_dimensions,
            parent_size,
            available_space,
            sizing_mode,
            ..
        } = inputs;

        // Take inline layout to satisfy borrow checker
        let mut inline_layout = self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .take_inline_layout()
            .unwrap();

        let style = self.nodes[node_id].layout_style();

        // Note: both horizontal and vertical percentage padding/borders are resolved against the container's inline size (i.e. width).
        // This is not a bug, but is how CSS is specified (see: https://developer.mozilla.org/en-US/docs/Web/CSS/padding#values)
        let padding = style
            .padding()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let border = style
            .border()
            .resolve_or_zero(parent_size.width, resolve_calc_value);
        let container_pb = padding + border;
        let pb_sum = container_pb.sum_axes();
        let box_sizing_adjustment = if style.box_sizing() == BoxSizing::ContentBox {
            pb_sum
        } else {
            Size::ZERO
        };

        // Scrollbar gutters are reserved when the `overflow` property is set to `Overflow::Scroll`.
        // However, the axis are switched (transposed) because a node that scrolls vertically needs
        // *horizontal* space to be reserved for a scrollbar
        let scrollbar_gutter = style.overflow().transpose().map(|overflow| match overflow {
            Overflow::Scroll => style.scrollbar_width(),
            _ => 0.0,
        });
        // TODO: make side configurable based on the `direction` property
        let mut content_box_inset = container_pb;
        content_box_inset.right += scrollbar_gutter.x;
        content_box_inset.bottom += scrollbar_gutter.y;

        let has_styles_preventing_being_collapsed_through = !style.is_block()
            || style.overflow().x.is_scroll_container()
            || style.overflow().y.is_scroll_container()
            || style.position().is_out_of_flow()
            || padding.top > 0.0
            || padding.bottom > 0.0
            || border.top > 0.0
            || border.bottom > 0.0;
        // || matches!(node_size.height, Some(h) if h > 0.0)
        // || matches!(node_min_size.height, Some(h) if h > 0.0)
        // || !inline_layout.text.is_empty();
        // || inline_layout.layout.inline_boxes().len() > 0;

        // Resolve node's preferred/min/max sizes (width/heights) against the available space (percentages resolve to pixel values)
        // For ContentSize mode, we pretend that the node has no size styles as these should be ignored.
        let (node_size, node_min_size, node_max_size, aspect_ratio) = match sizing_mode {
            SizingMode::ContentSize => {
                let node_size = known_dimensions;
                let node_min_size = Size::NONE;
                let node_max_size = Size::NONE;
                (node_size, node_min_size, node_max_size, None)
            }
            SizingMode::InherentSize => {
                let aspect_ratio = style.aspect_ratio();
                let style_size = style
                    .size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_apply_aspect_ratio(aspect_ratio)
                    .maybe_add(box_sizing_adjustment);
                let style_min_size = style
                    .min_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_apply_aspect_ratio(aspect_ratio)
                    .maybe_add(box_sizing_adjustment);
                let style_max_size = style
                    .max_size()
                    .maybe_resolve(parent_size, resolve_calc_value)
                    .maybe_add(box_sizing_adjustment);

                let node_size =
                    known_dimensions.or(style_size.maybe_clamp(style_min_size, style_max_size));
                (node_size, style_min_size, style_max_size, aspect_ratio)
            }
        };

        drop(style);

        // Short circuit if inline context contains no text or inline boxes
        if !has_styles_preventing_being_collapsed_through
            && inline_layout.text.is_empty()
            && inline_layout.layout.inline_boxes().len() == 0
        {
            // Put layout back
            self.nodes[node_id]
                .data
                .downcast_element_mut()
                .unwrap()
                .inline_layout_data = Some(inline_layout);
            return LayoutOutput::from_outer_size(
                Size::ZERO.maybe_max(container_pb.sum_axes().map(Some)),
            );
        }

        // Compute available space
        let available_space = Size {
            width: known_dimensions
                .width
                .map(AvailableSpace::from)
                .unwrap_or(available_space.width)
                .maybe_set(known_dimensions.width)
                .maybe_set(node_size.width)
                .map_definite_value(|size| {
                    (size.maybe_clamp(node_min_size.width, node_max_size.width)
                        - content_box_inset.horizontal_axis_sum())
                    .max(0.0)
                }),
            height: known_dimensions
                .height
                .map(AvailableSpace::from)
                .unwrap_or(available_space.height)
                .maybe_set(known_dimensions.height)
                .maybe_set(node_size.height)
                .map_definite_value(|size| {
                    (size.maybe_clamp(node_min_size.height, node_max_size.height)
                        - content_box_inset.vertical_axis_sum())
                    .max(0.0)
                }),
        };

        // Compute size of inline boxes
        let child_inputs = taffy::tree::LayoutInput {
            known_dimensions: Size::NONE,
            available_space,
            sizing_mode: SizingMode::InherentSize,
            parent_size: Size {
                width: available_space.width.into_option(),
                // Anonymous blocks do not establish the containing block for percentages.
                height: if self.nodes[node_id].is_anonymous() {
                    parent_size.height
                } else {
                    available_space.height.into_option()
                },
            },
            // Atomic inlines (e.g. inline-block) establish independent formatting
            // contexts: their margins never collapse with their children's margins.
            vertical_margins_are_collapsible: taffy::Line::FALSE,
            ..inputs
        };
        #[cfg(feature = "floats")]
        let float_child_inputs = taffy::tree::LayoutInput {
            available_space: Size::MAX_CONTENT,
            ..child_inputs
        };

        // Update inline boxes
        for ibox in inline_layout.layout.inline_boxes_mut() {
            let style = self.child_layout_style(&self.nodes[NodeId::from_u64(ibox.id)]);
            let margin = style
                .margin()
                .resolve_or_zero(inputs.parent_size, resolve_calc_value);

            #[cfg(feature = "floats")]
            let is_floated = style.float().is_floated();
            #[cfg(not(feature = "floats"))]
            let is_floated = false;

            let is_out_of_flow = style.position().is_out_of_flow();
            // The baseline of an inline-block is the baseline of its last in-flow line box,
            // unless it has no line boxes or it is a block-axis scroll container, in which
            // case it is the bottom margin edge (CSS 2 §10.8.1, css-align-3 §9.1
            // `baseline-source: auto`; `overflow: clip` is not a scroll container). Other
            // atomic inlines (flex, grid, table) export a baseline regardless of `overflow`,
            // clamped to their border box if they are scroll containers (css-align-3 §9.1).
            // A layout-contained box is treated as having no baseline (css-contain-1 §3.3).
            let overflow = style.overflow();
            let box_style = style.style.get_box();
            let is_flow = matches!(
                box_style.display.inside(),
                DisplayInside::Flow | DisplayInside::FlowRoot
            );
            let is_scroll_container = !matches!(overflow.y, Overflow::Visible | Overflow::Clip);
            let is_block_axis_scroll_container = is_flow && is_scroll_container;
            let contain_layout = box_style.clone_contain().contains(Contain::LAYOUT);
            let exports_baseline = !is_block_axis_scroll_container && !contain_layout;
            let box_inputs = inline_box_inputs(style.size().width, margin, child_inputs);
            drop(style);

            if is_out_of_flow || is_floated {
                ibox.width = 0.0;
                ibox.height = 0.0;
                ibox.baseline = None;
            } else {
                let output = self.compute_child_layout(taffy::NodeId::from(ibox.id), box_inputs);
                ibox.width = (margin.left + margin.right + output.size.width) * scale;
                ibox.baseline = if exports_baseline {
                    output
                        .baselines
                        .last
                        .or(output.baselines.first)
                        .map(|baseline| {
                            let baseline = if is_scroll_container {
                                baseline.clamp(0.0, output.size.height)
                            } else {
                                baseline
                            };
                            (margin.top + baseline) * scale
                        })
                } else {
                    None
                };
                // Vertical margins adjust the space the box reserves in the line. A box with a
                // baseline splits that space into ascent (`margin.top + baseline`) and descent
                // (`margin.bottom + height - baseline`), either of which may be negative. A box
                // without a baseline sits on the baseline and cannot reserve negative space.
                // Kept finite: huge author lengths can sum to infinity, which is taller than
                // the line breaker's `f32::MAX` height limit, and it then yields
                // `MaxHeightExceeded` for this box forever without advancing.
                let margin_box_height = margin.top + margin.bottom + output.size.height;
                ibox.height = if ibox.baseline.is_some() {
                    (margin_box_height * scale).min(f32::MAX)
                } else {
                    (margin_box_height.max(0.0) * scale).min(f32::MAX)
                };
            }
        }

        let text_indent = self.nodes[node_id]
            .primary_styles()
            .map(|s| s.clone_text_indent())
            .unwrap_or_else(GenericTextIndent::zero);
        let indent_options = IndentOptions {
            each_line: text_indent.each_line,
            hanging: text_indent.hanging,
        };
        // Percentage indents do not contribute to intrinsic widths.
        inline_layout.layout.set_text_indent(
            text_indent.length.resolve(CSSPixelLength::new(0.0)).px() * scale,
            indent_options,
        );

        let pbw = container_pb.horizontal_components().sum() * scale;
        let width = known_dimensions
            .width
            .map(|w| (w * scale) - pbw)
            .unwrap_or_else(|| {
                // TODO: Cache content widths.
                //
                // This is a little tricky as the size of the inline boxes may depend on whether we are sizing under
                // and a min-content or max-content constraint. So if we want to compute both widths in one pass then
                // we need to store both a min-content and max-content size on each box.
                let content_sizes = inline_layout.layout.calculate_content_widths();
                let min_content_width = content_sizes.min;
                let max_content_width = content_sizes.max;

                #[cfg(feature = "floats")]
                let float_width = match available_space.width {
                    AvailableSpace::Definite(_) => 0.0,
                    AvailableSpace::MinContent => {
                        let mut width: f32 = 0.0;
                        for ibox in inline_layout.layout.inline_boxes_mut() {
                            let (is_floated, margin) = {
                                let style =
                                    self.child_layout_style(&self.nodes[NodeId::from_u64(ibox.id)]);
                                (
                                    style.float().is_floated(),
                                    style
                                        .margin()
                                        .resolve_or_zero(inputs.parent_size, resolve_calc_value),
                                )
                            };

                            if is_floated {
                                let output = self.compute_child_layout(
                                    taffy::NodeId::from(ibox.id),
                                    subtract_margins(child_inputs, margin),
                                );
                                width = width.max(output.size.width + margin.left + margin.right);
                            }
                        }

                        width * scale
                    }
                    AvailableSpace::MaxContent => {
                        // When computing a max-content size the available width is effectively
                        // infinite, so floats never wrap onto a new "band" due to a lack of
                        // horizontal space. They only move below preceding floats when the `clear`
                        // property forces them to.
                        //
                        // Floats that share a band sit side-by-side and so their widths sum, whereas
                        // floats pushed onto a new band (via `clear`) stack vertically and so we
                        // take the maximum extent across bands rather than summing.
                        let mut left_band: f32 = 0.0;
                        let mut right_band: f32 = 0.0;
                        let mut width: f32 = 0.0;
                        for ibox in inline_layout.layout.inline_boxes_mut() {
                            let (float, clear, margin) = {
                                let style =
                                    self.child_layout_style(&self.nodes[NodeId::from_u64(ibox.id)]);
                                (
                                    style.float(),
                                    style.clear(),
                                    style
                                        .margin()
                                        .resolve_or_zero(inputs.parent_size, resolve_calc_value),
                                )
                            };

                            if float.is_floated() {
                                if matches!(clear, Clear::Left | Clear::Both) {
                                    left_band = 0.0;
                                }
                                if matches!(clear, Clear::Right | Clear::Both) {
                                    right_band = 0.0;
                                }

                                let output = self.compute_child_layout(
                                    taffy::NodeId::from(ibox.id),
                                    subtract_margins(child_inputs, margin),
                                );
                                let box_width = output.size.width + margin.left + margin.right;

                                match float {
                                    Float::Left => left_band += box_width,
                                    Float::Right => right_band += box_width,
                                    Float::None => {}
                                }
                                width = width.max(left_band + right_band);
                            }
                        }

                        width * scale
                    }
                };

                #[cfg(not(feature = "floats"))]
                let float_width = 0.0;

                let computed_width = match available_space.width {
                    AvailableSpace::MinContent => min_content_width.max(float_width),
                    AvailableSpace::MaxContent => max_content_width + float_width,
                    AvailableSpace::Definite(limit) => (limit * scale)
                        .min(max_content_width + float_width)
                        .max(min_content_width),
                }
                .ceil();

                let style_width = node_size.width.map(|w| w * scale);
                let min_width = node_min_size.width.map(|w| w * scale);
                let max_width = node_max_size.width.map(|w| w * scale);

                (style_width)
                    .unwrap_or(computed_width + pbw)
                    .max(computed_width)
                    .maybe_clamp(min_width, max_width)
                    - pbw
            });

        #[cfg(not(feature = "floats"))]
        let _ = block_ctx; // Suppress unused variable warning

        // Set block context width if this is a block context root
        #[cfg(feature = "floats")]
        let is_bfc_root = block_ctx.is_bfc_root();
        #[cfg(feature = "floats")]
        if is_bfc_root {
            block_ctx.set_width((width + pbw) / scale);
        }

        // Create sub-context to account for the inline layout's padding/border
        #[cfg(feature = "floats")]
        let outer_block_ctx = block_ctx;
        #[cfg(feature = "floats")]
        let mut block_ctx =
            outer_block_ctx.sub_context(container_pb.top, [container_pb.left, container_pb.right]);
        // block_ctx.apply_content_box_inset([container_pb.left, container_pb.right]);

        if inputs.run_mode == taffy::RunMode::ComputeSize
            && inputs.axis == RequestedAxis::Horizontal
        {
            // Put layout back
            self.nodes[node_id]
                .data
                .downcast_element_mut()
                .unwrap()
                .inline_layout_data = Some(inline_layout);

            let measured_size = inputs.known_dimensions.unwrap_or(taffy::Size {
                width: width.ceil() / scale,
                // Height is ignored if RequestedAxis if Horizontal
                height: 0.0,
            });

            let clamped_size = inputs
                .known_dimensions
                .or(node_size)
                .unwrap_or(measured_size + content_box_inset.sum_axes())
                .maybe_clamp(node_min_size, node_max_size)
                .maybe_max(container_pb.sum_axes().map(Some));

            return LayoutOutput::from_outer_size(clamped_size);
        }

        let resolved_text_indent = text_indent
            .length
            .resolve(CSSPixelLength::new((width / scale).max(0.0)))
            .px();
        inline_layout
            .layout
            .set_text_indent(resolved_text_indent * scale, indent_options);

        #[cfg(not(feature = "floats"))]
        {
            inline_layout.layout.break_all_lines(Some(width));
        }

        // Out-of-flow candidates bubbled up from this container and its in-flow subtree.
        // These are laid out by the out-of-flow positioning pass (`compute_oof_layout`).
        let mut oof_candidates = OofCandidates::new();

        // Whether any line may have been laid out next to a float, and so be
        // shorter than the container.
        #[cfg(not(feature = "floats"))]
        let had_floats = false;
        #[cfg(feature = "floats")]
        let mut had_floats = false;

        // Perform inline layout
        #[cfg(feature = "floats")]
        {
            let mut breaker = inline_layout.layout.break_lines();
            let initial_slot = block_ctx.find_content_slot(0.0, Clear::None, None);
            let mut has_active_floats = initial_slot.segment_id.is_some();
            had_floats |= has_active_floats;
            let state = breaker.state_mut();
            state.set_layout_max_advance(width);
            state.set_line_max_advance(initial_slot.width * scale);
            state.set_line_x(initial_slot.x * scale);
            state.set_line_y((initial_slot.y * scale) as f64);

            // TODO: revert state and retry layout if a line doesn't fit
            //
            // Save initial state. Saved state is used to revert the layout to a previous state if needed
            // (e.g. to revert a line that doesn't fit in the space it was laid out into)
            //
            // let mut saved_state = breaker.state().clone();

            while let Some(yield_data) = breaker.break_next() {
                match yield_data {
                    YieldData::LineBreak(_line_break_data) => {
                        let state = breaker.state_mut();

                        if has_active_floats {
                            // TODO: revert state and retry layout if a line doesn't fit
                            // saved_state = state.clone();

                            let min_y = state.line_y() / scale as f64;
                            let next_slot =
                                block_ctx.find_content_slot(min_y as f32, Clear::None, None);
                            has_active_floats = next_slot.segment_id.is_some();
                            had_floats |= has_active_floats;

                            state.set_line_max_advance(next_slot.width * scale);
                            state.set_line_x(next_slot.x * scale);
                            state.set_line_y((next_slot.y * scale) as f64);
                        } else {
                            state.set_line_x(0.0);
                            state.set_line_max_advance(width);
                        }

                        continue;
                    }
                    YieldData::MaxHeightExceeded(_data) => {
                        // TODO
                        continue;
                    }
                    YieldData::InlineBoxBreak(box_break_data) => {
                        let state = breaker.state_mut();
                        let node_id = NodeId::from_u64(box_break_data.inline_box_id);

                        let (direction, clear, margin) = {
                            let style = self.child_layout_style(&self.nodes[node_id]);
                            // We can assume that the box is a float because we only set `break_on_box: true` for floats
                            let direction = match style.float() {
                                Float::Left => taffy::FloatDirection::Left,
                                Float::Right => taffy::FloatDirection::Right,
                                Float::None => unreachable!(),
                            };
                            (
                                direction,
                                style.clear(),
                                style
                                    .margin()
                                    .resolve_or_zero(inputs.parent_size, resolve_calc_value),
                            )
                        };

                        let margin_sum = margin.sum_axes();

                        let mut output = self.compute_child_layout(
                            crate::taffy_node_id(node_id),
                            float_child_inputs,
                        );
                        let min_y = state.line_y() as f32 / scale;

                        // Note: `pos` is content-box relative
                        let pos = block_ctx.place_floated_box(
                            output.size + margin_sum,
                            min_y,
                            direction,
                            clear,
                            false,
                        );

                        let min_y = state.line_y() / scale as f64; //.max(pos.y as f64);
                        let next_slot =
                            block_ctx.find_content_slot(min_y as f32, Clear::None, None);
                        has_active_floats = next_slot.segment_id.is_some();
                        had_floats |= has_active_floats;

                        state.set_line_max_advance(next_slot.width * scale);
                        state.set_line_x(next_slot.x * scale);
                        state.set_line_y((next_slot.y * scale) as f64);

                        let location = taffy::Point {
                            x: pos.x + margin.left + container_pb.left,
                            y: pos.y + margin.top + container_pb.top,
                        };
                        let layout = self.nodes[node_id].unrounded_layout_mut();
                        layout.size = output.size;
                        layout.location = location;
                        layout.margin = margin;

                        // Translate anchors from item-relative to container-relative
                        // coordinates and collect candidates bubbled from the float's subtree
                        if !output.oof_candidates.is_empty() {
                            output.oof_candidates.translate(location);
                            oof_candidates.append(&mut output.oof_candidates);
                        }

                        // dbg!(&layout.size);
                        // dbg!(&layout.location);

                        // Floats are out-of-flow and must not contribute to the line's height.
                        state.append_inline_box_to_line(
                            box_break_data.advance,
                            f32::NEG_INFINITY,
                            f32::NEG_INFINITY,
                        );

                        // if float.is_floated() {
                        //     println!("INLINE FLOATED BOX ({}) {:?}", ibox.id, float);
                        //     println!(
                        //         "w:{} h:{} x:{}, y:{}",
                        //         layout.size.width, layout.size.height, 0, 0
                        //     );
                        // }
                    }
                }
            }
            breaker.finish();
        }

        // Propagate the height consumed by floats placed within this container to the
        // enclosing block context (so that the BFC root can contain them)
        #[cfg(feature = "floats")]
        let float_height_contribution = block_ctx.floated_content_height_contribution();
        #[cfg(feature = "floats")]
        outer_block_ctx.add_child_floated_content_height_contribution(
            container_pb.top + float_height_contribution,
        );

        let (alignment, last_line_alignment) = self.nodes[node_id]
            .primary_styles()
            .map(|s| {
                (
                    stylo_to_parley::text_align(s.clone_text_align()),
                    stylo_to_parley::text_align_last(s.clone_text_align_last()),
                )
            })
            .unwrap_or((parley::layout::Alignment::Start, None));

        inline_layout.layout.align(
            alignment,
            AlignmentOptions {
                align_when_overflowing: false,
                last_line_alignment,
            },
        );

        // `text-overflow`: lines are final now, so find the ones to truncate and
        // shape their marker (layout units: `width` is already scaled). Read from
        // style here rather than at construction so that a style-only change
        // (which relayouts without reconstructing) is honoured.
        inline_layout.overflow =
            self.compute_text_overflow(node_id, &inline_layout.layout, width, scale, had_floats);

        // Parley lays out empty text as a single strut-height line (text-editor semantics),
        // but a line box containing no text, inline boxes or other in-flow content is a
        // zero-height line box in CSS (CSS2 §9.4.2).
        let has_inline_content =
            !inline_layout.text.is_empty() || inline_layout.layout.inline_boxes().len() > 0;

        let mut height = if has_inline_content {
            inline_layout.layout.height()
        } else {
            0.0
        };

        // A forced line break (e.g. `<br>` or a preserved newline) at the end of the inline
        // content ends the final line box without starting a new one. Parley still emits an
        // empty line after it (so that editors have a line to place the cursor on), so that
        // line is excluded from the measured height (and from the last baseline).
        let line_count = inline_layout.layout.len();
        let mut trailing_empty_line = None;
        if line_count >= 2
            && let (Some(prev_line), Some(last_line)) = (
                inline_layout.layout.get(line_count - 2),
                inline_layout.layout.get(line_count - 1),
            )
            && prev_line.break_reason() == BreakReason::Explicit
            && last_line.text_range().is_empty()
            && last_line.items().next().is_none()
        {
            height -= last_line.metrics().line_height;
            trailing_empty_line = Some(line_count - 1);
        }

        #[cfg(feature = "floats")]
        {
            if is_bfc_root {
                height = height.max(float_height_contribution * scale)
            };
        }

        // Note: `width` and `height` are content-box measurements of the inline content.
        // `known_dimensions` must not be substituted in here: those are border-box sizes, and
        // using them for the scrollable overflow rect (which adds padding below) would
        // double-count padding, incorrectly making the container's content overflow it.
        let measured_size = taffy::Size {
            width: width / scale,
            height: height / scale,
        };

        // Unbreakable content (e.g. `white-space: pre` text) may overflow the max advance the
        // lines were broken at. `width` is the advance the lines were broken at, so the actual
        // extent of the line boxes must be taken from the laid-out lines for `content_size`.
        let content_width = f32_max(width, inline_layout.layout.width()) / scale;

        let clamped_size = inputs
            .known_dimensions
            .or(node_size)
            .unwrap_or(measured_size + content_box_inset.sum_axes())
            .maybe_clamp(node_min_size, node_max_size);
        let final_size = Size {
            width: clamped_size.width,
            height: f32_max(
                clamped_size.height,
                aspect_ratio
                    .map(|ratio| clamped_size.width / ratio)
                    .unwrap_or(0.0),
            ),
        }
        .maybe_max(container_pb.sum_axes().map(Some));

        let container_direction = self.nodes[node_id].layout_style().direction();

        // `align-content` aligns the line boxes (as a single unit) within the content box in
        // the block axis. Floats are positioned relative to the formatting context and are not
        // moved. The offset is stored on the text layout for painting and hit-testing.
        let align_content = BlockContainerStyle::align_content(&self.nodes[node_id].layout_style());
        let block_offset = if align_content.keyword() == taffy::AlignContentKeyword::Normal {
            0.0
        } else {
            let free_space =
                final_size.height - content_box_inset.vertical_axis_sum() - measured_size.height;
            taffy::compute_block_align_content_offset(align_content, free_space)
        };
        inline_layout.block_offset = block_offset;
        let line_box_top = container_pb.top + block_offset;

        // Store sizes and positions of inline boxes
        let mut ibox_order: u32 = 0;
        let mut span_cb_flags = StyleFlags::empty();
        for line in inline_layout.layout.lines() {
            for item in line.items() {
                if let parley::layout::PositionedLayoutItem::InlineBox(ibox) = item {
                    let order = ibox_order;
                    ibox_order += 1;
                    if inputs.run_mode == RunMode::PerformLayout {
                        span_cb_flags |=
                            self.inline_span_cb_flags(node_id, NodeId::from_u64(ibox.id));
                    }
                    let node = &self.nodes[NodeId::from_u64(ibox.id)];
                    let style = self.child_layout_style(node);
                    let padding = style
                        .padding()
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
                    let border = style
                        .border()
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);
                    let margin = style
                        .margin()
                        .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);

                    #[cfg(feature = "floats")]
                    let is_floated = style.float() != Float::None;
                    #[cfg(not(feature = "floats"))]
                    let is_floated = false;

                    let position = style.position();
                    let is_absolute = position.is_out_of_flow();
                    let item_direction = style.direction();
                    // Inline formatting contexts have no `justify-items`/`align-items` for an
                    // `auto` self-alignment to defer to, so it behaves as `normal`.
                    let justify_self =
                        OofItemStyle::justify_self(&style).unwrap_or(taffy::AlignItems::NORMAL);
                    let align_self =
                        OofItemStyle::align_self(&style).unwrap_or(taffy::AlignItems::NORMAL);

                    // The static position of an absolutely positioned box depends on the
                    // display its hypothetical box would have had (the display specified
                    // before position:absolute blockified it): inline-level boxes sit at
                    // their position within the line, while block-level boxes start at the
                    // content-box left edge of their containing block.
                    let is_inline_level =
                        style.style.get_box().original_display.outside() == DisplayOutside::Inline;

                    // Resolve relative inset offsets against the containing block
                    // (the content box of the inline container).
                    let container_content_size = final_size - content_box_inset.sum_axes();
                    let inset_style = style.inset();
                    let inset = taffy::Rect {
                        left: inset_style
                            .left
                            .maybe_resolve(container_content_size.width, resolve_calc_value),
                        right: inset_style
                            .right
                            .maybe_resolve(container_content_size.width, resolve_calc_value),
                        top: inset_style
                            .top
                            .maybe_resolve(container_content_size.height, resolve_calc_value),
                        bottom: inset_style
                            .bottom
                            .maybe_resolve(container_content_size.height, resolve_calc_value),
                    };
                    let box_inputs = inline_box_inputs(style.size().width, margin, child_inputs);
                    drop(style);

                    if is_absolute {
                        // The static-position rectangle
                        // (https://www.w3.org/TR/css-position-3/#staticpos-rect):
                        // - An inline-level box's rectangle is zero-width at its position
                        //   within the line (`ibox.y` is the baseline as out-of-flow boxes are
                        //   zero-sized) and spans the line box in the block axis.
                        // - A block-level box's rectangle spans the containing block's content
                        //   box in the inline axis and is zero-height below the line box.
                        let line_metrics = line.metrics();
                        let line_top = (line_metrics.block_min_coord / scale) + line_box_top;
                        let line_bottom = (line_metrics.block_max_coord / scale) + line_box_top;
                        let (inline_area, block_area) = if is_inline_level {
                            let x = (ibox.x / scale) + container_pb.left;
                            (
                                taffy::Line { start: x, end: x },
                                taffy::Line {
                                    start: line_top,
                                    end: line_bottom,
                                },
                            )
                        } else {
                            (
                                taffy::Line {
                                    start: container_pb.left,
                                    end: final_size.width - container_pb.right,
                                },
                                taffy::Line {
                                    start: line_bottom,
                                    end: line_bottom,
                                },
                            )
                        };

                        oof_candidates.push(OofCandidate {
                            node: taffy::NodeId::from(ibox.id),
                            order,
                            position,
                            static_position: taffy::Point {
                                x: AxisStaticPosition::from_alignment(
                                    justify_self.resolve_self_relative(
                                        item_direction,
                                        container_direction,
                                        true,
                                    ),
                                    inline_area,
                                    container_direction.is_rtl(),
                                ),
                                y: AxisStaticPosition::from_alignment(
                                    align_self.resolve_self_relative(
                                        item_direction,
                                        container_direction,
                                        false,
                                    ),
                                    block_area,
                                    false,
                                ),
                            },
                        });
                    } else if is_floated {
                        let layout = self.nodes[NodeId::from_u64(ibox.id)].unrounded_layout_mut();
                        layout.padding = padding; //.map(|p| p / scale);
                        layout.border = border; //.map(|p| p / scale);
                        layout.margin = margin;
                    } else {
                        // Re-measure the box to get its border-box size (this hits the layout
                        // cache). The size cannot be recovered from `ibox` dimensions as the
                        // space reserved in the line is clamped to be non-negative.
                        let mut output =
                            self.compute_child_layout(taffy::NodeId::from(ibox.id), box_inputs);
                        let size = output.size;
                        let node = &mut self.nodes[NodeId::from_u64(ibox.id)];

                        let is_relative = position == taffy::Position::Relative;
                        let inset_offset = if is_relative {
                            taffy::Point {
                                x: if container_direction == Direction::Rtl {
                                    inset.right.map(|x| -x).or(inset.left).unwrap_or(0.0)
                                } else {
                                    inset.left.or(inset.right.map(|x| -x)).unwrap_or(0.0)
                                },
                                y: inset.top.or(inset.bottom.map(|x| -x)).unwrap_or(0.0),
                            }
                        } else {
                            taffy::Point::ZERO
                        };

                        let layout = node.unrounded_layout_mut();
                        layout.size = size;
                        layout.scrollable_overflow_rect = output.scrollable_overflow_rect;
                        layout.location.x =
                            (ibox.x / scale) + margin.left + container_pb.left + inset_offset.x;
                        // Parley positions the margin box; offset to the border box even
                        // when a negative top margin makes it extend above the margin box.
                        layout.location.y =
                            (ibox.y / scale) + margin.top + line_box_top + inset_offset.y;
                        layout.padding = padding; //.map(|p| p / scale);
                        layout.border = border; //.map(|p| p / scale);
                        layout.margin = margin;

                        // Translate anchors from item-relative to container-relative
                        // coordinates and collect candidates bubbled from the box's subtree
                        if !output.oof_candidates.is_empty() {
                            let location = layout.location;
                            output.oof_candidates.translate(location);
                            oof_candidates.append(&mut output.oof_candidates);
                        }
                    }
                }
            }
        }

        // println!("INLINE LAYOUT FOR {:?}. max_advance: {:?}", node_id, max_advance);
        // dbg!(&inline_layout.text);
        // println!("Computed: w: {} h: {}", inline_layout.layout.width(), inline_layout.layout.height());
        // println!("known_dimensions: w: {:?} h: {:?}", inputs.known_dimensions.width, inputs.known_dimensions.height);
        // println!("\n");

        let line_baseline =
            |line: parley::Line<'_, _>| (line.metrics().baseline / scale) + line_box_top;
        let first_baseline = has_inline_content
            .then(|| inline_layout.layout.lines().next().map(line_baseline))
            .flatten();
        let last_line_index = trailing_empty_line.unwrap_or(line_count).checked_sub(1);
        let last_baseline = has_inline_content
            .then(|| last_line_index.and_then(|i| inline_layout.layout.get(i)))
            .flatten()
            .map(line_baseline);

        if inputs.run_mode == RunMode::PerformLayout {
            inline_layout.span_cb_flags = span_cb_flags;
        }

        // Put layout back
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);

        let oof_position_inset = taffy::Rect {
            left: border.left,
            right: border.right + scrollbar_gutter.x,
            top: border.top,
            bottom: border.bottom + scrollbar_gutter.y,
        };

        LayoutOutput {
            size: final_size,
            scrollable_overflow_rect: {
                let content_extent = taffy::Size {
                    width: content_width,
                    height: measured_size.height,
                } + padding.sum_axes();
                taffy::Rect {
                    left: 0.0,
                    right: content_extent.width,
                    top: 0.0,
                    bottom: content_extent.height + block_offset.max(0.0),
                }
            },
            baselines: taffy::Baselines {
                first: first_baseline,
                last: last_baseline,
            },
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: !has_styles_preventing_being_collapsed_through
                && final_size.height == 0.0
                && measured_size.height == 0.0,
            oof_candidates,
            oof_positioning_area: Some(OofPositioningArea {
                size: final_size - oof_position_inset.sum_axes(),
                offset: Point {
                    x: oof_position_inset.left,
                    y: oof_position_inset.top,
                },
            }),
        }
    }
}

#[inline(always)]
fn f32_max(a: f32, b: f32) -> f32 {
    a.max(b)
}

impl BaseDocument {
    /// The `text-overflow` record for an inline root whose lines were broken at
    /// `width` (layout units), or `None` if no line needs a marker.
    fn compute_text_overflow(
        &mut self,
        node_id: NodeId,
        layout: &parley::Layout<TextBrush>,
        width: f32,
        scale: f32,
        had_floats: bool,
    ) -> Option<Box<TextOverflowLayout<TextBrush>>> {
        // The property belongs to the block container. For the anonymous block
        // that wraps inline content next to block siblings that is the parent;
        // the anonymous wrappers around flex/grid items do not qualify.
        let node = &self.nodes[node_id];
        let owner_id = if node.is_anonymous() {
            node.parent?
        } else {
            node_id
        };
        let styles: style::servo_arc::Arc<style::properties::ComputedValues> =
            (*self.nodes[owner_id].primary_styles()?).clone();
        if owner_id != node_id
            && !matches!(
                styles.clone_display().inside(),
                DisplayInside::Flow | DisplayInside::FlowRoot
            )
        {
            return None;
        }

        // Only boxes that clip their inline overflow get a marker.
        if styles.get_box().overflow_x == style::values::computed::Overflow::Visible {
            return None;
        }

        // Only the inline-end marker is implemented. Stylo stores a single
        // value as `(clip, value)` with logical sides, and two values as
        // physical `(left, right)`.
        let is_rtl = layout.is_rtl();
        let text_overflow = styles.clone_text_overflow();
        let end_side = if is_rtl && !text_overflow.sides_are_logical {
            &text_overflow.first
        } else {
            &text_overflow.second
        };
        let marker_text = match end_side {
            TextOverflowSide::Clip => return None,
            TextOverflowSide::Ellipsis => "\u{2026}",
            TextOverflowSide::String(s) => s.as_ref(),
        };

        // Without floats every line box is as wide as the container, so if
        // the widest line fits, they all do.
        if !had_floats && layout.width() <= width + text_overflow::OVERFLOW_SLACK {
            return None;
        }

        let lines = text_overflow::overflowing_lines(layout, width);
        if lines.is_empty() {
            return None;
        }

        // The marker is styled by the block (css-overflow §5.2): shape it with
        // the block's parley style, which also gives font fallback.
        let parley_style = stylo_to_parley::style(owner_id, &styles);
        let mut marker_layout: parley::Layout<TextBrush> = parley::Layout::new();
        {
            let mut font_ctx = self.font_ctx.lock().unwrap();
            let mut builder =
                self.layout_ctx
                    .tree_builder(&mut font_ctx, scale, true, &parley_style);
            builder.push_text(marker_text);
            builder.build_into(&mut marker_layout);
        }
        marker_layout.break_all_lines(None);
        let marker = Marker::from_layout(&marker_layout, parley_style.brush)?;

        Some(Box::new(TextOverflowLayout {
            max_width: width,
            is_rtl,
            marker,
            lines,
        }))
    }
}
