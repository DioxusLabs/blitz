use std::ops::Range;

use super::{TextOffsetMap, is_segment_break};

/// The white space at the end of a line, as byte ranges of the layout text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEndWhitespace {
    /// Preserved spaces and tabs (`pre-wrap`) that hang past the end of the line. Before a
    /// forced break they only hang if they overflow.
    pub hanging: Range<usize>,
    /// The collapsible space that CSS removes at the end of the line. Parley keeps it in the
    /// text as a hanging space.
    pub removed: Range<usize>,
}

/// The white space at the end of the line spanning `line` of the layout `text`, not counting a
/// forced line break.
///
/// Parley only reports the total hanging advance of each line, so this scans back from the
/// line's end. If Parley reports these ranges itself one day, only this needs to change.
pub fn line_end_whitespace<Id: Copy + Ord>(
    text: &str,
    line: Range<usize>,
    map: &TextOffsetMap<Id>,
) -> LineEndWhitespace {
    let start = line.start;
    let text = &text[line];
    let mut end = text.len();
    if let Some(c) = text.chars().next_back().filter(|&c| is_segment_break(c)) {
        end -= c.len_utf8();
    }
    let removed_end = end;
    while let Some(c) = text[..end].chars().next_back()
        && map.is_collapsible_space(start + end - c.len_utf8(), c)
    {
        end -= c.len_utf8();
    }
    let hanging_end = end;
    while let Some(c) = text[..end].chars().next_back()
        && matches!(c, ' ' | '\t' | '\u{3000}')
        && map.is_pre_wrap(start + end - c.len_utf8())
    {
        end -= c.len_utf8();
    }
    LineEndWhitespace {
        hanging: start + end..start + hanging_end,
        removed: start + hanging_end..start + removed_end,
    }
}
