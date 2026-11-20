use std::ops::Range;

use parley::{Brush, Cluster, Layout, Line};

use super::{TextOffsetMap, is_segment_break};

/// The white space at the end of a line, as byte ranges of the layout text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEndWhitespace {
    /// Preserved spaces and tabs (`pre-wrap`) that hang past the end of the line.
    pub hanging: Range<usize>,
    /// The collapsible space that CSS removes at the end of the line. Parley keeps it in the
    /// text as a hanging space.
    pub removed: Range<usize>,
}

/// Total advances within this of the line's hanging advance are taken to match it.
const ADVANCE_EPSILON: f32 = 0.01;

/// The white space at the end of line `line` of `layout`, as byte ranges of the layout `text`,
/// not counting a forced line break.
///
/// Parley only reports the total hanging advance of each line, so this walks back over the
/// line's trailing white space clusters, in logical order, until their advances add up to it.
/// If Parley reports these ranges itself one day, only this needs to change.
pub fn line_end_whitespace<B: Brush, Id: Copy + Ord>(
    text: &str,
    layout: &Layout<B>,
    line: &Line<'_, B>,
    map: &TextOffsetMap<Id>,
) -> LineEndWhitespace {
    let line_range = line.text_range();
    let mut end = line_range.end;
    let mut cluster = end
        .checked_sub(1)
        .filter(|&last| last >= line_range.start)
        .and_then(|last| Cluster::from_byte_index(layout, last));
    if let Some(last) = &cluster
        && text[last.text_range()].starts_with(is_segment_break)
    {
        end = last.text_range().start;
        cluster = last.previous_logical();
    }

    let mut hanging_start = end;
    let mut remaining = line.metrics().hanging_advance;
    while remaining > ADVANCE_EPSILON
        && let Some(current) = cluster
    {
        let range = current.text_range();
        let advance = current.advance();
        // An inline box between clusters has no cluster of its own and ends the white space.
        if range.end != hanging_start
            || range.start < line_range.start
            || advance > remaining + ADVANCE_EPSILON
            || !text[range.clone()]
                .chars()
                .all(|c| matches!(c, ' ' | '\t' | '\u{3000}'))
        {
            break;
        }
        remaining -= advance;
        hanging_start = range.start;
        cluster = current.previous_logical();
    }

    let mut removed_start = end;
    while removed_start > hanging_start
        && let Some(c) = text[..removed_start].chars().next_back()
        && map.is_collapsible_space(removed_start - c.len_utf8(), c)
    {
        removed_start -= c.len_utf8();
    }
    LineEndWhitespace {
        hanging: hanging_start..removed_start,
        removed: removed_start..end,
    }
}
