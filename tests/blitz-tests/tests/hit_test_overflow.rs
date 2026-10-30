use blitz_dom::{DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn document(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

fn hit(doc: &HtmlDocument, x: f32, y: f32) -> Option<NodeId> {
    doc.hit(x, y).map(|hit| hit.node_id)
}

fn overflowing_child(overflow: &str) -> HtmlDocument {
    document(&format!(
        r#"<!doctype html><style>
        body {{ margin:20px }}
        #clip {{ width:100px;height:60px;padding:10px;border:5px solid;{overflow} }}
        #child {{ width:200px;height:120px }}
        </style><div id="clip"><div id="child"></div></div>"#
    ))
}

#[test]
fn overflow_clips_descendant_hits_at_the_padding_edge() {
    for overflow in ["hidden", "clip", "scroll", "auto"] {
        let doc = overflowing_child(&format!("overflow:{overflow}"));
        let child = id(&doc, "#child");
        assert_eq!(hit(&doc, 45.0, 45.0), Some(child), "{overflow}");
        assert_eq!(hit(&doc, 140.0, 45.0), Some(child), "padding: {overflow}");
        assert_eq!(
            hit(&doc, 147.0, 45.0),
            Some(id(&doc, "#clip")),
            "border: {overflow}"
        );
        assert_ne!(hit(&doc, 155.0, 45.0), Some(child), "{overflow}");
        assert_ne!(hit(&doc, 45.0, 125.0), Some(child), "{overflow}");
    }
}

#[test]
fn visible_overflow_remains_hittable() {
    let doc = overflowing_child("overflow:visible");
    let child = id(&doc, "#child");
    assert_eq!(hit(&doc, 155.0, 45.0), Some(child));
    assert_eq!(hit(&doc, 45.0, 125.0), Some(child));
}

#[test]
fn overflow_clips_only_the_specified_axis() {
    let doc = overflowing_child("overflow-x:clip;overflow-y:visible");
    let child = id(&doc, "#child");
    assert_eq!(hit(&doc, 45.0, 125.0), Some(child));
    assert_ne!(hit(&doc, 155.0, 45.0), Some(child));

    let doc = overflowing_child("overflow-y:clip;overflow-x:visible");
    let child = id(&doc, "#child");
    assert_eq!(hit(&doc, 155.0, 45.0), Some(child));
    assert_ne!(hit(&doc, 45.0, 125.0), Some(child));
}

#[test]
fn scrolled_overflow_clip_stays_at_the_scrollport() {
    let mut doc = document(
        r#"<!doctype html><style>body {margin:20px}
        #clip {width:100px;height:60px;overflow:auto}
        #spacer {height:80px} #child {width:200px;height:120px}
        </style><div id="clip"><div id="spacer"></div><div id="child"></div></div>"#,
    );
    let clip = id(&doc, "#clip");
    *doc.get_node_mut(clip).unwrap().scroll_offset_mut() = blitz_dom::Point { x: 40.0, y: 80.0 };
    let child = id(&doc, "#child");
    assert_eq!(hit(&doc, 30.0, 30.0), Some(child));
    assert_ne!(hit(&doc, 130.0, 30.0), Some(child));
    assert_ne!(hit(&doc, 30.0, 90.0), Some(child));
}

#[test]
fn abspos_escapes_only_clips_below_its_containing_block() {
    for z_index in ["auto", "1", "-1"] {
        for positioned_clip in [false, true] {
            let doc = document(&format!(
                r#"<!doctype html><style>body {{margin:20px}}
                #outer {{position:relative;z-index:0;width:250px;height:200px}}
                #clip {{width:100px;height:60px;overflow:hidden;{}}}
                #child {{position:absolute;left:80px;top:10px;width:100px;height:30px;z-index:{z_index}}}
                </style><div id="outer"><div id="clip"><div id="child"></div></div></div>"#,
                if positioned_clip {
                    "position:relative"
                } else {
                    ""
                },
            ));
            let child = id(&doc, "#child");
            assert_eq!(
                hit(&doc, 110.0, 40.0),
                Some(if z_index == "-1" {
                    id(&doc, "#clip")
                } else {
                    child
                }),
                "{positioned_clip} {z_index}"
            );
            assert_eq!(
                hit(&doc, 150.0, 40.0) == Some(child),
                !positioned_clip,
                "{positioned_clip} {z_index}"
            );
        }
    }
}

#[test]
fn paint_containment_clips_descendant_hits() {
    let doc = overflowing_child("contain:paint");
    let child = id(&doc, "#child");
    assert_eq!(hit(&doc, 45.0, 45.0), Some(child));
    assert_ne!(hit(&doc, 155.0, 45.0), Some(child));
}

#[test]
fn duckduckgo_style_wide_control_is_clipped_by_its_wrapper() {
    let doc = document(
        r#"<!doctype html><style>body {margin:20px}
        #clip {display:inline-block;position:relative;overflow:hidden;width:145px}
        #child {display:block;position:relative;width:140%;height:35px}
        #clip::after {position:absolute;right:10px;top:50%;pointer-events:none;content:" ";width:8px;height:5px}
        </style><div id="clip"><button id="child"></button></div>"#,
    );
    let child = id(&doc, "#child");
    assert_eq!(hit(&doc, 150.0, 30.0), Some(child));
    assert_ne!(hit(&doc, 180.0, 30.0), Some(child));
}

#[test]
fn overflow_clip_margin_extends_only_clip_overflow() {
    for (visual_box, edge) in [
        ("padding-box", 155.0),
        ("border-box", 160.0),
        ("content-box", 145.0),
    ] {
        let doc = overflowing_child(&format!(
            "overflow:clip;overflow-clip-margin:{visual_box} 10px"
        ));
        let child = id(&doc, "#child");
        assert_eq!(hit(&doc, edge - 1.0, 45.0), Some(child), "{visual_box}");
        assert_ne!(hit(&doc, edge + 1.0, 45.0), Some(child), "{visual_box}");
    }
    let doc = overflowing_child("overflow:hidden;overflow-clip-margin:50px");
    assert_ne!(hit(&doc, 155.0, 45.0), Some(id(&doc, "#child")));
}

#[test]
fn transformed_overflow_clip_uses_local_coordinates() {
    let doc = document(
        r#"<!doctype html><style>body {margin:20px}
        #clip {width:100px;height:60px;overflow:hidden;transform-origin:0 0;transform:translate(30px,10px) scale(1.5)}
        #child {width:200px;height:120px}
        </style><div id="clip"><div id="child"></div></div>"#,
    );
    let child = id(&doc, "#child");
    assert_eq!(hit(&doc, 60.0, 40.0), Some(child));
    assert_ne!(hit(&doc, 210.0, 40.0), Some(child));
}

#[test]
fn inline_text_obeys_overflow_clipping() {
    let doc = document(
        r#"<!doctype html><style>body {margin:20px}
        #clip {width:30px;overflow:hidden;white-space:nowrap;font:20px/1 monospace}
        </style><div id="clip"><span id="text">long overflowing text</span></div>"#,
    );
    let text = id(&doc, "#text");
    assert_eq!(doc.element_from_point(25.0, 30.0), Some(text));
    assert_ne!(doc.element_from_point(70.0, 30.0), Some(text));
}

#[test]
fn empty_clipping_box_remains_hittable_and_root_overflow_propagates() {
    let doc = document(
        r#"<!doctype html><style>html {height:40px;overflow:hidden} body {margin:0}
        #child {height:200px} #clip {width:100px;height:30px;overflow:hidden}
        </style><div id="child"><div id="clip"></div></div>"#,
    );
    assert_eq!(hit(&doc, 10.0, 10.0), Some(id(&doc, "#clip")));
    assert_eq!(hit(&doc, 10.0, 100.0), Some(id(&doc, "#child")));
    assert_eq!(doc.element_from_point(10.0, 310.0), None);
}
