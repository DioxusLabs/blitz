//! Mapping between offsets in the DOM and offsets in the text of an inline formatting context.

use blitz_traits::node_id::NodeId;
use std::ops::Range;

/// Which side of a position to prefer when several DOM positions map to the same layout offset
/// (e.g. at the boundary between two text nodes, or at a collapsed white space run).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Affinity {
    /// The position at the end of the preceding content.
    Upstream,
    /// The position at the start of the following content.
    Downstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitKind {
    /// Source and layout ranges have the same length and map byte for byte.
    Identity = 0,
    /// Source and layout ranges have different lengths (and either may be empty), so only their
    /// ends map to each other. Used for collapsed white space, text transforms that change
    /// length, inline boxes and generated text.
    Variable = 1,
    /// The single space a collapsible white space run collapsed to. Maps like `Variable`.
    KeptSpace = 2,
    /// Like `Identity`, in preserved white space that wraps (`pre-wrap`), where spaces at a
    /// line end hang.
    PreWrap = 3,
    /// Like `Identity`, in collapsible white space. Each U+0020 in it is a kept space that
    /// collapsing left unchanged, so lone spaces between words don't need units of their own.
    CollapsibleText = 4,
}

impl UnitKind {
    fn is_identity(self) -> bool {
        matches!(
            self,
            UnitKind::Identity | UnitKind::PreWrap | UnitKind::CollapsibleText
        )
    }
}

const KIND_SHIFT: u32 = 29;
const OFFSET_MASK: u32 = (1 << KIND_SHIFT) - 1;

/// The start of a mapped range. The range ends where the next unit of the same node starts.
#[derive(Clone, Copy)]
struct Unit {
    /// The source offset, with the [`UnitKind`] in the top three bits.
    source: u32,
    layout: u32,
}

impl Unit {
    fn new(kind: UnitKind, source: usize, layout: usize) -> Self {
        debug_assert!(source <= OFFSET_MASK as usize);
        Self {
            source: source as u32 | ((kind as u32) << KIND_SHIFT),
            layout: layout as u32,
        }
    }

    fn kind(self) -> UnitKind {
        match self.source >> KIND_SHIFT {
            0 => UnitKind::Identity,
            1 => UnitKind::Variable,
            2 => UnitKind::KeptSpace,
            3 => UnitKind::PreWrap,
            _ => UnitKind::CollapsibleText,
        }
    }

    fn source(self) -> usize {
        (self.source & OFFSET_MASK) as usize
    }

    fn layout(self) -> usize {
        self.layout as usize
    }
}

/// Maps offsets in the DOM nodes of an inline formatting context to byte offsets in the text
/// that was laid out, and back.
///
/// It is recorded while Blitz builds the layout text, as each node's text is transformed and
/// its white space collapsed, so nothing is inferred afterwards.
///
/// Source offsets are UTF-8 byte offsets into a text node's content. Inline boxes and `<br>`
/// elements have a source length of 1 (before and after the element). Generated text (list
/// markers) is attributed to the inline root with a source length of 0.
///
/// Each node owns a contiguous run of units in layout order, ending with a sentinel unit at
/// the node's source length, so plain text costs two 8-byte units per node.
#[derive(Clone, Default)]
pub struct TextOffsetMap {
    /// Each node's id and the index of its first unit, in layout order.
    nodes: Vec<(NodeId, u32)>,
    /// Indices into `nodes`, sorted by node id.
    by_id: Vec<u32>,
    /// All units, in layout order. Layout offsets never decrease.
    units: Vec<Unit>,
}

