use std::{ops::Range, sync::Arc};

use html5ever::local_name;
use style::values::specified::box_::DisplayInside;
use taffy::{compute_leaf_layout, style_helpers, LayoutPartialTree as _};

use crate::{stylo_to_taffy, Document};

pub struct TableTreeWrapper<'doc> {
    pub(crate) doc: &'doc mut Document,
    pub(crate) ctx: Arc<TableContext>,
}

#[derive(Debug, Clone)]
pub struct TableContext {
    style: taffy::Style,
    items: Vec<TableItem>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TableItemKind {
    Row,
    Cell,
}

#[derive(Debug, Clone)]
pub struct TableItem {
    kind: TableItemKind,
    node_id: usize,
    style: taffy::Style,
}

pub(crate) fn build_table_context(
    doc: &mut Document,
    table_root_node_id: usize,
) -> (TableContext, Vec<usize>) {
    let mut items: Vec<TableItem> = Vec::new();
    let mut row = 0u16;
    let mut col = 0u16;

    let root_node = &mut doc.nodes[table_root_node_id];

    let children = std::mem::take(&mut root_node.children);

    let Some(stylo_styles) = root_node.primary_styles() else {
        panic!("Ignoring table because it has no styles");
    };

    let mut style = stylo_to_taffy::entire_style(&stylo_styles);
    style.grid_auto_columns = Vec::new();
    style.grid_auto_rows = Vec::new();

    drop(stylo_styles);

    for child_id in children.iter().copied() {
        collect_table_cells(doc, child_id, &mut row, &mut col, &mut items);
    }

    style.grid_template_columns = vec![style_helpers::auto(); col as usize];
    style.grid_template_rows = vec![style_helpers::auto(); row as usize];

    let layout_children = items
        .iter()
        .filter(|item| item.kind == TableItemKind::Cell)
        .map(|cell| cell.node_id)
        .collect();
    let root_node = &mut doc.nodes[table_root_node_id];
    root_node.children = children;

    (TableContext { style, items }, layout_children)
}

pub(crate) fn collect_table_cells(
    doc: &mut Document,
    node_id: usize,
    row: &mut u16,
    col: &mut u16,
    cells: &mut Vec<TableItem>,
) {
    let node = &doc.nodes[node_id];

    if !node.is_element() {
        return;
    }

    let Some(display) = node.primary_styles().map(|s| s.clone_display()) else {
        println!("Ignoring table descendent because it has no styles");
        return;
    };

    match display.inside() {
        DisplayInside::TableRowGroup
        | DisplayInside::TableHeaderGroup
        | DisplayInside::TableFooterGroup
        | DisplayInside::Contents => {
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_table_cells(doc, child_id, row, col, cells);
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableRow => {
            *row += 1;
            *col = 0;

            {
                let stylo_style = &node.primary_styles().unwrap();
                let mut style = stylo_to_taffy::entire_style(stylo_style);
                style.grid_column = taffy::Line {
                    start: style_helpers::line(0),
                    end: style_helpers::line(-1),
                };
                style.grid_row = taffy::Line {
                    start: style_helpers::line(*row as i16),
                    end: style_helpers::span(1),
                };
                cells.push(TableItem {
                    kind: TableItemKind::Row,
                    node_id,
                    style,
                });
            }

            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_table_cells(doc, child_id, row, col, cells);
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableCell => {
            let stylo_style = &node.primary_styles().unwrap();
            let colspan: u16 = node
                .attr(local_name!("colspan"))
                .and_then(|val| val.parse().ok())
                .unwrap_or(1);
            let mut style = stylo_to_taffy::entire_style(stylo_style);
            style.grid_column = taffy::Line {
                start: style_helpers::line((*col + 1) as i16),
                end: style_helpers::span(colspan),
            };
            style.grid_row = taffy::Line {
                start: style_helpers::line(*row as i16),
                end: style_helpers::span(1),
            };
            cells.push(TableItem {
                kind: TableItemKind::Cell,
                node_id,
                style,
            });
            *col += colspan;
        }
        _ => {
            println!("Warning: ignoring non-table typed descendent of table");
        }
    }
}

pub struct RangeIter(Range<usize>);

impl Iterator for RangeIter {
    type Item = taffy::NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(taffy::NodeId::from)
    }
}

impl taffy::TraversePartialTree for TableTreeWrapper<'_> {
    type ChildIter<'a>
        = RangeIter
    where
        Self: 'a;

    #[inline(always)]
    fn child_ids(&self, _node_id: taffy::NodeId) -> Self::ChildIter<'_> {
        RangeIter(0..self.ctx.items.len())
    }

    #[inline(always)]
    fn child_count(&self, node_id: taffy::NodeId) -> usize {
        self.doc.child_count(node_id)
    }

    #[inline(always)]
    fn get_child_id(&self, _node_id: taffy::NodeId, index: usize) -> taffy::NodeId {
        index.into()
    }
}
impl taffy::TraverseTree for TableTreeWrapper<'_> {}

impl taffy::LayoutPartialTree for TableTreeWrapper<'_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;
    type CacheMut<'b>
        = &'b mut taffy::Cache
    where
        Self: 'b;

    fn get_core_container_style(&self, _node_id: taffy::NodeId) -> &taffy::Style {
        &self.ctx.style
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &taffy::Layout) {
        let node_id = taffy::NodeId::from(self.ctx.items[usize::from(node_id)].node_id);
        self.doc.set_unrounded_layout(node_id, layout)
    }

    fn get_cache_mut(&mut self, node_id: taffy::NodeId) -> &mut taffy::Cache {
        let node_id = taffy::NodeId::from(self.ctx.items[usize::from(node_id)].node_id);
        &mut self.doc.node_from_id_mut(node_id).cache
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: taffy::tree::LayoutInput,
    ) -> taffy::LayoutOutput {
        let cell = &self.ctx.items[usize::from(node_id)];
        match cell.kind {
            TableItemKind::Row => {
                compute_leaf_layout(inputs, &cell.style, |_, _| taffy::Size::ZERO)
            }
            TableItemKind::Cell => {
                let node_id = taffy::NodeId::from(cell.node_id);
                self.doc.compute_child_layout(node_id, inputs)
            }
        }
    }
}

impl taffy::LayoutGridContainer for TableTreeWrapper<'_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: taffy::NodeId) -> Self::GridItemStyle<'_> {
        &self.ctx.items[usize::from(child_node_id)].style
    }
}
