use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use floem_editor_core::buffer::{Buffer, InvalLines, rope_text::RopeText};
use kurbo::Point;
use parley::{
    Affinity, Alignment, AlignmentOptions, BoundingBox, Cursor, FontFamily, FontStyle, FontWeight,
    Layout, LineHeight, StyleProperty,
};
use peniko::Color;

use crate::highlight::{LineHighlights, StyledSpan};
use crate::{EditorStyle, TextBrush, TextContext};

pub(crate) type TextLayout = Layout<TextBrush>;

/// How many line (and line number) layouts to keep before dropping off-screen ones
const MAX_CACHED_LINES: usize = 1000;

/// Measurements derived from the [`EditorStyle`] and the resolved font
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Metrics {
    pub line_height: f64,
    pub char_width: f64,
}

impl Metrics {
    pub(crate) fn measure(cx: &mut TextContext, style: &EditorStyle) -> Self {
        let line_height = (style.font_size * style.line_height).round().max(1.0) as f64;
        let config = LayoutConfig {
            style: style.clone(),
            metrics: Metrics {
                line_height,
                char_width: 0.0,
            },
            color: Color::BLACK,
            wrap_width: None,
        };
        let layout = build_layout(cx, &config, "0", &[]);
        Self {
            line_height,
            char_width: layout.full_width() as f64,
        }
    }
}

/// Lays out one line of text (without its line ending).
///
/// Tabs are shaped as spaces with extra letter spacing up to the next tab stop, so byte offsets
/// in the layout match the buffer.
pub(crate) fn build_layout(
    cx: &mut TextContext,
    config: &LayoutConfig,
    text: &str,
    spans: &[StyledSpan],
) -> TextLayout {
    let LayoutConfig {
        style,
        metrics,
        color,
        wrap_width,
    } = config;
    let display: Cow<'_, str> = if text.contains('\t') {
        Cow::Owned(text.replace('\t', " "))
    } else {
        Cow::Borrowed(text)
    };
    let mut builder = cx
        .layout_cx
        .ranged_builder(&mut cx.font_cx, &display, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
        style.font_family.as_str().into(),
    )));
    builder.push_default(StyleProperty::FontSize(style.font_size));
    builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(
        metrics.line_height as f32,
    )));
    builder.push_default(StyleProperty::Brush(TextBrush(*color)));

    for span in spans {
        let range = span.range.start.min(text.len())..span.range.end.min(text.len());
        let style = span.style;
        if let Some(color) = style.foreground {
            builder.push(StyleProperty::Brush(TextBrush(color)), range.clone());
        }
        if style.bold {
            builder.push(StyleProperty::FontWeight(FontWeight::BOLD), range.clone());
        }
        if style.italic {
            builder.push(StyleProperty::FontStyle(FontStyle::Italic), range.clone());
        }
        if style.underline {
            builder.push(StyleProperty::Underline(true), range);
        }
    }

    if text.contains('\t') {
        let tab_size = style.tab_size.max(1);
        let mut col = 0;
        for (idx, ch) in text.char_indices() {
            if ch == '\t' {
                let width = tab_size - col % tab_size;
                let spacing = (width - 1) as f64 * metrics.char_width;
                builder.push(StyleProperty::LetterSpacing(spacing as f32), idx..idx + 1);
                col += width;
            } else {
                col += 1;
            }
        }
    }

    let mut layout = builder.build(&display);
    layout.set_alignment(Alignment::Start, AlignmentOptions::default());
    layout.break_all_lines(*wrap_width);
    layout
}

/// The top-left corner of the caret before byte `col` of a line layout. With soft wrapping, the
/// y position picks out the visual row.
pub(crate) fn caret_point(layout: &TextLayout, col: usize) -> Point {
    let rect = Cursor::from_byte_index(layout, col, Affinity::Downstream).geometry(layout, 0.0);
    Point::new(rect.x0, rect.y0)
}

/// The byte offset within a line layout closest to a point in it
pub(crate) fn col_at_point(layout: &TextLayout, point: Point) -> usize {
    Cursor::from_point(layout, point.x as f32, point.y as f32).index()
}

