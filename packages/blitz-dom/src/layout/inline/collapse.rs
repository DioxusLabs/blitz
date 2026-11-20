//! White space collapsing for an inline formatting context.
//!
//! Blitz collapses white space itself (CSS Text §4.1.1) so that it can record a
//! [`TextOffsetMap`] as it goes. The collapsed text is then pushed to Parley's [`TreeBuilder`],
//! which keeps the span tree. Parley collapses white space too, but text that is already
//! collapsed comes out unchanged, except that a space pending at the end of one text node is
//! committed or dropped by what follows it, exactly as here.
//!
//! Each collapsible run still pushes a single space to Parley in the span it was found in, so
//! that Parley can make the kept space a wrap opportunity when any span it collapsed from
//! wraps. A builder that took final text and style runs (Parley's `StyleRunBuilder`, once its
//! styles can have parents) would need that style variant from here instead.

use blitz_traits::node_id::NodeId;
use icu_properties::{
    CodePointMapData,
    props::{GeneralCategory, GeneralCategoryGroup},
};
use parley::{StyleProperty, TreeBuilder, WhiteSpaceCollapse};

use super::offset_map::{TextOffsetMap, TextOffsetMapBuilder, UnitKind};
use crate::node::{INLINE_BOX_CHAR, TextBrush};

pub(crate) fn is_segment_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn is_collapsible(mode: WhiteSpaceCollapse, c: char) -> bool {
    match mode {
        WhiteSpaceCollapse::Collapse => c.is_ascii_whitespace(),
        WhiteSpaceCollapse::PreserveBreaks => matches!(c, ' ' | '\t'),
        WhiteSpaceCollapse::Preserve | WhiteSpaceCollapse::BreakSpaces => false,
    }
}

/// The white space handling of the span a text node is in.
#[derive(Clone, Copy)]
pub(crate) struct WhiteSpace {
    pub(crate) collapse: WhiteSpaceCollapse,
    /// The space a collapsible run collapses to: U+3000 under `text-transform: full-width`.
    pub(crate) collapsed_space: &'static str,
    /// Whether `text-wrap-mode` is `wrap`.
    pub(crate) wrap: bool,
}

/// A collapsible run that will collapse to one space if content follows it.
struct Pending {
    /// The unit of the run's first character, which becomes the kept space.
    kept_unit: usize,
    space: &'static str,
}

/// The start of a range of a text node's transformed text, and where it came from.
#[derive(Clone, Copy)]
struct Segment {
    text: usize,
    source: usize,
    /// Whether the range maps byte for byte to the source.
    exact: bool,
}

/// Collapses the white space of an inline formatting context's text and records where each
/// part of the DOM ended up in the layout text.
#[derive(Default)]
pub(crate) struct WhiteSpaceCollapser {
    map: TextOffsetMapBuilder,
    /// Length of the text committed so far (pending white space isn't).
    len: usize,
    ends_with_break: bool,
    pending: Option<Pending>,
    /// The current text node's collapsed text, as pushed to Parley.
    out: String,
    /// The current text node's transformed text, aligned to its source.
    segments: Vec<Segment>,
}

impl WhiteSpaceCollapser {
    /// Collapses and pushes the content of the text node `node`. `text` is `source` after
    /// `text-transform`.
    pub(crate) fn push_text(
        &mut self,
        builder: &mut TreeBuilder<'_, TextBrush>,
        node: NodeId,
        source: &str,
        text: &str,
        white_space: WhiteSpace,
    ) {
        self.map.begin_node(node);
        if !text.is_empty() {
            align(source, text, &mut self.segments);
            self.out.clear();
            self.collapse(text, white_space);
            builder.push_text(&self.out);
            debug_assert_eq!(builder.text().len(), self.len, "{:?}", self.out);
        }
        self.map.end_node(source.len(), self.len);
    }

