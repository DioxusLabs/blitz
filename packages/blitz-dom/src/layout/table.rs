//! Table support around Taffy's table layout algorithm: the table-level data recorded when a
//! table's box tree is constructed (`TableContext`), and the geometry of the table's rows,
//! columns and cells derived from the laid out tree, which painting uses for row/column
//! backgrounds and collapsed borders.

use blitz_traits::node_id::NodeId;
use markup5ever::local_name;
use style::computed_values::border_collapse::T as BorderCollapse;
use style::properties::style_structs::Border;
use style::servo_arc::Arc as ServoArc;
use style::values::computed::{BorderStyle, Display};
use style::values::specified::box_::DisplayInside;
use taffy::TableRole;

use crate::BaseDocument;

/// Table-level data recorded on a table root when its box tree is constructed
#[derive(Debug)]
pub struct TableContext {
    /// The table's columns (`<col>` / `display: table-column` boxes), in grid order
    pub columns: Vec<TableColumn>,
    pub border_collapse: BorderCollapse,
    /// The border of the table's first cell, which all collapsed borders are drawn with
    pub border_style: Option<ServoArc<Border>>,
    /// In the collapsed borders model, the gutter the borders are laid out in between the
    /// cells (and between the table edge and the outer cells), in place of `border-spacing`
    pub collapsed_gutter: taffy::Size<f32>,
}

#[derive(Debug, Clone)]
pub struct TableColumn {
    pub node_id: NodeId,
    /// The column group (if any) that contains the column
    pub group: Option<NodeId>,
}

/// The geometry of a laid out table, relative to its border box
#[derive(Debug, Default)]
pub struct TableLayoutInfo {
    pub rows: Vec<TableRow>,
    pub cells: Vec<TableCell>,
    /// The horizontal extent of each column
    pub column_tracks: Vec<taffy::Line<f32>>,
    /// The vertical extent of each row
    pub row_tracks: Vec<taffy::Line<f32>>,
}

#[derive(Debug, Clone)]
pub struct TableRow {
    pub node_id: NodeId,
    /// The row group (if any) that contains the row
    pub group: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub struct TableCell {
    pub node_id: NodeId,
    /// Index (into `TableLayoutInfo::rows`) of the row the cell originates in
    pub row: u16,
    /// Index of the column the cell originates in
    pub column: u16,
    pub colspan: u16,
    /// The cell's border box, relative to the table's border box
    pub location: taffy::Point<f32>,
    pub size: taffy::Size<f32>,
}

/// Tracks the current column position while walking the table's cells, so that cells are
/// assigned the same columns Taffy's placement gives them (skipping columns still occupied
/// by rowspan cells from earlier rows).
#[derive(Debug, Default)]
struct ColumnCursor {
    /// The column the next cell in the current row will be placed in
    col: u16,
    /// For each column, the number of further rows it is occupied by a rowspan cell
    rowspans: Vec<u16>,
}

impl ColumnCursor {
    fn start_group(&mut self) {
        self.rowspans.clear();
    }

    fn start_row(&mut self) {
        self.col = 0;
        for remaining in self.rowspans.iter_mut() {
            *remaining = remaining.saturating_sub(1);
        }
    }

    /// Advance past occupied columns, returning the column of the next cell
    fn next_free(&mut self) -> u16 {
        while self.rowspans.get(self.col as usize).is_some_and(|r| *r > 0) {
            self.col += 1;
        }
        self.col
    }

