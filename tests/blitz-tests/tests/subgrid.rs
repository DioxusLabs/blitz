//! CSS Subgrid (`grid-template-columns/rows: subgrid`)

use blitz_dom::{DocumentConfig, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(html: &str, incremental: bool) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(incremental);
    doc.resolve(0.0);
    doc
}

fn layout(doc: &HtmlDocument, selector: &str) -> taffy::Layout {
    let node_id = doc.query_selector(selector).unwrap().unwrap();
    *doc.get_node(node_id).unwrap().final_layout()
}

#[test]
fn subgrid_adopts_parent_column_tracks() {
    const HTML: &str = r#"<html><body style="margin:0">
        <div style="display:grid; grid-template-columns: 10px 20px 30px 40px;">
            <div id="subgrid" style="display:grid; grid-column: 2 / 5; grid-template-columns: subgrid;">
                <div id="a" style="height:10px"></div>
                <div id="b" style="height:10px"></div>
                <div id="c" style="height:10px"></div>
            </div>
        </div>
    </body></html>"#;

    for incremental in [false, true] {
        let doc = make_doc(HTML, incremental);
        assert_eq!(layout(&doc, "#subgrid").location.x, 10.0);
        assert_eq!(layout(&doc, "#subgrid").size.width, 90.0);
        for (selector, x, width) in [("#a", 0.0, 20.0), ("#b", 20.0, 30.0), ("#c", 50.0, 40.0)] {
            let item = layout(&doc, selector);
            assert_eq!(
                (item.location.x, item.size.width),
                (x, width),
                "{selector} incremental={incremental}"
            );
        }
    }
}

#[test]
fn subgrid_line_names_expand_integer_repeats() {
    const HTML: &str = r#"<html><body style="margin:0">
        <div style="display:grid; grid-template-columns: 10px 20px 30px 40px;">
            <div style="display:grid; grid-column: 1 / 5; grid-template-columns: subgrid [a] repeat(2, [b]) [c];">
                <div id="second-b" style="grid-column: b 2; height:10px"></div>
                <div id="c" style="grid-column: c; height:10px"></div>
            </div>
        </div>
    </body></html>"#;

    let doc = make_doc(HTML, true);
    assert_eq!(layout(&doc, "#second-b").location.x, 30.0);
    assert_eq!(layout(&doc, "#second-b").size.width, 30.0);
    assert_eq!(layout(&doc, "#c").location.x, 60.0);
    assert_eq!(layout(&doc, "#c").size.width, 40.0);
}

#[test]
fn subgrid_descendant_change_relayouts_parent_tracks() {
    const HTML: &str = r#"<html><body style="margin:0">
        <div id="grid" style="display:grid; grid-template-rows: auto 10px;">
            <div id="subgrid" style="display:grid; grid-row: 1 / 3; grid-template-rows: subgrid;">
                <div id="leaf" style="height:20px"></div>
            </div>
        </div>
    </body></html>"#;

    for incremental in [false, true] {
        let mut doc = make_doc(HTML, incremental);
        assert_eq!(layout(&doc, "#grid").size.height, 30.0);
        assert_eq!(layout(&doc, "#subgrid").size.height, 30.0);

        let leaf = doc.query_selector("#leaf").unwrap().unwrap();
        doc.mutate().set_attribute(
            leaf,
            QualName::new(None, ns!(), "style".into()),
            "height:50px",
        );
        doc.resolve(0.0);

        assert_eq!(
            layout(&doc, "#leaf").size.height,
            50.0,
            "incremental={incremental}"
        );
        assert_eq!(
            layout(&doc, "#subgrid").size.height,
            60.0,
            "incremental={incremental}"
        );
        assert_eq!(
            layout(&doc, "#grid").size.height,
            60.0,
            "incremental={incremental}"
        );
    }
}