impl TextOffsetMap {
    /// The layout offset of `source` in `node`, or `None` if `node` isn't in this layout or the
    /// offset is past its end. Offsets inside collapsed white space or inside a character whose
    /// transform changed its length map to the end of the collapsed or transformed text.
    pub fn layout_offset(&self, node: NodeId, source: usize) -> Option<usize> {
        let units = &self.units[self.node_units(node)?];
        let (last, rest) = units.split_last()?;
        if source > last.source() {
            return None;
        }
        let index = units.partition_point(|unit| unit.source() <= source) - 1;
        let Some(unit) = rest.get(index) else {
            return Some(last.layout());
        };
        let next = units[index + 1];
        Some(if source == unit.source() {
            unit.layout()
        } else if unit.kind().is_identity() {
            unit.layout() + (source - unit.source())
        } else {
            next.layout()
        })
    }

    /// The DOM position (node and source offset) of the layout offset `layout`.
    pub fn source_position(&self, layout: usize, affinity: Affinity) -> Option<(NodeId, usize)> {
        let downstream = self.units.partition_point(|unit| unit.layout() <= layout);
        let index = match affinity {
            Affinity::Upstream => match self.units.partition_point(|unit| unit.layout() < layout) {
                0 => downstream,
                index => index,
            },
            Affinity::Downstream => downstream,
        }
        .checked_sub(1)?;
        let node = self
            .nodes
            .partition_point(|&(_, first)| first as usize <= index)
            - 1;
        let end = self.node_end(node);
        let unit = self.units[index];
        if index + 1 == end {
            return Some((self.nodes[node].0, unit.source()));
        }
        let next = self.units[index + 1];
        let source = if unit.kind().is_identity() {
            unit.source()
                + layout
                    .saturating_sub(unit.layout())
                    .min(next.source() - unit.source())
        } else if layout <= unit.layout() {
            unit.source()
        } else if layout >= next.layout() || affinity == Affinity::Downstream {
            next.source()
        } else {
            unit.source()
        };
        Some((self.nodes[node].0, source))
    }

    /// The range of layout text produced by `node`.
    pub fn layout_range(&self, node: NodeId) -> Option<Range<usize>> {
        let units = &self.units[self.node_units(node)?];
        Some(units.first()?.layout()..units.last()?.layout())
    }

    /// Whether the character `c` at `layout` is a space that a collapsible white space run
    /// collapsed to, which CSS removes at a line end.
    pub(crate) fn is_collapsible_space(&self, layout: usize, c: char) -> bool {
        match self.kind_at(layout) {
            Some(UnitKind::KeptSpace) => true,
            Some(UnitKind::CollapsibleText) => c == ' ',
            _ => false,
        }
    }

    /// Whether white space at `layout` is preserved and wraps, so it hangs at a line end.
    pub(crate) fn is_pre_wrap(&self, layout: usize) -> bool {
        self.kind_at(layout) == Some(UnitKind::PreWrap)
    }

    fn kind_at(&self, layout: usize) -> Option<UnitKind> {
        let index = self.units.partition_point(|unit| unit.layout() <= layout);
        Some(self.units[index.checked_sub(1)?].kind())
    }

    fn node_units(&self, node: NodeId) -> Option<Range<usize>> {
        let index = self
            .by_id
            .binary_search_by_key(&node.as_u64(), |&index| {
                self.nodes[index as usize].0.as_u64()
            })
            .ok()?;
        let index = self.by_id[index] as usize;
        Some(self.nodes[index].1 as usize..self.node_end(index))
    }

    fn node_end(&self, node: usize) -> usize {
        self.nodes
            .get(node + 1)
            .map_or(self.units.len(), |&(_, first)| first as usize)
    }
}

impl std::fmt::Debug for TextOffsetMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_list();
        for (index, &(node, _)) in self.nodes.iter().enumerate() {
            let units = &self.units[self.nodes[index].1 as usize..self.node_end(index)];
            let units: Vec<_> = units
                .iter()
                .map(|unit| (unit.kind(), unit.source(), unit.layout()))
                .collect();
            list.entry(&(node.as_u64(), units));
        }
        list.finish()
    }
}

