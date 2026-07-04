//! Dragging an overlay scrollbar thumb scrolls the container.

use blitz_dom::{DocumentConfig, EventDriver, NoopEventHandler};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point, PointerCoords,
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
        element: Point::default(),
        active_pointers: Default::default(),
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

/// Scrolls `scroller` down by `dy` content px through the scroll API, which
/// also shows its overlay scrollbars (thumbs are only interactive while
/// visible).
fn scroll_down(doc: &mut HtmlDocument, scroller: usize, dy: f64) {
    doc.scroll_by(Some(scroller), 0.0, -dy, &mut |_| {});
}

#[test]
fn dragging_the_thumb_scrolls_the_container() {
    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    // Scrolled to 450 (of 900), the 32px thumb sits 34px down its 68px of
    // track play: y in [36, 68] (2px margin).
    scroll_down(&mut doc, scroller, 450.0);
    drag(&mut doc, (97.0, 50.0), (97.0, 16.0));

    let offset = doc.get_node(scroller).unwrap().scroll_offset.y;
    // -34 thumb px * (900 scroll range / 68 track play) = -450 content px
    assert!(
        offset.abs() < 1.0,
        "expected scroll offset ~0 after dragging the thumb up 34px, got {offset}"
    );
}

#[test]
fn dragging_content_does_not_scroll() {
    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    // Same drag, but starting in the content area, left of the thumb.
    scroll_down(&mut doc, scroller, 450.0);
    drag(&mut doc, (50.0, 50.0), (50.0, 16.0));

    let offset = doc.get_node(scroller).unwrap().scroll_offset.y;
    assert_eq!(offset, 450.0, "content drags must not move the scrollbar");
}

#[test]
fn drag_clamps_at_the_end_of_the_track() {
    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    scroll_down(&mut doc, scroller, 450.0);
    drag(&mut doc, (97.0, 50.0), (97.0, 500.0));

    let offset = doc.get_node(scroller).unwrap().scroll_offset.y;
    assert!(
        (offset - 900.0).abs() < 1.0,
        "expected scroll offset clamped to 900, got {offset}"
    );
}

#[test]
fn faded_out_thumb_does_not_capture_drags() {
    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();

    // Never scrolled, so the scrollbars are hidden: a drag across where the
    // thumb would be must fall through to the content.
    drag(&mut doc, (97.0, 8.0), (97.0, 42.0));

    let offset = doc.get_node(scroller).unwrap().scroll_offset.y;
    assert_eq!(offset, 0.0, "hidden thumbs must not capture pointer events");
}

#[test]
fn pointer_hover_does_not_summon_hidden_scrollbars() {
    use anyrender::render_to_buffer;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_paint::paint_scene;

    fn render(doc: &mut HtmlDocument) -> Vec<u8> {
        render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| paint_scene(scene, doc.as_mut(), 1.0, 100, 100, 0, 0),
            100,
            100,
        )
    }

    let mut doc = scroller_doc();
    let at_rest = render(&mut doc);

    // Only scrolling shows overlay scrollbars: hovering where the thumb
    // would be must not fade them in.
    {
        let mut driver = EventDriver::new(&mut doc, NoopEventHandler);
        driver.handle_ui_event(UiEvent::PointerMove(pointer_event(
            95.0,
            8.0,
            MouseEventButtons::None,
        )));
    }
    let hovered = render(&mut doc);

    assert_eq!(at_rest, hovered, "hovering must not paint a thumb");
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
        [
            buffer[idx],
            buffer[idx + 1],
            buffer[idx + 2],
            buffer[idx + 3],
        ]
    }

    let mut doc = scroller_doc();
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();
    scroll_down(&mut doc, scroller, 50.0);

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
    assert_ne!(
        hovered, active,
        "thumb must change appearance while dragged"
    );
}

#[test]
#[ignore = "scrollbar-color is hardcoded to auto until stylo exposes it to the servo engine (servo/stylo#413)"]
fn white_author_thumb_still_signals_hover_and_drag() {
    // A near-white `scrollbar-color` thumb can't get lighter: hover/drag
    // feedback must blend towards the pole with contrast headroom (darken).
    use anyrender::render_to_buffer;
    use anyrender_vello_cpu::VelloCpuImageRenderer;
    use blitz_paint::paint_scene;

    fn thumb_pixel(doc: &mut HtmlDocument) -> [u8; 4] {
        let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| paint_scene(scene, doc.as_mut(), 1.0, 100, 100, 0, 0),
            100,
            100,
        );
        let idx = (8 * 100 + 95) * 4;
        [
            buffer[idx],
            buffer[idx + 1],
            buffer[idx + 2],
            buffer[idx + 3],
        ]
    }

    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0">
            <div id="scroller" style="width:100px; height:100px; overflow-y:auto; scrollbar-color:#ffffff transparent;">
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
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();
    scroll_down(&mut doc, scroller, 50.0);

    let base = thumb_pixel(&mut doc);

    {
        let mut driver = EventDriver::new(&mut doc, NoopEventHandler);
        driver.handle_ui_event(UiEvent::PointerMove(pointer_event(
            95.0,
            8.0,
            MouseEventButtons::None,
        )));
    }
    let hovered = thumb_pixel(&mut doc);
    assert_ne!(
        base, hovered,
        "a white author thumb must still visibly change on hover"
    );

    {
        let mut driver = EventDriver::new(&mut doc, NoopEventHandler);
        driver.handle_ui_event(UiEvent::PointerDown(pointer_event(
            95.0,
            8.0,
            MouseEventButtons::Primary,
        )));
    }
    let active = thumb_pixel(&mut doc);
    assert_ne!(
        hovered, active,
        "a white author thumb must still visibly change while dragged"
    );
}
