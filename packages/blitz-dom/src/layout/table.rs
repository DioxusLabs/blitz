use blitz_traits::node_id::NodeId;
use std::{ops::Range, sync::Arc};

use atomic_refcell::AtomicRefCell;
use markup5ever::local_name;
use style::properties::style_structs::Border;
use style::servo_arc::Arc as ServoArc;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::{
    Atom, computed_values::border_collapse::T as BorderCollapse,
    computed_values::table_layout::T as TableLayout,
};
use taffy::{
    DetailedGridInfo, LayoutPartialTree as _, ResolveOrZero, TrackSizingFunction, style_helpers,
};

use crate::BaseDocument;

use super::construct::{AnonKind, create_anonymous_node};
use super::damage::{CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC};
use super::resolve_calc_value;

pub struct TableTreeWrapper<'doc> {
    pub(crate) doc: &'doc mut BaseDocument,
    pub(crate) ctx: Arc<TableContext>,
}

#[derive(Debug, Clone)]
pub struct TableContext {
    pub style: taffy::Style<Atom>,
    pub cells: Vec<TableCell>,
    pub rows: Vec<TableRow>,
    pub computed_grid_info: AtomicRefCell<Option<DetailedGridInfo<Atom>>>,
    pub border_style: Option<ServoArc<Border>>,
    pub border_collapse: BorderCollapse,
}

// #[derive(Debug, Clone, Eq, PartialEq)]
// pub enum TableItemKind {
//     Row,
//     Cell,
// }

#[derive(Debug, Clone)]
pub struct TableCell {
    // kind: TableItemKind,
    node_id: NodeId,
    style: taffy::Style<Atom>,
}

#[derive(Debug, Clone)]
pub struct TableRow {
    // kind: TableItemKind,
    pub node_id: NodeId,
    pub height: f32,
}

pub(crate) fn build_table_context(
    doc: &mut BaseDocument,
    table_root_node_id: NodeId,
) -> (TableContext, Vec<NodeId>, Vec<NodeId>) {
    let root_node = &mut doc.nodes[table_root_node_id];

    let children = std::mem::take(&mut root_node.children);

    let Some(stylo_styles) = root_node.primary_styles() else {
        panic!("Ignoring table because it has no styles");
    };

    let mut style = stylo_taffy::to_taffy_style(&stylo_styles);
    style.item_is_table = true;
    // Use `dense` row-flow so that each cell scans the row from its
    // leftmost column for the first free track. Without `dense`,
    // `place_definite_secondary_axis_item` keeps a per-item secondary
    // cursor across rows, which means cells in later rows do not
    // backfill columns freed up by rowspan cells from earlier rows.
    style.grid_auto_flow = taffy::GridAutoFlow::RowDense;
    style.grid_auto_columns = Vec::new();
    style.grid_auto_rows = Vec::new();

    let is_fixed = match stylo_styles.clone_table_layout() {
        TableLayout::Fixed => true,
        TableLayout::Auto => false,
    };

    let border_collapse = stylo_styles.clone_border_collapse();
    let border_spacing = stylo_styles.clone_border_spacing().0;

    drop(stylo_styles);

    let mut builder = TableBuilder {
        table_root_node_id,
        is_fixed,
        border_collapse,
        row: 0,
        col: 0,
        cells: Vec::new(),
        rows: Vec::new(),
        columns: Vec::new(),
        first_cell_border: None,
        anonymous_nodes: Vec::new(),
        open_anon_row: None,
        open_anon_cell: None,
    };
    for child_id in children.iter().copied() {
        builder.visit(doc, child_id, false);
    }

    let TableBuilder {
        row,
        col,
        cells,
        rows,
        mut columns,
        first_cell_border,
        anonymous_nodes,
        ..
    } = builder;

    columns.resize(col as usize, style_helpers::auto());

    style.grid_template_columns = columns.into_iter().map(|dim| dim.into()).collect();
    style.grid_template_rows = vec![style_helpers::auto(); row as usize];

    style.gap = match border_collapse {
        BorderCollapse::Separate => {
            // In the separated borders model, `border-spacing` also applies between
            // the table border and the outermost cells, in addition to between cells.
            let spacing_x = border_spacing.width.px();
            let spacing_y = border_spacing.height.px();
            let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
            style.padding = taffy::Rect {
                left: style_helpers::length(padding.left + spacing_x),
                right: style_helpers::length(padding.right + spacing_x),
                top: style_helpers::length(padding.top + spacing_y),
                bottom: style_helpers::length(padding.bottom + spacing_y),
            };
            taffy::Size {
                width: style_helpers::length(spacing_x),
                height: style_helpers::length(spacing_y),
            }
        }
        BorderCollapse::Collapse => first_cell_border
            .as_ref()
            .map(|border| {
                let x = border
                    .border_left_width
                    .0
                    .max(border.border_right_width.0)
                    .to_f32_px();
                let y = border
                    .border_top_width
                    .0
                    .max(border.border_bottom_width.0)
                    .to_f32_px();
                taffy::Size {
                    width: style_helpers::length(x),
                    height: style_helpers::length(y),
                }
            })
            .unwrap_or(taffy::Size::ZERO.map(style_helpers::length)),
    };

    if border_collapse == BorderCollapse::Collapse {
        style.border = taffy::Rect {
            left: style.gap.width,
            right: style.gap.width,
            top: style.gap.height,
            bottom: style.gap.height,
        };
    }

    let layout_children = cells.iter().map(|cell| cell.node_id).collect();
    let root_node = &mut doc.nodes[table_root_node_id];
    root_node.children = children;

    (
        TableContext {
            style,
            cells,
            rows,
            computed_grid_info: AtomicRefCell::new(None),
            border_collapse,
            border_style: first_cell_border,
        },
        layout_children,
        anonymous_nodes,
    )
}