/// Highlight rectangles for the byte range `cols` of a line layout
pub(crate) fn range_rects(layout: &TextLayout, cols: Range<usize>) -> Vec<BoundingBox> {
    let anchor = Cursor::from_byte_index(layout, cols.start, Affinity::Downstream);
    let focus = Cursor::from_byte_index(layout, cols.end, Affinity::Upstream);
    parley::Selection::new(anchor, focus)
        .geometry(layout)
        .into_iter()
        .map(|(rect, _)| rect)
        .collect()
}

/// The byte range of `line`'s content (excluding its line ending)
pub(crate) fn line_range(buffer: &Buffer, line: usize) -> Range<usize> {
    buffer.offset_of_line(line)..buffer.line_end_offset(line, true)
}

/// Settings that every cached layout was built with. Changing any of them clears the cache.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutConfig {
    pub style: EditorStyle,
    pub metrics: Metrics,
    /// The default text color
    pub color: Color,
    /// The width to wrap lines at, if soft wrapping
    pub wrap_width: Option<f32>,
}

/// How many visual rows each line takes up (more than one when it soft wraps), with fast
/// prefix sums for converting between rows and lines. Lines that haven't been laid out yet
/// count as one row.
pub(crate) struct RowCounts {
    counts: Vec<u32>,
    /// A Fenwick tree over `counts`, 1-indexed
    tree: Vec<u64>,
}

impl RowCounts {
    fn new() -> Self {
        Self {
            counts: Vec::new(),
            tree: vec![0],
        }
    }

    fn reset(&mut self, num_lines: usize) {
        self.counts.clear();
        self.counts.resize(num_lines, 1);
        self.rebuild();
    }

    fn splice(&mut self, lines: Range<usize>, new_count: usize, num_lines: usize) {
        self.counts.splice(lines, std::iter::repeat_n(1, new_count));
        self.counts.resize(num_lines, 1);
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let n = self.counts.len();
        self.tree.clear();
        self.tree.push(0);
        self.tree.extend(self.counts.iter().map(|&c| c as u64));
        for i in 1..=n {
            let parent = i + (i & i.wrapping_neg());
            if parent <= n {
                self.tree[parent] += self.tree[i];
            }
        }
    }

    fn set(&mut self, line: usize, rows: u32) {
        let Some(count) = self.counts.get_mut(line) else {
            return;
        };
        let (old, new) = (*count as u64, rows.max(1) as u64);
        if old == new {
            return;
        }
        *count = new as u32;
        let mut i = line + 1;
        while i < self.tree.len() {
            self.tree[i] = self.tree[i] + new - old;
            i += i & i.wrapping_neg();
        }
    }

    /// The number of rows before `line`
    pub(crate) fn before(&self, line: usize) -> usize {
        let mut i = line.min(self.counts.len());
        let mut sum = 0;
        while i > 0 {
            sum += self.tree[i];
            i &= i - 1;
        }
        sum as usize
    }

    pub(crate) fn total(&self) -> usize {
        self.before(self.counts.len())
    }

    pub(crate) fn of(&self, line: usize) -> usize {
        self.counts.get(line).map_or(1, |&c| c as usize)
    }

    /// The line containing `row`, and the row within that line. Rows past the end are in the
    /// last line.
    pub(crate) fn line_at(&self, row: usize) -> (usize, usize) {
        let n = self.counts.len();
        if n == 0 {
            return (0, 0);
        }
        // Find the most lines whose rows total at most `row`
        let mut line = 0;
        let mut rest = row as u64;
        let mut step = n.next_power_of_two();
        while step > 0 {
            if line + step <= n && self.tree[line + step] <= rest {
                line += step;
                rest -= self.tree[line];
            }
            step /= 2;
        }
        if line >= n {
            (n - 1, self.of(n - 1) - 1)
        } else {
            (line, rest as usize)
        }
    }
}

