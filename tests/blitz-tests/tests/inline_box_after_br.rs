//! Inline boxes (e.g. `display: inline-block` links) that follow a forced line
//! break (`<br>`) must all contribute to the max-content width of the line
//! they end up on.
//!
//! Regression test: parley's `calculate_content_widths` detects a mandatory
//! break via the boundary flag on the text cluster *after* the break, so
//! inline boxes between the break and that cluster were attributed to the line
//! *before* the break, under-reporting the max-content width. In a
//! shrink-to-fit container this caused the last inline-block to wrap onto a
//! new line even though there was enough space (seen with the "Get Started" /
//! "Source Code" / "Sponsor" links on <https://freyaui.dev> at 800px wide).

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            font_ctx: Some(FontContext::new()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

#[test]
fn inline_blocks_after_br_stay_on_one_line() {
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="display:flex; flex-direction:column; align-items:center;">
                <div>
                    Some leading text<br>
                    <a id="a" style="display:inline-block; width:100px; height:30px;">A</a>
                    <a id="b" style="display:inline-block; width:100px; height:30px;">B</a>
                    <a id="c" style="display:inline-block; width:100px; height:30px;">C</a>
                </div>
            </div>
        </body></html>"#,
    );
    let y = |sel: &str| {
        let id = doc.query_selector(sel).unwrap().unwrap();
        doc.get_node(id).unwrap().final_layout().location.y
    };
    let (a, b, c) = (y("#a"), y("#b"), y("#c"));
    assert_eq!(a, b, "second inline-block wrapped to a new line");
    assert_eq!(b, c, "third inline-block wrapped to a new line");
}
