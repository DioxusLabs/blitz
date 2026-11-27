//! Syntax highlighting
//!
//! Highlighters implement [`SyntaxHighlighter`], a small line-at-a-time interface, so the engine
//! behind it can be swapped without touching the editor. The editor caches each line's end state
//! and spans, and after an edit re-highlights from the first changed line until the state matches
//! what it was before.

use std::ops::Range;

use floem_editor_core::buffer::{Buffer, InvalLines, rope_text::RopeText};
use peniko::Color;

use crate::layout::line_range;

#[cfg(feature = "syntaxmate")]
mod syntaxmate;
#[cfg(feature = "syntaxmate")]
pub use syntaxmate::SyntaxmateHighlighter;

/// How a span of text is styled. Unset fields use the editor's defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpanStyle {
    pub foreground: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// A styled byte range within one line
#[derive(Clone, Debug, PartialEq)]
pub struct StyledSpan {
    pub range: Range<usize>,
    pub style: SpanStyle,
}

/// A syntax highlighter that works one line at a time
pub trait SyntaxHighlighter {
    /// What the highlighter needs to carry from the end of one line to the start of the next
    /// (e.g. "inside a block comment"). Comparing states is how the editor tells that an edit
    /// stopped affecting later lines, so equality should be cheap.
    type State: Clone + PartialEq + 'static;

    /// The state at the start of the document
    fn initial_state(&self) -> Self::State;

    /// Appends the styled spans of `line` (which excludes its line ending) to `spans`, and
    /// advances `state` to the end of the line. Spans are byte ranges within `line`.
    fn highlight_line(&mut self, line: &str, state: &mut Self::State, spans: &mut Vec<StyledSpan>);
}

/// The editor's view of a highlighter, with the state type erased
pub(crate) trait LineHighlights {
    /// Forgets every line (e.g. after the whole text was replaced)
    fn reset(&mut self, num_lines: usize);
    /// Forgets the lines an edit touched, and shifts the ones after them
    fn invalidate(&mut self, inval: &InvalLines, num_lines: usize);
    /// Highlights every line up to and including `line` that isn't up to date, calling
    /// `changed` for each earlier line whose spans differ from before
    fn ensure(&mut self, buffer: &Buffer, line: usize, changed: &mut dyn FnMut(usize));
    /// The spans of `line`, which must have been passed to [`Self::ensure`] since the last edit
    fn spans(&self, line: usize) -> &[StyledSpan];
}

struct LineEntry<S> {
    /// The highlighter state at the end of the line
    end_state: S,
    spans: Vec<StyledSpan>,
}

/// Caches the end state and spans of each line for a [`SyntaxHighlighter`]
pub(crate) struct HighlightCache<H: SyntaxHighlighter> {
    highlighter: H,
    lines: Vec<Option<LineEntry<H::State>>>,
    /// Every line before this one has an up-to-date entry
    first_stale: usize,
}

impl<H: SyntaxHighlighter> HighlightCache<H> {
    pub(crate) fn new(highlighter: H, num_lines: usize) -> Self {
        let mut cache = Self {
            highlighter,
            lines: Vec::new(),
            first_stale: 0,
        };
        cache.reset(num_lines);
        cache
    }
}

impl<H: SyntaxHighlighter> LineHighlights for HighlightCache<H> {
    fn reset(&mut self, num_lines: usize) {
        self.lines.clear();
        self.lines.resize_with(num_lines, || None);
        self.first_stale = 0;
    }

    fn invalidate(&mut self, inval: &InvalLines, num_lines: usize) {
        let start = inval.start_line.min(self.lines.len());
        let end = (inval.start_line + inval.inval_count).min(self.lines.len());
        self.lines.splice(
            start..end,
            std::iter::repeat_with(|| None).take(inval.new_count),
        );
        self.lines.resize_with(num_lines, || None);
        self.first_stale = self.first_stale.min(start);
    }

    fn ensure(&mut self, buffer: &Buffer, line: usize, changed: &mut dyn FnMut(usize)) {
        if self.lines.len() < buffer.num_lines() {
            self.lines.resize_with(buffer.num_lines(), || None);
        }
        let mut spans = Vec::new();
        while self.first_stale <= line && self.first_stale < self.lines.len() {
            let index = self.first_stale;
            let mut state = match index.checked_sub(1) {
                Some(prev) => self.lines[prev].as_ref().unwrap().end_state.clone(),
                None => self.highlighter.initial_state(),
            };
            let text = buffer.slice_to_cow(line_range(buffer, index));
            spans.clear();
            self.highlighter
                .highlight_line(&text, &mut state, &mut spans);

            let old = self.lines[index].take();
            // An entry that survived the edit has the same text as before, so if it also ends
            // in the same state, the lines after it are unaffected too
            let converged = old.as_ref().is_some_and(|old| old.end_state == state);
            if old.is_some_and(|old| old.spans != spans) {
                changed(index);
            }
            self.lines[index] = Some(LineEntry {
                end_state: state,
                spans: std::mem::take(&mut spans),
            });

            self.first_stale = index + 1;
            if converged {
                while self
                    .lines
                    .get(self.first_stale)
                    .is_some_and(Option::is_some)
                {
                    self.first_stale += 1;
                }
            }
        }
    }

    fn spans(&self, line: usize) -> &[StyledSpan] {
        match self.lines.get(line) {
            Some(Some(entry)) if line < self.first_stale => &entry.spans,
            _ => &[],
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
