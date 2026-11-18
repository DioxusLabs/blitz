use blitz_dom::{Document, EventDriver, EventHandler, NodeId};
use blitz_test_harness::{Harness, HarnessOptions, mouse_pointer_event};
use blitz_traits::events::{
    BlitzWheelDelta, BlitzWheelEvent, DomEvent, DomEventData, EventDispatchSource, EventState,
    UiEvent,
};

struct Coordinates(Vec<(f32, f32)>);

impl EventHandler for &mut Coordinates {
    fn handle_event(
        &mut self,
        _: &[NodeId],
        event: &mut DomEvent,
        _: &mut dyn Document,
        _: &mut EventState,
    ) {
        match &event.data {
            DomEventData::PointerDown(event) | DomEventData::PointerLeave(event) => {
                self.0.push((event.element_x(), event.element_y()));
            }
            DomEventData::Wheel(event) => {
                self.0.push((event.element_x(), event.element_y()));
            }
            _ => {}
        }
    }
}

#[test]
fn pointer_and_wheel_element_coordinates_use_the_local_hit_point() {
    for scale in [1.0, 2.0] {
        for (transform, x, y) in [
            ("none", 35.0, 25.0),
            ("scale(2)", 70.0, 50.0),
            ("rotate(90deg)", -25.0, 35.0),
        ] {
            let mut harness = Harness::from_html_with(
                &format!(
                    "<style>body{{margin:0}}</style><div id=target style='width:100px;height:60px;padding:12px;border:3px solid;transform-origin:0 0;transform:{transform}'><div></div></div>"
                ),
                HarnessOptions {
                    scale,
                    ..Default::default()
                },
            );
            let pointer = mouse_pointer_event(x, y);
            let wheel = BlitzWheelEvent {
                coords: pointer.coords,
                delta: BlitzWheelDelta::Pixels(0.0, 0.0),
                phase: Default::default(),
                source: Default::default(),
                buttons: pointer.buttons,
                mods: pointer.mods,
                element: Default::default(),
            };
            let mut coordinates = Coordinates(Vec::new());
            let mut doc = harness.base_mut();
            let mut driver = EventDriver::new(&mut *doc, &mut coordinates);
            driver.handle_ui_event(UiEvent::PointerDown(pointer));
            driver.handle_ui_event(UiEvent::Wheel(wheel));
            assert_eq!(coordinates.0.len(), 2);
            for (x, y) in coordinates.0 {
                assert!((x - 35.0).abs() < 0.001, "{transform}: {x}");
                assert!((y - 25.0).abs() < 0.001, "{transform}: {y}");
            }
        }
    }
}

#[test]
fn no_hit_keeps_the_existing_element_coordinate_fallback() {
    let mut harness = Harness::from_html("<div id=target style='width:100px;height:60px'></div>");
    let target = harness.node("#target");
    let mut coordinates = Coordinates(Vec::new());
    let mut doc = harness.base_mut();
    let rect = doc.get_client_bounding_rect(target).unwrap();
    EventDriver::new(&mut *doc, &mut coordinates).handle_dom_event(DomEvent::new(
        target,
        DomEventData::PointerLeave(mouse_pointer_event(-10.0, -10.0)),
        EventDispatchSource::Script,
    ));
    assert_eq!(
        coordinates.0,
        [(-10.0 - rect.x as f32, -10.0 - rect.y as f32)]
    );
}
