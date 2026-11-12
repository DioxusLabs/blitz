use blitz_traits::node_id::NodeId;
use style::values::computed::{BaselineSource, Contain, Float as StyloFloat};
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use taffy::{
    AvailableSpace, AxisStaticPosition, BlockContainerStyle, BlockContext, BlockFormattingContext,
    BoxSizing, CollapsibleMarginSet, CompactLength, CoreStyle as _, Direction, LayoutInput,
    LayoutOutput, LayoutPartialTree as _, MaybeMath as _, MaybeResolve as _, OofCandidate,
    OofCandidates, OofItemStyle, OofPositioningArea, Overflow, Point, RequestedAxis,
    ResolveOrZero as _, RunMode, Size, SizingMode,
};

#[cfg(feature = "floats")]
use taffy::{BlockItemStyle as _, Clear, Float};

use stylo_taffy::{CopyStyleGetters, StyleFlags};

use super::Frame;
use crate::layout::{LayoutPassState, resolve_calc_value};
use crate::node::TextLayout;
use crate::text::BackendLayout;
#[cfg(not(feature = "floats"))]
use crate::text::NoLineExclusions;
use crate::text::{
    BoxMeasure, FloatClear, FloatSide, InlineLayoutEngine as _, LineArea, LinesInputs, Measure,
    Placement,
};
#[cfg(feature = "floats")]
use crate::text::{FloatRequest, LineExclusions, LineSlot, PlacedFloat, SlotSearch};

/// Subtract a child's margins from the definite axes of the available space it is laid out in.
/// Taffy's convention is that the parent subtracts a child's margins from the available space
/// before passing it to the child, and that the child does not subtract them again.
pub(crate) fn subtract_margins(
    mut child_inputs: LayoutInput,
    margin: taffy::Rect<f32>,
) -> LayoutInput {
    child_inputs.available_space = child_inputs
        .available_space
        .maybe_sub(margin.sum_axes())
        .maybe_max(Size::ZERO);
    child_inputs
}

/// Layout inputs for a float in an inline formatting context, from `child_inputs`, those of the
/// pass: sized in the containing block's inline room less its margins, with any sizing keyword on
/// its `width` style resolved, and as tall as it needs.
pub(crate) fn float_box_inputs(
    width_style: taffy::Dimension,
    margin: taffy::Rect<f32>,
    child_inputs: LayoutInput,
) -> LayoutInput {
    let mut inputs = inline_box_inputs(width_style, margin, child_inputs);
    inputs.available_space.height = AvailableSpace::MaxContent;
    inputs
}