    /// Pushes text generated for `node` (a list marker), which has no source offsets.
    pub(crate) fn push_generated(
        &mut self,
        builder: &mut TreeBuilder<'_, TextBrush>,
        node: NodeId,
        text: &str,
        white_space: WhiteSpace,
    ) {
        self.push_text(builder, node, "", text, white_space);
    }

    /// Pushes the forced line break of a `<br>` element.
    pub(crate) fn push_line_break(
        &mut self,
        builder: &mut TreeBuilder<'_, TextBrush>,
        node: NodeId,
    ) {
        self.map.begin_node(node);
        self.pending = None;
        self.map.push(UnitKind::Identity, 0, self.len);
        self.len += 1;
        self.ends_with_break = true;
        self.map.end_node(1, self.len);
        builder.push_style_modification_span(&[StyleProperty::WhiteSpaceCollapse(
            WhiteSpaceCollapse::Preserve,
        )]);
        builder.push_text("\n");
        builder.pop_style_span();
    }

    /// Records the position of an inline box. Only in-flow boxes take up a character
    /// (U+FFFC) of the text, and only they end a collapsible run.
    pub(crate) fn push_inline_box(&mut self, node: NodeId, in_flow: bool) {
        self.map.begin_node(node);
        if in_flow {
            self.flush();
        }
        self.map.push(UnitKind::Variable, 0, self.len);
        if in_flow {
            self.len += INLINE_BOX_CHAR.len_utf8();
            self.ends_with_break = false;
        }
        self.map.end_node(1, self.len);
    }

    /// Finishes the formatting context, dropping any white space at its end.
    pub(crate) fn finish(self) -> TextOffsetMap {
        self.map.finish()
    }

    fn collapse(&mut self, text: &str, white_space: WhiteSpace) {
        let mode = white_space.collapse;
        if !is_collapsible(mode, ' ') && !is_collapsible(mode, '\n') {
            // Preserved white space: only a preserved segment break drops a pending run.
            if text.starts_with(is_segment_break) {
                self.pending = None;
            }
            self.flush();
            let kind = if mode == WhiteSpaceCollapse::Preserve && white_space.wrap {
                UnitKind::PreWrap
            } else {
                UnitKind::Identity
            };
            self.commit(text, 0..text.len(), kind);
            return;
        }

        let mut pos = 0;
        while let Some(c) = text[pos..].chars().next() {
            let rest = &text[pos..];
            let run_len = rest
                .find(|c| !is_collapsible(mode, c))
                .unwrap_or(rest.len());
            if run_len > 0 {
                self.collapse_run(pos, c.len_utf8(), run_len, white_space.collapsed_space);
                self.out.push(' ');
                pos += run_len;
            } else if is_segment_break(c) {
                self.pending = None;
                self.commit(text, pos..pos + c.len_utf8(), UnitKind::Identity);
                pos += c.len_utf8();
            } else {
                let content_len = rest
                    .find(|c| is_collapsible(mode, c) || is_segment_break(c))
                    .unwrap_or(rest.len());
                self.flush();
                self.commit(text, pos..pos + content_len, UnitKind::Identity);
                pos += content_len;
            }
        }
    }

    /// Records a collapsible run at `pos` whose first character is `first_len` bytes long.
    fn collapse_run(&mut self, pos: usize, first_len: usize, run_len: usize, space: &'static str) {
        let source = self.source_at(pos);
        if self.pending.is_some() {
            self.map.push(UnitKind::Variable, source, self.len);
            return;
        }
        let kept_unit = self.map.push(UnitKind::KeptSpace, source, self.len);
        self.pending = Some(Pending { kept_unit, space });
        if first_len < run_len {
            let source = self.source_at(pos + first_len);
            self.map.push(UnitKind::Variable, source, self.len);
        }
    }

