//! Pseudo-element (`::before`/`::after`) styles must continue to update under
//! incremental layout even when the style change doesn't trigger box
//! construction (e.g. a transition of repaint/relayout-only properties).
//!
//! Regression test for hover popups implemented as pseudo-elements with a CSS
//! transition (as seen on old.reddit.com/r/rust's downvote button): with
//! incremental layout enabled the popup appeared but never animated, and its
//! text stayed at the transition's start color (transparent).

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    .arrow { width: 100px; height: 100px; position: relative; }
    .arrow:after {
        display: block;
        visibility: hidden;
        position: absolute;
        margin-left: 32px;
        background-color: #FF7500;
        color: rgba(255,255,255,0);
        content: "Only for content that does not contribute to the discussion.";
        transition: all 0.25s ease;
    }
    .arrow:hover:after { visibility: visible; color: #FFF; margin-left: 48px; }
</style></head>
<body><div class="arrow"></div></body></html>
"#;

fn after_pseudo_styles_after_hover(incremental: bool) -> (f32, f32) {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(incremental);
    doc.resolve(0.0);

    // Hover over the .arrow element and run the transition to completion
    doc.set_hover_to(50.0, 50.0);
    for i in 0..=10 {
        doc.resolve(i as f64 * 0.05);
    }

    let arrow_id = doc.query_selector(".arrow").unwrap().unwrap();
    let after_id = doc
        .get_node(arrow_id)
        .unwrap()
        .after()
        .expect("::after node should exist");
    let after = doc.get_node(after_id).unwrap();
    let styles = after
        .primary_styles()
        .expect("::after node should have styles");
    let color_alpha = styles.clone_color().alpha;
    let x = after.final_layout().location.x;
    (color_alpha, x)
}

#[test]
fn pseudo_element_styles_update_with_incremental_layout() {
    let (non_inc_alpha, non_inc_x) = after_pseudo_styles_after_hover(false);
    let (inc_alpha, inc_x) = after_pseudo_styles_after_hover(true);

    // Sanity-check the non-incremental baseline: transition should have
    // finished, leaving fully-opaque text and the final margin-left of 48px.
    assert_eq!(non_inc_alpha, 1.0);
    assert_eq!(non_inc_x, 48.0);

    // Incremental mode must produce the same result
    assert_eq!(inc_alpha, 1.0);
    assert_eq!(inc_x, 48.0);
}
