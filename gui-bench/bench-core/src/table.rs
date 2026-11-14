//! The keyed table workload: the js-framework-benchmark operations.

/// js-framework-benchmark's operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableOp {
    /// Replace all rows with `n` new rows.
    Create(usize),
    /// Append `n` new rows.
    Append(usize),
    /// Append " !!!" to the label of every 10th row.
    UpdateEvery10th,
    /// Select the row at this index (deselecting any other).
    Select(usize),
    /// Swap the rows at index 1 and 998.
    SwapRows,
    /// Remove the row at this index.
    Remove(usize),
    Clear,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: usize,
    pub label: String,
}

const ADJECTIVES: &[&str] = &[
    "pretty",
    "large",
    "big",
    "small",
    "tall",
    "short",
    "long",
    "handsome",
    "plain",
    "quaint",
    "clean",
    "elegant",
    "easy",
    "angry",
    "crazy",
    "helpful",
    "mushy",
    "odd",
    "unsightly",
    "adorable",
    "important",
    "inexpensive",
    "cheap",
    "expensive",
    "fancy",
];
const COLOURS: &[&str] = &[
    "red", "yellow", "blue", "green", "pink", "brown", "purple", "brown", "white", "black",
    "orange",
];
const NOUNS: &[&str] = &[
    "table", "chair", "house", "bbq", "desk", "car", "pony", "cookie", "sandwich", "burger",
    "pizza", "mouse", "keyboard",
];

/// Deterministic row generator, so every toolkit renders the same labels.
#[derive(Clone, Debug)]
pub struct RowGen {
    next_id: usize,
    rng: u64,
}

impl Default for RowGen {
    fn default() -> Self {
        RowGen {
            next_id: 1,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl RowGen {
    fn pick(&mut self, words: &'static [&'static str]) -> &'static str {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let r = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        words[(r >> 33) as usize % words.len()]
    }

    pub fn row(&mut self) -> Row {
        let label = format!(
            "{} {} {}",
            self.pick(ADJECTIVES),
            self.pick(COLOURS),
            self.pick(NOUNS)
        );
        let id = self.next_id;
        self.next_id += 1;
        Row { id, label }
    }

    pub fn rows(&mut self, n: usize) -> Vec<Row> {
        (0..n).map(|_| self.row()).collect()
    }
}

/// A plain model apps can use when their toolkit has no state model of its own
/// (egui, GPUI). Applying an op to it is part of the `update` phase.
#[derive(Default, Debug)]
pub struct TableModel {
    pub rows: Vec<Row>,
    pub selected: Option<usize>,
    pub rowgen: RowGen,
}

impl TableModel {
    pub fn apply(&mut self, op: TableOp) {
        match op {
            TableOp::Create(n) => {
                self.rows = self.rowgen.rows(n);
                self.selected = None;
            }
            TableOp::Append(n) => {
                let new = self.rowgen.rows(n);
                self.rows.extend(new);
            }
            TableOp::UpdateEvery10th => {
                for row in self.rows.iter_mut().step_by(10) {
                    row.label.push_str(" !!!");
                }
            }
            TableOp::Select(index) => self.selected = self.rows.get(index).map(|r| r.id),
            TableOp::SwapRows => {
                if self.rows.len() > 998 {
                    self.rows.swap(1, 998);
                }
            }
            TableOp::Remove(index) => {
                if index < self.rows.len() {
                    self.rows.remove(index);
                }
            }
            TableOp::Clear => {
                self.rows.clear();
                self.selected = None;
            }
        }
    }
}

/// One js-framework-benchmark benchmark: untimed setup ops, then the timed op.
pub struct Bench {
    pub name: &'static str,
    pub setup: &'static [TableOp],
    pub op: TableOp,
}

pub const BENCHES: &[Bench] = &[
    Bench {
        name: "create_1k",
        setup: &[TableOp::Clear],
        op: TableOp::Create(1_000),
    },
    Bench {
        name: "replace_1k",
        setup: &[TableOp::Create(1_000)],
        op: TableOp::Create(1_000),
    },
    Bench {
        name: "update_every_10th",
        setup: &[TableOp::Create(1_000)],
        op: TableOp::UpdateEvery10th,
    },
    Bench {
        name: "select_row",
        setup: &[TableOp::Create(1_000)],
        op: TableOp::Select(1),
    },
    Bench {
        name: "swap_rows",
        setup: &[TableOp::Create(1_000)],
        op: TableOp::SwapRows,
    },
    Bench {
        name: "remove_row",
        setup: &[TableOp::Create(1_000)],
        op: TableOp::Remove(1),
    },
    Bench {
        name: "create_10k",
        setup: &[TableOp::Clear],
        op: TableOp::Create(10_000),
    },
    Bench {
        name: "append_1k_to_10k",
        setup: &[TableOp::Create(10_000)],
        op: TableOp::Append(1_000),
    },
    Bench {
        name: "clear_1k",
        setup: &[TableOp::Create(1_000)],
        op: TableOp::Clear,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_follow_js_framework_benchmark() {
        let mut m = TableModel::default();
        m.apply(TableOp::Create(1_000));
        assert_eq!(m.rows.len(), 1_000);
        let (a, b) = (m.rows[1].id, m.rows[998].id);
        m.apply(TableOp::SwapRows);
        assert_eq!((m.rows[1].id, m.rows[998].id), (b, a));
        m.apply(TableOp::UpdateEvery10th);
        assert!(m.rows[10].label.ends_with(" !!!") && !m.rows[11].label.ends_with(" !!!"));
        m.apply(TableOp::Select(1));
        assert_eq!(m.selected, Some(b));
        m.apply(TableOp::Remove(1));
        assert_eq!(m.rows.len(), 999);
        m.apply(TableOp::Append(1_000));
        assert_eq!(m.rows.len(), 1_999);
        m.apply(TableOp::Clear);
        assert!(m.rows.is_empty() && m.selected.is_none());
    }

    #[test]
    fn rows_are_deterministic() {
        assert_eq!(RowGen::default().rows(100), RowGen::default().rows(100));
    }
}
