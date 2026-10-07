//! Atomic inline boxes (e.g. `inline-block`) are aligned in their line by the baseline they
//! export, rather than by their bottom margin edge.
//!
//! NOTE: These tests measure real text so they require a usable font. Without
//! the `system-fonts` feature (enabled by default when testing the whole
//! workspace) text measures 0x0 and the assertions pass vacuously.

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

fn rect(doc: &HtmlDocument, selector: &str) -> (f32, f32) {
    let id = doc.query_selector(selector).unwrap().unwrap();
    let layout = doc.get_node(id).unwrap().final_layout();
    (layout.location.y, layout.size.height)
}

const STYLE: &str = "<style>body { margin: 0; font-size: 20px; line-height: 20px; } span { display: inline-block; }</style>";

#[test]
fn inline_block_aligns_by_text_baseline() {
    let doc = layout_doc(&format!(
        r#"{STYLE}<div><span id="a">X</span><span id="b" style="padding-bottom: 30px">X</span></div>"#
    ));
    let (a_y, _) = rect(&doc, "#a");
    let (b_y, _) = rect(&doc, "#b");
    assert_eq!(a_y, b_y);
}

#[test]
fn inline_block_aligns_by_last_line_baseline() {
    let doc = layout_doc(&format!(
        r#"{STYLE}<div><span id="a">X<br>X</span><span id="b">X</span></div>"#
    ));
    let (a_y, a_height) = rect(&doc, "#a");
    let (b_y, b_height) = rect(&doc, "#b");
    assert_eq!(a_height, 40.0);
    assert_eq!(b_height, 20.0);
    assert_eq!(b_y - a_y, 20.0);
}

#[test]
fn scroll_container_inline_block_aligns_by_bottom_margin_edge() {
    let doc = layout_doc(&format!(
        r#"{STYLE}<div><span id="a">X</span><span id="b" style="overflow: hidden; margin-bottom: 5px">X</span></div>"#
    ));
    let (a_y, a_height) = rect(&doc, "#a");
    let (b_y, b_height) = rect(&doc, "#b");
    // `b`'s bottom margin edge sits on the baseline, which is above `a`'s bottom edge.
    assert!(b_y + b_height + 5.0 < a_y + a_height);
}

#[test]
fn trailing_br_does_not_move_last_baseline() {
    let doc = layout_doc(&format!(
        r#"{STYLE}<div><span id="a">X<br></span><span id="b">X</span></div>"#
    ));
    let (a_y, a_height) = rect(&doc, "#a");
    let (b_y, _) = rect(&doc, "#b");
    assert_eq!(a_height, 20.0);
    assert_eq!(a_y, b_y);
}

#[test]
fn negative_vertical_margins_offset_the_border_box() {
    for tag in ["img", "span"] {
        for alignment in [
            "baseline",
            "middle",
            "text-top",
            "text-bottom",
            "top",
            "bottom",
            "-14px",
            "10px",
        ] {
            for (top, bottom) in [
                (-30, 0),
                (0, -30),
                (-30, -40),
                (-120, 0),
                (0, -120),
                (0, 0),
                (30, 0),
                (0, 30),
            ] {
                let reserved_height = (100 + top + bottom).max(0);
                let doc = layout_doc(&format!(
                    r#"{STYLE}
                    <div id="actual"><{tag} id="a" style="width:100px; height:100px;
                        vertical-align:{alignment}; margin-top:{top}px; margin-bottom:{bottom}px"></{tag}></div>
                    <div id="reference"><{tag} id="b" style="width:100px; height:{reserved_height}px;
                        vertical-align:{alignment}; position:relative; top:{top}px"></{tag}></div>"#
                ));
                let context = format!("{tag}, {alignment}, margins: {top}px {bottom}px");
                let (a_y, a_height) = rect(&doc, "#a");
                let (b_y, b_height) = rect(&doc, "#b");
                assert_eq!(a_height, 100.0, "{context}");
                assert_eq!(b_height, reserved_height as f32, "{context}");
                assert_eq!(a_y, b_y, "{context}");
                assert_eq!(
                    rect(&doc, "#actual").1,
                    rect(&doc, "#reference").1,
                    "{context}"
                );
            }
        }
    }
}