/// Records a [`TextOffsetMap`] in layout order.
#[derive(Default)]
pub(crate) struct TextOffsetMapBuilder {
    map: TextOffsetMap,
}

impl TextOffsetMapBuilder {
    pub(crate) fn begin_node(&mut self, node: NodeId) {
        self.map.nodes.push((node, self.map.units.len() as u32));
    }

    /// Starts a unit of `kind` at `source` in the current node and `layout`, returning its index.
    pub(crate) fn push(&mut self, kind: UnitKind, source: usize, layout: usize) -> usize {
        let units = &mut self.map.units;
        let node_start = self
            .map
            .nodes
            .last()
            .map_or(0, |&(_, first)| first as usize);
        if kind.is_identity()
            && units.len() > node_start
            && let Some(last) = units.last()
            && last.kind() == kind
            && source - last.source() == layout - last.layout()
        {
            return units.len() - 1;
        }
        debug_assert!(units.last().is_none_or(|last| last.layout() <= layout));
        units.push(Unit::new(kind, source, layout));
        units.len() - 1
    }

    /// Ends the current node, whose source is `source_len` long, at `layout`.
    pub(crate) fn end_node(&mut self, source_len: usize, layout: usize) {
        self.map
            .units
            .push(Unit::new(UnitKind::Identity, source_len, layout));
    }

    /// Moves the units from `first` onwards `delta` bytes later in the layout text.
    pub(crate) fn shift(&mut self, first: usize, delta: usize) {
        for unit in &mut self.map.units[first..] {
            unit.layout += delta as u32;
        }
    }

    pub(crate) fn finish(mut self) -> TextOffsetMap {
        let map = &mut self.map;
        map.by_id = (0..map.nodes.len() as u32).collect();
        map.by_id
            .sort_unstable_by_key(|&index| map.nodes[index as usize].0.as_u64());
        self.map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(id: u64) -> NodeId {
        NodeId::from_u64(id)
    }

    /// `"a  b"` + `"c"` collapsed to `"a bc"`.
    fn collapsed() -> TextOffsetMap {
        let mut builder = TextOffsetMapBuilder::default();
        builder.begin_node(id(1));
        builder.push(UnitKind::Identity, 0, 0);
        builder.push(UnitKind::KeptSpace, 1, 1);
        builder.push(UnitKind::Variable, 2, 2);
        builder.push(UnitKind::Identity, 3, 2);
        builder.end_node(4, 3);
        builder.begin_node(id(2));
        builder.push(UnitKind::Identity, 0, 3);
        builder.end_node(1, 4);
        builder.finish()
    }

    #[test]
    fn maps_source_to_layout() {
        let map = collapsed();
        let offsets: Vec<_> = (0..=4).map(|i| map.layout_offset(id(1), i)).collect();
        assert_eq!(offsets, [Some(0), Some(1), Some(2), Some(2), Some(3)]);
        assert_eq!(map.layout_offset(id(1), 5), None);
        assert_eq!(map.layout_offset(id(2), 1), Some(4));
        assert_eq!(map.layout_offset(id(3), 0), None);
        assert_eq!(map.layout_range(id(2)), Some(3..4));
    }

    #[test]
    fn maps_layout_to_source() {
        let map = collapsed();
        assert_eq!(map.source_position(2, Affinity::Upstream), Some((id(1), 2)));
        assert_eq!(
            map.source_position(2, Affinity::Downstream),
            Some((id(1), 3))
        );
        assert_eq!(map.source_position(3, Affinity::Upstream), Some((id(1), 4)));
        assert_eq!(
            map.source_position(3, Affinity::Downstream),
            Some((id(2), 0))
        );
        assert_eq!(map.source_position(4, Affinity::Upstream), Some((id(2), 1)));
        assert!(map.is_collapsible_space(1, ' '));
        assert!(!map.is_collapsible_space(0, 'a'));
        assert!(!map.is_collapsible_space(2, 'b'));
    }
}