    /// Resolves a pending run: it collapses to one space unless it is at the start of the
    /// formatting context or follows a preserved segment break, in which case it is removed.
    fn flush(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if self.len == 0 || self.ends_with_break {
            return;
        }
        self.map.shift(pending.kept_unit + 1, pending.space.len());
        self.len += pending.space.len();
        self.ends_with_break = false;
    }

    /// Commits `text[range]` unchanged, mapping the parts that match the source as `exact`.
    fn commit(&mut self, text: &str, range: std::ops::Range<usize>, exact: UnitKind) {
        let first = self.segments.partition_point(|seg| seg.text <= range.start) - 1;
        let kind = |seg: &Segment| {
            if seg.exact { exact } else { UnitKind::Variable }
        };
        let source = self.source_at(range.start);
        self.map.push(kind(&self.segments[first]), source, self.len);
        for seg in self.segments[first + 1..]
            .iter()
            .take_while(|seg| seg.text < range.end)
        {
            self.map
                .push(kind(seg), seg.source, self.len + seg.text - range.start);
        }
        let text = &text[range];
        self.out.push_str(text);
        self.len += text.len();
        self.ends_with_break = text.ends_with(is_segment_break);
    }

    /// The source offset of the transformed text offset `pos`.
    fn source_at(&self, pos: usize) -> usize {
        let index = self.segments.partition_point(|seg| seg.text <= pos) - 1;
        let seg = self.segments[index];
        if seg.exact {
            seg.source + (pos - seg.text)
        } else if pos == seg.text {
            seg.source
        } else {
            self.segments[index + 1].source
        }
    }
}

/// Aligns `text` (`source` after `text-transform`) to `source`. Characters map one to one when
/// the character count is unchanged. Otherwise case mappings that expand a character (`ß` →
/// `SS`) and combining marks removed by tailored casing are matched character by character, and
/// anything that can't be matched maps as one range.
fn align(source: &str, text: &str, segments: &mut Vec<Segment>) {
    segments.clear();
    let mut push = |text: usize, source: usize, exact: bool| {
        if let Some(last) = segments.last()
            && last.exact
            && exact
            && text - last.text == source - last.source
        {
            return;
        }
        segments.push(Segment {
            text,
            source,
            exact,
        });
    };

    let (mut s, mut t) = (0, 0);
    if source == text {
        push(0, 0, true);
        s = source.len();
        t = text.len();
    } else if source.chars().count() == text.chars().count() {
        for (c, d) in source.chars().zip(text.chars()) {
            push(t, s, c.len_utf8() == d.len_utf8());
            s += c.len_utf8();
            t += d.len_utf8();
        }
    } else {
        let general_category = CodePointMapData::<GeneralCategory>::new();
        while let (Some(c), Some(d)) = (source[s..].chars().next(), text[t..].chars().next()) {
            let rest = &text[t..];
            let expansion = matching_prefix(rest, c.to_uppercase())
                .or_else(|| matching_prefix(rest, c.to_lowercase()))
                .filter(|&len| len > d.len_utf8());
            if let Some(len) = expansion {
                push(t, s, false);
                t += len;
            } else if c != d && GeneralCategoryGroup::Mark.contains(general_category.get(c)) {
                push(t, s, false);
            } else {
                push(t, s, c.len_utf8() == d.len_utf8());
                t += d.len_utf8();
            }
            s += c.len_utf8();
        }
    }
    if s < source.len() || t < text.len() {
        push(t, s, false);
    }
    push(text.len(), source.len(), true);
    // The end segment is needed even when it merged into an exact one before it.
    if segments.last().is_none_or(|last| last.text != text.len()) {
        segments.push(Segment {
            text: text.len(),
            source: source.len(),
            exact: true,
        });
    }
}

fn matching_prefix(text: &str, mapped: impl Iterator<Item = char>) -> Option<usize> {
    let mut text = text.chars();
    let mut len = 0;
    for c in mapped {
        if text.next() != Some(c) {
            return None;
        }
        len += c.len_utf8();
    }
    Some(len)
}
