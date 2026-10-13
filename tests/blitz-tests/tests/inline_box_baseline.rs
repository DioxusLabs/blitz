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
    for overflow in ["hidden", "auto", "scroll"] {
        let doc = layout_doc(&format!(
            r#"{STYLE}<div><span id="a">X</span><span id="b" style="overflow: {overflow}; margin-bottom: 5px">X</span></div>"#
        ));
        let (a_y, a_height) = rect(&doc, "#a");
        let (b_y, b_height) = rect(&doc, "#b");
        // `b`'s bottom margin edge sits on the baseline, which is above `a`'s bottom edge.
        assert!(b_y + b_height + 5.0 < a_y + a_height, "{overflow}");
    }
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
fn baseline_source_selects_inline_block_line() {
    for (source, line_offset) in [("auto", 20.0), ("first", 0.0), ("last", 20.0)] {
        let doc = layout_doc(&format!(
            r#"{STYLE}<div><span id="box" style="baseline-source: {source}">X<br>X</span><span id="anchor">X</span></div>"#
        ));
        let (box_y, box_height) = rect(&doc, "#box");
        let (anchor_y, anchor_height) = rect(&doc, "#anchor");
        assert_eq!(box_height, 40.0);
        assert_eq!(anchor_height, 20.0);
        assert_eq!(anchor_y - box_y, line_offset, "{source}");
    }
}

#[test]
fn other_atomic_inlines_default_to_first_baseline() {
    for display in ["inline-flex", "inline-grid", "inline-table"] {
        for source in ["auto", "first"] {
            let doc = layout_doc(&format!(
                r#"{STYLE}<div><span id="box" style="display: {display}; flex-direction: column; baseline-source: {source}"><div>X</div><div>X</div></span><span id="anchor">X</span></div>"#
            ));
            let (box_y, box_height) = rect(&doc, "#box");
            let (anchor_y, anchor_height) = rect(&doc, "#anchor");
            assert_eq!(box_height, 40.0);
            assert_eq!(anchor_height, 20.0);
            assert_eq!(box_y, anchor_y, "{display}, {source}");
        }
    }
}

#[test]
fn explicit_baseline_source_uses_scroll_container_content() {
    for (source, line_offset) in [("first", 0.0), ("last", 20.0)] {
        for overflow in ["hidden", "auto", "scroll"] {
            let doc = layout_doc(&format!(
                r#"{STYLE}<div><span id="box" style="width: 100px; baseline-source: {source}; overflow: {overflow}; margin-bottom: 5px; padding-bottom: 10px">X<br>X</span><span id="anchor">X</span></div>"#
            ));
            let (box_y, _) = rect(&doc, "#box");
            let (anchor_y, anchor_height) = rect(&doc, "#anchor");
            assert_eq!(anchor_height, 20.0);
            assert_eq!(anchor_y - box_y, line_offset, "{source}, {overflow}");
        }
    }
}

#[test]
fn overflow_clip_preserves_auto_baseline() {
    for overflow in ["overflow: clip", "overflow-x: clip", "overflow-y: clip"] {
        let doc = layout_doc(&format!(
            r#"{STYLE}<div><span id="box" style="{overflow}; padding-bottom: 30px">X<br>X</span><span id="anchor">X</span></div>"#
        ));
        let (box_y, box_height) = rect(&doc, "#box");
        let (anchor_y, _) = rect(&doc, "#anchor");
        assert_eq!(box_height, 70.0);
        assert_eq!(anchor_y - box_y, 20.0, "{overflow}");
    }
}

#[test]
fn explicit_scroll_container_baselines_are_clamped() {
    for source in ["first", "last"] {
        let doc = layout_doc(&format!(
            r#"{STYLE}<div><span id="box" style="baseline-source: {source}; overflow: hidden; height: 10px">X<br>X</span><span id="anchor" style="height: 10px; width: 10px"></span></div>"#
        ));
        let (box_y, box_height) = rect(&doc, "#box");
        let (anchor_y, anchor_height) = rect(&doc, "#anchor");
        assert_eq!(box_height, 10.0);
        assert_eq!(anchor_height, 10.0);
        assert_eq!(box_y, anchor_y, "{source}");
    }
}

#[test]
fn baseline_source_does_not_override_layout_containment() {
    for source in ["auto", "first", "last"] {
        for contain in ["layout", "content", "strict"] {
            let doc = layout_doc(&format!(
                r#"{STYLE}<div><span id="box" style="baseline-source: {source}; contain: {contain}; width: 100px; height: 40px; margin-bottom: 5px">X<br>X</span><span id="anchor" style="height: 40px; width: 100px; margin-bottom: 5px"></span></div>"#
            ));
            let (box_y, box_height) = rect(&doc, "#box");
            let (anchor_y, anchor_height) = rect(&doc, "#anchor");
            assert_eq!(box_height, 40.0);
            assert_eq!(anchor_height, 40.0);
            assert_eq!(box_y, anchor_y, "{source}, {contain}");
        }
    }
}

#[test]
fn baseline_source_selects_scrollable_button_line() {
    for (source, line_offset) in [("auto", 20.0), ("first", 0.0), ("last", 20.0)] {
        let doc = layout_doc(&format!(
            r#"{STYLE}<div><button id="box" style="font: inherit; padding: 0; border: 0; baseline-source: {source}; overflow: hidden">X<br>X</button><span id="anchor">X</span></div>"#
        ));
        let (box_y, box_height) = rect(&doc, "#box");
        let (anchor_y, anchor_height) = rect(&doc, "#anchor");
        assert_eq!(box_height, 40.0);
        assert_eq!(anchor_height, 20.0);
        assert_eq!(anchor_y - box_y, line_offset, "{source}");
    }
}
