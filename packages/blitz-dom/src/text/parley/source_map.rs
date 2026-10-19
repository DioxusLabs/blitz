//! The map from each text node's content to where it is in the laid-out text, recorded as the
//! text is pushed to Parley's tree builder.
//!
//! White space collapsing happens in the tree builder, so what it did with a text node's content
//! is read back from the text it appended.

use blitz_traits::node_id::NodeId;
use icu_properties::props::{GeneralCategory, GeneralCategoryGroup};
use icu_properties::{CodePointMapData, CodePointMapDataBorrowed};
use parley::{Brush, InlineBox, InlineBoxKind, TreeBuilder};
use style::computed_values::white_space_collapse::T as WhiteSpaceCollapse;
use style::values::computed::TextTransform;

use super::text_transform::{full_size_kana, full_width};

const GENERAL_CATEGORY: CodePointMapDataBorrowed<'static, GeneralCategory> =
    CodePointMapData::<GeneralCategory>::new();

/// Set in [`Unit::source`] where the unit's content is replaced as a whole.
const REPLACED: u32 = 1 << 31;

/// A stretch of a text node's content, up to the node's next unit, and where it starts in the
/// laid-out text.
///
/// The content either keeps its byte offsets, or is replaced as a whole: offsets within it then
/// fall after what replaced it. Each node's last unit holds its content's length and where it
/// ends.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Unit {
    /// The byte offset in the node's content, and [`REPLACED`].
    source: u32,
    /// The byte offset in the laid-out text.
    layout: u32,
}

impl Unit {
    fn new(source: usize, layout: usize, replaced: bool) -> Self {
        Self {
            source: source as u32 | if replaced { REPLACED } else { 0 },
            layout: layout as u32,
        }
    }

    fn source(self) -> usize {
        (self.source & !REPLACED) as usize
    }

    fn is_replaced(self) -> bool {
        self.source & REPLACED != 0
    }
}

/// Where each text node's content is in an inline formatting context's laid-out text.
#[derive(Clone, Debug, Default)]
pub(crate) struct SourceMap {
    /// Each text node and its first unit, in the order the nodes were pushed.
    nodes: Vec<(NodeId, u32)>,
    units: Vec<Unit>,
}

impl SourceMap {
    pub(super) fn clear(&mut self) {
        self.nodes.clear();
        self.units.clear();
    }

    /// Where byte `offset` of text node `node`'s content falls in the laid-out text.
    pub(crate) fn layout_offset(&self, node: NodeId, offset: usize) -> Option<usize> {
        let index = self.nodes.iter().position(|&(id, _)| id == node)?;
        let start = self.nodes[index].1 as usize;
        let end = self
            .nodes
            .get(index + 1)
            .map_or(self.units.len(), |&(_, first)| first as usize);
        let units = &self.units[start..end];
        let offset = offset.min(units.last()?.source());
        let index = units.partition_point(|unit| unit.source() <= offset) - 1;
        let unit = units[index];
        Some(if !unit.is_replaced() {
            unit.layout as usize + (offset - unit.source())
        } else if offset == unit.source() {
            unit.layout as usize
        } else {
            units[index + 1].layout as usize
        })
    }
}

/// Records a [`SourceMap`] as text and inline boxes are pushed to a tree builder.
pub(super) struct SourceMapRecorder<'a> {
    map: &'a mut SourceMap,
    /// While the tree builder has collapsible white space pending: the first of the units that
    /// follow that white space, if it is kept.
    after_pending: usize,
}

impl<'a> SourceMapRecorder<'a> {
    /// Records into `map`, replacing what it held.
    pub(super) fn new(map: &'a mut SourceMap) -> Self {
        map.clear();
        Self {
            map,
            after_pending: 0,
        }
    }

