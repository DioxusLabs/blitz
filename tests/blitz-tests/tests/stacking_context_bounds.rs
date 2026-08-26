//! Hit testing and paint culling only descend into a node when the point (or
//! viewport) intersects its overflow bounds. Those bounds must therefore cover
//! everything painted as part of the node's subtree, including out-of-flow
//! descendants whose containing block is an *ancestor* of the node (e.g. an
//! abspos box inside a non-positioned `opacity` stacking context): such a box
//! is still a stacked entry of that context, but its geometry is accounted for
//! at the containing block rather than at the context root.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_test_harness::Harness;
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use markup5ever::{QualName, local_name, ns};
use std::sync::Arc;

const PAGE: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    #cb { position: relative; width: 600px; height: 400px; }
    #sc { opacity: 0.5; margin-left: 100px; width: 50px; height: 50px; background: red; }
    #escaped { position: absolute; right: 0; bottom: 0; width: 100px; height: 100px; background: green; }
</style></head>
<body>
    <div id="cb"><div id="sc"><div id="escaped"></div></div></div>
</body></html>"#;

fn style_attr() -> QualName {
    QualName::new(None, ns!(), local_name!("style"))
}

fn set_style(harness: &mut Harness, node: NodeId, style: &str) {
    harness
        .base_mut()
        .mutate()
        .set_attribute(node, style_attr(), style);
    harness.pump();
}

#[test]
fn abspos_escaping_stacking_context_is_hit() {
    let mut harness = Harness::from_html(PAGE);
    let escaped = harness.node("#escaped");
    let rect = harness.layout_rect("#escaped");
    assert_eq!((rect.x, rect.y), (500.0, 300.0), "positioned against #cb");

    // Far outside #sc's own box, but inside the escaped entry
    assert_eq!(harness.hit_node(550.0, 350.0), escaped);
    harness.move_mouse_to(550.0, 350.0);
    assert_eq!(harness.hovered(), Some(escaped));

    // Nothing else leaks out of #sc
    assert_eq!(harness.hit_node(300.0, 200.0), harness.node("#cb"));
    assert_eq!(harness.hit_node(120.0, 20.0), harness.node("#sc"));

    // Moving the containing block moves the entry; the bounds must follow
    let cb = harness.node("#cb");
    set_style(
        &mut harness,
        cb,
        "position: relative; width: 300px; height: 200px",
    );
    let rect = harness.layout_rect("#escaped");
    assert_eq!((rect.x, rect.y), (200.0, 100.0));
    assert_eq!(harness.hit_node(250.0, 150.0), escaped);
    assert_ne!(
        harness.hit(550.0, 350.0).map(|hit| hit.node_id),
        Some(escaped),
        "old position is no longer the entry"
    );

    // As does moving the entry itself
    set_style(&mut harness, escaped, "left: 0; bottom: 0; top: auto");
    let rect = harness.layout_rect("#escaped");
    assert_eq!((rect.x, rect.y), (0.0, 100.0));
    assert_eq!(harness.hit_node(50.0, 150.0), escaped);
    assert_ne!(
        harness.hit(250.0, 150.0).map(|hit| hit.node_id),
        Some(escaped)
    );
}

#[test]
fn abspos_escaping_scrolled_out_stacking_context_is_painted() {
    const HTML: &str = r#"<!DOCTYPE html>
    <html><head><style>
        body { margin: 0; background: white; }
        #cb { position: relative; height: 1000px; }
        #sc { isolation: isolate; width: 20px; height: 20px; background: red; }
        #escaped { position: absolute; top: 950px; left: 0; width: 100px; height: 50px; background: green; }
    </style></head>
    <body><div id="cb"><div id="sc"><div id="escaped"></div></div></div></body></html>"#;

    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    // #sc is now 900px above the viewport; #escaped fills its bottom half
    doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 900.0 });
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (75 * 100 + 50) * 4;
    let pixel = [buffer[idx], buffer[idx + 1], buffer[idx + 2]];
    assert_eq!(pixel, [0, 128, 0]);
}