/// Per-line layouts, built on demand and invalidated by edits
pub(crate) struct LineCache {
    config: Option<LayoutConfig>,
    lines: Vec<Option<TextLayout>>,
    /// How many entries of `lines` hold a layout
    cached: usize,
    /// Gutter line number layouts, keyed by line number. These never change with edits.
    numbers: HashMap<usize, TextLayout>,
    /// The widest line laid out so far
    max_width: f64,
    highlights: Option<Box<dyn LineHighlights>>,
    rows: RowCounts,
}

impl LineCache {
    pub(crate) fn new() -> Self {
        Self {
            config: None,
            lines: Vec::new(),
            cached: 0,
            numbers: HashMap::new(),
            max_width: 0.0,
            highlights: None,
            rows: RowCounts::new(),
        }
    }

    pub(crate) fn rows(&self) -> &RowCounts {
        &self.rows
    }

    /// Replaces the syntax highlighter, and drops layouts styled by the old one
    pub(crate) fn set_highlights(
        &mut self,
        highlights: Option<Box<dyn LineHighlights>>,
        num_lines: usize,
    ) {
        self.highlights = highlights;
        self.reset(num_lines);
    }

    /// Clears the cache if the settings changed. Returns whether they did.
    pub(crate) fn configure(&mut self, config: LayoutConfig, num_lines: usize) -> bool {
        if self.config.as_ref() == Some(&config) {
            return false;
        }
        self.config = Some(config);
        self.reset(num_lines);
        true
    }

    /// Drops every line layout (e.g. after the whole text was replaced)
    pub(crate) fn reset(&mut self, num_lines: usize) {
        self.lines.clear();
        self.lines.resize_with(num_lines, || None);
        self.cached = 0;
        self.rows.reset(num_lines);
        self.numbers.clear();
        self.max_width = 0.0;
        if let Some(highlights) = &mut self.highlights {
            highlights.reset(num_lines);
        }
    }

    /// Drops the layouts of lines an edit touched, and shifts the ones after them
    pub(crate) fn invalidate(&mut self, inval: &InvalLines, num_lines: usize) {
        let start = inval.start_line.min(self.lines.len());
        let end = (inval.start_line + inval.inval_count).min(self.lines.len());
        self.cached -= self.lines[start..end].iter().flatten().count();
        self.lines.splice(
            start..end,
            std::iter::repeat_with(|| None).take(inval.new_count),
        );
        self.rows.splice(start..end, inval.new_count, num_lines);
        // Guard against the edit's line accounting disagreeing with the buffer
        if self.lines.len() != num_lines {
            self.lines.resize_with(num_lines, || None);
            self.cached = self.lines.iter().flatten().count();
        }
        if let Some(highlights) = &mut self.highlights {
            highlights.invalidate(inval, num_lines);
        }
    }

    /// Brings highlighting up to date through `line`, dropping the layouts of earlier lines
    /// whose highlighting changed (e.g. after opening a block comment above them)
    fn update_highlights(&mut self, buffer: &Buffer, line: usize) {
        let Some(highlights) = &mut self.highlights else {
            return;
        };
        let lines = &mut self.lines;
        let cached = &mut self.cached;
        highlights.ensure(buffer, line, &mut |changed| {
            if lines.get_mut(changed).and_then(Option::take).is_some() {
                *cached -= 1;
            }
        });
    }

    fn spans(&self, line: usize) -> &[StyledSpan] {
        self.highlights.as_ref().map_or(&[], |h| h.spans(line))
    }

    /// Once more than [`MAX_CACHED_LINES`] layouts are cached, drops those for lines outside
    /// `keep`. Checking only past that limit keeps the cost of scanning the cache low.
    pub(crate) fn evict_outside(&mut self, keep: Range<usize>) {
        if self.cached > MAX_CACHED_LINES {
            for (line, slot) in self.lines.iter_mut().enumerate() {
                if !keep.contains(&line) && slot.take().is_some() {
                    self.cached -= 1;
                }
            }
        }
        if self.numbers.len() > MAX_CACHED_LINES {
            self.numbers
                .retain(|number, _| keep.contains(&(number - 1)));
        }
    }

