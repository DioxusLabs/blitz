//! Document selections store DOM boundary points independently of layout.

use crate::{BaseDocument, Node, NodeTree, node::TextLayout};
use blitz_traits::node_id::NodeId;
use kurbo::{Affine, Point};
use markup5ever::local_name;
use std::{cmp::Ordering, collections::HashMap, ops::Range};
use style::computed_values::visibility::T as Visibility;
use style::values::computed::UserSelect;

/// A DOM boundary point. Text offsets are UTF-8 byte offsets; other offsets
/// count DOM children. Anonymous layout boxes are never boundary containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionPoint {
    pub node: NodeId,
    pub offset: usize,
}

/// Anchor and focus in DOM order-independent boundary-point coordinates.
#[derive(Clone, Debug, Default)]
pub struct TextSelection {
    pub anchor: Option<SelectionPoint>,
    pub focus: Option<SelectionPoint>,
}

impl TextSelection {
    pub fn is_active(&self) -> bool {
        self.anchor.is_some() && self.focus.is_some() && self.anchor != self.focus
    }
    pub fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
    }
}

#[derive(Clone)]
pub(crate) struct TextSource {
    pub node: NodeId,
    pub text: String,
    pub offsets: Vec<Range<usize>>,
    pub collapse_spaces: bool,
    pub collapse_breaks: bool,
}

impl TextSource {
    fn next_char(&self, cursor: &mut (usize, usize)) -> Option<(char, Range<usize>)> {
        let byte = cursor.0;
        let ch = self.text[byte..].chars().next()?;
        let offset = self
            .offsets
            .get(cursor.1)
            .cloned()
            .unwrap_or(byte..byte + ch.len_utf8());
        cursor.0 += ch.len_utf8();
        cursor.1 += 1;
        Some((ch, offset))
    }
}

#[derive(Clone)]
pub(crate) struct TextMapping {
    pub range: Range<usize>,
    pub start: SelectionPoint,
    pub end: SelectionPoint,
}

impl TextMapping {
    pub(crate) fn is_linear(&self) -> bool {
        self.start.node == self.end.node
            && self.end.offset.checked_sub(self.start.offset) == Some(self.range.len())
    }
}

impl TextLayout {
    pub(crate) fn rebuild_selection_map(&mut self, nodes: &NodeTree) {
        if self.selection_map_built {
            return;
        }
        self.selection_map_built = true;
        self.selection_map.clear();
        if self.sources.is_empty() {
            return;
        }
        let mut clusters: Vec<_> = self
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .flat_map(|run| run.clusters())
            .filter_map(|cluster| Some((cluster.text_range(), cluster.style().brush.text_node?)))
            .collect();
        clusters.sort_by_key(|(range, _)| range.start);
        clusters.dedup_by_key(|(range, _)| range.start);
        let source_indices: HashMap<_, _> = self
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| (source.node, index))
            .collect();
        let mut cursors = vec![(0, 0); self.sources.len()];
        for (range, id) in clusters {
            let Some(&first_source) = source_indices.get(&id) else {
                continue;
            };
            let mut source_index = first_source;
            for (index, ch) in self.text[range.clone()].char_indices() {
                // A grapheme cluster can span several DOM text nodes.
                while source_index < self.sources.len() {
                    let source = &self.sources[source_index];
                    let cursor = &mut cursors[source_index];
                    let collapsible = |c: char| {
                        (source.collapse_spaces && matches!(c, ' ' | '\t'))
                            || (source.collapse_breaks && matches!(c, '\n' | '\r'))
                    };
                    let mut matched = None;
                    while let Some((raw, offset)) = source.next_char(cursor) {
                        if raw != ch && !(ch == ' ' && collapsible(raw)) {
                            continue;
                        }
                        let mut end = offset.end;
                        if ch == ' ' && collapsible(raw) {
                            while source.text[cursor.0..]
                                .chars()
                                .next()
                                .is_some_and(collapsible)
                            {
                                end = source.next_char(cursor).unwrap().1.end;
                            }
                        }
                        matched = Some(offset.start..end);
                        break;
                    }
                    let Some(offset) = matched else {
                        source_index += 1;
                        continue;
                    };
                    let node = &nodes[source.node];
                    let (start, end) = if node.text_data().is_some() {
                        (
                            node.dom_text_point(offset.start),
                            node.dom_text_point(offset.end),
                        )
                    } else {
                        (node.dom_edge(false), node.dom_edge(true))
                    };
                    if let (Some(start), Some(end)) = (start, end) {
                        let mapping = TextMapping {
                            range: range.start + index..range.start + index + ch.len_utf8(),
                            start,
                            end,
                        };
                        if let Some(last) = self.selection_map.last_mut()
                            && last.end == mapping.start
                            && last.range.end == mapping.range.start
                            && last.is_linear()
                            && mapping.is_linear()
                        {
                            last.end = mapping.end;
                            last.range.end = mapping.range.end;
                        } else {
                            self.selection_map.push(mapping);
                        }
                    }
                    break;
                }
            }
        }
    }

    pub(crate) fn dom_point_at_offset(&self, offset: usize) -> Option<SelectionPoint> {
        self.selection_map
            .iter()
            .find(|mapping| mapping.range.end > offset)
            .map(|mapping| {
                if mapping.is_linear() {
                    SelectionPoint {
                        node: mapping.start.node,
                        offset: mapping.start.offset + offset.saturating_sub(mapping.range.start),
                    }
                } else {
                    mapping.start
                }
            })
            .or_else(|| self.selection_map.last().map(|mapping| mapping.end))
    }
}

