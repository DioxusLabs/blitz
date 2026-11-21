use blitz_traits::node_id::NodeId;
use std::{ops::Range, sync::Arc};
use style::logical_geometry::WritingMode;
use style::properties::ComputedValues;
use stylo_taffy::CopyStyleGetters;

use atomic_refcell::AtomicRefCell;
use markup5ever::local_name;
use style::properties::style_structs::Border;
use style::servo_arc::Arc as ServoArc;
use style::typed_om::NumericBaseType;
use style::values::computed::calc::CalcPercentageLeaf;
use style::values::computed::length_percentage::{
    CalcLengthPercentage, CalcNode, ComputedLeaf, Unpacked as UnpackedLengthPercentage,
};
use style::values::computed::{Length, LengthPercentage};
use style::values::generics::Optional;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::{
    Atom, computed_values::border_collapse::T as BorderCollapse,
    computed_values::table_layout::T as TableLayout, values::computed::BorderStyle,
};
use style_traits::values::specified::AllowedNumericType;
use taffy::{
    DetailedGridInfo, LayoutPartialTree as _, MaybeResolve, ResolveOrZero, TrackSizingFunction,
    style_helpers,
};

use crate::BaseDocument;
use crate::layout::LayoutPassState;

use super::damage::{CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC};
use super::resolve_calc_value;

pub struct TableTreeWrapper<'a, 'doc> {
    pub(crate) doc: &'a mut LayoutPassState<'doc>,
    pub(crate) ctx: Arc<TableContext>,
    /// The table's style with column widths from the automatic table layout
    /// algorithm, when they could be computed (see [`Self::auto_layout_style`]).
    pub(crate) style_override: Option<taffy::Style<Atom>>,
}

