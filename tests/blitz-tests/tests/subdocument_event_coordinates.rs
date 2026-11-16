use blitz_dom::{
    BaseDocument, DocGuard, DocGuardMut, Document, DocumentConfig, EventDriver, NoopEventHandler,
    Point, UserAgentStylesheet,
};
use blitz_html::HtmlDocument;
use blitz_test_harness::pointer_event;
use blitz_traits::events::{
    BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButton, MouseEventButtons, UiEvent,
};
use keyboard_types::Modifiers;
use std::sync::{Arc, Mutex};

struct Child {
    doc: BaseDocument,
    events: Arc<Mutex<Vec<UiEvent>>>,
}

impl Document for Child {
    fn inner(&self) -> DocGuard<'_> {
        DocGuard::Ref(&self.doc)
    }
    fn inner_mut(&mut self) -> DocGuardMut<'_> {
        DocGuardMut::Ref(&mut self.doc)
    }
    fn handle_ui_event(&mut self, event: UiEvent) {
        self.events.lock().unwrap().push(event.clone());
        EventDriver::new(&mut self.doc, NoopEventHandler).handle_ui_event(event);
    }
}

fn document(html: &str) -> BaseDocument {
    HtmlDocument::from_html(
        html,
        DocumentConfig {
            ua_stylesheets: vec![UserAgentStylesheet::default_sheet()],
            ..Default::default()
        },
    )
    .into_inner()
}

fn check(
    host_style: &str,
    ancestor_style: &str,
    scroll_container: bool,
    viewport_scroll: bool,
    x: f32,
    y: f32,
) {
    let mut parent = document(&format!(
        r#"<html><body style="margin:0"><div id="ancestor" style="{ancestor_style}"><div id="host" style="width:200px;height:200px;{host_style}"></div><div style="height:1000px"></div></div></body></html>"#
    ));
    parent.viewport_mut().window_size = (600, 600);
    let host = parent.query_selector("#host").unwrap().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let child = document("<html><body style='margin:0;height:1000px'></body></html>");
    parent.set_sub_document(
        host,
        Box::new(Child {
            doc: child,
            events: events.clone(),
        }),
    );
    parent.resolve(0.0);
    parent
        .subdoc_mut(host)
        .unwrap()
        .inner_mut()
        .set_viewport_scroll(Point { x: 0.0, y: 30.0 });
    if scroll_container {
        let ancestor = parent.query_selector("#ancestor").unwrap().unwrap();
        parent.get_node_mut(ancestor).unwrap().scroll_offset_mut().y = 40.0;
    }
    if viewport_scroll {
        parent.set_viewport_scroll(Point { x: 0.0, y: 40.0 });
    }
    let mut pointer = pointer_event(
        BlitzPointerId::Mouse,
        x,
        y,
        MouseEventButton::Main,
        MouseEventButtons::empty(),
        Modifiers::default(),
    );
    if viewport_scroll {
        pointer.coords.page_y += 40.0;
    }
    assert_eq!(
        parent
            .hit(pointer.page_x(), pointer.page_y())
            .unwrap()
            .node_id,
        host
    );
    let mut driver = EventDriver::new(&mut parent, NoopEventHandler);
    driver.handle_ui_event(UiEvent::PointerMove(pointer.clone()));
    driver.handle_ui_event(UiEvent::PointerDown(pointer.clone()));
    driver.handle_ui_event(UiEvent::Wheel(BlitzWheelEvent {
        delta: BlitzWheelDelta::Pixels(0.0, 10.0),
        coords: pointer.coords,
        buttons: MouseEventButtons::empty(),
        mods: Modifiers::default(),
        element: Default::default(),
    }));
    let events = events.lock().unwrap();
    for event in events.iter() {
        let coords = match event {
            UiEvent::PointerMove(e) | UiEvent::PointerDown(e) => &e.coords,
            UiEvent::Wheel(e) => &e.coords,
            _ => continue,
        };
        assert_eq!(
            (
                coords.client_x,
                coords.client_y,
                coords.page_x,
                coords.page_y
            ),
            (10.0, 10.0, 10.0, 40.0),
            "{event:?}"
        );
    }
    assert!(events.iter().any(|e| matches!(e, UiEvent::Wheel(_))));
}

#[test]
fn translated_host() {
    check(
        "transform:translate(100px,100px)",
        "",
        false,
        false,
        110.0,
        110.0,
    );
}

#[test]
fn scaled_host_uses_local_hit_coordinates() {
    check(
        "transform:scale(2);transform-origin:0 0",
        "",
        false,
        false,
        20.0,
        20.0,
    );
}
#[test]
fn translated_ancestor() {
    check(
        "",
        "transform:translate(100px,100px)",
        false,
        false,
        110.0,
        110.0,
    );
}
#[test]
fn border_and_padding() {
    check(
        "border:20px solid;padding:10px",
        "",
        false,
        false,
        40.0,
        40.0,
    );
}
#[test]
fn scrolled_ancestor() {
    check(
        "margin-top:100px",
        "height:200px;overflow:scroll",
        true,
        false,
        10.0,
        70.0,
    );
}
#[test]
fn scrolled_viewport() {
    check("margin-top:100px", "", false, true, 10.0, 70.0);
}
#[test]
fn offset_wheel() {
    check("margin:100px", "", false, false, 110.0, 110.0);
}