impl Node {
    fn dom_text_point(&self, offset: usize) -> Option<SelectionPoint> {
        let mut child = self;
        while let Some(parent_id) = child.parent {
            let parent = self.with(parent_id);
            if parent.is_anonymous() {
                return parent.dom_edge(false);
            }
            if !parent.children.contains(&child.id) {
                return child.dom_edge(false);
            }
            child = parent;
        }
        Some(SelectionPoint {
            node: self.id,
            offset,
        })
    }

    pub(crate) fn dom_edge(&self, after: bool) -> Option<SelectionPoint> {
        let parent = self.with(self.parent?);
        if parent.before() == Some(self.id) {
            return Some(SelectionPoint {
                node: parent.id,
                offset: 0,
            });
        }
        if parent.after() == Some(self.id) {
            return Some(SelectionPoint {
                node: parent.id,
                offset: parent.children.len(),
            });
        }
        if self.is_anonymous() {
            let child = if after {
                self.children.last()
            } else {
                self.children.first()
            };
            return child
                .and_then(|id| self.with(*id).dom_edge(after))
                .or_else(|| {
                    Some(SelectionPoint {
                        node: self.parent?,
                        offset: 0,
                    })
                });
        }
        if let Some(index) = parent.children.iter().position(|id| *id == self.id) {
            Some(SelectionPoint {
                node: parent.id,
                offset: index + usize::from(after),
            })
        } else {
            Some(SelectionPoint {
                node: parent.id,
                offset: 0,
            })
        }
    }

    pub(crate) fn selection_child_boundary(
        &self,
        point: Point,
        scale: f64,
    ) -> Option<SelectionPoint> {
        for &id in self
            .layout_children
            .borrow()
            .iter()
            .flatten()
            .chain(self.hoisted_children.borrow().iter())
        {
            let child = self.with(id);
            if child.containing_block() != Some(self.id) {
                continue;
            }
            let local = child.selection_transform_to_parent(scale).inverse() * point;
            let size = child.final_layout().size;
            if local.x >= 0.0
                && local.x <= size.width as f64
                && local.y >= 0.0
                && local.y <= size.height as f64
            {
                if let Some(position) = child.selection_child_boundary(local, scale) {
                    return Some(position);
                }
            }
        }
        let mut end = None;
        let layout = self.final_layout();
        let content_top = (layout.padding.top + layout.border.top) as f64;
        let content_left = (layout.padding.left + layout.border.left) as f64;
        let x = (point.x - content_left) as f32;
        let y = (point.y - content_top) as f32;
        if self.text_selection_allowed()
            && let Some(ild) = self
                .element_data()
                .and_then(|el| el.inline_layout_data.as_ref())
            && x >= 0.0
            && x <= layout.content_box_width()
            && y >= 0.0
            && y < ild.layout.height() / ild.layout.scale()
            && let Some(offset) = self.text_offset_at_point(x, y)
            && let Some(position) = ild.dom_point_at_offset(offset)
        {
            return Some(position);
        }
        let at_start = point.y <= content_top || point.x <= content_left;
        for &id in self.layout_children.borrow().iter().flatten() {
            let child = self.with(id);
            if child.taffy_position().is_out_of_flow() {
                continue;
            }
            let local = child.selection_transform_to_parent(scale).inverse() * point;
            let size = child.final_layout().size;
            if local.y < size.height as f64 && (local.x < size.width as f64 || local.y < 0.0) {
                return child.dom_edge(false);
            }
            end = child.dom_edge(true);
        }
        if self.is_anonymous() {
            self.dom_edge(!at_start)
        } else {
            end.or(Some(SelectionPoint {
                node: self.id,
                offset: if at_start { 0 } else { self.children.len() },
            }))
        }
    }