    pub(crate) fn max_width(&self) -> f64 {
        self.max_width
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lines.len()
    }

    #[cfg(test)]
    pub(crate) fn cached(&self) -> (usize, usize) {
        debug_assert_eq!(self.cached, self.lines.iter().flatten().count());
        (self.cached, self.numbers.len())
    }

    pub(crate) fn line(
        &mut self,
        cx: &mut TextContext,
        buffer: &Buffer,
        line: usize,
    ) -> &TextLayout {
        if line >= self.lines.len() {
            self.lines
                .resize_with(buffer.num_lines().max(line + 1), || None);
            self.rows.counts.resize(self.lines.len(), 1);
            self.rows.rebuild();
        }
        // This may drop this line's layout if an edit above changed its highlighting
        self.update_highlights(buffer, line);
        if self.lines[line].is_none() {
            let config = self
                .config
                .as_ref()
                .expect("LineCache used before configure");
            let text = buffer.slice_to_cow(line_range(buffer, line));
            let layout = build_layout(cx, config, &text, self.spans(line));
            self.max_width = self.max_width.max(layout.full_width() as f64);
            self.rows.set(line, layout.len() as u32);
            self.cached += 1;
            self.lines[line] = Some(layout);
        }
        self.lines[line].as_ref().unwrap()
    }

    pub(crate) fn number(&mut self, cx: &mut TextContext, number: usize) -> &TextLayout {
        let config = self
            .config
            .as_ref()
            .expect("LineCache used before configure");
        self.numbers
            .entry(number)
            .or_insert_with(|| build_layout(cx, config, &number.to_string(), &[]))
    }

    /// Lays out a line with IME composition text spliced in at byte `col`
    pub(crate) fn line_with_preedit(
        &mut self,
        cx: &mut TextContext,
        buffer: &Buffer,
        line: usize,
        col: usize,
        preedit: &str,
    ) -> TextLayout {
        self.update_highlights(buffer, line);
        let config = self
            .config
            .as_ref()
            .expect("LineCache used before configure");
        let mut text = buffer.slice_to_cow(line_range(buffer, line)).into_owned();
        let col = col.min(text.len());
        text.insert_str(col, preedit);
        // Shift highlighting after the composition text along with the text
        let shift = |offset: usize| {
            if offset >= col {
                offset + preedit.len()
            } else {
                offset
            }
        };
        let spans: Vec<_> = self
            .spans(line)
            .iter()
            .map(|span| StyledSpan {
                range: shift(span.range.start)..shift(span.range.end),
                style: span.style,
            })
            .collect();
        build_layout(cx, config, &text, &spans)
    }
}

#[cfg(test)]
mod tests {
    use super::RowCounts;

    fn rows(counts: &[u32]) -> RowCounts {
        let mut rows = RowCounts::new();
        rows.reset(counts.len());
        for (line, &count) in counts.iter().enumerate() {
            rows.set(line, count);
        }
        rows
    }

    #[test]
    fn prefix_sums_and_lookup_match_a_linear_scan() {
        let counts = [1, 3, 1, 2, 1, 1, 4, 1, 1];
        let rows = rows(&counts);
        let mut before = 0;
        for (line, &count) in counts.iter().enumerate() {
            assert_eq!(rows.before(line), before);
            for row in 0..count as usize {
                assert_eq!(rows.line_at(before + row), (line, row));
            }
            before += count as usize;
        }
        assert_eq!(rows.total(), before);
        // Rows past the end are in the last row of the last line
        assert_eq!(rows.line_at(before + 10), (8, 0));
    }

    #[test]
    fn splicing_resets_the_new_lines_to_one_row() {
        let mut rows = rows(&[2, 2, 2, 2]);
        rows.splice(1..3, 3, 5);
        assert_eq!(rows.total(), 2 + 3 + 2);
        assert_eq!(rows.line_at(4), (3, 0));
        rows.set(2, 5);
        assert_eq!(rows.before(4), 2 + 1 + 5 + 1);
    }
}
