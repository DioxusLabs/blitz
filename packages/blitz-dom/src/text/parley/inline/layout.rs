//! Measuring, breaking and placing an inline formatting context with Parley.

#[cfg(feature = "parallel-construct")]
use std::cell::RefCell;

use blitz_traits::node_id::NodeId;
#[cfg(feature = "parallel-construct")]
use parley::LayoutContext;
use parley::{AlignmentOptions, BreakReason, IndentOptions};
use style::properties::ComputedValues;
use style::values::{computed::CSSPixelLength, generics::text::GenericTextIndent};
use stylo_taffy::CopyStyleGetters;

#[cfg(feature = "floats")]
use parley::{BreakerState, LineBreakData, YieldData};

#[cfg(feature = "parallel-construct")]
use super::super::TextBrush;
use super::super::{TextContext, stylo_to_parley};
use super::{TextLayout, build};
use crate::text::{
    BoxMeasure, ContentWidths, FloatClear, FloatSide, InlineLayoutEngine, LineExclusions,
    LinesExtent, Placement,
};
#[cfg(feature = "floats")]
use crate::text::{FloatRequest, LineSlot, SlotSearch};

// FIXME: static thread_local FontCtx isn't necessarily correct in multi-document context.
// Should use thread_local crate with ThreadLocal value store in the Document.
#[cfg(feature = "parallel-construct")]
thread_local! {
    static LAYOUT_CTX: RefCell<Option<Box<LayoutContext<TextBrush>>>> = const { RefCell::new(None) };
}

/// Tolerance (in CSS px) for content fitting into a slot
#[cfg(feature = "floats")]
const FIT_TOLERANCE: f32 = 0.001;

/// Lay out the next line into `slot`
#[cfg(feature = "floats")]
fn set_line_slot(state: &mut BreakerState, slot: &LineSlot, slack: f32) {
    // With huge insets, f32 rounding can make the slot slightly wider than the layout
    let max_advance = (slot.width + slack).min(state.layout_max_advance());
    state.set_line_max_advance(max_advance);
    state.set_line_x(slot.left);
    state.set_line_y(slot.top as f64);
}

/// The slot to retry a just-broken line in if it doesn't fit the slot it was laid out into
/// (CSS 2.1 §9.5): a line taller than its slot is retried in the slot narrowed by the floats
/// beside its full height, and a line whose content overflows its slot is moved down to the
/// next slot (or below the floats if there is none). Like browsers, content that can't wrap
/// (`text-wrap-mode: nowrap`) is left to overflow its slot instead of being moved down.
#[cfg(feature = "floats")]
fn retry_line_slot(
    exclusions: &impl LineExclusions,
    slot: &LineSlot,
    line: &LineBreakData,
    wraps: bool,
    scale: f32,
) -> Option<LineSlot> {
    let segment = slot.segment?;
    let tolerance = FIT_TOLERANCE * scale;
    let line_y = line.line_y_start as f32;
    let line_height = (line.line_y_end - line.line_y_start) as f32;
    let advance = line.metrics.advance - line.metrics.hanging_advance;

    if wraps && advance > slot.width + tolerance {
        let next_slot = exclusions.slot(line_y, line_height, SlotSearch::After(segment));
        if next_slot.segment.is_some() {
            return Some(next_slot);
        }
        return Some(exclusions.slot(line_y, line_height, SlotSearch::BelowFloats));
    }

    if line_height <= slot.height {
        return None;
    }
    let narrowed_slot = exclusions.slot(line_y, line_height, SlotSearch::Any);
    let narrowed =
        narrowed_slot.left > slot.left + tolerance || narrowed_slot.width < slot.width - tolerance;
    narrowed.then_some(narrowed_slot)
}

