//! In automatic table layout, a column with a percentage width gets that share of a table with
//! a width of its own, but no less than its content needs; the other columns get what's left.
//!
//! Regression test: Amazon's shipping progress bar is a row of a `50%` bar, an 18px dot and
//! a `50%` bar. The percentages were ignored and the dot's cell took the room the bars should
//! have had.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

/// The widths of `#l`, `#m` and `#r` in a 300px wide table, each cell with the given style.
fn widths(cols: &str, l: &str, m: &str, r: &str) -> (f32, f32, f32) {
    let doc = layout(&format!(
        r#"<body style="margin:0"><table width="300" style="border-spacing:0">{cols}<tr>
        <td id="l" style="padding:0;{l}"><div style="height:4px"></div></td>
        <td id="m" style="padding:0;{m}"><div style="width:18px;height:18px"></div></td>
        <td id="r" style="padding:0;{r}"><div style="height:4px"></div></td>
        </tr></table></body>"#
    ));
    let width = |sel: &str| {
        let id = doc.query_selector(sel).unwrap().expect(sel);
        doc.get_node(id).unwrap().final_layout().size.width
    };
    (width("#l"), width("#m"), width("#r"))
}

#[test]
fn percentages_adding_up_to_the_whole_leave_the_rest_its_least() {
    let w = widths("", "width:50%", "", "width:50%");
    assert_eq!(w, (141.0, 18.0, 141.0));
}

#[test]
fn percentage_attributes_count_as_css_does() {
    let doc = layout(
        r#"<body style="margin:0"><table width="300" style="border-spacing:0"><tr>
        <td id="l" width="50%" style="padding:0"><div style="height:4px"></div></td>
        <td id="m" style="padding:0"><div style="width:18px;height:18px"></div></td>
        <td id="r" width="50%" style="padding:0"><div style="height:4px"></div></td>
        </tr></table></body>"#,
    );
    let width = |sel: &str| {
        let id = doc.query_selector(sel).unwrap().expect(sel);
        doc.get_node(id).unwrap().final_layout().size.width
    };
    let w = (width("#l"), width("#m"), width("#r"));
    assert_eq!(w, (141.0, 18.0, 141.0));
}

#[test]
fn what_percentages_leave_goes_to_the_other_columns() {
    let w = widths("", "width:30%", "", "width:30%");
    assert_eq!(w, (90.0, 120.0, 90.0));
}

#[test]
fn a_percentage_column_is_no_narrower_than_its_content() {
    let doc = layout(
        r#"<body style="margin:0"><table width="300" style="border-spacing:0"><tr>
        <td id="l" width="10%" style="padding:0"><div style="width:100px;height:4px"></div></td>
        <td id="r" style="padding:0"><div style="width:20px;height:4px"></div></td>
        </tr></table></body>"#,
    );
    let width = |sel: &str| {
        let id = doc.query_selector(sel).unwrap().expect(sel);
        doc.get_node(id).unwrap().final_layout().size.width
    };
    assert_eq!((width("#l"), width("#r")), (100.0, 200.0));
}

#[test]
fn col_percentages_count_as_cells_do() {
    let cols = r#"<col width="50%"><col><col width="50%">"#;
    let w = widths(cols, "", "", "");
    assert_eq!(w, (141.0, 18.0, 141.0));
}
