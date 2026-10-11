//! Repaint- and relayout-only restyles must update surviving anonymous boxes.
//! Use hover rather than attribute mutation, which would reconstruct them.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

#[test]
fn color_only_restyle_repaints_anonymous_text() {
    use anyrender::render_to_buffer;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_paint::paint_scene;

    let mut doc = make_doc(
        "#owner { display: flex; background: white; } #owner:hover { color: red; }",
        r#"<div id="owner">painted</div>"#,
        true,
    );
    let render = |doc: &mut HtmlDocument| {
        render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| paint_scene(scene, doc, 1.0, 400, 600, 0, 0),
            400,
            600,
        )
    };
    let before = render(&mut doc);
    assert!(before.as_chunks::<4>().0.contains(&[0, 0, 0, 255]));
    assert!(
        !before
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[..3] == [255, 0, 0])
    );
    hover(&mut doc, "#owner");
    doc.resolve(0.0);
    let after = render(&mut doc);
    assert!(
        after
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[..3] == [255, 0, 0])
    );
    assert!(!after.as_chunks::<4>().0.contains(&[0, 0, 0, 255]));
}

fn make_doc(css: &str, body: &str, incremental: bool) -> HtmlDocument {
    let html = format!(
        r#"<!DOCTYPE html><html><head><style>
            body {{ margin: 0; color: black; font-size: 16px; }}
            #owner {{ width: 200px; height: 100px; padding: 10px; }}
            {css}
        </style></head><body>{body}</body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(incremental);
    doc.resolve(0.0);
    doc
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

fn anonymous_children(doc: &HtmlDocument, owner: NodeId) -> Vec<NodeId> {
    doc.get_node(owner)
        .unwrap()
        .layout_children
        .borrow()
        .as_ref()
        .unwrap()
        .iter()
        .copied()
        .filter(|&child| doc.get_node(child).unwrap().is_anonymous())
        .collect()
}

fn color(doc: &HtmlDocument, node: NodeId) -> [u8; 3] {
    let styles = doc.get_node(node).unwrap().primary_styles().unwrap();
    let color = styles.clone_color().into_srgb_legacy();
    let [r, g, b, _] = color.raw_components();
    [r, g, b].map(|component| (component * 255.0).round() as u8)
}

fn hover(doc: &mut HtmlDocument, selector: &str) {
    let target = id(doc, selector);
    let node = doc.get_node(target).unwrap();
    assert!(node.final_layout().size.width > 0.0);
    assert!(node.final_layout().size.height > 0.0);
    let pos = node.absolute_position(1.0, 1.0);
    assert!(doc.set_hover_to(pos.x, pos.y));
    assert_eq!(doc.get_hover_node_id(), Some(target));
}

#[test]
fn color_only_restyle_updates_flex_grid_and_block_text() {
    for display in ["flex", "grid", "block"] {
        for incremental in [false, true] {
            let css = format!(
                "#owner {{ display: {display}; }}
                 #owner:hover {{ color: red; }}"
            );
            let mut doc = make_doc(
                &css,
                r#"<div id="owner">first<div id="wrapped">wrapped</div>last</div>"#,
                incremental,
            );
            let owner = id(&doc, "#owner");
            let before = anonymous_children(&doc, owner);
            assert_eq!(before.len(), 2, "display={display}");
            let caches: Vec<_> = before
                .iter()
                .map(|&child| {
                    let node = doc.get_node(child).unwrap();
                    assert_eq!(color(&doc, child), [0, 0, 0]);
                    assert!(!node.cache().is_empty());
                    node.cache().clone()
                })
                .collect();

            hover(&mut doc, "#owner");
            doc.resolve(0.0);
            let after = anonymous_children(&doc, owner);
            assert_eq!(after.len(), 2);
            assert_eq!(color(&doc, id(&doc, "#wrapped")), [255, 0, 0]);
            for (index, &child) in after.iter().enumerate() {
                assert_eq!(color(&doc, child), [255, 0, 0]);
                if incremental {
                    assert_eq!(child, before[index]);
                    assert_eq!(doc.get_node(child).unwrap().cache(), &caches[index]);
                }
            }
        }
    }
}

#[test]
fn color_only_restyle_updates_flex_button_text() {
    let mut doc = make_doc(
        "#owner { display: inline-flex; } #owner:hover { color: red; }",
        r#"<button id="owner">label</button>"#,
        true,
    );
    let owner = id(&doc, "#owner");
    let before = anonymous_children(&doc, owner);
    assert_eq!(before.len(), 1);
    assert_eq!(color(&doc, before[0]), [0, 0, 0]);
    hover(&mut doc, "#owner");
    doc.resolve(0.0);
    assert_eq!(anonymous_children(&doc, owner), before);
    assert_eq!(color(&doc, before[0]), [255, 0, 0]);
}

#[test]
fn anonymous_text_inherits_restyled_ancestor_color() {
    let mut doc = make_doc(
        "#ancestor { padding: 10px; } #ancestor:hover { color: red; }
         #owner { display: flex; }",
        r#"<div id="ancestor"><div id="owner">inherited</div></div>"#,
        true,
    );
    let owner = id(&doc, "#owner");
    let before = anonymous_children(&doc, owner);
    assert_eq!(before.len(), 1);
    assert_eq!(color(&doc, before[0]), [0, 0, 0]);
    hover(&mut doc, "#ancestor");
    doc.resolve(0.0);
    assert_eq!(color(&doc, owner), [255, 0, 0]);
    assert_eq!(anonymous_children(&doc, owner), before);
    assert_eq!(color(&doc, before[0]), [255, 0, 0]);
}

#[test]
fn pseudo_owned_anonymous_text_updates_without_reconstruction() {
    for display in ["flex", "grid"] {
        let css = format!(
            "#owner::before {{ content: 'generated'; display: {display}; }}
             #owner:hover::before {{ color: red; }}"
        );
        let mut doc = make_doc(&css, r#"<div id="owner"></div>"#, true);
        let pseudo = doc.get_node(id(&doc, "#owner")).unwrap().before().unwrap();
        let before = anonymous_children(&doc, pseudo);
        assert_eq!(before.len(), 1);
        assert_eq!(color(&doc, before[0]), [0, 0, 0]);
        hover(&mut doc, "#owner");
        doc.resolve(0.0);
        assert_eq!(color(&doc, pseudo), [255, 0, 0]);
        assert_eq!(anonymous_children(&doc, pseudo), before);
        assert_eq!(color(&doc, before[0]), [255, 0, 0]);
    }
}

#[test]
fn anonymous_text_tracks_intermediate_transition_frames() {
    let mut doc = make_doc(
        "#owner { display: flex; transition: color 1s linear; }
         #owner:hover { color: red; }",
        r#"<div id="owner">transition</div>"#,
        true,
    );
    let owner = id(&doc, "#owner");
    let before = anonymous_children(&doc, owner);
    assert_eq!(before.len(), 1);
    assert_eq!(color(&doc, before[0]), [0, 0, 0]);
    hover(&mut doc, "#owner");
    doc.resolve(0.0);
    doc.resolve(0.5);
    let halfway = color(&doc, owner);
    assert!(halfway[0] > 0 && halfway[0] < 255, "{halfway:?}");
    assert_eq!(anonymous_children(&doc, owner), before);
    assert_eq!(color(&doc, before[0]), halfway);
    doc.resolve(1.1);
    assert_eq!(anonymous_children(&doc, owner), before);
    assert_eq!(color(&doc, before[0]), [255, 0, 0]);
}

fn line_offset(doc: &HtmlDocument, node: NodeId) -> f32 {
    doc.get_node(node)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap()
        .layout
        .lines()
        .next()
        .unwrap()
        .metrics()
        .offset
}

#[test]
fn relayout_only_restyle_updates_anonymous_text_alignment() {
    let mut offsets = Vec::new();
    for incremental in [false, true] {
        let mut doc = make_doc(
            "#owner { display: flex; flex-direction: column; text-align: left; }
             #owner:hover { text-align: right; }",
            r#"<div id="owner">aligned</div>"#,
            incremental,
        );
        let owner = id(&doc, "#owner");
        let before = anonymous_children(&doc, owner);
        assert_eq!(before.len(), 1);
        assert!(!doc.get_node(before[0]).unwrap().cache().is_empty());
        assert_eq!(line_offset(&doc, before[0]), 0.0);
        hover(&mut doc, "#owner");
        doc.resolve(0.0);
        let after = anonymous_children(&doc, owner);
        assert_eq!(after.len(), 1);
        if incremental {
            assert_eq!(after, before);
        }
        let offset = line_offset(&doc, after[0]);
        assert!(offset > 0.0, "incremental={incremental}");
        offsets.push(offset);
    }
    assert_eq!(offsets[0], offsets[1]);
}

#[test]
fn reconstruction_creates_anonymous_boxes_with_current_styles() {
    let mut doc = make_doc(
        "#owner { display: flex; } #owner:hover { font-size: 32px; color: red; }",
        r#"<div id="owner">reconstructed</div>"#,
        true,
    );
    let owner = id(&doc, "#owner");
    let before = anonymous_children(&doc, owner);
    assert_eq!(before.len(), 1);
    hover(&mut doc, "#owner");
    doc.resolve(0.0);
    let after = anonymous_children(&doc, owner);
    assert_eq!(after.len(), 1);
    assert_ne!(after, before);
    assert!(doc.get_node(before[0]).is_none());
    let styles = doc.get_node(after[0]).unwrap().primary_styles().unwrap();
    assert_eq!(styles.get_font().font_size.computed_size.0.px(), 32.0);
    assert_eq!(color(&doc, after[0]), [255, 0, 0]);
}

#[test]
fn local_restyle_preserves_unchanged_ancestor_and_unrelated_styles() {
    let mut doc = make_doc(
        "#owner, #unrelated { display: flex; }
         #owner:hover { color: red; }",
        r#"<div id="ancestor">lead<div id="owner">changed</div>trail</div>
           <div id="unrelated">unchanged</div>"#,
        true,
    );
    let mut unchanged = anonymous_children(&doc, id(&doc, "#ancestor"));
    assert_eq!(unchanged.len(), 2);
    let unrelated = anonymous_children(&doc, id(&doc, "#unrelated"));
    assert_eq!(unrelated.len(), 1);
    unchanged.extend(unrelated);
    let styles: Vec<_> = unchanged
        .iter()
        .map(|&child| {
            doc.get_node(child)
                .unwrap()
                .primary_styles()
                .unwrap()
                .clone()
        })
        .collect();
    doc.resolve(0.0);
    hover(&mut doc, "#owner");
    doc.resolve(0.0);
    for (&child, old_style) in unchanged.iter().zip(&styles) {
        let current = doc.get_node(child).unwrap().primary_styles().unwrap();
        assert!(std::ptr::eq(&**current, &**old_style));
        assert_eq!(color(&doc, child), [0, 0, 0]);
    }
    let changed = anonymous_children(&doc, id(&doc, "#owner"));
    assert_eq!(changed.len(), 1);
    assert_eq!(color(&doc, changed[0]), [255, 0, 0]);
}

#[test]
fn non_inherited_restyle_retains_anonymous_style_allocation() {
    let mut doc = make_doc(
        "#owner { display: flex; } #owner:hover { width: 250px; }",
        r#"<div id="owner">unchanged</div>"#,
        true,
    );
    let owner = id(&doc, "#owner");
    let before = anonymous_children(&doc, owner);
    assert_eq!(before.len(), 1);
    let old_style = doc
        .get_node(before[0])
        .unwrap()
        .primary_styles()
        .unwrap()
        .clone();
    hover(&mut doc, "#owner");
    doc.resolve(0.0);
    assert_eq!(anonymous_children(&doc, owner), before);
    let current = doc.get_node(before[0]).unwrap().primary_styles().unwrap();
    assert!(std::ptr::eq(&**current, &*old_style));
}