impl TableTreeWrapper<'_, '_> {
    /// The table's style with its columns set to the widths the automatic table
    /// layout algorithm gives them, for a table laid out at a definite width.
    ///
    /// Taffy sizes `auto` grid tracks by handing free space out equally, so
    /// narrow columns (a quantity, a unit) reach their max-content width while a
    /// column of long text stays narrow and its rows grow tall. Browsers hand
    /// space out in proportion to each column's max-content width instead, and
    /// honour percentage widths on cells. This follows Blink's table layout
    /// (see [`distribute_auto_table_width`]).
    ///
    /// A table whose specified width is less than its columns' min-content
    /// widths is widened to them, as in browsers: also during intrinsic sizing,
    /// so that a float or a cell holding the table makes room for it. During
    /// intrinsic sizing an auto-width table is as wide as its columns' min- or
    /// max-content widths. A cell's own `width` sets its column's width but
    /// not its min-content width, so a nested table of fixed-width cells still
    /// shrinks to fit its container.
    ///
    /// Returns `None` (keeping Taffy's sizing) for fixed-layout tables or for
    /// `<col>` sizes other than auto and lengths.
    pub(crate) fn auto_layout_style(
        &mut self,
        inputs: taffy::LayoutInput,
    ) -> Option<taffy::Style<Atom>> {
        if self.ctx.is_fixed || self.ctx.cells.is_empty() {
            return None;
        }
        let style = &self.ctx.style;
        // During intrinsic sizing (min- or max-content), a table without a
        // specified length is as wide as its columns' min- or max-content
        // widths in the table algorithm. Taffy's would differ: a column with a
        // fixed width would not shrink below it.
        let (available_width, intrinsic) =
            match (inputs.available_space.width, inputs.known_dimensions.width) {
                (_, Some(width)) | (taffy::AvailableSpace::Definite(width), None) => (width, None),
                (space, None) => (0.0, Some(space)),
            };
        let has_length_width = style.size.width.tag() == taffy::CompactLength::LENGTH_TAG;
        let n = style.grid_template_columns.len();
        if n == 0 {
            return None;
        }
        let mut columns: Vec<ColumnWidth> = Vec::with_capacity(n);
        for track in style.grid_template_columns.iter() {
            let taffy::GridTemplateComponent::Single(track) = track else {
                return None;
            };
            let max = track.max.into_raw();
            columns.push(match max.tag() {
                taffy::CompactLength::LENGTH_TAG => ColumnWidth::Fixed(max.value()),
                taffy::CompactLength::AUTO_TAG => ColumnWidth::Auto,
                _ => return None,
            });
        }
        let parent_width = inputs.parent_size.width;
        let padding = style
            .padding
            .resolve_or_zero(parent_width, resolve_calc_value);
        let border = style
            .border
            .resolve_or_zero(parent_width, resolve_calc_value);
        let gap = style
            .gap
            .width
            .resolve_or_zero(parent_width, resolve_calc_value);
        let extras = padding.left + padding.right + border.left + border.right;
        let gaps = gap * (n - 1) as f32;
        // The table's border-box width if it is known or specified; otherwise
        // (`width: auto`) it is clamped to the columns' content widths below.
        let specified = inputs.known_dimensions.width.or_else(|| {
            if intrinsic.is_some() && !has_length_width {
                return None;
            }
            let width = style
                .size
                .width
                .maybe_resolve(parent_width, resolve_calc_value)?;
            Some(match style.box_sizing {
                taffy::BoxSizing::ContentBox => width + extras,
                taffy::BoxSizing::BorderBox => width,
            })
        });

        // Min- and max-content widths of the columns, from the cells that span
        // one column; then widened for the cells that span several.
        let mut min = vec![0.0f32; n];
        let mut max = vec![0.0f32; n];
        let mut spanning = Vec::new();
        for index in 0..self.ctx.cells.len() {
            let cell = &self.ctx.cells[index];
            let (column, colspan, width) =
                (cell.column as usize, cell.colspan as usize, cell.width);
            if column >= n {
                continue;
            }
            let mut measure = |space: taffy::AvailableSpace| {
                self.compute_child_layout(
                    index.into(),
                    taffy::LayoutInput {
                        known_dimensions: taffy::Size::NONE,
                        known_dimensions_are_definite: taffy::Size {
                            width: true,
                            height: true,
                        },
                        parent_size: taffy::Size::NONE,
                        available_space: taffy::Size {
                            width: space,
                            height: taffy::AvailableSpace::MaxContent,
                        },
                        // A cell's `width` sets its column's width, not its
                        // min-content width: measure the content alone.
                        sizing_mode: taffy::SizingMode::ContentSize,
                        axis: taffy::RequestedAxis::Horizontal,
                        run_mode: taffy::RunMode::ComputeSize,
                        vertical_margins_are_collapsible: taffy::Line::FALSE,
                    },
                )
                .size
                .width
            };
            let cell_min = measure(taffy::AvailableSpace::MinContent);
            let cell_max = measure(taffy::AvailableSpace::MaxContent).max(cell_min);
            let colspan = colspan.min(n - column);
            if colspan == 1 {
                min[column] = min[column].max(cell_min);
                max[column] = max[column].max(cell_max);
                columns[column] = columns[column].with_cell(width);
            } else {
                spanning.push((column, colspan, cell_min, cell_max));
            }
        }
        // A spanning cell wider than its columns widens them in proportion to
        // their widths (equally if they are empty).
        let widen = |sizes: &mut [f32], wanted: f32, colspan: usize| {
            let current: f32 = sizes.iter().sum::<f32>() + gap * (colspan - 1) as f32;
            let extra = wanted - current;
            if extra > 0.0 {
                let total: f32 = sizes.iter().sum();
                for size in sizes.iter_mut() {
                    let share = if total > 0.0 {
                        *size / total
                    } else {
                        1.0 / colspan as f32
                    };
                    *size += extra * share;
                }
            }
        };
        for (column, colspan, cell_min, cell_max) in spanning {
            let range = column..column + colspan;
            widen(&mut min[range.clone()], cell_min, colspan);
            widen(&mut max[range], cell_max, colspan);
        }
        for ((max, &min), column) in max.iter_mut().zip(&min).zip(&columns) {
            *max = match *column {
                ColumnWidth::Fixed(f) => min.max(f),
                _ => max.max(min),
            };
        }
        let sum_min: f32 = min.iter().sum();
        let mut available = match (specified, intrinsic) {
            (Some(width), _) => width - extras - gaps,
            (None, Some(taffy::AvailableSpace::MinContent)) => sum_min,
            (None, Some(_)) => max.iter().sum(),
            (None, None) => (available_width - extras - gaps)
                .min(max.iter().sum())
                .max(sum_min),
        };
        let mut style = self.ctx.style.clone();
        if available + 0.01 < sum_min {
            // Narrower than its columns' min-content widths: browsers widen
            // the table, over its `width` and `max-width`. A width the parent
            // has already fixed is kept (the content overflows).
            if inputs.known_dimensions.width.is_some() {
                return None;
            }
            available = sum_min;
            let border_box = sum_min + gaps + extras;
            let width = match style.box_sizing {
                taffy::BoxSizing::ContentBox => border_box - extras,
                taffy::BoxSizing::BorderBox => border_box,
            };
            style.size.width = style_helpers::length(width);
            style.min_size.width = style_helpers::length(width);
            style.max_size.width = style_helpers::auto();
        }
        let widths =
            distribute_auto_table_width(available, &min, &max, &columns, specified.is_some());
        style.grid_template_columns = widths.into_iter().map(style_helpers::length).collect();
        Some(style)
    }
}

/// A column's width in the automatic table layout algorithm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnWidth {
    Auto,
    /// A border-box length
    Fixed(f32),
    /// A fraction (0 to 1) of the table's content width
    Percent(f32),
}

impl ColumnWidth {
    /// The column's width once a cell with `cell` width is in it: percentages
    /// win over lengths, and the larger of two of a kind wins.
    fn with_cell(self, cell: ColumnWidth) -> ColumnWidth {
        use ColumnWidth::*;
        match (self, cell) {
            (Percent(a), Percent(b)) => Percent(a.max(b)),
            (Percent(_), _) => self,
            (_, Percent(_)) => cell,
            (Fixed(a), Fixed(b)) => Fixed(a.max(b)),
            (Fixed(_), Auto) => self,
            (Auto, _) => cell,
        }
    }
}

