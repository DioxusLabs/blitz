//! A table bakes the widths of its columns into the grid it is laid out as when its boxes are
//! built, so a change to a column's style has to rebuild them.
//!
//! Regression test: a script setting a new width on a `<col>` left the cells of its column at
//! the old width.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<html><body style="margin: 0">
    <table style="border-spacing: 0">
        <colgroup><col id="column" style="width: 100px"></colgroup>
        <tr><td id="cell" style="padding: 0">a</td></tr>
    </table>
</body></html>"#;

#[test]
fn a_new_column_width_reaches_the_cells() {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let cell = doc.get_element_by_id("cell").unwrap();
    assert_eq!(doc.get_node(cell).unwrap().final_layout().size.width, 100.0);

    let column = doc.get_element_by_id("column").unwrap();
    doc.mutate().set_style_property(column, "width", "50px");
    doc.resolve(0.0);
    assert_eq!(doc.get_node(cell).unwrap().final_layout().size.width, 50.0);
}
