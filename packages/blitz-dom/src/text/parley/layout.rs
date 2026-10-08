//! Measuring, breaking and placing an inline formatting context with Parley.

use blitz_traits::node_id::NodeId;
use parley::{AlignmentOptions, BreakReason, IndentOptions};
use style::properties::ComputedValues;
use style::values::{computed::CSSPixelLength, generics::text::GenericTextIndent};

#[cfg(feature = "floats")]
use parley::YieldData;

use super::{TextLayout, style as stylo_to_parley};
#[cfg(feature = "floats")]
use crate::text::FloatRequest;
use crate::text::{BoxMeasure, ContentWidths, LineExclusions, LinesExtent, Placement};

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
                (margin.left + margin.right + measured.size.width) * scale,
                (margin.top + margin.bottom + measured.size.height) * scale,
            ))
        }));
        // The boxes are measured in the order the content holds them.
        let mut next = 0;
        for ibox in self.layout.inline_boxes_mut() {
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
                continue;
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
        //
        // A float takes room along the line only as the content is measured: with a soft wrap
        // opportunity either side, the widest float is as wide as the content gets at
        // min-content, and at max-content each paragraph is as wide as its text and its floats.
        let floats = &self.floats;
        let measured_as_floats = |ibox: &parley::InlineBox| {
            floats
                .iter()
                .find(|(id, ..)| *id == ibox.id)
                .map(|(_, _, width, _)| *width)
        };
        if !floats.is_empty() {
            for ibox in self.layout.inline_boxes_mut() {
                if let Some(width) = measured_as_floats(ibox) {
                    ibox.kind = parley::InlineBoxKind::InFlow;
                    ibox.width = width;
                }
            }
        }
        let widths = self.layout.calculate_content_widths();
        if !self.floats.is_empty() {
            let floats = &self.floats;
            for ibox in self.layout.inline_boxes_mut() {
                if floats.iter().any(|(id, ..)| *id == ibox.id) {
                    ibox.kind = parley::InlineBoxKind::CustomOutOfFlow;
                    ibox.width = 0.0;
                }
            }
        }
        ContentWidths {
            min: widths.min,
            max: widths.max,
        }
    }

    /// Breaks the lines around the floats, and aligns them.
    pub(super) fn break_into_lines(
        &mut self,
        width: f32,
        style: Option<&ComputedValues>,
        exclusions: &mut impl LineExclusions,
    ) {
        // Percentage indents are of the content box the lines are broken in.
        self.set_indent(style, width / self.layout.scale());

        #[cfg(not(feature = "floats"))]
        {
            let _ = exclusions;
            self.layout.break_all_lines(Some(width));
        }

        #[cfg(feature = "floats")]
        {
            let mut breaker = self.layout.break_lines();
            let (left, right) = exclusions.band(0.0, 0.0);
            let state = breaker.state_mut();
            state.set_layout_max_advance(width);
            state.set_line_max_advance(right - left);
            state.set_line_x(left);
            state.set_line_y(0.0);

            while let Some(yield_data) = breaker.break_next() {
                match yield_data {
                    YieldData::LineBreak(_line_break_data) => {
                        let state = breaker.state_mut();
                        let top = state.line_y() as f32;
                        let (left, right) = exclusions.band(top, top);
                        state.set_line_max_advance(right - left);
                        state.set_line_x(left);
                    }
                    YieldData::MaxHeightExceeded(_data) => {
                        // TODO
                        continue;
                    }
                    YieldData::InlineBoxBreak(box_break_data) => {
                        // Only floats break the lines at their boxes.
                        let state = breaker.state_mut();
                        let top = state.line_y() as f32;
                        let key = box_break_data.inline_box_id;
                        if let Some(&(_, side, inline_size, block_size)) =
                            self.floats.iter().find(|(id, ..)| *id == key)
                        {
                            exclusions.place(FloatRequest {
                                key,
                                side,
                                inline_size,
                                block_size,
                                block_start: top,
                            });
                        }
                        let (left, right) = exclusions.band(top, top);
                        state.set_line_max_advance(right - left);
                        state.set_line_x(left);

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

        let (alignment, last_line_alignment) = style
            .map(|s| {
                (
                    stylo_to_parley::text_align(s.slow_clone_text_align()),
                    stylo_to_parley::text_align_last(s.slow_clone_text_align_last()),
                )
            })
            .unwrap_or((parley::layout::Alignment::Start, None));

        self.layout.align(
            alignment,
            AlignmentOptions {
                align_when_overflowing: false,
                last_line_alignment,
            },
        );
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
        self.layout.lines().flat_map(|line| {
            let metrics = *line.metrics();
            line.items().filter_map(move |item| match item {
                parley::layout::PositionedLayoutItem::InlineBox(ibox) => Some(Placement {
                    node: NodeId::from_u64(ibox.id),
                    x: ibox.x,
                    // An out-of-flow box is zero-sized, so this is the baseline for it.
                    top: ibox.y,
                    line_top: metrics.block_min_coord,
                    line_bottom: metrics.block_max_coord,
                    // A block-level box's static position is below its line.
                    block_start: metrics.block_max_coord,
                    rtl: None,
                }),
                parley::layout::PositionedLayoutItem::GlyphRun(_) => None,
            })
        })
    }
}
