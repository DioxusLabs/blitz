//! Dragging an overlay scrollbar thumb scrolls the container.

use blitz_dom::{DocumentConfig, EventDriver, NoopEventHandler};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};

use std::sync::Arc;

fn pointer_event(x: f32, y: f32, buttons: MouseEventButtons) -> BlitzPointerEvent {
    BlitzPointerEvent {
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
        buttons,
        mods: Default::default(),
        details: PointerDetails::default(),
    }
}

fn drag(doc: &mut HtmlDocument, from: (f32, f32), to: (f32, f32)) {
    let mut driver = EventDriver::new(doc, NoopEventHandler);
    driver.handle_ui_event(UiEvent::PointerDown(pointer_event(
        from.0,
        from.1,
        MouseEventButtons::Primary,
    )));
    driver.handle_ui_event(UiEvent::PointerMove(pointer_event(
        to.0,
        to.1,
        MouseEventButtons::Primary,
    )));
    driver.handle_ui_event(UiEvent::PointerUp(pointer_event(
        to.0,
        to.1,
        MouseEventButtons::None,
    )));
}

fn scroller_doc() -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:auto;">
                <div style="height:1000px;"></div>
            </div>
        </body></html>"#,
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

#[test]
fn dragging_the_thumb_scrolls_the_container() {
    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    // Thumb starts at the top right (16px long, 6px wide, 2px margin).
    drag(&mut doc, (97.0, 8.0), (97.0, 50.0));

    let offset = doc.get_node(scroller).unwrap().scroll_offset.y;
    // 42 thumb px * (900 scroll range / 84 track play) = 450 content px
    assert!(
        (offset - 450.0).abs() < 1.0,
        "expected scroll offset ~450 after dragging the thumb 42px, got {offset}"
    );
}

#[test]
fn dragging_content_does_not_scroll() {
    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    // Same drag, but starting in the content area, left of the thumb.
    drag(&mut doc, (50.0, 8.0), (50.0, 50.0));

    let offset = doc.get_node(scroller).unwrap().scroll_offset.y;
    assert_eq!(offset, 0.0, "content drags must not move the scrollbar");
}

#[test]
fn drag_clamps_at_the_end_of_the_track() {
    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    drag(&mut doc, (97.0, 8.0), (97.0, 500.0));

    let offset = doc.get_node(scroller).unwrap().scroll_offset.y;
    assert!(
        (offset - 900.0).abs() < 1.0,
        "expected scroll offset clamped to 900, got {offset}"
    );
}


#[test]
fn thumb_brightens_on_hover_and_drag() {
    use anyrender::render_to_buffer;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_paint::paint_scene;

    fn thumb_pixel(doc: &mut HtmlDocument) -> [u8; 4] {
        let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| paint_scene(scene, doc.as_mut(), 1.0, 100, 100, 0, 0),
            100,
            100,
        );
        // Scrolled by 50 -> thumb sits around y=8; sample inside it.
        let idx = (8 * 100 + 95) * 4;
        [buffer[idx], buffer[idx + 1], buffer[idx + 2], buffer[idx + 3]]
    }

    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();
    doc.get_node_mut(scroller).unwrap().scroll_offset.y = 50.0;

    let base = thumb_pixel(&mut doc);

    // Hover the thumb
    {
        let mut driver = EventDriver::new(&mut doc, NoopEventHandler);
        driver.handle_ui_event(UiEvent::PointerMove(pointer_event(
            95.0,
            8.0,
            MouseEventButtons::None,
        )));
    }
    let hovered = thumb_pixel(&mut doc);
    assert_ne!(base, hovered, "thumb must change appearance on hover");

    // Start dragging the thumb
    {
        let mut driver = EventDriver::new(&mut doc, NoopEventHandler);
        driver.handle_ui_event(UiEvent::PointerDown(pointer_event(
            95.0,
            8.0,
            MouseEventButtons::Primary,
        )));
    }
    let active = thumb_pixel(&mut doc);
    assert_ne!(hovered, active, "thumb must change appearance while dragged");
}