/// Walks a table's descendants, mapping rows/cells into the table grid and
/// generating anonymous rows/cells around misplaced children per the box
/// fixup rules of CSS 2.2 §17.2.1: consecutive runs of non-table-internal
/// children share a single anonymous cell, and cells (real or anonymous)
/// occurring outside a row share a single anonymous row.
struct TableBuilder {
    table_root_node_id: NodeId,
    is_fixed: bool,
    border_collapse: BorderCollapse,
    row: u16,
    col: u16,
    cells: Vec<TableCell>,
    rows: Vec<TableRow>,
    columns: Vec<TrackSizingFunction>,
    first_cell_border: Option<ServoArc<Border>>,
    /// Anonymous row/cell nodes created during this build. Recorded on the
    /// table root (via `LayoutChildren::anonymous_blocks`) so they are
    /// deallocated the next time it is reconstructed.
    anonymous_nodes: Vec<NodeId>,
    /// The anonymous row currently accepting cells that occur outside a real
    /// row. Closed by any real row or row group.
    open_anon_row: Option<NodeId>,
    /// The anonymous cell currently accepting misplaced children. Closed by
    /// any table-internal sibling.
    open_anon_cell: Option<NodeId>,
}

impl TableBuilder {
    /// `in_row` is true when `node_id` is a child of a real table-row.
    fn visit(&mut self, doc: &mut BaseDocument, node_id: NodeId, in_row: bool) {
        let node = &mut doc.nodes[node_id];

        if !node.is_element() {
            // Non-whitespace text gets an anonymous cell. Whitespace-only
            // text only joins an already-open anonymous cell.
            if node.is_text_node() && (!node.is_whitespace_node() || self.open_anon_cell.is_some())
            {
                self.push_into_anon_cell(doc, node_id, in_row);
            }
            return;
        }

        let Some(display) = node.primary_styles().map(|s| s.clone_display()) else {
            #[cfg(feature = "tracing")]
            tracing::info!("Ignoring table descendent because it has no styles");
            return;
        };

        if display.outside() == DisplayOutside::None {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            return;
        }

        match display.inside() {
            DisplayInside::TableRowGroup
            | DisplayInside::TableHeaderGroup
            | DisplayInside::TableFooterGroup => {
                self.open_anon_cell = None;
                self.open_anon_row = None;
                self.visit_children(doc, node_id, false);
            }
            // display:contents is transparent for box generation: its
            // children participate as if they were siblings of the contents
            // node, so open anonymous runs are neither closed nor reopened.
            DisplayInside::Contents => {
                doc.nodes[node_id]
                    .remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                self.visit_children(doc, node_id, in_row);
            }
            DisplayInside::TableRow => {
                self.open_anon_cell = None;
                self.open_anon_row = None;

                doc.nodes[node_id]
                    .remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                self.row += 1;
                self.col = 0;

                self.rows.push(TableRow {
                    node_id,
                    height: 0.0,
                });

                self.visit_children(doc, node_id, true);
                self.open_anon_cell = None;
            }
            DisplayInside::TableCell => {
                self.open_anon_cell = None;
                if !in_row {
                    self.ensure_anon_row(doc);
                }
                self.push_cell(doc, node_id, true);
            }
            // Non-table-internal children generate an anonymous table cell
            // around them, with consecutive runs sharing a single cell.
            DisplayInside::Flow
            | DisplayInside::FlowRoot
            | DisplayInside::Flex
            | DisplayInside::Grid
            | DisplayInside::Table => {
                self.push_into_anon_cell(doc, node_id, in_row);
            }
            DisplayInside::TableColumnGroup | DisplayInside::TableColumn => {
                doc.nodes[node_id]
                    .remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                //Ignore
            }
            DisplayInside::None => {
                doc.nodes[node_id]
                    .remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                // Ignore
            }
        }
    }

