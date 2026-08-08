//! Fragment navigation: resolving a URL fragment (the `#...` part) to an element
//! and scrolling the viewport to it.

use blitz_dom::{Document, DocumentConfig, FontContext, ScrollBehavior, ScrollLogicalPosition};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point,
        PointerCoords, PointerDetails, UiEvent,
    },
    shell::{ColorScheme, Viewport},
};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            // A short window so that tall content is scrollable.
            viewport: Some(Viewport::new(800, 200, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            font_ctx: Some(FontContext::new()),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

/// Drive `resolve` repeatedly (advancing the wall-clock-based scroll animation)
/// until the document reports it is no longer animating, or a timeout elapses.
fn drive_until_settled(doc: &mut HtmlDocument) {
    let start = Instant::now();
    while doc.is_animating() {
        std::thread::sleep(Duration::from_millis(8));
        doc.resolve(0.0);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "scroll animation did not settle within 5s"
        );
    }
}

fn click(doc: &mut HtmlDocument, x: f32, y: f32) {
    let event = BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons: MouseEventButtons::from(MouseEventButton::Main),
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    };
    doc.handle_ui_event(UiEvent::PointerDown(event.clone()));
    doc.handle_ui_event(UiEvent::PointerUp(event));
}

const HTML: &str = r#"<html><body style="margin:0">
    <div style="height:1000px"></div>
    <div id="target" style="height:50px"></div>
    <a name="named"></a>
    <div style="height:1000px"></div>
</body></html>"#;

#[test]
fn get_fragment_target_matches_id() {
    let doc = layout_doc(HTML);
    let by_id = doc.query_selector("#target").unwrap().unwrap();
    assert_eq!(doc.get_fragment_target("target"), Some(by_id));
}

#[test]
fn get_fragment_target_matches_named_anchor() {
    let doc = layout_doc(HTML);
    let node = doc.get_fragment_target("named");
    assert!(node.is_some(), "named anchor should be found");
}

#[test]
fn get_fragment_target_returns_none_for_unknown() {
    let doc = layout_doc(HTML);
    assert_eq!(doc.get_fragment_target("does-not-exist"), None);
}

#[test]
fn scroll_to_fragment_scrolls_to_element() {
    let mut doc = layout_doc(HTML);

    let target = doc.query_selector("#target").unwrap().unwrap();
    let target_y = doc.get_node(target).unwrap().final_layout().location.y as f64;
    assert!(target_y > 0.0);

    let found = doc.scroll_to_fragment("target");
    assert!(found);
    assert_eq!(doc.viewport_scroll().y, target_y);
}

#[test]
fn scroll_to_fragment_top_scrolls_to_top() {
    let mut doc = layout_doc(HTML);

    // First scroll down to the target...
    doc.scroll_to_fragment("target");
    assert!(doc.viewport_scroll().y > 0.0);

    // ...then an empty fragment should return us to the top of the document.
    let found = doc.scroll_to_fragment("");
    assert!(found);
    assert_eq!(doc.viewport_scroll().y, 0.0);
}

#[test]
fn scroll_to_fragment_unknown_returns_false() {
    let mut doc = layout_doc(HTML);
    assert!(!doc.scroll_to_fragment("does-not-exist"));
    assert_eq!(doc.viewport_scroll().y, 0.0);
}

#[test]
fn scroll_to_fragment_smooth_animates_to_element() {
    let mut doc = layout_doc(HTML);

    let target = doc.query_selector("#target").unwrap().unwrap();
    let target_y = doc.get_node(target).unwrap().final_layout().location.y as f64;
    assert!(target_y > 0.0);

    // Starting a smooth scroll should register an animation but not jump instantly.
    let found = doc.scroll_to_fragment_smooth("target");
    assert!(found);
    assert!(doc.is_animating(), "smooth scroll should be animating");
    assert!(
        doc.viewport_scroll().y < target_y,
        "smooth scroll should not jump instantly to the target"
    );

    // Once the animation completes, we should have landed exactly on the target and the
    // animation should have settled.
    drive_until_settled(&mut doc);
    assert!(!doc.is_animating());
    assert_eq!(doc.viewport_scroll().y, target_y);
}

#[test]
fn scroll_to_fragment_smooth_top_animates_to_top() {
    let mut doc = layout_doc(HTML);

    // Jump to the target instantly first.
    doc.scroll_to_fragment("target");
    assert!(doc.viewport_scroll().y > 0.0);

    // A smooth scroll back to the top should animate down to 0.
    let found = doc.scroll_to_fragment_smooth("");
    assert!(found);
    assert!(doc.is_animating());

    drive_until_settled(&mut doc);
    assert_eq!(doc.viewport_scroll().y, 0.0);
}

#[test]
fn scroll_to_fragment_smooth_unknown_returns_false() {
    let mut doc = layout_doc(HTML);
    assert!(!doc.scroll_to_fragment_smooth("does-not-exist"));
    assert!(!doc.is_animating());
    assert_eq!(doc.viewport_scroll().y, 0.0);
}

#[test]
fn scroll_into_view_smooth_animates() {
    let mut doc = layout_doc(HTML);

    let target = doc.query_selector("#target").unwrap().unwrap();
    let target_y = doc.get_node(target).unwrap().final_layout().location.y as f64;

    doc.scroll_into_view(
        target,
        ScrollBehavior::Smooth,
        ScrollLogicalPosition::Start,
        ScrollLogicalPosition::Nearest,
    );
    assert!(doc.is_animating());

    drive_until_settled(&mut doc);
    assert_eq!(doc.viewport_scroll().y, target_y);
}