    fn place(&mut self, colspan: u16, rowspan: u16) {
        let end = self.col + colspan;
        if self.rowspans.len() < end as usize {
            self.rowspans.resize(end as usize, 0);
        }
        for remaining in &mut self.rowspans[self.col as usize..end as usize] {
            *remaining = (*remaining).max(rowspan - 1);
        }
        self.col = end;
    }
}

/// The used width of one border side: border widths are not adjusted for
/// border-style in computed styles, so a border with `none`/`hidden` style
/// must be treated as zero-width.
fn side_width(width: app_units::Au, style: BorderStyle) -> f32 {
    if style.none_or_hidden() {
        0.0
    } else {
        width.to_f32_px()
    }
}

/// The role a node plays in table layout, from its `display`
pub(crate) fn table_role(doc: &BaseDocument, node_id: NodeId) -> TableRole {
    doc.nodes[node_id]
        .display_style()
        .map(stylo_taffy::convert::table_role)
        .unwrap_or(TableRole::Other)
}

/// The `colspan` and `rowspan` of a cell
pub(crate) fn cell_spans(doc: &BaseDocument, cell_id: NodeId) -> (u16, u16) {
    cell_spans_of(&doc.nodes[cell_id])
}

pub(crate) fn cell_spans_of(node: &crate::Node) -> (u16, u16) {
    let colspan: u16 = node
        .attr(local_name!("colspan"))
        .and_then(|val| val.parse::<u16>().ok())
        .map(|v| v.clamp(1, 1000))
        .unwrap_or(1);
    let rowspan: u16 = node
        .attr(local_name!("rowspan"))
        .and_then(|val| val.parse::<u16>().ok())
        .map(|v| v.clamp(1, 65534))
        .unwrap_or(1);
    (colspan, rowspan)
}

/// The number of columns a `<col>` or `<colgroup>` spans (HTML `span`)
pub(crate) fn column_span_of(node: &crate::Node) -> u16 {
    node.attr(local_name!("span"))
        .and_then(|val| val.parse::<u16>().ok())
        .map(|v| v.clamp(1, 1000))
        .unwrap_or(1)
}

/// Record the table-level data for the table rooted at `table_root_node_id`
pub(crate) fn build_table_context(doc: &BaseDocument, table_root_node_id: NodeId) -> TableContext {
    let root = &doc.nodes[table_root_node_id];
    let border_collapse = root
        .primary_styles()
        .map(|s| s.get_inherited_table().border_collapse)
        .unwrap_or(BorderCollapse::Separate);

    let mut columns = Vec::new();
    for child_id in root.children.iter().copied() {
        collect_columns(doc, child_id, &mut columns);
    }

    let border_style = first_cell(doc, table_root_node_id).and_then(|cell_id| {
        doc.nodes[cell_id]
            .primary_styles()
            .map(|s| s.clone_border())
    });

    let collapsed_gutter = match (&border_style, border_collapse) {
        (Some(border), BorderCollapse::Collapse) => taffy::Size {
            width: side_width(border.border_left_width.0, border.border_left_style).max(
                side_width(border.border_right_width.0, border.border_right_style),
            ),
            height: side_width(border.border_top_width.0, border.border_top_style).max(side_width(
                border.border_bottom_width.0,
                border.border_bottom_style,
            )),
        },
        _ => taffy::Size::ZERO,
    };

    // Cells without a visible border have nothing to collapse: the table keeps its own border
    let border_style = border_style.filter(|_| collapsed_gutter != taffy::Size::ZERO);

    TableContext {
        columns,
        border_collapse,
        border_style,
        collapsed_gutter,
    }
}

/// The first cell element of the table in layout order, if any: header row groups are laid out
/// before the other row groups and footer row groups after them, whatever their document order
fn first_cell(doc: &BaseDocument, table_root_node_id: NodeId) -> Option<NodeId> {
    let group_order = |display: Display| match display.inside() {
        DisplayInside::TableHeaderGroup => 0,
        DisplayInside::TableFooterGroup => 2,
        _ => 1,
    };
    (0..3).find_map(|pass| {
        doc.nodes[table_root_node_id]
            .children
            .iter()
            .copied()
            .filter(|&child_id| {
                doc.nodes[child_id]
                    .display_style()
                    .is_some_and(|display| group_order(display) == pass)
            })
            .find_map(|child_id| first_cell_in(doc, child_id))
    })
}

/// The first cell element in document order within `node_id` (a cell, row, row group or
/// `display: contents` element), if any
fn first_cell_in(doc: &BaseDocument, node_id: NodeId) -> Option<NodeId> {
    let node = &doc.nodes[node_id];
    if !node.is_element() {
        return None;
    }
    let display = node.display_style()?;
    match display.inside() {
        DisplayInside::TableCell => Some(node_id),
        DisplayInside::TableRow
        | DisplayInside::TableRowGroup
        | DisplayInside::TableHeaderGroup
        | DisplayInside::TableFooterGroup
        | DisplayInside::Contents => node
            .children
            .iter()
            .copied()
            .find_map(|child_id| first_cell_in(doc, child_id)),
        _ => None,
    }
}

/// Collect `<col>` elements (and `display: table-column` elements), recursing into column
/// groups
fn collect_columns(doc: &BaseDocument, node_id: NodeId, columns: &mut Vec<TableColumn>) {
    let node = &doc.nodes[node_id];
    if !node.is_element() {
        return;
    }
    let Some(display) = node.display_style() else {
        return;
    };

    match display.inside() {
        DisplayInside::TableColumnGroup => {
            let first_column = columns.len();
            for child_id in node.children.iter().copied() {
                collect_columns(doc, child_id, columns);
            }
            for column in &mut columns[first_column..] {
                column.group.get_or_insert(node_id);
            }
        }
        DisplayInside::TableColumn => {
            let span: u16 = node
                .attr(local_name!("span"))
                .and_then(|val| val.parse::<u16>().ok())
                .map(|v| v.max(1))
                .unwrap_or(1);
            for _ in 0..span {
                columns.push(TableColumn {
                    node_id,
                    group: None,
                });
            }
        }
        _ => {}
    }
}

/// Derive the geometry of the laid out table rooted at `table_root_node_id` from the final
/// layouts of its row groups, rows and cells
pub fn table_layout_info(doc: &BaseDocument, table_root_node_id: NodeId) -> TableLayoutInfo {
    let mut info = TableLayoutInfo::default();
    let mut cursor = ColumnCursor::default();

    let layout_children = |node_id: NodeId| -> Vec<NodeId> {
        doc.nodes[node_id]
            .layout_children
            .borrow()
            .as_deref()
            .map(|children| children.to_vec())
            .unwrap_or_default()
    };
    let is_in_flow = |node_id: NodeId| {
        !doc.nodes[node_id].is_out_of_flow()
            && doc.nodes[node_id]
                .display_style()
                .is_none_or(|d| d.inside() != DisplayInside::None)
    };

    let push_row = |info: &mut TableLayoutInfo,
                    cursor: &mut ColumnCursor,
                    row_id: NodeId,
                    group: Option<NodeId>,
                    origin: taffy::Point<f32>| {
        let row_layout = doc.nodes[row_id].final_layout();
        let row_origin = taffy::Point {
            x: origin.x + row_layout.location.x,
            y: origin.y + row_layout.location.y,
        };
        let row_index = info.rows.len() as u16;
        info.rows.push(TableRow {
            node_id: row_id,
            group,
        });
        info.row_tracks.push(taffy::Line {
            start: row_origin.y,
            end: row_origin.y + row_layout.size.height,
        });
        cursor.start_row();
        for cell_id in layout_children(row_id) {
            if !is_in_flow(cell_id) {
                continue;
            }
            let (colspan, rowspan) = cell_spans(doc, cell_id);
            let column = cursor.next_free();
            cursor.place(colspan, rowspan);
            let layout = doc.nodes[cell_id].final_layout();
            info.cells.push(TableCell {
                node_id: cell_id,
                row: row_index,
                column,
                colspan,
                location: taffy::Point {
                    x: row_origin.x + layout.location.x,
                    y: row_origin.y + layout.location.y,
                },
                size: layout.size,
            });
        }
    };

    for child_id in layout_children(table_root_node_id) {
        if !is_in_flow(child_id) {
            continue;
        }
        match table_role(doc, child_id) {
            TableRole::RowGroup => {
                cursor.start_group();
                let group_layout = doc.nodes[child_id].final_layout();
                for row_id in layout_children(child_id) {
                    if !is_in_flow(row_id) {
                        continue;
                    }
                    push_row(
                        &mut info,
                        &mut cursor,
                        row_id,
                        Some(child_id),
                        group_layout.location,
                    );
                }
            }
            // Columns and column groups hold no cells
            TableRole::Column | TableRole::ColumnGroup => {}
            _ => push_row(&mut info, &mut cursor, child_id, None, taffy::Point::ZERO),
        }
    }

    // Column extents, from the cells spanning a single column. Columns only ever covered
    // by spanning cells share the span's width equally.
    let column_count = info
        .cells
        .iter()
        .map(|cell| (cell.column + cell.colspan) as usize)
        .max()
        .unwrap_or(0);
    let mut tracks: Vec<Option<taffy::Line<f32>>> = vec![None; column_count];
    for cell in info.cells.iter().filter(|cell| cell.colspan == 1) {
        let track = tracks[cell.column as usize].get_or_insert(taffy::Line {
            start: cell.location.x,
            end: cell.location.x + cell.size.width,
        });
        track.start = track.start.min(cell.location.x);
        track.end = track.end.max(cell.location.x + cell.size.width);
    }
    for cell in info.cells.iter().filter(|cell| cell.colspan > 1) {
        let range = cell.column as usize..(cell.column + cell.colspan) as usize;
        let unresolved = tracks[range.clone()].iter().filter(|t| t.is_none()).count();
        if unresolved == 0 {
            continue;
        }
        let resolved_width: f32 = tracks[range.clone()]
            .iter()
            .flatten()
            .map(|t| t.end - t.start)
            .sum();
        let each = ((cell.size.width - resolved_width) / unresolved as f32).max(0.0);
        let mut x = cell.location.x;
        for track in &mut tracks[range] {
            match track {
                Some(track) => x = track.end,
                None => {
                    *track = Some(taffy::Line {
                        start: x,
                        end: x + each,
                    });
                    x += each;
                }
            }
        }
    }
    info.column_tracks = tracks
        .into_iter()
        .map(|t| {
            t.unwrap_or(taffy::Line {
                start: 0.0,
                end: 0.0,
            })
        })
        .collect();

    info
}