    fn visit_children(&mut self, doc: &mut BaseDocument, node_id: NodeId, in_row: bool) {
        let children = std::mem::take(&mut doc.nodes[node_id].children);
        for child_id in children.iter().copied() {
            self.visit(doc, child_id, in_row);
        }
        doc.nodes[node_id].children = children;
    }

    /// Open an anonymous row to hold cells occurring outside a real row.
    fn ensure_anon_row(&mut self, doc: &mut BaseDocument) {
        if self.open_anon_row.is_none() {
            let anon_id = create_anonymous_node(doc, self.table_root_node_id, AnonKind::TableRow);
            // Anonymous rows are not layout children, so construction damage
            // would never be cleared by the resolve pass.
            doc.nodes[anon_id].remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            self.anonymous_nodes.push(anon_id);

            self.row += 1;
            self.col = 0;
            self.rows.push(TableRow {
                node_id: anon_id,
                height: 0.0,
            });
            self.open_anon_row = Some(anon_id);
        }
    }

    /// Append a misplaced (non-table-internal) child to the currently-open
    /// anonymous cell, opening one (and an anonymous row if needed) first.
    fn push_into_anon_cell(&mut self, doc: &mut BaseDocument, node_id: NodeId, in_row: bool) {
        if self.open_anon_cell.is_none() {
            if !in_row {
                self.ensure_anon_row(doc);
            }
            let container_id = doc.nodes[node_id].parent.unwrap_or(self.table_root_node_id);
            let anon_id = create_anonymous_node(doc, container_id, AnonKind::TableCell);
            self.anonymous_nodes.push(anon_id);
            self.push_cell(doc, anon_id, false);
            self.open_anon_cell = Some(anon_id);
        }
        doc.nodes[self.open_anon_cell.unwrap()]
            .children
            .push(node_id);
    }