/// The column widths Blink's table layout (`DistributeInlineSizeToComputedInlineSizeAuto`)
/// gives a table `available` wide (its content width less the spacing between
/// columns), following CSS Tables 3's width distribution: four guesses, each
/// at least as wide as the one before:
///
/// 1. min: every column at its min-content width;
/// 2. percentage: percentage columns at their share of the table;
/// 3. specified: fixed columns at their width as well;
/// 4. max: auto columns at their max-content width as well.
///
/// Between two guesses the columns that change grow from the narrower guess to
/// the wider one, in proportion to how much they grow: an auto column of long
/// text and a short one both get the same fraction of the way from their
/// min-content to their max-content width. Above the max guess, auto columns
/// share what is left in proportion to their max-content width; without auto
/// columns, fixed columns in a table with a specified width do, in
/// proportion to their width; otherwise percentage columns, in proportion to
/// their percentage. `constrained` says whether the table has a specified
/// width.
fn distribute_auto_table_width(
    available: f32,
    min: &[f32],
    max: &[f32],
    columns: &[ColumnWidth],
    constrained: bool,
) -> Vec<f32> {
    // Percentages beyond 100% in total are cut from the last columns.
    let mut percent_left = 1.0f32;
    let percent: Vec<Option<f32>> = columns
        .iter()
        .map(|column| match *column {
            ColumnWidth::Percent(p) => {
                let p = p.min(percent_left).max(0.0);
                percent_left -= p;
                Some(p)
            }
            _ => None,
        })
        .collect();
    let is_fixed = |c: usize| matches!(columns[c], ColumnWidth::Fixed(_));
    // A fixed column's max-content width is its width (at least its
    // min-content width), whatever its content.
    let max: Vec<f32> = (0..min.len())
        .map(|c| match columns[c] {
            ColumnWidth::Fixed(f) => min[c].max(f),
            _ => max[c],
        })
        .collect();
    let max = &max[..];
    let target = available.max(min.iter().sum());
    let percent_width = |c: usize, p: f32| min[c].max(p * target);

    let n = min.len();
    let (mut min_guess, mut percent_guess, mut specified_guess, mut max_guess) =
        (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let (mut percent_count, mut fixed_count, mut auto_count) = (0usize, 0usize, 0usize);
    let (mut total_percent, mut fixed_sum, mut auto_sum) = (0.0f32, 0.0f32, 0.0f32);
    for c in 0..n {
        min_guess += min[c];
        if let Some(p) = percent[c] {
            let w = percent_width(c, p);
            percent_guess += w;
            specified_guess += w;
            max_guess += w;
            percent_count += 1;
            total_percent += p;
        } else if is_fixed(c) {
            percent_guess += min[c];
            specified_guess += max[c];
            max_guess += max[c];
            fixed_count += 1;
            fixed_sum += max[c];
        } else {
            percent_guess += min[c];
            specified_guess += min[c];
            max_guess += max[c];
            auto_count += 1;
            auto_sum += max[c];
        }
    }

    let is_percent = |c: usize| percent[c].is_some();
    let is_auto = |c: usize| !is_percent(c) && !is_fixed(c);
    let percent_of = |c: usize| percent[c].map_or(min[c], |p| percent_width(c, p));
    // Each column's width in the guesses after the min guess.
    let percent_widths: Vec<f32> = (0..n).map(percent_of).collect();
    let specified_widths: Vec<f32> = (0..n)
        .map(|c| {
            if is_fixed(c) {
                max[c]
            } else {
                percent_widths[c]
            }
        })
        .collect();
    let max_widths: Vec<f32> = (0..n)
        .map(|c| {
            if is_auto(c) {
                max[c]
            } else {
                specified_widths[c]
            }
        })
        .collect();
    // Between guesses `from` and `to`, the `count` columns of `kind` share
    // `space` in proportion to how much they grow.
    let between =
        |from: &[f32], to: &[f32], kind: &dyn Fn(usize) -> bool, count: usize, space: f32| {
            let increase: f32 = (0..n).filter(|&c| kind(c)).map(|c| to[c] - from[c]).sum();
            (0..n)
                .map(|c| match () {
                    _ if !kind(c) => to[c],
                    _ if increase > 0.0 => from[c] + space * (to[c] - from[c]) / increase,
                    _ => from[c] + space / count as f32,
                })
                .collect::<Vec<f32>>()
        };

    if target <= min_guess {
        min.to_vec()
    } else if target <= percent_guess {
        between(
            min,
            &percent_widths,
            &is_percent,
            percent_count,
            target - min_guess,
        )
    } else if target <= specified_guess {
        between(
            &percent_widths,
            &specified_widths,
            &is_fixed,
            fixed_count,
            target - percent_guess,
        )
    } else if target <= max_guess {
        between(
            &specified_widths,
            &max_widths,
            &is_auto,
            auto_count,
            target - specified_guess,
        )
    } else {
        let space = target - max_guess;
        let share = |weight: f32, sum: f32, count: usize| {
            if sum > 0.0 {
                space * weight / sum
            } else {
                space / count as f32
            }
        };
        (0..n)
            .map(|c| {
                let base = if is_percent(c) { percent_of(c) } else { max[c] };
                base + if auto_count > 0 {
                    if is_auto(c) {
                        share(max[c], auto_sum, auto_count)
                    } else {
                        0.0
                    }
                } else if fixed_count > 0 && constrained {
                    if is_fixed(c) {
                        share(max[c], fixed_sum, fixed_count)
                    } else {
                        0.0
                    }
                } else if percent_count > 0 {
                    match percent[c] {
                        Some(p) => share(p, total_percent, percent_count),
                        None => 0.0,
                    }
                } else {
                    0.0
                }
            })
            .collect()
    }
}

// Deliberately not `Clone`: `style` may hold raw pointers into `calc_values`.
#[derive(Debug)]
pub struct TableContext {
    pub style: taffy::Style<Atom>,
    pub cells: Vec<TableCell>,
    /// Absolutely positioned descendants of the table's row groups and rows (and of the
    /// table itself). They take no part in the table grid.
    pub oof_children: Vec<(NodeId, taffy::Position)>,
    pub rows: Vec<TableRow>,
    pub columns: Vec<TableColumn>,
    pub computed_grid_info: AtomicRefCell<Option<DetailedGridInfo<Atom>>>,
    pub border_style: Option<ServoArc<Border>>,
    pub border_collapse: BorderCollapse,
    /// Whether the table uses the fixed table layout algorithm
    pub is_fixed: bool,
    /// Backing storage for `calc()` track sizes synthesised by the table layout code.
    /// Taffy stores calc values as raw pointers, so these must outlive the `style`.
    #[allow(dead_code)]
    calc_values: Vec<LengthPercentage>,
}

// #[derive(Debug, Clone, Eq, PartialEq)]
// pub enum TableItemKind {
//     Row,
//     Cell,
// }

#[derive(Debug, Clone)]
pub struct TableCell {
    // kind: TableItemKind,
    pub node_id: NodeId,
    /// Index (into `TableContext::rows`) of the row the cell originates in
    pub row: u16,
    /// Index of the column the cell originates in
    pub column: u16,
    /// The number of columns the cell spans
    pub colspan: u16,
    /// The cell's specified width (border-box lengths and percentages)
    pub width: ColumnWidth,
    style: taffy::Style<Atom>,
}

/// Tracks the current column position while walking the table's cells, so that
/// cells are assigned the same columns Taffy's dense auto-placement will give them
/// (skipping columns still occupied by rowspan cells from earlier rows).
#[derive(Debug, Default)]
struct ColumnCursor {
    /// The column the next cell in the current row will be placed in
    col: u16,
    /// The total number of columns seen so far
    num_columns: u16,
    /// For each column, the number of further rows it is occupied by a rowspan cell
    rowspans: Vec<u16>,
}

impl ColumnCursor {
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
            *remaining = rowspan - 1;
        }
        self.col = end;
        self.num_columns = self.num_columns.max(end);
    }
}

