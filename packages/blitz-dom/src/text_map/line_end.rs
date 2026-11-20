use std::ops::Range;

use parley::{Brush, Line};

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

/// The white space at the end of the laid out `line` of the layout `text`, not counting a
/// forced line break.
///
/// Parley only reports the total hanging advance of each line, so this walks back over the
/// line's trailing white space clusters until their advances add up to it. If Parley reports
/// these ranges itself one day, only this needs to change.
pub fn line_end_whitespace<B: Brush, Id: Copy + Ord>(
    text: &str,
    line: &Line<'_, B>,
    map: &TextOffsetMap<Id>,
) -> LineEndWhitespace {
    let mut clusters: Vec<(Range<usize>, f32)> = line
        .runs()
        .flat_map(|run| run.clusters())
        .map(|cluster| (cluster.text_range(), cluster.advance()))
        .collect();
    clusters.sort_unstable_by_key(|(range, _)| range.start);

    let mut end = line.text_range().end;
    let mut clusters = &clusters[..];
    if let Some(((range, _), rest)) = clusters.split_last()
        && range.end == end
        && text[range.clone()].starts_with(is_segment_break)
    {
        end = range.start;
        clusters = rest;
    }

    let mut hanging_start = end;
    let mut remaining = line.metrics().hanging_advance;
    for (range, advance) in clusters.iter().rev() {
        // An inline box between clusters has no cluster of its own and ends the white space.
        if range.end != hanging_start
            || *advance > remaining + ADVANCE_EPSILON
            || !text[range.clone()]
                .chars()
                .all(|c| matches!(c, ' ' | '\t' | '\u{3000}'))
        {
            break;
        }
        remaining -= advance;
        hanging_start = range.start;
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