    /// Pushes `pushed`, which is text node `node`'s content `source` after `transform`, and
    /// records where it went.
    pub(super) fn push_text<B: Brush>(
        &mut self,
        tree: &mut TreeBuilder<'_, B>,
        node: NodeId,
        source: &str,
        pushed: &str,
        transform: TextTransform,
        collapse: WhiteSpaceCollapse,
    ) {
        let Appended { start, was_pending } = self.push(tree, pushed, is_collapsing(collapse));
        let rendered = &tree.text()[start..];

        self.map.nodes.push((node, self.map.units.len() as u32));
        if transform.is_empty() && rendered.len() == source.len() {
            // Nothing was removed, so every byte is where it was.
            if !source.is_empty() {
                self.map.units.push(Unit::new(0, start, false));
            }
        } else if !transform.is_empty()
            || !record_collapsed_units(
                &mut self.map.units,
                source,
                rendered,
                start,
                collapse,
                was_pending,
            )
        {
            record_units(
                &mut self.map.units,
                source,
                rendered,
                start,
                transform,
                collapse,
                was_pending,
            );
        }
        let end = self.map.units.len();
        self.map
            .units
            .push(Unit::new(source.len(), start + rendered.len(), false));

        // White space that is pending after this node's content is this node's.
        if tree.has_pending_whitespace() && (!was_pending || !rendered.is_empty()) {
            self.after_pending = end;
        }
    }

    /// Pushes text that no text node holds, such as a list marker's or a forced line break.
    pub(super) fn push_generated_text<B: Brush>(
        &mut self,
        tree: &mut TreeBuilder<'_, B>,
        text: &str,
        collapsing: bool,
    ) {
        let Appended { start, was_pending } = self.push(tree, text, collapsing);
        if tree.has_pending_whitespace() && (!was_pending || tree.text().len() > start) {
            self.after_pending = self.map.units.len();
        }
    }

    /// Pushes `text`, returning where what it appended starts, after the space that white space
    /// pending before it was kept as.
    fn push<B: Brush>(
        &mut self,
        tree: &mut TreeBuilder<'_, B>,
        text: &str,
        collapsing: bool,
    ) -> Appended {
        let was_pending = tree.has_pending_whitespace();
        let before = tree.text().len();
        tree.push_text(text);
        let appended = &tree.text()[before..];

        // Collapsing text never starts with a space of its own, and other text is appended as
        // it is.
        let kept_pending = was_pending
            && appended.starts_with(' ')
            && (collapsing || appended.len() == text.len() + 1);
        if kept_pending {
            self.keep_pending();
        }
        Appended {
            start: before + usize::from(kept_pending),
            was_pending,
        }
    }

    /// Pushes an inline box, which keeps pending white space where it is in flow.
    pub(super) fn push_inline_box<B: Brush>(
        &mut self,
        tree: &mut TreeBuilder<'_, B>,
        inline_box: InlineBox,
    ) {
        let in_flow = inline_box.kind == InlineBoxKind::InFlow;
        let before = tree.text().len();
        tree.push_inline_box(inline_box);
        if in_flow && tree.text().len() > before {
            self.keep_pending();
        }
    }

    /// The pending white space was kept as a space, which the units after it follow.
    fn keep_pending(&mut self) {
        for unit in &mut self.map.units[self.after_pending..] {
            unit.layout += 1;
        }
    }
}

struct Appended {
    /// Where the pushed text's own content starts in the laid-out text.
    start: usize,
    /// Whether collapsible white space was pending before the push.
    was_pending: bool,
}

fn is_collapsing(collapse: WhiteSpaceCollapse) -> bool {
    matches!(
        collapse,
        WhiteSpaceCollapse::Collapse | WhiteSpaceCollapse::PreserveBreaks
    )
}

/// Records the units of `source`, which was appended as `rendered` at `start` with nothing but
/// white space collapsing between them. Returns `false`, recording nothing, where there was more.
fn record_collapsed_units(
    units: &mut Vec<Unit>,
    source: &str,
    rendered: &str,
    start: usize,
    collapse: WhiteSpaceCollapse,
    mut in_white_space: bool,
) -> bool {
    let (source, rendered) = (source.as_bytes(), rendered.as_bytes());
    let is_collapsible = |byte: &u8| match collapse {
        WhiteSpaceCollapse::Collapse => byte.is_ascii_whitespace(),
        WhiteSpaceCollapse::PreserveBreaks => matches!(byte, b' ' | b'\t'),
        _ => false,
    };

    let first_unit = units.len();
    let (mut offset, mut cursor) = (0, 0);
    let mut in_kept_unit = false;
    while offset < source.len() {
        let mut kept = source[offset..]
            .iter()
            .zip(&rendered[cursor..])
            .take_while(|(source, rendered)| source == rendered)
            .count();
        if kept > 0 {
            in_white_space = is_collapsible(&source[offset + kept - 1]);
        } else if !is_collapsible(&source[offset]) {
            units.truncate(first_unit);
            return false;
        } else if !in_white_space && rendered.get(cursor) == Some(&b' ') {
            // The first of a white space sequence, kept as a space.
            in_white_space = true;
            kept = 1;
        }

        if kept > 0 {
            if !in_kept_unit {
                units.push(Unit::new(offset, start + cursor, false));
                in_kept_unit = true;
            }
            offset += kept;
            cursor += kept;
        } else {
            // White space that was removed, as is the white space after it.
            units.push(Unit::new(offset, start + cursor, true));
            in_kept_unit = false;
            in_white_space = true;
            offset += source[offset..]
                .iter()
                .take_while(|byte| is_collapsible(byte))
                .count();
        }
    }
    true
}