    pub(crate) fn text_selection_allowed(&self) -> bool {
        let mut node = self;
        let mut user_select = UserSelect::Auto;
        let mut visibility = None;
        loop {
            if let Some(el) = node.element_data() {
                if el.has_attr(local_name!("disabled"))
                    || matches!(
                        el.name.local,
                        local_name!("input")
                            | local_name!("textarea")
                            | local_name!("select")
                            | local_name!("button")
                    )
                    || node.subdoc().is_some()
                {
                    return false;
                }
            }
            if let Some(style) = node.primary_styles() {
                visibility.get_or_insert_with(|| style.clone_visibility());
                if user_select == UserSelect::Auto {
                    user_select = style.clone_user_select();
                }
            }
            let Some(parent) = node.parent else {
                return user_select != UserSelect::None
                    && !matches!(visibility, Some(Visibility::Hidden | Visibility::Collapse));
            };
            node = self.with(parent);
        }
    }

    pub(crate) fn selection_transform_to_parent(&self, scale: f64) -> Affine {
        let layout = self.final_layout();
        let mut x = layout.location.x as f64 - self.scroll_offset().x;
        let mut y = layout.location.y as f64 - self.scroll_offset().y;
        if let Some(parent_id) = self.containing_block() {
            let parent = self.with(parent_id);
            if parent.flags.is_inline_root() && !self.taffy_position().is_out_of_flow() {
                let layout = parent.final_layout();
                x += (layout.padding.left + layout.border.left) as f64;
                y += (layout.padding.top + layout.border.top) as f64;
            }
            if self.taffy_position() == taffy::Position::Fixed
                && parent.containing_block().is_none()
            {
                x += parent.scroll_offset().x + self.tree().viewport_scroll().x;
                y += parent.scroll_offset().y + self.tree().viewport_scroll().y;
            }
        }
        Affine::translate((x, y))
            * Affine::scale(1.0 / scale)
            * self
                .transform()
                .as_deref()
                .copied()
                .unwrap_or(Affine::IDENTITY)
            * Affine::scale(scale)
    }
}

impl BaseDocument {
    pub(crate) fn valid_selection_point(&self, point: SelectionPoint) -> bool {
        self.get_node(point.node).is_some_and(|node| {
            let mut current = node;
            while let Some(parent_id) = current.parent {
                let Some(parent) = self.get_node(parent_id) else {
                    return false;
                };
                if !parent.children.contains(&current.id) {
                    return false;
                }
                current = parent;
            }
            !node.is_anonymous()
                && current.id == self.root_node().id
                && node.flags.is_in_document()
                && if let Some(text) = node.text_data() {
                    text.content.is_char_boundary(point.offset)
                } else {
                    point.offset <= node.children.len()
                }
        })
    }

    pub(crate) fn compare_selection_points(
        &self,
        a: SelectionPoint,
        b: SelectionPoint,
    ) -> Ordering {
        if a.node == b.node {
            return a.offset.cmp(&b.offset);
        }
        let child_index = |ancestor, mut child| {
            while let Some(node) = self.get_node(child) {
                let parent = node.parent?;
                if parent == ancestor {
                    return self.nodes[parent]
                        .children
                        .iter()
                        .position(|id| *id == child);
                }
                child = parent;
            }
            None
        };
        if let Some(index) = child_index(a.node, b.node) {
            return if a.offset <= index {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        if let Some(index) = child_index(b.node, a.node) {
            return if b.offset <= index {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
        self.compare_document_order(a.node, b.node)
    }
}