impl InlineLayoutEngine for TextLayout {
    fn build_layouts(
        cx: &mut TextContext,
        nodes: &crate::NodeTree,
        scale: f32,
        layouts: &mut [(NodeId, &mut Self)],
    ) {
        #[cfg(feature = "parallel-construct")]
        {
            use rayon::prelude::*;
            let cx = &*cx;
            layouts.par_iter_mut().for_each(|(node_id, layout)| {
                let mut layout_ctx = LAYOUT_CTX
                    .take()
                    .unwrap_or_else(|| Box::new(LayoutContext::new()));
                let mut font_ctx = cx
                    .thread_font_contexts
                    .get_or(|| RefCell::new(Box::new(cx.font_ctx.lock().unwrap().clone())))
                    .borrow_mut();
                build::build_inline_layout_into(
                    nodes,
                    &mut layout_ctx,
                    &mut font_ctx,
                    layout,
                    scale,
                    *node_id,
                );
                LAYOUT_CTX.set(Some(layout_ctx));
            });
        }
        #[cfg(not(feature = "parallel-construct"))]
        for (node_id, layout) in layouts {
            build::build_inline_layout_into(
                nodes,
                &mut cx.layout_ctx,
                &mut cx.font_ctx.lock().unwrap(),
                layout,
                scale,
                *node_id,
            );
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.holds_nothing()
    }

    fn prepare(
        &mut self,
        sizes: &[BoxMeasure],
        style: Option<&::style::properties::ComputedValues>,
    ) {
        self.prepare_lines(sizes, style);
    }

    #[inline]
    fn content_widths(&mut self) -> ContentWidths {
        self.widths()
    }

    fn break_lines(
        &mut self,
        width: f32,
        padding_border: f32,
        style: Option<&::style::properties::ComputedValues>,
        exclusions: &mut impl LineExclusions,
    ) {
        self.break_into_lines(width, padding_border, style, exclusions);
    }

    #[inline]
    fn extent(&self) -> LinesExtent {
        self.lines_extent()
    }

    fn placements(&self) -> impl Iterator<Item = Placement> {
        self.box_placements()
    }
}

impl TextLayout {
    /// Whether the content holds no text and no inline boxes.
    pub(super) fn holds_nothing(&self) -> bool {
        self.text.is_empty() && self.layout.inline_boxes().len() == 0
    }

    /// Gives the inline boxes their sizes, and sets the first line's indent.
    pub(super) fn prepare_lines(&mut self, sizes: &[BoxMeasure], style: Option<&ComputedValues>) {
        let scale = self.layout.scale();
        self.floats.clear();
        self.floats.extend(sizes.iter().filter_map(|measured| {
            let margin = measured.margin;
            Some((
                measured.node.as_u64(),
                measured.float?,
                measured.clear,
                (margin.left + margin.right + measured.size.width) * scale,
                (margin.top + margin.bottom + measured.size.height) * scale,
            ))
        }));
        // The boxes are measured in the order the content holds them.
        let mut next = 0;
        let box_count = self.layout.inline_boxes().len();
        for index in 0..box_count {
            self.layout.edit_inline_box(index, |ibox| {
                let node = NodeId::from_u64(ibox.id);
                let measured = match sizes.get(next) {
                    Some(measured) if measured.node == node => {
                        next += 1;
                        Some(measured)
                    }
                    _ => sizes.iter().find(|measured| measured.node == node),
                };
                let measured = measured.filter(|_| ibox.kind == parley::InlineBoxKind::InFlow);
                let Some(measured) = measured else {
                    // An absolutely positioned box or a float takes no room in the line.
                    ibox.width = 0.0;
                    ibox.height = 0.0;
                    ibox.baseline = None;
                    return;
                };
                let margin = measured.margin;
                ibox.width = (margin.left + margin.right + measured.size.width) * scale;
                ibox.baseline = measured
                    .baseline
                    .map(|baseline| (margin.top + baseline) * scale);
                // Vertical margins adjust the space the box reserves in the line. A box with a
                // baseline splits that space into ascent (`margin.top + baseline`) and descent
                // (`margin.bottom + height - baseline`), either of which may be negative. A box
                // without a baseline sits on the baseline and cannot reserve negative space.
                // Kept finite: huge author lengths can sum to infinity, which is taller than
                // the line breaker's `f32::MAX` height limit, and it then yields
                // `MaxHeightExceeded` for this box forever without advancing.
                let margin_box_height = margin.top + margin.bottom + measured.size.height;
                ibox.height = if ibox.baseline.is_some() {
                    (margin_box_height * scale).min(f32::MAX)
                } else {
                    (margin_box_height.max(0.0) * scale).min(f32::MAX)
                };
            });
        }

        // Percentage indents do not contribute to intrinsic widths.
        self.set_indent(style, 0.0);
    }

