use std::cell::Cell;
use std::rc::Rc;

use floem_editor_core::buffer::Buffer;
use floem_editor_core::buffer::rope_text::RopeText;
use floem_editor_core::cursor::CursorAffinity;
use floem_editor_core::editor::EditType;
use floem_editor_core::selection::Selection;
use peniko::Color;

use super::{HighlightCache, LineHighlights, SpanStyle, StyledSpan, SyntaxHighlighter};

pub(crate) const COMMENT: SpanStyle = SpanStyle {
    foreground: Some(Color::from_rgb8(0, 128, 0)),
    bold: false,
    italic: true,
    underline: false,
};

/// Highlights `/* ... */` block comments, counting how many lines it highlights
pub(crate) struct BlockComments {
    pub(crate) calls: Rc<Cell<usize>>,
}

impl SyntaxHighlighter for BlockComments {
    type State = bool;

    fn initial_state(&self) -> bool {
        false
    }

    fn highlight_line(&mut self, line: &str, in_comment: &mut bool, spans: &mut Vec<StyledSpan>) {
        self.calls.set(self.calls.get() + 1);
        let mut pos = 0;
        while pos < line.len() {
            let rest = &line[pos..];
            if *in_comment {
                let end = rest.find("*/").map_or(line.len(), |i| pos + i + 2);
                spans.push(StyledSpan {
                    range: pos..end,
                    style: COMMENT,
                });
                *in_comment = end == line.len() && !rest.ends_with("*/");
                pos = end;
            } else if let Some(i) = rest.find("/*") {
                pos += i;
                *in_comment = true;
            } else {
                break;
            }
        }
    }
}

fn cache(text: &str) -> (HighlightCache<BlockComments>, Buffer, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    let buffer = Buffer::new(text);
    let highlighter = BlockComments {
        calls: calls.clone(),
    };
    (
        HighlightCache::new(highlighter, buffer.num_lines()),
        buffer,
        calls,
    )
}

fn insert(
    buffer: &mut Buffer,
    cache: &mut HighlightCache<BlockComments>,
    offset: usize,
    text: &str,
) {
    let selection = Selection::caret(offset, CursorAffinity::Backward);
    let (_, _, inval) = buffer.edit([(&selection, text)], EditType::InsertChars);
    cache.invalidate(&inval, buffer.num_lines());
}

fn ensure(cache: &mut HighlightCache<BlockComments>, buffer: &Buffer, line: usize) -> Vec<usize> {
    let mut changed = Vec::new();
    cache.ensure(buffer, line, &mut |line| changed.push(line));
    changed
}

#[test]
fn highlights_lazily_up_to_the_requested_line() {
    let (mut cache, buffer, calls) = cache("a\nb\nc\nd");
    ensure(&mut cache, &buffer, 1);
    assert_eq!(calls.get(), 2);
    // Already up to date
    ensure(&mut cache, &buffer, 0);
    assert_eq!(calls.get(), 2);
    assert!(cache.spans(3).is_empty());
}

#[test]
fn carries_state_across_lines() {
    let (mut cache, buffer, _) = cache("x /* a\nb\nc */ y\nz");
    ensure(&mut cache, &buffer, 3);
    assert_eq!(cache.spans(0)[0].range, 2..6);
    assert_eq!(cache.spans(1)[0].range, 0..1);
    assert_eq!(cache.spans(2)[0].range, 0..4);
    assert!(cache.spans(3).is_empty());
}

#[test]
fn edits_that_do_not_change_the_state_stop_after_the_edited_line() {
    let (mut cache, mut buffer, calls) = cache("a\nb\nc\nd\ne");
    ensure(&mut cache, &buffer, 4);
    calls.set(0);

    insert(&mut buffer, &mut cache, 2, "x");
    let changed = ensure(&mut cache, &buffer, 4);
    // Line 1 is re-highlighted (it was edited), then line 2 ends in the same state as before
    assert_eq!(calls.get(), 2);
    assert!(changed.is_empty());
}

#[test]
fn opening_a_comment_reports_the_lines_it_restyles() {
    let (mut cache, mut buffer, _) = cache("a\nb\nc\nd");
    ensure(&mut cache, &buffer, 3);

    insert(&mut buffer, &mut cache, 0, "/*");
    let changed = ensure(&mut cache, &buffer, 3);
    assert_eq!(changed, vec![1, 2, 3]);
    assert_eq!(cache.spans(2)[0].style, COMMENT);

    // Closing it again restyles them back
    let end = buffer.offset_of_line(1) - 1;
    insert(&mut buffer, &mut cache, end, "*/");
    let changed = ensure(&mut cache, &buffer, 3);
    assert_eq!(changed, vec![1, 2, 3]);
    assert!(cache.spans(2).is_empty());
}

#[test]
fn inserted_lines_are_highlighted() {
    let (mut cache, mut buffer, _) = cache("/*\nb\n*/\nd");
    ensure(&mut cache, &buffer, 3);
    let offset = buffer.offset_of_line(1);
    insert(&mut buffer, &mut cache, offset, "new\nlines\n");
    ensure(&mut cache, &buffer, 5);
    assert_eq!(buffer.num_lines(), 6);
    for line in 1..=4 {
        assert_eq!(cache.spans(line)[0].style, COMMENT, "line {line}");
    }
    assert!(cache.spans(5).is_empty());
}

#[cfg(feature = "syntaxmate")]
mod syntaxmate {
    use std::path::Path;

    use crate::{SyntaxHighlighter, SyntaxmateHighlighter};

    #[test]
    fn highlights_rust() {
        let mut highlighter = SyntaxmateHighlighter::new("rust", "github-dark").unwrap();
        let mut state = highlighter.initial_state();
        let mut spans = Vec::new();
        highlighter.highlight_line("fn main() { let x = \"hi\"; }", &mut state, &mut spans);
        let at = |offset: usize| {
            spans
                .iter()
                .find(|s| s.range.contains(&offset))
                .map(|s| s.style.foreground)
        };
        // `fn`, `let` and the string are colored, and differently from each other
        assert!(at(0).flatten().is_some());
        assert!(at(20).flatten().is_some());
        assert_ne!(at(0), at(20));
    }

    #[test]
    fn carries_block_comments_across_lines() {
        let mut highlighter = SyntaxmateHighlighter::new("rust", "github-dark").unwrap();
        let mut state = highlighter.initial_state();
        let mut spans = Vec::new();
        highlighter.highlight_line("/* start", &mut state, &mut spans);
        let comment = spans[0].style;
        spans.clear();
        highlighter.highlight_line("let x = 1;", &mut state, &mut spans);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].range, 0..10);
        assert_eq!(spans[0].style, comment);
    }

    #[test]
    fn unknown_names_are_errors() {
        assert!(SyntaxmateHighlighter::new("not-a-language", "github-dark").is_err());
        assert!(SyntaxmateHighlighter::new("rust", "not-a-theme").is_err());
    }

    #[test]
    fn detects_languages() {
        let detect = SyntaxmateHighlighter::detect_language;
        assert_eq!(
            detect(Some(Path::new("main.rs")), "").as_deref(),
            Some("rust")
        );
        assert_eq!(
            detect(None, "#!/usr/bin/env python3\n").as_deref(),
            Some("python")
        );
    }

    #[test]
    fn editor_theme_uses_the_theme_colors() {
        let highlighter = SyntaxmateHighlighter::new("rust", "github-light").unwrap();
        let theme = highlighter.editor_theme();
        assert_eq!(theme.background, peniko::Color::from_rgb8(0xff, 0xff, 0xff));
    }
}
