use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use floem_editor_core::buffer::{Buffer, InvalLines, rope_text::RopeText};
use parley::{
    Affinity, Alignment, AlignmentOptions, BoundingBox, Cursor, FontFamily, Layout, LineHeight,
    StyleProperty,
};
use peniko::Color;

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
        let line_height = (style.font_size * style.line_height).round().max(1.0);
        let layout = build_layout(cx, style, line_height, 0.0, Color::BLACK, "0");
        Self {
            line_height: line_height as f64,
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
    style: &EditorStyle,
    line_height: f32,
    char_width: f64,
    color: Color,
    text: &str,
) -> TextLayout {
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
    builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(line_height)));
    builder.push_default(StyleProperty::Brush(TextBrush(color)));

    if text.contains('\t') {
        let tab_size = style.tab_size.max(1);
        let mut col = 0;
        for (idx, ch) in text.char_indices() {
            if ch == '\t' {
                let width = tab_size - col % tab_size;
                let spacing = (width - 1) as f64 * char_width;
                builder.push(StyleProperty::LetterSpacing(spacing as f32), idx..idx + 1);
                col += width;
            } else {
                col += 1;
            }
        }
    }

    let mut layout = builder.build(&display);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}

/// The x position of the caret before byte `col` of a line layout
pub(crate) fn caret_x(layout: &TextLayout, col: usize) -> f64 {
    Cursor::from_byte_index(layout, col, Affinity::Downstream)
        .geometry(layout, 0.0)
        .x0
}

/// The byte offset within a line layout closest to `x`
pub(crate) fn col_at_x(layout: &TextLayout, x: f64) -> usize {
    let y = layout.height() / 2.0;
    Cursor::from_point(layout, x as f32, y).index()
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
struct LayoutConfig {
    style: EditorStyle,
    metrics: Metrics,
    color: Color,
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
}

impl LineCache {
    pub(crate) fn new() -> Self {
        Self {
            config: None,
            lines: Vec::new(),
            cached: 0,
            numbers: HashMap::new(),
            max_width: 0.0,
        }
    }

    /// Clears the cache if the settings changed. Returns whether they did.
    pub(crate) fn configure(
        &mut self,
        style: &EditorStyle,
        metrics: Metrics,
        color: Color,
        num_lines: usize,
    ) -> bool {
        let config = LayoutConfig {
            style: style.clone(),
            metrics,
            color,
        };
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
        self.numbers.clear();
        self.max_width = 0.0;
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
        // Guard against the edit's line accounting disagreeing with the buffer
        if self.lines.len() != num_lines {
            self.lines.resize_with(num_lines, || None);
            self.cached = self.lines.iter().flatten().count();
        }
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
        let config = self
            .config
            .as_ref()
            .expect("LineCache used before configure");
        if line >= self.lines.len() {
            self.lines
                .resize_with(buffer.num_lines().max(line + 1), || None);
        }
        let slot = &mut self.lines[line];
        if slot.is_none() {
            let text = buffer.slice_to_cow(line_range(buffer, line));
            let layout = build_layout(
                cx,
                &config.style,
                config.metrics.line_height as f32,
                config.metrics.char_width,
                config.color,
                &text,
            );
            self.max_width = self.max_width.max(layout.full_width() as f64);
            self.cached += 1;
            *slot = Some(layout);
        }
        slot.as_ref().unwrap()
    }

    pub(crate) fn number(&mut self, cx: &mut TextContext, number: usize) -> &TextLayout {
        let config = self
            .config
            .as_ref()
            .expect("LineCache used before configure");
        self.numbers.entry(number).or_insert_with(|| {
            build_layout(
                cx,
                &config.style,
                config.metrics.line_height as f32,
                config.metrics.char_width,
                config.color,
                &number.to_string(),
            )
        })
    }

    /// Lays out a line with IME composition text spliced in at byte `col`
    pub(crate) fn line_with_preedit(
        &self,
        cx: &mut TextContext,
        buffer: &Buffer,
        line: usize,
        col: usize,
        preedit: &str,
    ) -> TextLayout {
        let config = self
            .config
            .as_ref()
            .expect("LineCache used before configure");
        let mut text = buffer.slice_to_cow(line_range(buffer, line)).into_owned();
        text.insert_str(col.min(text.len()), preedit);
        build_layout(
            cx,
            &config.style,
            config.metrics.line_height as f32,
            config.metrics.char_width,
            config.color,
            &text,
        )
    }
}