/// Layout inputs for an atomic inline box, with any sizing keyword on its `width` style
/// (`min-content`, `max-content`, `fit-content`, `fit-content(...)`, `stretch`) resolved
/// into the available space or known width the box is measured with.
pub(crate) fn inline_box_inputs(
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
    pub(crate) fn inline_span_cb_flags(&self, root_id: NodeId, box_id: NodeId) -> StyleFlags {
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
        let inline_layout = self.nodes[node_id]
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

        let suppresses_baseline = style.contain().suppresses_baseline();
        let overflow = style.overflow();
        let is_scroll_container =
            overflow.x.is_scroll_container() || overflow.y.is_scroll_container();
        drop(style);

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
        let frame = Frame {
            inputs,
            node_size,
            node_min_size,
            node_max_size,
            aspect_ratio,
            padding,
            border,
            scrollbar_gutter,
            container_pb,
            content_box_inset,
            child_inputs,
            available_space,
            collapses_through: has_styles_preventing_being_collapsed_through,
            suppresses_baseline,
            is_scroll_container,
            scale,
        };
        self.lay_out_lines(node_id, inline_layout, frame, block_ctx)
    }

    /// Measures the inline formatting context's boxes, breaks its lines and places them and what
    /// they hold, in the room `frame` describes.
    pub(super) fn lay_out_lines(
        &mut self,
        node_id: NodeId,
        mut inline_layout: Box<TextLayout>,
        frame: Frame,
        block_ctx: &mut BlockContext<'_>,
    ) -> LayoutOutput {
        let Frame {
            inputs,
            node_size,
            node_min_size,
            node_max_size,
            aspect_ratio,
            padding,
            border,
            scrollbar_gutter,
            container_pb,
            content_box_inset,
            child_inputs,
            available_space,
            collapses_through,
            suppresses_baseline,
            is_scroll_container,
            scale: _,
        } = frame;
        let known_dimensions = inputs.known_dimensions;

        // Short circuit if inline context contains no text or inline boxes
        if !collapses_through && inline_layout.backend.is_empty() {
            self.put_inline_layout(node_id, inline_layout);
            return LayoutOutput::from_outer_size(
                Size::ZERO.maybe_max(container_pb.sum_axes().map(Some)),
            );
        }

        // Whether the lines run down the page, where the backend sets them so, which turns what
        // Taffy measures across them.
        let vertical = BackendLayout::SETS_WRITING_MODES && {
            let node = &self.nodes[node_id];
            node.primary_styles()
                .map(|computed| computed.writing_mode.is_vertical())
                .or_else(|| {
                    node.parent.and_then(|parent| {
                        self.nodes[parent]
                            .primary_styles()
                            .map(|computed| computed.writing_mode.is_vertical())
                    })
                })
                .unwrap_or(false)
        };
        // What the pass needs of the atomic inlines, which says how far each is laid out to
        // measure it. A pass that places them lays them out.
        let pass = match inputs.run_mode {
            RunMode::ComputeSize if !vertical && inputs.axis == RequestedAxis::Horizontal => {
                Measure::InlineSizes
            }
            RunMode::ComputeSize => Measure::Sizes,
            _ => Measure::Layout,
        };
        // Where the boxes are placed, they are laid out. The requested axis means nothing to a
        // full layout, so every pass asks for one alike.
        let child_inputs = taffy::LayoutInput {
            run_mode: RunMode::PerformLayout,
            axis: RequestedAxis::Both,
            ..child_inputs
        };
        // The containing block's inline size, which percentages are of.
        let basis = match available_space.width {
            AvailableSpace::Definite(width) => width,
            _ => 0.0,
        };
        let sizes = self.measure_inline_boxes(node_id, child_inputs, pass);
        inline_layout.backend.prepare(
            self,
            node_id,
            LinesInputs {
                sizes: &sizes,
                pass,
                basis,
                vertical,
            },
        );
        if inline_layout.backend.line_flow().is_vertical() {
            return self.lay_out_vertical_lines(
                node_id,
                inline_layout,
                frame,
                LinePass { pass, child_inputs },
            );
        }

        let pbw = container_pb.horizontal_components().sum();
        let width = known_dimensions.width.map(|w| w - pbw).unwrap_or_else(|| {
            let widths = inline_layout.backend.content_widths();
            let computed_width = match available_space.width {
                AvailableSpace::MinContent => widths.min.max(widths.float_min),
                AvailableSpace::MaxContent => widths.max,
                AvailableSpace::Definite(limit) => limit.min(widths.max).max(widths.min),
            };

            node_size
                .width
                .unwrap_or(computed_width + pbw)
                .max(computed_width)
                .maybe_clamp(node_min_size.width, node_max_size.width)
                - pbw
        });

        // Set block context width if this is a block context root
        #[cfg(feature = "floats")]
        let is_bfc_root = block_ctx.is_bfc_root();
        #[cfg(feature = "floats")]
        if is_bfc_root {
            block_ctx.set_width(width + pbw);
        }

        // Create sub-context to account for the inline layout's padding/border
        #[cfg(feature = "floats")]
        let outer_block_ctx = block_ctx;
        #[cfg(feature = "floats")]
        let mut block_ctx = outer_block_ctx.sub_context(
            container_pb.top,
            taffy::FloatInsets::new(container_pb.left, container_pb.right),
        );
        #[cfg(not(feature = "floats"))]
        let _ = block_ctx;

        if inputs.run_mode == taffy::RunMode::ComputeSize
            && inputs.axis == RequestedAxis::Horizontal
        {
            self.put_inline_layout(node_id, inline_layout);

            let measured_size = inputs.known_dimensions.unwrap_or(taffy::Size {
                width,
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

        // Out-of-flow candidates bubbled up from this container and its in-flow subtree.
        // These are laid out by the out-of-flow positioning pass (`compute_oof_layout`).
        let mut oof_candidates = OofCandidates::new();

        // Where the content box ends where its `height` or `max-height` puts an end to it, which
        // `line-clamp: auto` keeps the lines within.
        let inset_height = content_box_inset.vertical_components().sum();
        let block_end = known_dimensions
            .height
            .or(node_size.height)
            .or(node_max_size.height)
            .map(|height| (height - inset_height).max(0.0));
        let room_above = if inline_layout.backend.takes_room_above() {
            let margin = self.nodes[node_id]
                .layout_style()
                .margin()
                .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
            self.room_above(node_id, margin.top, padding.top, container_pb.top, inputs)
        } else {
            0.0
        };
        let area = LineArea {
            width,
            room_above,
            block_end,
            sizes_only: pass != Measure::Layout,
            pass: self.nodes.geometry_generation(),
        };
        let style = self.nodes[node_id].primary_styles().map(|s| (*s).clone());
        // The floats the lines place are read back from the block formatting context.
        #[cfg(feature = "floats")]
        let float_mark = block_ctx.float_mark();
        #[cfg(feature = "floats")]
        inline_layout.backend.break_lines(
            &mut self.text,
            area,
            pbw,
            style.as_deref(),
            &mut TaffyFloats {
                ctx: &mut block_ctx,
                placed: Vec::new(),
            },
        );
        #[cfg(not(feature = "floats"))]
        inline_layout.backend.break_lines(
            &mut self.text,
            area,
            pbw,
            style.as_deref(),
            &mut NoLineExclusions,
        );
        drop(style);

        let extent = inline_layout.backend.extent(padding.bottom);
        #[cfg_attr(not(feature = "floats"), allow(unused_mut))]
        let mut height = extent.height;

        #[cfg(feature = "floats")]
        let float_height_contribution = block_ctx.floated_content_height_contribution();
        // A block formatting context's root holds its floats.
        #[cfg(feature = "floats")]
        if is_bfc_root {
            height = height.max(float_height_contribution);
        }

        // Note: `width` and `height` are content-box measurements of the inline content.
        // `known_dimensions` must not be substituted in here: those are border-box sizes, and
        // using them for the scrollable overflow rect (which adds padding below) would
        // double-count padding, incorrectly making the container's content overflow it.
        let measured_size = taffy::Size { width, height };

        // Unbreakable content (e.g. `white-space: pre` text) may overflow the max advance the
        // lines were broken at. `width` is the advance the lines were broken at, so the actual
        // extent of the line boxes must be taken from the laid-out lines for `content_size`.
        let content_width = width.max(extent.width);

        let clamped_size = inputs
            .known_dimensions
            .or(node_size)
            .unwrap_or(measured_size + content_box_inset.sum_axes())
            .maybe_clamp(node_min_size, node_max_size);
        let final_size = Size {
            width: clamped_size.width,
            height: clamped_size.height.max(
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
        let content = final_size - content_box_inset.sum_axes();
        inline_layout.block_offset = block_offset;
        inline_layout.backend.set_content_size(content);
        let line_box_top = container_pb.top + block_offset;

        // Store sizes and positions of inline boxes. A pass that only sizes the block places
        // nothing.
        let mut span_cb_flags = StyleFlags::empty();
        let placements = (pass == Measure::Layout)
            .then(|| inline_layout.backend.placements())
            .into_iter()
            .flatten()
            .enumerate();
        let frame_at = PlacedFrame {
            child_inputs,
            container_pb,
            line_box_top,
            content_box_inset,
            final_size,
            container_direction,
        };
        for (order, placed) in placements {
            if inputs.run_mode == RunMode::PerformLayout {
                span_cb_flags |= self.inline_span_cb_flags(node_id, placed.node);
            }
            self.place_inline_box(&placed, order as u32, &frame_at, &mut oof_candidates);
        }

        // Each float goes where Taffy placed it as the lines reached it.
        #[cfg(feature = "floats")]
        if inputs.run_mode == RunMode::PerformLayout {
            for (key, pos) in block_ctx.floats_placed_since(float_mark) {
                // The block's initial letter goes into the context alone: its box is painted from
                // the lines.
                let Some(node) = BackendLayout::float_node(key) else {
                    continue;
                };
                self.place_float(node, pos, child_inputs, container_pb, &mut oof_candidates);
            }
        }

        // Propagate the height consumed by floats placed within this container to the
        // enclosing block context (so that the BFC root can contain them)
        #[cfg(feature = "floats")]
        outer_block_ctx.add_child_floated_content_height_contribution(
            container_pb.top + float_height_contribution,
        );

        let line_baseline = |baseline: f32| baseline + line_box_top;
        let first_baseline = extent
            .first_baseline
            .filter(|_| !suppresses_baseline)
            .map(line_baseline);
        let last_baseline = extent
            .last_baseline
            .filter(|_| !suppresses_baseline)
            .map(line_baseline);

        if inputs.run_mode == RunMode::PerformLayout {
            inline_layout.span_cb_flags = span_cb_flags;
        }

        self.put_inline_layout(node_id, inline_layout);

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
                let mut rect = taffy::Rect {
                    left: 0.0,
                    right: content_extent.width,
                    top: 0.0,
                    bottom: content_extent.height + block_offset.max(0.0),
                };
                let client_width =
                    final_size.width - border.left - border.right - scrollbar_gutter.x;
                let is_rtl = container_direction.is_rtl();
                if is_rtl {
                    rect.left = (client_width - rect.right).min(0.0);
                    rect.right = client_width.max(0.0);
                }
                if is_scroll_container {
                    rect.left = if is_rtl { rect.left } else { 0.0 };
                    rect.top = 0.0;
                }
                taffy::ScrollableOverflow::new(
                    rect,
                    taffy::Point {
                        x: is_scroll_container && is_rtl,
                        y: false,
                    },
                )
            },
            baselines: taffy::Baselines {
                first: first_baseline,
                last: last_baseline,
            },
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            // A line of no height still stops the margins collapsing through: only phantom lines,
            // which make no line box, let them (CSS 2, section 8.3.1; CSS Inline 3, "invisible
            // line boxes").
            margins_can_collapse_through: !collapses_through
                && final_size.height == 0.0
                && measured_size.height == 0.0
                && extent.first_baseline.is_none(),
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

    /// Sets where a float the lines flowed around goes, and hands on the absolutely positioned
    /// boxes it holds.
    #[cfg(feature = "floats")]
    fn place_float(
        &mut self,
        node: NodeId,
        pos: taffy::Point<f32>,
        child_inputs: taffy::LayoutInput,
        container_pb: taffy::Rect<f32>,
        oof_candidates: &mut OofCandidates,
    ) {
        let (width_style, margin, padding, border) = {
            let style = self.child_layout_style(&self.nodes[node]);
            (
                style.size().width,
                style
                    .margin()
                    .resolve_or_zero(child_inputs.parent_size, resolve_calc_value),
                style
                    .padding()
                    .resolve_or_zero(child_inputs.parent_size, resolve_calc_value),
                style
                    .border()
                    .resolve_or_zero(child_inputs.parent_size, resolve_calc_value),
            )
        };
        let mut output = self.compute_child_layout(
            crate::taffy_node_id(node),
            float_box_inputs(width_style, margin, child_inputs),
        );
        let layout = self.nodes[node].unrounded_layout_mut();
        layout.size = output.size;
        layout.scrollable_overflow_rect = output.scrollable_overflow_rect;
        layout.location.x = pos.x + margin.left + container_pb.left;
        layout.location.y = pos.y + margin.top + container_pb.top;
        layout.padding = padding;
        layout.border = border;
        layout.margin = margin;
        // The candidates the float holds, from its own corner to this block's.
        if !output.oof_candidates.is_empty() {
            output.oof_candidates.translate(layout.location);
            oof_candidates.append(&mut output.oof_candidates);
        }
    }

    /// Sets where an atomic inline goes, or hands an absolutely positioned box to its containing
    /// block as a candidate placed from its static position, with every one the atomic inline
    /// holds.
    fn place_inline_box(
        &mut self,
        placed: &Placement,
        order: u32,
        at: &PlacedFrame,
        oof_candidates: &mut OofCandidates,
    ) {
        let PlacedFrame {
            child_inputs,
            container_pb,
            line_box_top,
            content_box_inset,
            final_size,
            container_direction,
        } = *at;
        let node = &self.nodes[placed.node];
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

        // A float goes where the lines flowed around it.
        #[cfg(feature = "floats")]
        if style.float() != Float::None {
            return;
        }

        let position = style.position();
        let is_absolute = position.is_out_of_flow();
        let item_direction = style.direction();
        // Inline formatting contexts have no `justify-items`/`align-items` for an
        // `auto` self-alignment to defer to, so it behaves as `normal`.
        let justify_self = OofItemStyle::justify_self(&style).unwrap_or(taffy::AlignItems::NORMAL);
        let align_self = OofItemStyle::align_self(&style).unwrap_or(taffy::AlignItems::NORMAL);

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
            //   within the line and spans the line box in the block axis.
            // - A block-level box's rectangle spans the containing block's content
            //   box in the inline axis and is zero-height where the lines say it starts.
            let line_top = placed.line_top + line_box_top;
            let line_bottom = placed.line_bottom + line_box_top;
            let (inline_area, block_area) = if is_inline_level {
                let x = placed.x + container_pb.left;
                (
                    taffy::Line { start: x, end: x },
                    taffy::Line {
                        start: line_top,
                        end: line_bottom,
                    },
                )
            } else {
                let block_start = placed.block_start + line_box_top;
                (
                    taffy::Line {
                        start: container_pb.left,
                        end: final_size.width - container_pb.right,
                    },
                    taffy::Line {
                        start: block_start,
                        end: block_start,
                    },
                )
            };
            // An inline-level box's start faces the way the lines say, where they do; in a
            // vertical block the horizontal axis runs across the lines.
            let inline_rtl = match placed.rtl {
                Some(rtl) if is_inline_level => rtl,
                _ => container_direction.is_rtl(),
            };

            oof_candidates.push(OofCandidate {
                node: crate::taffy_node_id(placed.node),
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
                        inline_rtl,
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
            return;
        }

        // Re-measure the box to get its border-box size (this hits the layout
        // cache). The size cannot be recovered from the room the line reserves for it,
        // as that is clamped to be non-negative.
        let mut output = self.compute_child_layout(crate::taffy_node_id(placed.node), box_inputs);
        let size = output.size;

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
        // The inline boxes it is in move it as `position: relative` moves them, where the
        // backend paints them moved.
        let shift = BackendLayout::inline_shift(
            self,
            placed.node,
            container_content_size,
            container_direction == Direction::Rtl,
        );

        let layout = self.nodes[placed.node].unrounded_layout_mut();
        layout.size = size;
        layout.scrollable_overflow_rect = output.scrollable_overflow_rect;
        layout.location.x = placed.x + margin.left + container_pb.left + inset_offset.x + shift.x;
        // The lines place the margin box; offset to the border box even when a
        // negative top margin makes it extend above the margin box.
        layout.location.y = placed.top + margin.top + line_box_top + inset_offset.y + shift.y;
        layout.padding = padding;
        layout.border = border;
        layout.margin = margin;

        // Translate anchors from item-relative to container-relative
        // coordinates and collect candidates bubbled from the box's subtree
        if !output.oof_candidates.is_empty() {
            let location = layout.location;
            output.oof_candidates.translate(location);
            oof_candidates.append(&mut output.oof_candidates);
        }
    }

    /// Lays out an inline formatting context whose lines run down the page: the room along them
    /// is the box's height, and how far they stack across is its width. Taffy has no writing
    /// modes, so the box is sized here in physical terms, and what the lines place is turned onto
    /// the page. The floats of the blocks around it are not in its lines' way: Taffy places them
    /// in horizontal terms.
    fn lay_out_vertical_lines(
        &mut self,
        node_id: NodeId,
        mut inline_layout: Box<TextLayout>,
        frame: Frame,
        lines: LinePass,
    ) -> LayoutOutput {
        let Frame {
            inputs,
            node_size,
            node_min_size,
            node_max_size,
            padding,
            border,
            scrollbar_gutter,
            container_pb,
            content_box_inset,
            available_space,
            scale,
            ..
        } = frame;
        let LinePass { pass, child_inputs } = lines;
        let known_dimensions = inputs.known_dimensions;
        let flow = inline_layout.backend.line_flow();
        let margin = self.nodes[node_id]
            .layout_style()
            .margin()
            .resolve_or_zero(inputs.parent_size.width, resolve_calc_value);
        let pbh = container_pb.vertical_components().sum();
        // Lengths along and across the lines are rounded up to whole device pixels.
        let ceil = |length: f32| (length * scale).ceil() / scale;
        // Its own height, or where it has none the height it has room for, or failing both as
        // long as its longest line. A block in a block container whose lines run the same way
        // stretches into the room; any other box fits its content into it.
        let widths = inline_layout.backend.content_widths();
        let fit = |along: f32| ceil(along.min(widths.max).max(widths.min));
        let along = match known_dimensions.height.or(node_size.height) {
            Some(height) => height - pbh,
            None => {
                let room = match available_space.height {
                    AvailableSpace::Definite(height) => Some(height),
                    _ => None,
                };
                // The initial containing block's height, which an orthogonal flow without a
                // definite parent height fits into.
                let icb = self.stylist.device().au_viewport_size().height.to_f32_px();
                // An orthogonal flow whose parent has no definite height has at most the
                // parent's `max-height`, as Chrome takes its fallback inline size.
                let room = match self.parent_max_height(node_id) {
                    Some(cap) => {
                        let cap = (cap.min(icb) - margin.vertical_components().sum()).max(0.0);
                        Some(room.map_or(cap, |room| room.min(cap)))
                    }
                    None => room,
                };
                // Asked for its intrinsic height, it answers its lines' intrinsic length.
                let intrinsic = inputs.run_mode == RunMode::ComputeSize
                    && inputs.axis == RequestedAxis::Vertical;
                match room {
                    None if intrinsic && available_space.height == AvailableSpace::MinContent => {
                        ceil(widths.min)
                    }
                    None if intrinsic => ceil(widths.max),
                    Some(room) if self.stretches_along(node_id) => room - pbh,
                    Some(room) => fit(room - pbh),
                    // Within a parent of definite height, an orthogonal flow fits into that
                    // height, less its margins, and within any other parent into the initial
                    // containing block.
                    None => match inputs.parent_size.height {
                        Some(height) => fit(height - margin.vertical_components().sum() - pbh),
                        None if self.is_orthogonal(node_id) => {
                            fit(icb - margin.vertical_components().sum() - pbh)
                        }
                        None => ceil(widths.max),
                    },
                }
            }
        };
        // The padding at the block's start and end, which a vertical line's over and under sides
        // face: the right and left where the lines stack from the right, the left and right in
        // the others.
        let (start_padding, end_padding) = if flow.stacks_from_right() {
            (padding.right, padding.left)
        } else {
            (padding.left, padding.right)
        };
        let area = LineArea {
            width: along,
            room_above: start_padding,
            block_end: None,
            sizes_only: pass != Measure::Layout,
            pass: self.nodes.geometry_generation(),
        };
        let style = self.nodes[node_id].primary_styles().map(|s| (*s).clone());
        crate::text::PERF_COUNTS[1].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // Lines that run down the page place their floats in the lines' own axes, which Taffy's
        // float placement takes for its own: in a block formatting context of the block's own,
        // as wide as its lines are long.
        #[cfg(feature = "floats")]
        let floats = {
            let mut bfc = BlockFormattingContext::new();
            let mut ctx = bfc.root_block_context();
            ctx.set_width(along);
            let mut room = TaffyFloats {
                ctx: &mut ctx,
                placed: Vec::new(),
            };
            inline_layout.backend.break_lines(
                &mut self.text,
                area,
                pbh,
                style.as_deref(),
                &mut room,
            );
            room.placed
        };
        #[cfg(not(feature = "floats"))]
        let floats: [(u64, crate::text::PlacedFloat); 0] = {
            inline_layout.backend.break_lines(
                &mut self.text,
                area,
                pbh,
                style.as_deref(),
                &mut NoLineExclusions,
            );
            []
        };
        drop(style);
        // How far the lines and the floats reach across the block from its start.
        let across = floats
            .iter()
            .map(|(_, float)| float.bottom)
            .fold(inline_layout.backend.extent(end_padding).height, f32::max);
        let measured_size = Size {
            width: ceil(across),
            height: along,
        };
        // A vertical block in a horizontal one is an orthogonal flow: where its width is `auto`
        // it is as wide as its lines stack, as Chrome sizes it, rather than stretched across its
        // container.
        let auto_width = self.nodes[node_id].layout_style().size().width.is_auto();
        let own_size = Size {
            width: if auto_width {
                None
            } else {
                known_dimensions.width.or(node_size.width)
            },
            height: known_dimensions.height.or(node_size.height),
        };
        let final_size = own_size
            .unwrap_or(measured_size + content_box_inset.sum_axes())
            .maybe_clamp(node_min_size, node_max_size)
            .maybe_max(container_pb.sum_axes().map(Some));
        let content = final_size - content_box_inset.sum_axes();
        inline_layout.block_offset = 0.0;
        inline_layout.backend.set_content_size(content);

        let mut oof_candidates = OofCandidates::new();
        let container_direction = self.nodes[node_id].layout_style().direction();
        // A pass that only sizes the block places nothing.
        if pass == Measure::Layout {
            let placements: Vec<Placement> = inline_layout.backend.placements().collect();
            let frame_at = PlacedFrame {
                child_inputs,
                container_pb,
                line_box_top: container_pb.top,
                content_box_inset,
                final_size,
                container_direction,
            };
            for (order, placed) in placements.iter().enumerate() {
                self.place_inline_box(placed, order as u32, &frame_at, &mut oof_candidates);
            }
            #[cfg(feature = "floats")]
            for (key, float) in &floats {
                let Some(node) = BackendLayout::float_node(*key) else {
                    continue;
                };
                let (x, y) = inline_layout
                    .backend
                    .place_on_page([float.left, float.right], [float.top, float.bottom]);
                let pos = taffy::Point { x, y };
                self.place_float(node, pos, child_inputs, container_pb, &mut oof_candidates);
            }
        }
        self.put_inline_layout(node_id, inline_layout);

        let oof_position_inset = taffy::Rect {
            left: border.left,
            right: border.right + scrollbar_gutter.x,
            top: border.top,
            bottom: border.bottom + scrollbar_gutter.y,
        };
        let content_extent = measured_size + padding.sum_axes();
        LayoutOutput {
            size: final_size,
            scrollable_overflow_rect: taffy::ScrollableOverflow::new(
                taffy::Rect {
                    left: 0.0,
                    right: content_extent.width,
                    top: 0.0,
                    bottom: content_extent.height,
                },
                taffy::Point { x: false, y: false },
            ),
            baselines: taffy::Baselines {
                first: None,
                last: None,
            },
            top_margin: CollapsibleMarginSet::ZERO,
            bottom_margin: CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: false,
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

    /// Whether `node_id`, whose lines run down the page, stretches along them into the room it
    /// is given: whether it is an in-flow block in a block container whose lines run the same
    /// way.
    fn stretches_along(&self, node_id: NodeId) -> bool {
        let node = &self.nodes[node_id];
        if let Some(style) = node.primary_styles() {
            let display = style.slow_clone_display();
            if display.outside() != DisplayOutside::Block
                || style.get_box().float != style::computed_values::float::T::None
                || style.get_box().position.is_absolutely_positioned()
            {
                return false;
            }
        }
        let Some(parent) = node.layout_parent.get().map(|id| &self.nodes[id]) else {
            return false;
        };
        parent.taffy_display() == taffy::Display::Block
            && parent
                .primary_styles()
                .is_some_and(|style| style.writing_mode.is_vertical())
    }

    /// Whether `node_id`, whose lines run down the page, is an orthogonal flow: whether its
    /// layout parent's lines run across its own.
    fn is_orthogonal(&self, node_id: NodeId) -> bool {
        self.nodes[node_id]
            .layout_parent
            .get()
            .and_then(|parent| self.nodes[parent].primary_styles())
            .is_some_and(|style| !style.writing_mode.is_vertical())
    }

    /// The content height `node_id`'s layout parent may grow to where its own height is `auto`
    /// and its `max-height` a length: that length, no less than its `min-height`, in CSS pixels.
    fn parent_max_height(&self, node_id: NodeId) -> Option<f32> {
        let parent = self.nodes[node_id].layout_parent.get()?;
        let style = self.child_layout_style(&self.nodes[parent]);
        if !style.size().height.is_auto() {
            return None;
        }
        // The parent's own percentages are of a box this does not know.
        let unknown: Option<f32> = None;
        let max = style
            .max_size()
            .height
            .maybe_resolve(unknown, resolve_calc_value)?;
        let min = style
            .min_size()
            .height
            .maybe_resolve(unknown, resolve_calc_value)
            .unwrap_or(0.0);
        let edges = if style.box_sizing() == taffy::BoxSizing::BorderBox {
            let padding = style.padding().resolve_or_zero(unknown, resolve_calc_value);
            let border = style.border().resolve_or_zero(unknown, resolve_calc_value);
            (padding + border).vertical_components().sum()
        } else {
            0.0
        };
        Some((max.max(min) - edges).max(0.0))
    }

    /// The room over the block's first line its ruby annotations and emphasis marks may take
    /// before the line moves down for them, in CSS pixels, as Chrome's block layout lends it
    /// (`ComputeInitialBlockStartAnnotationSpace`): the block's padding over the line, and where
    /// no border is in the way and its margins collapse, the margin collapsed between it and the
    /// block before it, that block's end padding, and the room that block's last line left under
    /// its content.
    fn room_above(
        &self,
        node_id: NodeId,
        margin_top: f32,
        padding_top: f32,
        padding_border_top: f32,
        inputs: taffy::LayoutInput,
    ) -> f32 {
        let mut room = padding_top;
        // A block that starts a formatting context of its own (`overflow: hidden`, `flow-root`)
        // is lent no margin, as Chrome lends none to a new formatting context.
        if padding_border_top > padding_top || !inputs.vertical_margins_are_collapsible.start {
            return room;
        }
        let own = margin_top.max(0.0);
        let Some(before) = self.block_before(node_id) else {
            return room + own;
        };
        let node = &self.nodes[before];
        let style = self.child_layout_style(node);
        let parent_size = inputs.parent_size;
        let margin = style
            .margin()
            .resolve_or_zero(parent_size, resolve_calc_value)
            .bottom
            .max(0.0);
        let padding = style
            .padding()
            .resolve_or_zero(parent_size, resolve_calc_value)
            .bottom;
        let border = style
            .border()
            .resolve_or_zero(parent_size, resolve_calc_value)
            .bottom;
        drop(style);
        room += own.max(margin);
        if border > 0.0 {
            return room;
        }
        let lent = node
            .element_data()
            .and_then(|element| element.inline_layout_data.as_ref())
            .map_or(0.0, |text| text.backend.room_below());
        room + padding + lent.max(0.0)
    }

    /// The in-flow block laid out before `node_id` among its layout siblings, if there is one.
    fn block_before(&self, node_id: NodeId) -> Option<NodeId> {
        let parent = self.nodes[node_id].layout_parent.get()?;
        let children = self.nodes[parent].layout_children.borrow();
        let children = children.as_ref()?;
        let at = children.iter().position(|child| *child == node_id)?;
        children[..at].iter().rev().copied().find(|&child| {
            let node = &self.nodes[child];
            if !matches!(
                node.data,
                crate::NodeData::Element(_) | crate::NodeData::AnonymousBlock(_)
            ) {
                return false;
            }
            node.primary_styles().is_some_and(|computed| {
                !computed.get_box().display.is_none()
                    && !computed.slow_clone_position().is_absolutely_positioned()
                    && matches!(
                        computed.slow_clone_float(),
                        style::values::computed::Float::None
                    )
            })
        })
    }

    /// Hands the inline layout back to its node.
    pub(crate) fn put_inline_layout(&mut self, node_id: NodeId, inline_layout: Box<TextLayout>) {
        self.nodes[node_id]
            .data
            .downcast_element_mut()
            .unwrap()
            .inline_layout_data = Some(inline_layout);
    }

    /// Lays out each atomic inline of the inline formatting context rooted at `node_id` as its
    /// line holds it, and each float as the lines flow around it: its border box, and an atomic
    /// inline's baseline down from its top. Absolutely positioned boxes take no room in the lines,
    /// and are not measured.
    ///
    /// A float is sized in the room beside nothing, less its margins.
    ///
    /// `pass` says how far each box is laid out: a pass that places nothing only measures it,
    /// along the line alone in an inline-size pass.
    /// `child_inputs` are the inputs of a full layout.
    ///
    /// `baseline-source: auto` is the last baseline for an inline-block and the first otherwise.
    /// An inline-block's last baseline is its last line box's, and a flex, grid or table box's
    /// first baseline its first; a replaced element, or an inline-block other than a button that
    /// clips its overflow and takes its last baseline, has none, and sits on its margin box's
    /// bottom.
    fn measure_inline_boxes(
        &mut self,
        node_id: NodeId,
        child_inputs: taffy::LayoutInput,
        pass: Measure,
    ) -> Vec<BoxMeasure> {
        // Taken for the loop, which lays the children out, and put back after it.
        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        let mut sizes = Vec::with_capacity(children.as_ref().map_or(0, |children| children.len()));
        let rtl = self.nodes[node_id].primary_styles().is_some_and(|style| {
            style.get_inherited_box().direction == style::computed_values::direction::T::Rtl
        });
        for node in children.iter().flatten().copied() {
            let held = &self.nodes[node];
            let style = self.child_layout_style(held);
            let margin = style
                .margin()
                .resolve_or_zero(child_inputs.parent_size, resolve_calc_value);

            #[cfg(feature = "floats")]
            let is_floated = style.float().is_floated();
            #[cfg(not(feature = "floats"))]
            let is_floated = false;

            if style.position().is_out_of_flow() {
                continue;
            }
            if is_floated {
                // `inline-start` and `inline-end` are the containing block's sides, as its
                // direction has them.
                let float_side = match (held.primary_styles().map(|s| s.slow_clone_float()), rtl) {
                    (Some(StyloFloat::Right), _)
                    | (Some(StyloFloat::InlineStart), true)
                    | (Some(StyloFloat::InlineEnd), false) => FloatSide::Right,
                    _ => FloatSide::Left,
                };
                #[cfg(feature = "floats")]
                let clear = match style.clear() {
                    Clear::None => FloatClear::None,
                    Clear::Left => FloatClear::Left,
                    Clear::Right => FloatClear::Right,
                    Clear::Both => FloatClear::Both,
                };
                #[cfg(not(feature = "floats"))]
                let clear = FloatClear::None;
                let pass_inputs = match pass {
                    Measure::Layout => child_inputs,
                    Measure::Sizes => taffy::LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        ..child_inputs
                    },
                    Measure::InlineSizes => taffy::LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        axis: RequestedAxis::Horizontal,
                        ..child_inputs
                    },
                };
                let box_inputs = float_box_inputs(style.size().width, margin, pass_inputs);
                drop(style);
                let output = self.compute_child_layout(crate::taffy_node_id(node), box_inputs);
                sizes.push(BoxMeasure {
                    node,
                    size: output.size,
                    margin,
                    baseline: None,
                    float: Some(float_side),
                    clear,
                });
                continue;
            }
            // `baseline-source: auto` selects the last baseline for inline-blocks and the first
            // for other atomic inlines. A block-axis scroll-container inline-block (except buttons)
            // instead uses its bottom margin edge, but only with `auto` (css-align-3 §9.1).
            // `overflow: clip` is not a scroll container. Exported scroll-container baselines
            // are clamped to the border box.
            // A layout-contained box is treated as having no baseline (css-contain-1 §3.3).
            let overflow = style.overflow();
            let box_style = style.style.get_box();
            let is_flow = matches!(
                style.effective_display().inside(),
                DisplayInside::Flow | DisplayInside::FlowRoot
            );
            let baseline_source = box_style.slow_clone_baseline_source();
            let uses_last_baseline = match baseline_source {
                BaselineSource::Auto => is_flow,
                BaselineSource::First => false,
                BaselineSource::Last => true,
            };
            let is_scroll_container = !matches!(overflow.y, Overflow::Visible | Overflow::Clip);
            let is_block_axis_scroll_container = is_flow
                && is_scroll_container
                && !style
                    .flags
                    .intersects(StyleFlags::IS_BUTTON | StyleFlags::IS_BUTTON_INPUT);
            let contain_layout = box_style.slow_clone_contain().contains(Contain::LAYOUT);
            let exports_baseline = !(baseline_source == BaselineSource::Auto
                && is_block_axis_scroll_container)
                && !contain_layout;
            let pass_inputs = match pass {
                Measure::Layout => child_inputs,
                Measure::Sizes => taffy::LayoutInput {
                    run_mode: RunMode::ComputeSize,
                    ..child_inputs
                },
                Measure::InlineSizes => taffy::LayoutInput {
                    run_mode: RunMode::ComputeSize,
                    axis: RequestedAxis::Horizontal,
                    ..child_inputs
                },
            };
            let box_inputs = inline_box_inputs(style.size().width, margin, pass_inputs);
            drop(style);

            let output = self.compute_child_layout(crate::taffy_node_id(node), box_inputs);
            let baseline = if exports_baseline {
                let baseline = if uses_last_baseline {
                    output.baselines.last.or(output.baselines.first)
                } else {
                    output.baselines.first.or(output.baselines.last)
                };
                baseline.map(|baseline| {
                    if is_scroll_container {
                        baseline.clamp(0.0, output.size.height)
                    } else {
                        baseline
                    }
                })
            } else {
                None
            };
            sizes.push(BoxMeasure {
                node,
                size: output.size,
                margin,
                baseline,
                float: None,
                clear: FloatClear::None,
            });
        }
        *self.nodes[node_id].layout_children.borrow_mut() = children;
        sizes
    }
}

/// Taffy's block formatting context, as an inline formatting context's lines read and place
/// floats in it.
#[cfg(feature = "floats")]
pub(crate) struct TaffyFloats<'c, 'bfc> {
    pub(crate) ctx: &'c mut taffy::BlockContext<'bfc>,
    /// The floats placed so far, by key, in the order they were placed. Taffy cannot take a float
    /// back, so a float asked for again (as a trial break the lines take back asks again) stays
    /// where it was placed first.
    pub(crate) placed: Vec<(u64, PlacedFloat)>,
}

#[cfg(feature = "floats")]
impl LineExclusions for TaffyFloats<'_, '_> {
    fn slot(&self, top: f32, height: f32, search: SlotSearch) -> LineSlot {
        let (clear, after) = match search {
            SlotSearch::Any => (taffy::Clear::None, None),
            SlotSearch::After(segment) => (taffy::Clear::None, Some(segment)),
            SlotSearch::BelowFloats => (taffy::Clear::Both, None),
        };
        let slot = self.ctx.find_content_slot(top, height, clear, after);
        LineSlot {
            top: slot.y,
            left: slot.x,
            width: slot.width,
            height: slot.height,
            segment: slot.segment_id,
        }
    }

    fn place(&mut self, float: FloatRequest) -> PlacedFloat {
        if let Some(&(_, placed)) = self.placed.iter().find(|(key, _)| *key == float.key) {
            return placed;
        }
        let pos = self.ctx.place_floated_box(
            float.key,
            taffy::Size {
                width: float.inline_size,
                height: float.block_size,
            },
            float.block_start,
            match float.side {
                FloatSide::Left => taffy::FloatDirection::Left,
                FloatSide::Right => taffy::FloatDirection::Right,
            },
            match float.clear {
                FloatClear::None => taffy::Clear::None,
                FloatClear::Left => taffy::Clear::Left,
                FloatClear::Right => taffy::Clear::Right,
                FloatClear::Both => taffy::Clear::Both,
            },
            false,
        );
        let (left, top) = (pos.x, pos.y);
        let placed = PlacedFloat {
            left,
            right: left + float.inline_size,
            top,
            bottom: top + float.block_size,
        };
        self.placed.push((float.key, placed));
        placed
    }
}

/// What the pass that lays out an inline formatting context measured its boxes for.
#[derive(Clone, Copy)]
struct LinePass {
    pass: Measure,
    /// The inputs of a full layout of an atomic inline or a float.
    child_inputs: taffy::LayoutInput,
}

/// Where the boxes an inline formatting context's lines hold are placed from.
#[derive(Clone, Copy)]
struct PlacedFrame {
    /// The inputs of a full layout of an atomic inline.
    child_inputs: taffy::LayoutInput,
    /// The inline root's padding and border.
    container_pb: taffy::Rect<f32>,
    /// Where the line boxes start, down from the border box's top, in CSS pixels.
    line_box_top: f32,
    content_box_inset: taffy::Rect<f32>,
    /// The inline root's border box.
    final_size: Size<f32>,
    container_direction: Direction,
}