    /// Sets the lines' `text-indent` as `style`, the inline root's computed style, says, with
    /// percentages of `basis` CSS pixels.
    fn set_indent(&mut self, style: Option<&ComputedValues>, basis: f32) {
        let text_indent = style
            .map(|s| s.slow_clone_text_indent())
            .unwrap_or_else(GenericTextIndent::zero);
        let amount = text_indent
            .length
            .resolve(CSSPixelLength::new(basis.max(0.0)))
            .px();
        self.layout.set_text_indent(
            amount * self.layout.scale(),
            IndentOptions {
                each_line: text_indent.each_line,
                hanging: text_indent.hanging,
            },
        );
    }

    /// The content's min-content and max-content widths, with its floats.
    pub(super) fn widths(&mut self) -> ContentWidths {
        // TODO: Cache content widths.
        //
        // This is a little tricky as the size of the inline boxes may depend on whether we are sizing under
        // and a min-content or max-content constraint. So if we want to compute both widths in one pass then
        // we need to store both a min-content and max-content size on each box.
        let widths = self.layout.calculate_content_widths();

        // When computing a max-content size the available width is effectively infinite, so
        // floats never wrap onto a new "band" due to a lack of horizontal space. They only move
        // below preceding floats when the `clear` property forces them to.
        //
        // Floats that share a band sit side-by-side and so their widths sum, whereas floats
        // pushed onto a new band (via `clear`) stack vertically and so we take the maximum extent
        // across bands rather than summing.
        let mut float_min: f32 = 0.0;
        let mut left_band: f32 = 0.0;
        let mut right_band: f32 = 0.0;
        let mut float_max: f32 = 0.0;
        for &(_, side, clear, width, _) in &self.floats {
            float_min = float_min.max(width);
            if matches!(clear, FloatClear::Left | FloatClear::Both) {
                left_band = 0.0;
            }
            if matches!(clear, FloatClear::Right | FloatClear::Both) {
                right_band = 0.0;
            }
            match side {
                FloatSide::Left => left_band += width,
                FloatSide::Right => right_band += width,
            }
            float_max = float_max.max(left_band + right_band);
        }

        ContentWidths {
            min: widths.min,
            max: widths.max + float_max,
            float_min,
        }
    }