/// Records the units of `source`, which was appended as `rendered` at `start`.
fn record_units(
    units: &mut Vec<Unit>,
    source: &str,
    rendered: &str,
    start: usize,
    transform: TextTransform,
    collapse: WhiteSpaceCollapse,
    mut in_white_space: bool,
) {
    #[derive(PartialEq)]
    enum Last {
        Kept,
        Removed,
        Replaced,
    }

    let mut cursor = 0;
    let mut last = Last::Replaced;
    for (offset, c) in source.char_indices() {
        let len = rendered_len(
            c,
            &rendered[cursor..],
            transform,
            collapse,
            &mut in_white_space,
        );
        let this = if len == c.len_utf8() {
            Last::Kept
        } else if len == 0 {
            Last::Removed
        } else {
            Last::Replaced
        };
        // Kept content continues its unit, as does content removed after removed content.
        if this == Last::Replaced || this != last {
            units.push(Unit::new(offset, start + cursor, this != Last::Kept));
        }
        last = this;
        cursor += len;
    }
}

/// The byte length of what `c` was appended as, which `rendered` starts with.
fn rendered_len(
    c: char,
    rendered: &str,
    transform: TextTransform,
    collapse: WhiteSpaceCollapse,
    in_white_space: &mut bool,
) -> usize {
    let collapsible = match collapse {
        WhiteSpaceCollapse::Collapse => c.is_ascii_whitespace(),
        WhiteSpaceCollapse::PreserveBreaks => matches!(c, ' ' | '\t'),
        _ => false,
    };
    if collapsible {
        // Only the first of a white space sequence is kept, where any is.
        let first = !std::mem::replace(in_white_space, true);
        return usize::from(first && rendered.starts_with(' '));
    }
    *in_white_space = false;

    let mut bytes = [0; 4];
    let original = c.encode_utf8(&mut bytes);
    if rendered.starts_with(&*original) {
        return original.len();
    }
    if c.is_ascii()
        && rendered
            .as_bytes()
            .first()
            .is_some_and(|b| b.eq_ignore_ascii_case(&bytes[0]))
    {
        return 1;
    }
    // Case expansions count only where they are in the laid-out text.
    let map_width = |c| {
        let c = if transform.contains(TextTransform::FULL_WIDTH) {
            full_width(c)
        } else {
            c
        };
        if transform.contains(TextTransform::FULL_SIZE_KANA) {
            full_size_kana(c)
        } else {
            c
        }
    };
    let mapped = if transform.contains(TextTransform::UPPERCASE)
        || transform.contains(TextTransform::CAPITALIZE)
    {
        matching_prefix(rendered, c.to_uppercase().map(map_width))
    } else if transform.contains(TextTransform::LOWERCASE) {
        matching_prefix(rendered, c.to_lowercase().map(map_width))
    } else {
        None
    };
    if let Some(len) = mapped {
        len
    } else if !transform.is_empty() && GeneralCategoryGroup::Mark.contains(GENERAL_CATEGORY.get(c))
    {
        // Tailored casing can remove combining marks (e.g. Greek uppercase acute).
        0
    } else {
        // math-auto, full-width, full-size-kana and one-to-one tailored case mappings preserve
        // character count.
        rendered.chars().next().map_or(0, char::len_utf8)
    }
}

