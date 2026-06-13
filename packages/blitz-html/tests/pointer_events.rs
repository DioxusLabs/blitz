//! `pointer-events: none` removes an element as a hit-test target: pointer
//! events pass through to whatever is underneath. Descendants are skipped via
//! inheritance, but a descendant that restores `pointer-events: auto` is
//! targetable again (css-ui-4).

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(200, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn hit_id(doc: &HtmlDocument, x: f32, y: f32) -> usize {
    doc.hit(x, y).expect("hit should land somewhere").node_id
}

fn node_id(doc: &HtmlDocument, selector: &str) -> usize {
    doc.query_selector(selector).unwrap().expect(selector)
}

#[test]
fn overlay_with_pointer_events_none_passes_through_to_button() {
    // The titlebar shape: an in-flow button row with a full-size positioned
    // overlay (paints and hit-tests above in-flow content) that sets
    // pointer-events: none.
    let doc = doc(r#"<html><body style="margin:0">
        <div style="position:relative; width:200px; height:36px;">
            <button id="btn" style="position:static; width:44px; height:36px; margin-left:156px; display:block;">x</button>
            <div id="overlay" style="position:absolute; left:0; top:0; right:0; bottom:0; pointer-events:none;">
                <span>Kopuz</span>
            </div>
        </div>
    </body></html>"#);

    let btn = node_id(&doc, "#btn");
    let hit = hit_id(&doc, 178.0, 18.0);
    // The hit may be the button itself or its text child; resolve via ancestors
    let hit_or_ancestor = std::iter::successors(Some(hit), |&id| {
        doc.get_node(id).and_then(|n| n.parent)
    })
    .any(|id| id == btn);
    assert!(
        hit_or_ancestor,
        "expected hit on #btn (node {btn}) but hit node {hit}"
    );
}

#[test]
fn element_with_pointer_events_none_is_not_a_target() {
    let doc = doc(r#"<html><body style="margin:0">
        <div id="under" style="width:100px; height:100px;">
            <div id="blocker" style="width:100px; height:100px; pointer-events:none;"></div>
        </div>
    </body></html>"#);

    let blocker = node_id(&doc, "#blocker");
    let under = node_id(&doc, "#under");
    let hit = hit_id(&doc, 50.0, 50.0);
    assert_ne!(hit, blocker, "pointer-events:none element must not be hit");
    assert_eq!(hit, under, "the event should fall through to the parent");
}

#[test]
fn descendant_restoring_pointer_events_auto_is_targetable() {
    let doc = doc(r#"<html><body style="margin:0">
        <div style="pointer-events:none; width:200px; height:100px;">
            <div id="inner" style="pointer-events:auto; width:50px; height:50px;"></div>
        </div>
    </body></html>"#);

    let inner = node_id(&doc, "#inner");
    assert_eq!(hit_id(&doc, 25.0, 25.0), inner);
}

#[test]
fn text_inside_pointer_events_none_overlay_is_not_a_target() {
    let doc = doc(r#"<html><body style="margin:0">
        <div id="under" style="position:relative; width:200px; height:36px;">
            <div id="overlay" style="position:absolute; left:0; top:0; right:0; bottom:0; pointer-events:none;">
                <span id="label" style="font-size:20px;">KOPUZKOPUZKOPUZ</span>
            </div>
        </div>
    </body></html>"#);

    let overlay = node_id(&doc, "#overlay");
    let label = node_id(&doc, "#label");
    let hit = hit_id(&doc, 30.0, 14.0);
    assert_ne!(hit, overlay);
    assert_ne!(hit, label, "text in a pointer-events:none subtree must not be hit");
}
