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

const ESCAPING_ENTRY_PAGE: &str = r#"<!DOCTYPE html>
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
    let mut harness = Harness::from_html(ESCAPING_ENTRY_PAGE);
    let escaped = harness.node("#escaped");
    let rect = harness.layout_rect("#escaped");
    assert_eq!((rect.x, rect.y), (500.0, 300.0));

    assert_eq!(harness.hit_node(550.0, 350.0), escaped);
    assert_eq!(harness.hit_node(300.0, 200.0), harness.node("#cb"));
    assert_eq!(harness.hit_node(120.0, 20.0), harness.node("#sc"));

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
        Some(escaped)
    );

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
    doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 900.0 });
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let idx = (75 * 100 + 50) * 4;
    assert_eq!([buffer[idx], buffer[idx + 1], buffer[idx + 2]], [0, 128, 0]);
}

#[test]
fn distant_stacking_context_text_is_not_hit() {
    const HTML: &str = r##"<!DOCTYPE html>
    <html><head><style>
        body { margin: 0; font-size: 16px; line-height: 20px; }
        #toc { padding: 10px 40px; }
        #spacer { height: 3000px; }
        h4 { position: relative; z-index: 3; margin: 0 40px; }
        h4 .self-link { position: absolute; top: 0; left: -30px; width: 20px; height: 20px; }
    </style></head>
    <body>
        <div id="toc"><a id="toc-link" href="#heading">Table of contents link</a></div>
        <div id="spacer"></div>
        <h4 id="heading"><span class="secno">1.2.3</span> Heading text <a class="self-link" href="#heading"></a></h4>
    </body></html>"##;

    let mut harness = Harness::from_html(HTML);
    let link = harness.node("#toc-link");
    let rect = harness
        .base()
        .inline_fragment_rects(link)
        .expect("link is a non-atomic inline")[0];
    let (x, y) = (
        (rect.x + rect.width / 2.0) as f32,
        (rect.y + rect.height / 2.0) as f32,
    );

    assert_eq!(harness.hit_node(x, y), link);
    harness.move_mouse_to(x, y);
    assert_eq!(harness.hovered(), Some(link));
    assert_eq!(harness.hit_node(600.0, y), harness.node("#toc"));
    assert_eq!(harness.hit_node(x, 200.0), harness.node("#spacer"));

    let heading = harness.node("h4");
    let (hx, hy) = harness.layout_rect("h4").center();
    let hit = harness.hit_node(hx, hy);
    assert!(hit == heading || hit == harness.node(".secno"));
    let self_link = harness.node(".self-link");
    let (sx, sy) = harness.layout_rect_of(self_link).center();
    assert_eq!(harness.hit_node(sx, sy), self_link);
}

#[test]
fn independently_scrolled_entries_disable_context_pruning() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            #context { isolation: isolate; }
            #scroller { overflow: auto; width: 100px; height: 100px; }
            #entry { position: relative; z-index: 1; margin-top: 200px; }
        </style></head>
        <body>
            <div id="context"><div id="scroller"><div id="entry">entry</div></div></div>
        </body></html>"#,
    );
    let base = harness.base();
    let context = &base.tree()[harness.node("#context")];
    let stacking_context = context.stacking_context.as_ref().unwrap();
    assert!(stacking_context.has_entries());
    assert_eq!(stacking_context.content_bounds, None);
}

#[test]
fn context_bounds_include_entry_transforms() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; }
            #cb { position: relative; width: 500px; height: 100px; }
            #context { isolation: isolate; width: 20px; height: 20px; }
            #entry {
                position: absolute;
                left: 100px;
                top: 0;
                width: 50px;
                height: 50px;
                transform: translateX(200px);
            }
        </style></head>
        <body><div id="cb"><div id="context"><div id="entry"></div></div></div></body></html>"#,
    );
    assert_eq!(harness.hit_node(325.0, 25.0), harness.node("#entry"));
    assert_eq!(harness.hit_node(125.0, 25.0), harness.node("#cb"));
}
