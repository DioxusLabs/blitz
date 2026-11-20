//! A table's `border` attribute gives the table a border of that many pixels,
//! outset, and its cells a thin inset border, unless the value is 0.

use blitz_test_harness::Harness;
use style::values::specified::BorderStyle;

/// (top width in px, top style) of the table and of its first cell.
fn borders(table_attrs: &str) -> ((f32, BorderStyle), (f32, BorderStyle)) {
    let harness = Harness::from_html(&format!(
        "<html><body><table id=t {table_attrs}><tr><td id=c>x</td></tr></table></body></html>"
    ));
    let border_of = |selector| {
        let id = harness.node(selector);
        let doc = harness.base();
        let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
        let border = styles.get_border();
        (
            border.border_top_width.0.to_f32_px(),
            border.border_top_style,
        )
    };
    (border_of("#t"), border_of("#c"))
}

#[test]
fn border_sets_table_and_cell_borders() {
    let (table, cell) = borders("border=3");
    assert_eq!(table, (3.0, BorderStyle::Outset));
    assert_eq!(cell, (1.0, BorderStyle::Inset));
}

#[test]
fn border_without_a_number_is_one_pixel() {
    for attrs in ["border", "border=''", "border=thick"] {
        let (table, cell) = borders(attrs);
        assert_eq!(table, (1.0, BorderStyle::Outset), "{attrs}");
        assert_eq!(cell, (1.0, BorderStyle::Inset), "{attrs}");
    }
}

#[test]
fn zero_border_draws_nothing() {
    let (table, cell) = borders("border=0");
    assert_eq!(table.1, BorderStyle::None);
    assert_eq!(cell.1, BorderStyle::None);
}

#[test]
fn frame_and_rules_override_border() {
    let (table, cell) = borders("border=2 frame=void rules=none");
    assert_eq!(table.1, BorderStyle::Hidden);
    assert_eq!(cell.1, BorderStyle::None);
}

#[test]
fn author_css_overrides_border() {
    let (table, cell) = borders("border=2 style='border: 4px solid'");
    assert_eq!(table, (4.0, BorderStyle::Solid));
    assert_eq!(cell, (1.0, BorderStyle::Inset));
}