#[derive(Debug, Clone)]
pub struct TableColumn {
    pub node_id: NodeId,
    /// The column group (if any) that contains the column
    pub group: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub struct TableRow {
    // kind: TableItemKind,
    pub node_id: NodeId,
    /// The row group (if any) that contains the row
    pub group: Option<NodeId>,
    /// The row's specified `height`, if it is a length
    pub height: Option<f32>,
    /// Whether the row or any cell originating in it (and spanning only this row) has a
    /// specified length `height`. Such rows do not receive excess table height when there
    /// are unconstrained rows to receive it instead.
    pub is_constrained: bool,
}

/// The used width of one border side: border widths are not adjusted for
/// border-style in computed styles, so a border with `none`/`hidden` style
/// must be treated as zero-width.
/// The writing mode the table's grid runs in (the table's own)
#[cfg_attr(not(feature = "writing-mode"), allow(unused_variables))]
fn table_wm(doc: &BaseDocument, table_root_node_id: NodeId) -> WritingMode {
    #[cfg(feature = "writing-mode")]
    {
        if doc.try_root_element().map(|root| root.id) == Some(table_root_node_id) {
            doc.root_layout_wm(table_root_node_id)
        } else {
            doc.nodes[table_root_node_id].writing_mode()
        }
    }
    #[cfg(not(feature = "writing-mode"))]
    {
        WritingMode::empty()
    }
}

/// Convert a table part's style into the table's writing mode
fn table_taffy_style(style: &ComputedValues, table_wm: WritingMode) -> taffy::Style<Atom> {
    stylo_taffy::to_taffy_style_in(style, table_wm)
}

fn side_width(width: app_units::Au, style: BorderStyle) -> f32 {
    if style.none_or_hidden() {
        0.0
    } else {
        width.to_f32_px()
    }
}

/// Build a `calc(<percent> + <length>)` track sizing function. The calc value is
/// boxed and stored in `calc_values` so that the raw pointer Taffy holds stays valid.
fn percent_plus_length(
    calc_values: &mut Vec<LengthPercentage>,
    percent: f32,
    length: f32,
) -> TrackSizingFunction {
    let node = CalcNode::Sum(
        vec![
            CalcNode::Leaf(ComputedLeaf::Percentage(CalcPercentageLeaf::new(
                percent,
                Optional::Some(NumericBaseType::Length),
            ))),
            CalcNode::Leaf(ComputedLeaf::Length(Length::new(length))),
        ]
        .into(),
    );
    let value = LengthPercentage::new_calc(node, AllowedNumericType::NonNegative);
    let dim = match value.unpack() {
        UnpackedLengthPercentage::Calc(calc) => {
            let ptr = calc as *const CalcLengthPercentage as *const ();
            // SAFETY: the pointer targets the heap allocation owned by `value`, which
            // is kept alive by `calc_values` for as long as the TableContext exists.
            unsafe { taffy::Dimension::from_raw(taffy::CompactLength::calc(ptr)) }
        }
        UnpackedLengthPercentage::Length(len) => style_helpers::length(len.px()),
        UnpackedLengthPercentage::Percentage(p) => style_helpers::percent(p.0),
    };
    calc_values.push(value);
    dim.into()
}

pub(crate) fn build_table_context(
    doc: &mut BaseDocument,
    table_root_node_id: NodeId,
) -> (TableContext, Vec<NodeId>) {
    let mut cells: Vec<TableCell> = Vec::new();
    let mut oof_children: Vec<(NodeId, taffy::Position)> = Vec::new();
    let mut rows: Vec<TableRow> = Vec::new();
    let mut row = 0u16;
    let mut cursor = ColumnCursor::default();

    let wm = table_wm(doc, table_root_node_id);
    let root_node = &mut doc.nodes[table_root_node_id];

    let children = std::mem::take(&mut root_node.children);

    let Some(stylo_styles) = root_node.primary_styles() else {
        panic!("Ignoring table because it has no styles");
    };

    let mut style = table_taffy_style(&stylo_styles, wm);
    style.item_is_table = true;
    // Use `dense` row-flow so that each cell scans the row from its
    // leftmost column for the first free track. Without `dense`,
    // `place_definite_secondary_axis_item` keeps a per-item secondary
    // cursor across rows, which means cells in later rows do not
    // backfill columns freed up by rowspan cells from earlier rows.
    style.grid_auto_flow = taffy::GridAutoFlow::RowDense;
    style.grid_auto_columns = Vec::new();
    style.grid_auto_rows = Vec::new();

    // The fixed table layout algorithm only applies when the table has a non-auto width
    let is_fixed = match stylo_styles.clone_table_layout() {
        TableLayout::Fixed => !style.size.width.is_auto(),
        TableLayout::Auto => false,
    };

    let border_collapse = stylo_styles.clone_border_collapse();
    let border_spacing = stylo_styles.clone_border_spacing().0;

    drop(stylo_styles);

    let mut columns: Vec<TableColumn> = Vec::new();
    let mut column_sizes: Vec<taffy::TrackSizingFunction> = Vec::new();
    for child_id in children.iter().copied() {
        collect_columns(doc, child_id, wm, &mut columns, &mut column_sizes);
    }
    // Percentage column widths only take effect in the fixed table layout algorithm
    if !is_fixed {
        for column in column_sizes.iter_mut() {
            if column.max.into_raw().tag() == taffy::CompactLength::PERCENT_TAG {
                *column = style_helpers::auto();
            }
        }
    }
    let mut first_cell_border: Option<ServoArc<Border>> = None;
    // Percentage widths set on first-row cells: (column index, percentage, padding + border)
    let mut percent_columns: Vec<(u16, f32, f32)> = Vec::new();
    let mut calc_values: Vec<LengthPercentage> = Vec::new();
    // Header row groups are laid out before other rows, and footer row groups after
    let row_group_order = |doc: &BaseDocument, child_id: NodeId| -> u8 {
        let display = doc.nodes[child_id]
            .primary_styles()
            .map(|s| s.clone_display());
        match display.map(|d| d.inside()) {
            Some(DisplayInside::TableHeaderGroup) => 0,
            Some(DisplayInside::TableFooterGroup) => 2,
            _ => 1,
        }
    };
    for order in 0..3 {
        for child_id in children.iter().copied() {
            if row_group_order(doc, child_id) != order {
                continue;
            }
            collect_table_cells(
                doc,
                child_id,
                wm,
                is_fixed,
                border_collapse,
                &mut row,
                &mut cursor,
                &mut cells,
                &mut oof_children,
                &mut rows,
                &mut column_sizes,
                &mut first_cell_border,
                &mut percent_columns,
            );
        }
    }
    let remaining_column = if is_fixed {
        style_helpers::minmax(style_helpers::length(0.0), style_helpers::fr(1.0))
    } else {
        style_helpers::auto()
    };
    // Trailing `<col>` columns which contain no cells and have no definite width
    // (i.e. would be zero-width) are dropped, so they don't add border-spacing.
    while column_sizes.len() > cursor.num_columns as usize
        && column_sizes
            .last()
            .is_some_and(|c| c.min.into_raw().tag() == taffy::CompactLength::AUTO_TAG)
    {
        column_sizes.pop();
        columns.pop();
    }
    let num_columns = cursor.num_columns.max(column_sizes.len() as u16);
    column_sizes.resize(num_columns as usize, remaining_column);
    if is_fixed {
        // In the fixed table layout algorithm, percentage column widths resolve against
        // the table's content width minus the horizontal border-spacing between columns,
        // whereas Taffy resolves percentage tracks against the container's inner width
        // (from which only the outer border-spacing has been removed via padding). Fold
        // the inner border-spacing into the percentage using `calc(N% - N * spacing)`.
        let spacing_x = match border_collapse {
            BorderCollapse::Separate => border_spacing.width.px(),
            BorderCollapse::Collapse => 0.0,
        };
        let inner_spacing = spacing_x * num_columns.saturating_sub(1) as f32;
        let mut percent_track = |percent: f32, extra: f32| -> TrackSizingFunction {
            let extra = extra - percent * inner_spacing;
            if extra == 0.0 {
                style_helpers::percent(percent)
            } else {
                percent_plus_length(&mut calc_values, percent, extra)
            }
        };

        for column in column_sizes.iter_mut() {
            if column.max.is_auto() {
                *column = remaining_column;
            } else if column.max.into_raw().tag() == taffy::CompactLength::PERCENT_TAG {
                *column = percent_track(column.max.into_raw().value(), 0.0);
            }
        }

        // Percentage widths on cells apply to the cell's content box, so the
        // cell's horizontal padding and border must be added to the column width.
        for (col_idx, percent, extra) in percent_columns {
            if let Some(column) = column_sizes.get_mut(col_idx as usize) {
                *column = percent_track(percent, extra);
            }
        }
    }

    style.grid_template_columns = column_sizes.into_iter().map(|dim| dim.into()).collect();
    // A row's `height` is a minimum: the row grows to fit the content of its cells.
    // Excess table height is distributed to unconstrained rows (via the stretching of
    // `auto` tracks) if there are any, and to all rows otherwise. So constrained rows
    // use a `max-content` (rather than `auto`) maximum when there are unconstrained rows.
    let has_unconstrained_rows = rows.iter().any(|row| !row.is_constrained);
    style.grid_template_rows = rows
        .iter()
        .map(|row| {
            let min = match row.height {
                Some(height) => style_helpers::length(height),
                None => style_helpers::auto(),
            };
            let max = if row.is_constrained && has_unconstrained_rows {
                style_helpers::max_content()
            } else {
                style_helpers::auto()
            };
            style_helpers::minmax(min, max)
        })
        .collect();

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
                let x = side_width(border.border_left_width.0, border.border_left_style).max(
                    side_width(border.border_right_width.0, border.border_right_style),
                );
                let y = side_width(border.border_top_width.0, border.border_top_style).max(
                    side_width(border.border_bottom_width.0, border.border_bottom_style),
                );
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

    let layout_children = cells
        .iter()
        .map(|cell| cell.node_id)
        .chain(oof_children.iter().map(|(node_id, _)| *node_id))
        .collect();
    let root_node = &mut doc.nodes[table_root_node_id];
    root_node.children = children;

    (
        TableContext {
            style,
            cells,
            oof_children,
            rows,
            columns,
            computed_grid_info: AtomicRefCell::new(None),
            border_collapse,
            border_style: first_cell_border,
            is_fixed,
            calc_values,
        },
        layout_children,
    )
}

/// Collect `<col>` elements (and `display: table-column` elements) along with their
/// widths, which take precedence over cell widths in the fixed table layout algorithm.
/// Columns without a specified width are recorded as `auto`.
fn collect_columns(
    doc: &mut BaseDocument,
    node_id: NodeId,
    wm: WritingMode,
    columns: &mut Vec<TableColumn>,
    column_sizes: &mut Vec<TrackSizingFunction>,
) {
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
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_columns(doc, child_id, wm, columns, column_sizes);
            }
            doc.nodes[node_id].children = children;
            for column in &mut columns[first_column..] {
                column.group.get_or_insert(node_id);
            }
        }
        DisplayInside::TableColumn => {
            let style = table_taffy_style(&node.primary_styles().unwrap(), wm);
            let span: u16 = node
                .attr(local_name!("span"))
                .and_then(|val| val.parse::<u16>().ok())
                .map(|v| v.max(1))
                .unwrap_or(1);
            let column: TrackSizingFunction = match style.size.width.tag() {
                taffy::CompactLength::LENGTH_TAG => {
                    // A definite `max-width` clamps the column width; a zero width is treated as auto
                    let mut width = style.size.width.value();
                    if style.max_size.width.into_raw().tag() == taffy::CompactLength::LENGTH_TAG {
                        width = width.min(style.max_size.width.into_raw().value());
                    }
                    if width > 0.0 {
                        style_helpers::length(width)
                    } else {
                        style_helpers::auto()
                    }
                }
                taffy::CompactLength::PERCENT_TAG => {
                    style_helpers::percent(style.size.width.value())
                }
                // Browsers treat calc() widths on columns as auto
                _ => style_helpers::auto(),
            };
            // A definite `min-width` on a column acts as a floor on its width
            let column = match style.min_size.width.into_raw().tag() {
                taffy::CompactLength::LENGTH_TAG if column.max.is_auto() => style_helpers::minmax(
                    style_helpers::length(style.min_size.width.into_raw().value()),
                    style_helpers::auto(),
                ),
                _ => column,
            };
            for _ in 0..span {
                columns.push(TableColumn {
                    node_id,
                    group: None,
                });
                column_sizes.push(column);
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_table_cells(
    doc: &mut BaseDocument,
    node_id: NodeId,
    wm: WritingMode,
    is_fixed: bool,
    border_collapse: BorderCollapse,
    row: &mut u16,
    cursor: &mut ColumnCursor,
    cells: &mut Vec<TableCell>,
    oof_children: &mut Vec<(NodeId, taffy::Position)>,
    rows: &mut Vec<TableRow>,
    columns: &mut Vec<TrackSizingFunction>,
    first_cell_border: &mut Option<ServoArc<Border>>,
    percent_columns: &mut Vec<(u16, f32, f32)>,
) {
    let node = &mut doc.nodes[node_id];

    if !node.is_element() {
        return;
    }

    let Some(display) = node.display_style() else {
        #[cfg(feature = "tracing")]
        tracing::info!("Ignoring table descendent because it has no styles");
        return;
    };

    if display.outside() == DisplayOutside::None {
        node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
        return;
    }

    // Absolutely positioned boxes are out-of-flow: they are laid out by their containing
    // block rather than as part of the table grid.
    let position = node.primary_styles().unwrap().clone_position();
    if position.is_absolutely_positioned() {
        oof_children.push((node_id, stylo_taffy::convert::position(position)));
        return;
    }

    match display.inside() {
        DisplayInside::TableRowGroup
        | DisplayInside::TableHeaderGroup
        | DisplayInside::TableFooterGroup
        | DisplayInside::Contents => {
            let first_row = rows.len();
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                doc.nodes[child_id]
                    .remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                collect_table_cells(
                    doc,
                    child_id,
                    wm,
                    is_fixed,
                    border_collapse,
                    row,
                    cursor,
                    cells,
                    oof_children,
                    rows,
                    columns,
                    first_cell_border,
                    percent_columns,
                );
            }
            doc.nodes[node_id].children = children;
            if display.inside() != DisplayInside::Contents {
                for row in &mut rows[first_row..] {
                    row.group.get_or_insert(node_id);
                }
            }
        }
        DisplayInside::TableRow => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            *row += 1;
            cursor.start_row();

            let height = node.primary_styles().and_then(|style| {
                let height = stylo_taffy::convert::dimension(style.get_height());
                (height.tag() == taffy::CompactLength::LENGTH_TAG).then(|| height.value())
            });
            rows.push(TableRow {
                node_id,
                group: None,
                height,
                is_constrained: height.is_some(),
            });

            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_table_cells(
                    doc,
                    child_id,
                    wm,
                    is_fixed,
                    border_collapse,
                    row,
                    cursor,
                    cells,
                    oof_children,
                    rows,
                    columns,
                    first_cell_border,
                    percent_columns,
                );
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableCell => {
            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
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
            let mut style = table_taffy_style(stylo_style, wm);
            let col = cursor.next_free();

            // In the collapsed borders model the borders are laid out as gutters between
            // the cells (see the table's `gap`) rather than as part of the cells
            if border_collapse == BorderCollapse::Collapse {
                style.border = taffy::Rect::ZERO.map(style_helpers::length);
            }

            if first_cell_border.is_none() {
                *first_cell_border = Some(stylo_style.clone_border());
            }

            // In the fixed table layout algorithm the widths of columns are not
            // affected by cell contents, so cells must not impose a min-content
            // floor on their tracks.
            if is_fixed {
                style.min_size.width = style_helpers::length(0.0);
            }

            // Column widths from `<col>` elements take precedence over cell widths
            let col_needs_width =
                (col as usize) >= columns.len() || columns[col as usize].max.is_auto();
            if *row == 1 && col_needs_width {
                let column: TrackSizingFunction = match style.size.width.tag() {
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
                        if is_fixed {
                            let extra = match style.box_sizing {
                                taffy::BoxSizing::ContentBox => {
                                    let padding =
                                        style.padding.resolve_or_zero(None, resolve_calc_value);
                                    let border =
                                        style.border.resolve_or_zero(None, resolve_calc_value);
                                    padding.left + padding.right + border.left + border.right
                                }
                                taffy::BoxSizing::BorderBox => 0.0,
                            };
                            // Resolved in `build_table_context` once the column count is known
                            percent_columns.push((col, style.size.width.value(), extra));
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
                if (col as usize) < columns.len() {
                    if !column.max.is_auto() {
                        columns[col as usize] = column;
                    }
                } else {
                    columns.resize(col as usize, style_helpers::auto());
                    columns.push(column);
                }
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
                start: style_helpers::line(*row as i16),
                end: style_helpers::span(rowspan),
            };
            if rowspan == 1 && style.size.height.tag() == taffy::CompactLength::LENGTH_TAG {
                if let Some(row) = rows.last_mut() {
                    row.is_constrained = true;
                }
            }

            let width = match style.size.width.tag() {
                taffy::CompactLength::LENGTH_TAG => {
                    let len = style.size.width.value();
                    ColumnWidth::Fixed(match style.box_sizing {
                        taffy::BoxSizing::ContentBox => {
                            let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                            let border = style.border.resolve_or_zero(None, resolve_calc_value);
                            len + padding.left + padding.right + border.left + border.right
                        }
                        taffy::BoxSizing::BorderBox => len,
                    })
                }
                taffy::CompactLength::PERCENT_TAG if style.size.width.value() > 0.0 => {
                    ColumnWidth::Percent(style.size.width.value())
                }
                _ => ColumnWidth::Auto,
            };
            style.size.width = style_helpers::auto();
            cells.push(TableCell {
                node_id,
                row: row.saturating_sub(1),
                column: col,
                colspan: colspan.max(1),
                width,
                style,
            });

            cursor.place(colspan, rowspan);
        }
        DisplayInside::Flow
        | DisplayInside::FlowRoot
        | DisplayInside::Flex
        | DisplayInside::Grid
        | DisplayInside::GridLanes => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // Probably a table caption: ignore
            // println!(
            //     "Warning: ignoring non-table typed descendent of table ({:?})",
            //     display.inside()
            // );
        }
        DisplayInside::TableColumnGroup | DisplayInside::TableColumn | DisplayInside::Table => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            //Ignore
        }
        DisplayInside::None => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // Ignore
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

impl taffy::TraversePartialTree for TableTreeWrapper<'_, '_> {
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
impl taffy::TraverseTree for TableTreeWrapper<'_, '_> {}

impl taffy::LayoutPartialTree for TableTreeWrapper<'_, '_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type CustomIdent = Atom;

    fn get_core_container_style(&self, _node_id: taffy::NodeId) -> &taffy::Style<Atom> {
        self.style_override.as_ref().unwrap_or(&self.ctx.style)
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

impl taffy::LayoutGridContainer for TableTreeWrapper<'_, '_> {
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

#[cfg(test)]
mod tests {
    use super::{ColumnWidth, distribute_auto_table_width};
    use ColumnWidth::*;

    fn round(widths: Vec<f32>) -> Vec<f32> {
        widths
            .into_iter()
            .map(|w| (w * 10.0).round() / 10.0)
            .collect()
    }

    #[test]
    fn auto_columns_grow_from_min_to_max_content_by_the_same_fraction() {
        // A quantity column (min 20, max 40) next to a description (min 60,
        // max 400) in 300px: the 220px above the min-content widths go in
        // proportion to how much each column can grow (20 : 340), as in
        // Chromium. WebKit gave shares of the max-content widths (27.3, 272.7).
        let widths =
            distribute_auto_table_width(300.0, &[20.0, 60.0], &[40.0, 400.0], &[Auto; 2], true);
        assert_eq!(round(widths), vec![32.2, 267.8]);
    }

    #[test]
    fn narrow_columns_keep_their_min_content_width() {
        // Proportional shares below a column's min-content width are raised to
        // it, and the columns after it give the difference back.
        let widths = distribute_auto_table_width(
            200.0,
            &[30.0, 30.0, 50.0],
            &[30.0, 30.0, 500.0],
            &[Auto; 3],
            true,
        );
        assert_eq!(round(widths), vec![30.0, 30.0, 140.0]);
    }

    #[test]
    fn wide_tables_grow_auto_columns_in_proportion() {
        let widths =
            distribute_auto_table_width(400.0, &[10.0, 10.0], &[50.0, 150.0], &[Auto; 2], true);
        assert_eq!(round(widths), vec![100.0, 300.0]);
    }

    #[test]
    fn percentage_and_fixed_columns_come_first() {
        // 50% of 400 and a 100px column; the auto column gets the rest even
        // though its max-content width is larger.
        let widths = distribute_auto_table_width(
            400.0,
            &[10.0, 10.0, 10.0],
            &[20.0, 20.0, 900.0],
            &[Percent(0.5), Fixed(100.0), Auto],
            true,
        );
        assert_eq!(round(widths), vec![200.0, 100.0, 100.0]);
    }

    #[test]
    fn overallocated_percentages_are_cut_from_the_last_columns() {
        let widths = distribute_auto_table_width(
            100.0,
            &[0.0, 0.0],
            &[10.0, 10.0],
            &[Percent(0.8), Percent(0.6)],
            true,
        );
        assert_eq!(round(widths), vec![80.0, 20.0]);
    }

    #[test]
    fn min_content_widths_are_kept_when_space_runs_out() {
        // Fixed widths that do not fit shrink towards their min-content width.
        let widths = distribute_auto_table_width(
            100.0,
            &[20.0, 20.0],
            &[80.0, 80.0],
            &[Fixed(80.0), Fixed(80.0)],
            true,
        );
        assert_eq!(round(widths), vec![50.0, 50.0]);
    }
}