    /// Map a cell (real or anonymous) into the table grid. `is_real_cell`
    /// controls whether the cell's border participates in the table's
    /// collapsed-border approximation.
    fn push_cell(&mut self, doc: &mut BaseDocument, node_id: NodeId, is_real_cell: bool) {
        let node = &mut doc.nodes[node_id];
        let stylo_style = &node.primary_styles().unwrap();
        let colspan: u16 = node
            .attr(local_name!("colspan"))
            .and_then(|val| val.parse().ok())
            .unwrap_or(1);
        let rowspan: u16 = node
            .attr(local_name!("rowspan"))
            .and_then(|val| val.parse::<u16>().ok())
            .map(|v| v.clamp(1, 65534))
            .unwrap_or(1);
        let mut style = stylo_taffy::to_taffy_style(stylo_style);

        if is_real_cell && self.first_cell_border.is_none() {
            self.first_cell_border = Some(stylo_style.clone_border());
        }

        // Cells occurring before any row are placed in an anonymous row
        if self.row == 0 {
            self.row = 1;
        }

        if self.row == 1 {
            let column = match style.size.width.tag() {
                taffy::CompactLength::LENGTH_TAG => {
                    let len = style.size.width.value();
                    let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                    let border = style.border.resolve_or_zero(None, resolve_calc_value);
                    match style.box_sizing {
                        taffy::BoxSizing::ContentBox => style_helpers::length(
                            len + padding.left + padding.right + border.left + border.right,
                        ),
                        taffy::BoxSizing::BorderBox => style_helpers::length(len),
                    }
                }
                taffy::CompactLength::PERCENT_TAG => {
                    if self.is_fixed {
                        style_helpers::percent(style.size.width.value())
                    } else {
                        style_helpers::auto()
                    }
                }
                taffy::CompactLength::AUTO_TAG => style_helpers::auto(),
                // Dimension values are always length, percentage, auto or calc(),
                // so any other tag is a calc() value. Pass it through so that
                // Taffy resolves it against the table's inner width.
                _ => style.size.width.into(),
            };
            self.columns.push(column);
        }

        // Zero-out cell borders is BorderCollapse is Collapse
        // Borders are handled at the table level in this mode
        if self.border_collapse == BorderCollapse::Collapse {
            style.border = taffy::Rect::ZERO.map(style_helpers::length);
        }

        // The margin properties do not apply to table-internal elements
        style.margin = taffy::Rect::ZERO.map(style_helpers::length);

        // Let Taffy auto-place the column. Combined with
        // `grid_auto_flow: RowDense` set on the table root, each cell
        // scans from the first track in its row for a free position,
        // which makes cells automatically skip columns occupied by
        // rowspan cells from earlier rows.
        style.grid_column = taffy::Line {
            start: style_helpers::auto(),
            end: style_helpers::span(colspan),
        };
        style.grid_row = taffy::Line {
            start: style_helpers::line(self.row as i16),
            end: style_helpers::span(rowspan),
        };
        style.size.width = style_helpers::auto();
        self.cells.push(TableCell { node_id, style });

        self.col += colspan;
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
        RangeIter(0..self.ctx.cells.len())
    }

    #[inline(always)]
    fn child_count(&self, _node_id: taffy::NodeId) -> usize {
        self.ctx.cells.len()
    }

    #[inline(always)]
    fn get_child_id(&self, _node_id: taffy::NodeId, index: usize) -> taffy::NodeId {
        index.into()
    }
}
impl taffy::TraverseTree for TableTreeWrapper<'_> {}

impl taffy::LayoutPartialTree for TableTreeWrapper<'_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type CustomIdent = Atom;

    fn get_core_container_style(&self, _node_id: taffy::NodeId) -> &taffy::Style<Atom> {
        &self.ctx.style
    }

    fn resolve_calc_value(&self, calc_ptr: *const (), parent_size: f32) -> f32 {
        resolve_calc_value(calc_ptr, parent_size)
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &taffy::Layout) {
        let node_id = crate::taffy_node_id(self.ctx.cells[usize::from(node_id)].node_id);
        self.doc.set_unrounded_layout(node_id, layout)
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: taffy::tree::LayoutInput,
    ) -> taffy::LayoutOutput {
        let cell = &self.ctx.cells[usize::from(node_id)];
        let node_id = crate::taffy_node_id(cell.node_id);
        self.doc.compute_child_layout(node_id, inputs)
    }
}

impl taffy::LayoutGridContainer for TableTreeWrapper<'_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: taffy::NodeId) -> Self::GridItemStyle<'_> {
        &self.ctx.cells[usize::from(child_node_id)].style
    }

    fn set_detailed_grid_info(
        &mut self,
        _node_id: taffy::NodeId,
        detailed_grid_info: DetailedGridInfo<Atom>,
    ) {
        *self.ctx.computed_grid_info.borrow_mut() = Some(detailed_grid_info);
    }
}