    /// Breaks the lines around the floats, and aligns them.
    pub(super) fn break_into_lines(
        &mut self,
        width: f32,
        padding_border: f32,
        style: Option<&ComputedValues>,
        exclusions: &mut impl LineExclusions,
    ) {
        // Percentage indents are of the content box the lines are broken in.
        self.set_indent(style, width / self.layout.scale());

        // Content sized to fit exactly (e.g. shrink-to-fit) can end up a few ulps wider than
        // `width`: the width goes through round trips in CSS px, and the line breaker sums
        // advances in a different order to `calculate_content_widths`. Give the line breaker a
        // few ulps of slack in that case so that the content doesn't wrap.
        let max_advance = {
            let max_content_width = self.layout.calculate_content_widths().max;
            let fit_width = width.max(max_content_width);
            let rounding_error = (fit_width + padding_border).abs() * 8.0 * f32::EPSILON;
            if (max_content_width - width).abs() <= rounding_error {
                fit_width + rounding_error
            } else {
                width
            }
        };

        let (alignment, last_line_alignment) = style
            .map(|s| {
                (
                    stylo_to_parley::text_align(s.clone_text_align()),
                    stylo_to_parley::text_align_last(s.clone_text_align_last()),
                )
            })
            .unwrap_or((parley::layout::Alignment::Start, None));

        self.layout.set_alignment(
            alignment,
            AlignmentOptions {
                align_when_overflowing: false,
                last_line_alignment,
            },
        );

        self.line_bottoms = [0.0; 2];
        self.lines_broken_around_floats = 0;

        #[cfg(not(feature = "floats"))]
        {
            let _ = exclusions;
            self.layout.break_all_lines(Some(max_advance));
        }

        #[cfg(feature = "floats")]
        {
            let scale = self.layout.scale();
            let slack = max_advance - width;
            let wraps = style.is_none_or(|s| {
                s.clone_text_wrap_mode() == stylo_to_parley::stylo::TextWrapMode::Wrap
            });
            let floats = &self.floats;
            let line_bottoms = &mut self.line_bottoms;
            let line_count = &mut self.lines_broken_around_floats;

            let mut breaker = self.layout.break_lines();
            let mut line_slot = exclusions.slot(0.0, 0.0, SlotSearch::Any);
            let mut has_active_floats = line_slot.segment.is_some();
            // Floats met mid-line that don't fit beside it, placed below it once it is broken.
            let mut pending_floats: Vec<FloatRequest> = Vec::new();
            let state = breaker.state_mut();
            state.set_layout_max_advance(max_advance);
            set_line_slot(state, &line_slot, slack);

            // The state at the start of the current line, which is reverted to when the line
            // needs to be retried lower down (or narrower) to avoid floats.
            // `None` if the line can't be retried (no floats beside it, or a float was placed
            // part way through it).
            let mut line_start_state = has_active_floats.then(|| breaker.state().clone());

            while let Some(yield_data) = breaker.break_next() {
                match yield_data {
                    YieldData::LineBreak(line_break_data) => {
                        if let Some(saved_state) = line_start_state.take()
                            && let Some(retry_slot) = retry_line_slot(
                                exclusions,
                                &line_slot,
                                &line_break_data,
                                wraps,
                                scale,
                            )
                        {
                            // Floats queued during this line attempt will be encountered again
                            pending_floats.clear();
                            line_slot = retry_slot;
                            has_active_floats = line_slot.segment.is_some();
                            breaker.revert_to(saved_state);
                            set_line_slot(breaker.state_mut(), &line_slot, slack);
                            line_start_state = has_active_floats.then(|| breaker.state().clone());
                            continue;
                        }
                        line_bottoms[*line_count % 2] = line_break_data.line_y_end;
                        *line_count += 1;

                        let state = breaker.state_mut();

                        if !pending_floats.is_empty() {
                            let top = state.line_y() as f32;
                            for float in pending_floats.drain(..) {
                                exclusions.place(FloatRequest {
                                    block_start: top,
                                    ..float
                                });
                            }
                            has_active_floats = true;
                        }

                        if has_active_floats {
                            line_slot =
                                exclusions.slot(state.line_y() as f32, 0.0, SlotSearch::Any);
                            has_active_floats = line_slot.segment.is_some();
                            set_line_slot(state, &line_slot, slack);
                        } else {
                            state.set_line_x(0.0);
                            state.set_line_max_advance(max_advance);
                        }

                        line_start_state = has_active_floats.then(|| breaker.state().clone());
                    }
                    YieldData::MaxHeightExceeded(_data) => {
                        // TODO
                        continue;
                    }
                    YieldData::InlineBoxBreak(box_break_data) => {
                        // Only floats break the lines at their boxes.
                        let state = breaker.state_mut();
                        let key = box_break_data.inline_box_id;
                        if let Some(&(_, side, clear, inline_size, block_size)) =
                            floats.iter().find(|(id, ..)| *id == key)
                        {
                            let float = FloatRequest {
                                key,
                                side,
                                clear,
                                inline_size,
                                block_size,
                                block_start: state.line_y() as f32,
                            };
                            // A float that doesn't fit beside the content already on the line
                            // is placed below the line (CSS 2.1 §9.5.1), as are any floats after
                            // it (a float may not be higher than an earlier float).
                            let fits_on_line = pending_floats.is_empty()
                                && (box_break_data.advance <= 0.0
                                    || box_break_data.advance + inline_size
                                        <= state.line_max_advance() + FIT_TOLERANCE * scale);
                            if !fits_on_line {
                                pending_floats.push(float);
                            } else {
                                exclusions.place(float);
                                line_slot =
                                    exclusions.slot(state.line_y() as f32, 0.0, SlotSearch::Any);
                                has_active_floats = line_slot.segment.is_some();
                                set_line_slot(state, &line_slot, slack);

                                // The float can't be unplaced, so the line can no longer be
                                // retried
                                line_start_state = None;
                            }
                        }

                        // Floats are out-of-flow and must not contribute to the line's height.
                        state.append_inline_box_to_line(
                            box_break_data.advance,
                            f32::NEG_INFINITY,
                            f32::NEG_INFINITY,
                        );
                    }
                }
            }
            breaker.finish();
        }
    }