const ALIGNMENT_HTML: &str = r#"<html><body style="margin:0; width:2000px; height:2000px; position:relative">
    <div id="alignment-target" style="position:absolute; left:1000px; top:1000px; width:100px; height:50px"></div>
</body></html>"#;

#[test]
fn scroll_into_view_aligns_each_axis() {
    let mut doc = layout_doc(ALIGNMENT_HTML);
    let target = doc.query_selector("#alignment-target").unwrap().unwrap();

    for (position, expected_x, expected_y) in [
        (ScrollLogicalPosition::Start, 1000.0, 1000.0),
        (ScrollLogicalPosition::Center, 650.0, 925.0),
        (ScrollLogicalPosition::End, 300.0, 850.0),
        (ScrollLogicalPosition::Nearest, 300.0, 850.0),
    ] {
        doc.set_viewport_scroll(blitz_dom::Point::ZERO);
        doc.scroll_into_view(target, ScrollBehavior::Instant, position, position);
        assert_eq!(doc.viewport_scroll().x, expected_x);
        assert_eq!(doc.viewport_scroll().y, expected_y);
    }

    doc.set_viewport_scroll(blitz_dom::Point::ZERO);
    doc.scroll_into_view(
        target,
        ScrollBehavior::Instant,
        ScrollLogicalPosition::Start,
        ScrollLogicalPosition::End,
    );
    assert_eq!(doc.viewport_scroll().x, 300.0);
    assert_eq!(doc.viewport_scroll().y, 1000.0);
}

#[test]
fn scroll_into_view_nearest_preserves_visible_target() {
    let mut doc = layout_doc(ALIGNMENT_HTML);
    let target = doc.query_selector("#alignment-target").unwrap().unwrap();
    doc.set_viewport_scroll(blitz_dom::Point { x: 500.0, y: 900.0 });

    doc.scroll_into_view(
        target,
        ScrollBehavior::Instant,
        ScrollLogicalPosition::Nearest,
        ScrollLogicalPosition::Nearest,
    );

    assert_eq!(doc.viewport_scroll().x, 500.0);
    assert_eq!(doc.viewport_scroll().y, 900.0);
}

#[test]
fn fragment_link_uses_smooth_scroll_behavior_from_root_style() {
    let mut doc = layout_doc(
        r##"<html style="scroll-behavior:smooth"><body style="margin:0">
            <a href="#target" style="display:block;width:100px;height:20px">Target</a>
            <div style="height:1000px"></div>
            <div id="target" style="height:50px"></div>
        </body></html>"##,
    );

    click(&mut doc, 5.0, 5.0);
    assert!(doc.is_animating());
    drive_until_settled(&mut doc);
    assert!(doc.viewport_scroll().y > 0.0);
}

#[test]
fn fragment_link_uses_auto_scroll_behavior_from_root_style() {
    let mut doc = layout_doc(
        r##"<html style="scroll-behavior:auto"><body style="margin:0">
            <a href="#target" style="display:block;width:100px;height:20px">Target</a>
            <div style="height:1000px"></div>
            <div id="target" style="height:50px"></div>
        </body></html>"##,
    );

    click(&mut doc, 5.0, 5.0);
    assert!(!doc.is_animating());
    assert!(doc.viewport_scroll().y > 0.0);
}

/// A page with a nested scrollable container (`#scroller`) whose content overflows.
const SCROLLER_HTML: &str = r#"<html><body style="margin:0">
    <div id="scroller" style="height:100px; overflow:scroll; scroll-behavior:smooth">
        <div style="height:1000px"></div>
    </div>
</body></html>"#;

#[test]
fn scroll_to_sets_element_offset_and_clamps() {
    let mut doc = layout_doc(SCROLLER_HTML);
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    // Scroll the element's own content down by 200px.
    doc.scroll_to(scroller, 0.0, 200.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().y, 200.0);

    // Scrolling far past the end clamps to the element's maximum scroll offset.
    let max = doc
        .get_node(scroller)
        .unwrap()
        .final_layout()
        .scroll_height() as f64;
    assert!(max > 0.0);
    doc.scroll_to(scroller, 0.0, 100_000.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().y, max);

    // Scrolling to a negative offset clamps to 0.
    doc.scroll_to(scroller, 0.0, -100.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().y, 0.0);
}

#[test]
fn scroll_to_smooth_animates_element_offset() {
    let mut doc = layout_doc(SCROLLER_HTML);
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    doc.scroll_to(scroller, 0.0, 200.0, ScrollBehavior::Smooth);
    assert!(doc.is_animating(), "node scroll should be animating");
    assert!(
        doc.get_node(scroller).unwrap().scroll_offset().y < 200.0,
        "smooth scroll should not jump instantly to the target"
    );

    drive_until_settled(&mut doc);
    assert!(!doc.is_animating());
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().y, 200.0);
}

#[test]
fn scroll_by_uses_relative_offsets_and_clamps() {
    let mut doc = layout_doc(SCROLLER_HTML);
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    doc.scroll_to(scroller, 0.0, 100.0, ScrollBehavior::Instant);
    doc.scroll_by(scroller, 0.0, 75.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().y, 175.0);

    doc.scroll_by(scroller, 0.0, -300.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().y, 0.0);
}

#[test]
fn auto_behavior_uses_scroll_behavior_style() {
    let mut doc = layout_doc(SCROLLER_HTML);
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    doc.scroll_to(scroller, 0.0, 200.0, ScrollBehavior::Auto);
    assert!(doc.is_animating());
    drive_until_settled(&mut doc);
    assert_eq!(doc.get_node(scroller).unwrap().scroll_offset().y, 200.0);
}
