//! White space collapsing for an inline formatting context.
//!
//! White space is collapsed here (CSS Text §4.1.1) rather than by Parley so that a
//! [`TextOffsetMap`] can be recorded as it goes. The collapsed text is then pushed to Parley's [`TreeBuilder`],
//! which keeps the span tree. Parley collapses white space too, but text that is already
//! collapsed comes out unchanged, except that a space pending at the end of one text node is
//! committed or dropped by what follows it, exactly as here.
//!
//! Each collapsible run still pushes a single space to Parley in the span it was found in, so
//! that Parley can make the kept space a wrap opportunity when any span it collapsed from
//! wraps. A builder that took final text and style runs (Parley's `StyleRunBuilder`, once its
//! styles can have parents) would need that style variant from here instead.

use std::ops::Range;

use parley::{Brush, StyleProperty, TreeBuilder, WhiteSpaceCollapse};

use super::INLINE_BOX_CHAR;
use super::offset_map::{TextOffsetMap, TextOffsetMapBuilder, UnitKind};

/// Whether `c` is a segment break (CSS Text §4.1.1).
pub fn is_segment_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Whether Parley collapses `c` with the given `white-space-collapse`.
pub(crate) fn is_collapsible(mode: WhiteSpaceCollapse, c: char) -> bool {
    match mode {
        WhiteSpaceCollapse::Collapse => c.is_ascii_whitespace(),
        WhiteSpaceCollapse::PreserveBreaks => matches!(c, ' ' | '\t'),
        WhiteSpaceCollapse::Preserve | WhiteSpaceCollapse::BreakSpaces => false,
    }
}

/// The white space handling of the span a text node is in.
#[derive(Debug, Clone, Copy)]
pub struct WhiteSpace {
    pub collapse: WhiteSpaceCollapse,
    /// The space a collapsible run collapses to: U+3000 under `text-transform: full-width`.
    pub collapsed_space: &'static str,
}

/// A collapsible run that will collapse to one space if content follows it.
struct Pending {
    /// The unit of the run's first character, which becomes the kept space.
    kept_unit: usize,
    space: &'static str,
}

/// A range of a text node's transformed text that `text-transform` made from source text of a
/// different length (`ß` → `SS`, a dropped combining mark, a full-width letter). Only its ends
/// map; text outside every change is its source, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChange {
    pub source: Range<usize>,
    pub text: Range<usize>,
}

/// Collapses the white space of an inline formatting context's text and records where each
/// part of each source node (identified by `Id`) ended up in the layout text, pushing the
/// collapsed text to a Parley [`TreeBuilder`].
pub struct WhiteSpaceCollapser<Id> {
    map: TextOffsetMapBuilder<Id>,
    /// Length of the text committed so far (pending white space isn't).
    len: usize,
    ends_with_break: bool,
    pending: Option<Pending>,
    /// The current text node's collapsed text, as pushed to Parley.
    out: String,
    /// The first of the current text node's changes not yet committed.
    next_change: usize,
}

impl<Id> Default for WhiteSpaceCollapser<Id> {
    fn default() -> Self {
        Self {
            map: TextOffsetMapBuilder::default(),
            len: 0,
            ends_with_break: false,
            pending: None,
            out: String::new(),
            next_change: 0,
        }
    }
}

impl<Id: Copy + Ord> WhiteSpaceCollapser<Id> {
    /// Collapses and pushes the content of the text node `node`. `text` is `source` after
    /// `text-transform`, which reports where their lengths differ as `changes`, in order.
    pub fn push_text<B: Brush>(
        &mut self,
        builder: &mut TreeBuilder<'_, B>,
        node: Id,
        source: &str,
        text: &str,
        changes: &[TextChange],
        white_space: WhiteSpace,
    ) {
        self.map.begin_node(node);
        if !text.is_empty() {
            self.next_change = 0;
            self.out.clear();
            self.collapse(text, changes, white_space);
            builder.push_text(&self.out);
            debug_assert_eq!(builder.text().len(), self.len, "{:?}", self.out);
        }
        self.map.end_node(source.len(), self.len);
    }

    /// Pushes text generated for `node` (e.g. a list marker), which has no source offsets.
    pub fn push_generated<B: Brush>(
        &mut self,
        builder: &mut TreeBuilder<'_, B>,
        node: Id,
        text: &str,
        white_space: WhiteSpace,
    ) {
        let generated = TextChange {
            source: 0..0,
            text: 0..text.len(),
        };
        self.push_text(builder, node, "", text, &[generated], white_space);
    }