    /// How far the lines reach, and where their baselines are.
    pub(super) fn lines_extent(&self) -> LinesExtent {
        // Parley lays out empty text as a single strut-height line (text-editor semantics),
        // but a line box containing no text, inline boxes or other in-flow content is a
        // zero-height line box in CSS (CSS2 §9.4.2).
        let has_inline_content = !self.holds_nothing();

        let mut height = if has_inline_content {
            self.layout.height()
        } else {
            0.0
        };

        // A forced line break (e.g. `<br>` or a preserved newline) at the end of the inline
        // content ends the final line box without starting a new one. Parley still emits an
        // empty line after it (so that editors have a line to place the cursor on), so that
        // line is excluded from the measured height (and from the last baseline).
        let line_count = self.layout.len();
        let mut trailing_empty_line = None;
        if line_count >= 2
            && let (Some(prev_line), Some(last_line)) = (
                self.layout.get(line_count - 2),
                self.layout.get(line_count - 1),
            )
            && prev_line.break_reason() == BreakReason::Explicit
            && last_line.text_range().is_empty()
            && last_line.items().next().is_none()
        {
            height -= last_line.metrics().line_height;
            trailing_empty_line = Some(line_count - 1);
        }

        // Parley's height is the sum of the line heights, which misses the space skipped when a
        // line is moved down past floats, so extend it to the bottom of the last counted line
        // (Parley doesn't count an empty last line either).
        let counted_lines = match self.layout.get(line_count.wrapping_sub(1)) {
            Some(last_line) if last_line.is_empty() => line_count - 1,
            _ => trailing_empty_line.unwrap_or(line_count),
        };
        let broken = self.lines_broken_around_floats;
        if has_inline_content
            && let Some(bottom) = counted_lines
                .checked_sub(1)
                .filter(|&last| last < broken && last + 2 >= broken)
                .map(|last| self.line_bottoms[last % 2])
        {
            height = height.max(bottom as f32);
        }

        let line_baseline = |line: parley::Line<'_, _>| line.metrics().baseline;
        let first_baseline = has_inline_content
            .then(|| self.layout.lines().next().map(line_baseline))
            .flatten();
        let last_line_index = trailing_empty_line.unwrap_or(line_count).checked_sub(1);
        let last_baseline = has_inline_content
            .then(|| last_line_index.and_then(|i| self.layout.get(i)))
            .flatten()
            .map(line_baseline);

        LinesExtent {
            height,
            width: self.layout.width(),
            first_baseline,
            last_baseline,
        }
    }

    /// Where the lines put each inline box.
    pub(super) fn box_placements(&self) -> impl Iterator<Item = Placement> + '_ {
        // Lines that hold no boxes are not walked.
        let lines = (self.layout.inline_boxes().len() > 0).then(|| self.layout.lines());
        lines.into_iter().flatten().flat_map(|line| {
            let metrics = *line.metrics();
            let mut line_has_in_flow_content = false;
            line.items().filter_map(move |item| match item {
                parley::layout::PositionedLayoutItem::InlineBox(ibox) => {
                    let placement = Placement {
                        node: NodeId::from_u64(ibox.id),
                        x: ibox.x,
                        // An out-of-flow box is zero-sized, so this is the baseline for it.
                        top: ibox.y,
                        line_top: metrics.block_min_coord,
                        line_bottom: metrics.block_max_coord,
                        // A block-level box's static position is at the top of its line, or below
                        // it if in-flow content precedes it on the line (its hypothetical box
                        // would start a new line after that content).
                        block_start: if line_has_in_flow_content {
                            metrics.block_max_coord
                        } else {
                            metrics.block_min_coord
                        },
                    };
                    line_has_in_flow_content |= ibox.kind == parley::InlineBoxKind::InFlow;
                    Some(placement)
                }
                parley::layout::PositionedLayoutItem::GlyphRun(run) => {
                    line_has_in_flow_content |= run.advance() > 0.0;
                    None
                }
            })
        })
    }
}