fn matching_prefix(text: &str, mapped: impl Iterator<Item = char>) -> Option<usize> {
    let mut text = text.chars();
    let mut bytes = 0;
    for c in mapped {
        if text.next() != Some(c) {
            return None;
        }
        bytes += c.len_utf8();
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use parley::{FontContext, LayoutContext, StyleProperty, TextStyle};

    use super::*;

    /// What a test pushes, in order. Text nodes are numbered from 0 as they are pushed.
    enum Push<'a> {
        Text(&'a str),
        /// A text node's content, and what `uppercase` made of it.
        Uppercase(&'a str, &'a str),
        Pre(&'a str),
        Marker(&'a str),
        Box,
        Float,
        Break,
    }
    use Push::*;

    fn build(pushes: &[Push<'_>]) -> (String, SourceMap) {
        let mut font_ctx = FontContext::new();
        let mut layout_ctx = LayoutContext::<()>::new();
        let style = TextStyle {
            white_space_collapse: parley::WhiteSpaceCollapse::Collapse,
            ..Default::default()
        };
        let mut tree = layout_ctx.tree_builder(&mut font_ctx, 1.0, true, &style);
        let mut map = SourceMap::default();
        let mut recorder = SourceMapRecorder::new(&mut map);
        let mut node = 0;
        let mut text = |recorder: &mut SourceMapRecorder,
                        tree: &mut TreeBuilder<'_, ()>,
                        source: &str,
                        pushed: &str,
                        transform,
                        collapse| {
            let id = NodeId::from_u64(node);
            node += 1;
            recorder.push_text(tree, id, source, pushed, transform, collapse);
        };
        let preserve = [StyleProperty::WhiteSpaceCollapse(
            parley::WhiteSpaceCollapse::Preserve,
        )];
        let inline_box = |kind| InlineBox {
            id: 0,
            kind,
            index: 0,
            width: 0.0,
            height: 0.0,
            baseline: None,
            vertical_align: Default::default(),
        };
        for push in pushes {
            match *push {
                Text(source) => text(
                    &mut recorder,
                    &mut tree,
                    source,
                    source,
                    TextTransform::NONE,
                    WhiteSpaceCollapse::Collapse,
                ),
                Uppercase(source, pushed) => text(
                    &mut recorder,
                    &mut tree,
                    source,
                    pushed,
                    TextTransform::UPPERCASE,
                    WhiteSpaceCollapse::Collapse,
                ),
                Pre(source) => {
                    tree.push_style_modification_span(&preserve);
                    text(
                        &mut recorder,
                        &mut tree,
                        source,
                        source,
                        TextTransform::NONE,
                        WhiteSpaceCollapse::Preserve,
                    );
                    tree.pop_style_span();
                }
                Marker(marker) => recorder.push_generated_text(&mut tree, marker, true),
                Box => recorder.push_inline_box(&mut tree, inline_box(InlineBoxKind::InFlow)),
                Float => {
                    recorder.push_inline_box(&mut tree, inline_box(InlineBoxKind::CustomOutOfFlow))
                }
                Break => {
                    tree.push_style_modification_span(&preserve);
                    recorder.push_generated_text(&mut tree, "\n", false);
                    tree.pop_style_span();
                }
            }
        }
        let (_, text) = tree.build();
        (text, map)
    }

    /// The laid-out text, and where each offset of each text node's content is in it.
    fn offsets(pushes: &[Push<'_>]) -> (String, Vec<Vec<usize>>) {
        let (text, map) = build(pushes);
        let mut nodes = Vec::new();
        for push in pushes {
            let (Text(source) | Uppercase(source, _) | Pre(source)) = push else {
                continue;
            };
            let id = NodeId::from_u64(nodes.len() as u64);
            nodes.push(
                (0..=source.len())
                    .filter(|&offset| source.is_char_boundary(offset))
                    .map(|offset| map.layout_offset(id, offset).unwrap())
                    .collect(),
            );
        }
        (text, nodes)
    }

    fn check(pushes: &[Push<'_>], text: &str, nodes: &[&[usize]]) {
        let (actual_text, actual_nodes) = offsets(pushes);
        assert_eq!(actual_text, text);
        assert_eq!(actual_nodes, nodes);
    }

    #[test]
    fn collapsed_units_match_the_units_recorded_per_character() {
        use WhiteSpaceCollapse::{Collapse, PreserveBreaks};
        for (source, rendered, collapse, pending) in [
            ("a  b\n\t c ", "a b c", Collapse, false),
            ("  a b", "a b", Collapse, false),
            ("  a b", "a b", Collapse, true),
            ("\n", "", Collapse, false),
            ("\n", " ", Collapse, false),
            ("é \n ü  ", "é ü", Collapse, false),
            ("a \n b\tc", "a\nb c", PreserveBreaks, false),
        ] {
            let (mut fast, mut slow) = (Vec::new(), Vec::new());
            assert!(record_collapsed_units(
                &mut fast, source, rendered, 7, collapse, pending
            ));
            let none = TextTransform::NONE;
            record_units(&mut slow, source, rendered, 7, none, collapse, pending);
            assert_eq!(fast, slow, "{source:?}");
        }
        let mut units = Vec::new();
        assert!(!record_collapsed_units(
            &mut units, "ab", "xb", 0, Collapse, false
        ));
        assert!(units.is_empty());
    }

    #[test]
    fn unchanged_text_keeps_its_offsets() {
        check(
            &[Text("ab"), Text("c\nd")],
            "abc d",
            &[&[0, 1, 2], &[2, 3, 4, 5]],
        );
    }

    #[test]
    fn unknown_node_and_offset_past_the_end() {
        let (_, map) = build(&[Text("ab")]);
        assert_eq!(map.layout_offset(NodeId::from_u64(7), 0), None);
        assert_eq!(map.layout_offset(NodeId::from_u64(0), 9), Some(2));
    }

    #[test]
    fn collapsed_white_space_falls_after_its_space() {
        check(&[Text("a  \n b")], "a b", &[&[0, 1, 2, 2, 2, 2, 3]]);
    }

    #[test]
    fn leading_and_trailing_white_space_is_removed() {
        check(&[Text(" a ")], "a", &[&[0, 0, 1, 1]]);
        check(&[Text("  ")], "", &[&[0, 0, 0]]);
        check(&[Text("")], "", &[&[0]]);
    }

    #[test]
    fn trailing_white_space_is_kept_by_the_text_after_it() {
        check(&[Text("a "), Text("b")], "a b", &[&[0, 1, 2], &[2, 3]]);
        check(&[Text("a "), Text(" b")], "a b", &[&[0, 1, 2], &[2, 2, 3]]);
        check(
            &[Text("a"), Text(" "), Text("b")],
            "a b",
            &[&[0, 1], &[1, 2], &[2, 3]],
        );
        // White space only nodes within pending white space follow its space.
        check(
            &[Text("a "), Text(" "), Text(""), Text("b")],
            "a b",
            &[&[0, 1, 2], &[2, 2], &[2], &[2, 3]],
        );
        check(&[Text("a "), Pre(" b")], "a  b", &[&[0, 1, 2], &[2, 3, 4]]);
        check(&[Text("a "), Pre("b")], "a b", &[&[0, 1, 2], &[2, 3]]);
    }

    #[test]
    fn white_space_on_both_sides_of_an_inline_box_is_kept() {
        check(
            &[Text("a "), Box, Text(" b")],
            "a  b",
            &[&[0, 1, 2], &[2, 3, 4]],
        );
        check(&[Text(" "), Box, Text(" ")], "", &[&[0, 0], &[0, 0]]);
        check(&[Box, Text(" a")], " a", &[&[0, 1, 2]]);
    }

    #[test]
    fn white_space_collapses_across_an_out_of_flow_box() {
        check(
            &[Text("a "), Float, Text(" b")],
            "a b",
            &[&[0, 1, 2], &[2, 2, 3]],
        );
    }

    #[test]
    fn forced_break_removes_the_white_space_around_it() {
        check(
            &[Text("a "), Break, Text(" b")],
            "a\nb",
            &[&[0, 1, 1], &[2, 2, 3]],
        );
    }

    #[test]
    fn text_follows_a_marker() {
        check(&[Marker("• "), Text(" a")], "• a", &[&[4, 4, 5]]);
        check(&[Marker("1."), Text(" a")], "1. a", &[&[2, 3, 4]]);
    }

    #[test]
    fn case_expansion_is_replaced_as_a_whole() {
        check(
            &[Uppercase("aß  b", "ASS  B"), Text("c")],
            "ASS Bc",
            &[&[0, 1, 3, 4, 4, 5], &[5, 6]],
        );
    }
}