    /// Pushes a forced line break element (`<br>`).
    pub fn push_line_break<B: Brush>(&mut self, builder: &mut TreeBuilder<'_, B>, node: Id) {
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
    ///
    /// An out-of-flow box is recorded after any collapsible space before it, in DOM order,
    /// though Parley anchors it before that space (see [`TextOffsetMap::layout_range`]).
    pub fn push_inline_box(&mut self, node: Id, in_flow: bool) {
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
    pub fn finish(self) -> TextOffsetMap<Id> {
        self.map.finish()
    }

    fn collapse(&mut self, text: &str, changes: &[TextChange], white_space: WhiteSpace) {
        let mode = white_space.collapse;
        if !is_collapsible(mode, ' ') && !is_collapsible(mode, '\n') {
            // Preserved white space: only a preserved segment break drops a pending run.
            if text.starts_with(is_segment_break) {
                self.pending = None;
            }
            self.flush();
            self.commit(text, changes, 0..text.len(), UnitKind::Identity);
            return;
        }

        let mut pos = 0;
        while let Some(c) = text[pos..].chars().next() {
            let rest = &text[pos..];
            let run_len = rest
                .find(|c| !is_collapsible(mode, c))
                .unwrap_or(rest.len());
            if run_len > 0 {
                self.collapse_run(
                    changes,
                    pos,
                    c.len_utf8(),
                    run_len,
                    white_space.collapsed_space,
                );
                self.out.push(' ');
                pos += run_len;
            } else if is_segment_break(c) {
                self.pending = None;
                self.commit(text, changes, pos..pos + c.len_utf8(), UnitKind::Identity);
                pos += c.len_utf8();
            } else {
                let mut len = content_len(rest, mode);
                // A lone space between content in this node is kept as is, so commit it with
                // the content around it rather than as a run of its own.
                if white_space.collapsed_space == " " {
                    while rest.as_bytes().get(len) == Some(&b' ') {
                        match content_len(&rest[len + 1..], mode) {
                            0 => break,
                            after => len += 1 + after,
                        }
                    }
                }
                self.flush();
                self.commit(text, changes, pos..pos + len, UnitKind::CollapsibleText);
                pos += len;
            }
        }
    }

    /// Records a collapsible run at `pos` whose first character is `first_len` bytes long.
    fn collapse_run(
        &mut self,
        changes: &[TextChange],
        pos: usize,
        first_len: usize,
        run_len: usize,
        space: &'static str,
    ) {
        let source = source_at(changes, pos);
        if self.pending.is_some() {
            self.map.push(UnitKind::Variable, source, self.len);
            return;
        }
        let kept_unit = self.map.push(UnitKind::KeptSpace, source, self.len);
        self.pending = Some(Pending { kept_unit, space });
        if first_len < run_len {
            let source = source_at(changes, pos + first_len);
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

    /// Commits `text[range]` unchanged, mapping the parts outside `changes` as `exact`.
    fn commit(&mut self, text: &str, changes: &[TextChange], range: Range<usize>, exact: UnitKind) {
        let (base, start) = (self.len, range.start);
        let layout = |at: usize| base + at - start;
        // Changes before `range` were in collapsed white space.
        let mut next = self.next_change;
        while changes
            .get(next)
            .is_some_and(|change| change.text.start < start)
        {
            next += 1;
        }
        let starts_change = |next: usize, at: usize| {
            changes
                .get(next)
                .is_some_and(|change| change.text.start == at)
        };
        if !starts_change(next, start) {
            match changes.get(change_around(changes, start)) {
                Some(change) if change.text.start < start && start < change.text.end => {
                    self.map
                        .push(UnitKind::Variable, change.source.end, layout(start));
                    if change.text.end < range.end && !starts_change(next, change.text.end) {
                        self.map
                            .push(exact, change.source.end, layout(change.text.end));
                    }
                }
                _ => {
                    self.map
                        .push(exact, source_at(changes, start), layout(start));
                }
            }
        }
        // A change that takes up no text belongs to the text before it.
        while let Some(change) = changes.get(next).filter(|change| {
            change.text.start < range.end
                || (change.text.is_empty() && change.text.start == range.end)
        }) {
            next += 1;
            self.map.push(
                UnitKind::Variable,
                change.source.start,
                layout(change.text.start),
            );
            if change.text.end < range.end && !starts_change(next, change.text.end) {
                self.map
                    .push(exact, change.source.end, layout(change.text.end));
            }
        }
        self.next_change = next;
        self.push_out(&text[range]);
    }

    fn push_out(&mut self, text: &str) {
        self.out.push_str(text);
        self.len += text.len();
        self.ends_with_break = text.ends_with(is_segment_break);
    }
}

/// The index of the last change starting at or before `pos`, or `changes.len()` if none does.
fn change_around(changes: &[TextChange], pos: usize) -> usize {
    changes
        .partition_point(|change| change.text.start <= pos)
        .checked_sub(1)
        .unwrap_or(changes.len())
}

/// The source offset of the transformed text offset `pos`.
fn source_at(changes: &[TextChange], pos: usize) -> usize {
    match changes.get(change_around(changes, pos)) {
        None => pos,
        Some(change) if pos < change.text.end => {
            if pos == change.text.start {
                change.source.start
            } else {
                change.source.end
            }
        }
        Some(change) => change.source.end + (pos - change.text.end),
    }
}

/// The length of the content (neither collapsible white space nor a segment break) at the
/// start of `text`.
fn content_len(text: &str, mode: WhiteSpaceCollapse) -> usize {
    text.find(|c| is_collapsible(mode, c) || is_segment_break(c))
        .unwrap_or(text.len())
}
