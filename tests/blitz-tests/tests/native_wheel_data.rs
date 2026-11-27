//! A Dioxus Native app reads a wheel event's gesture phase and source by
//! downcasting its data to `NativeWheelData`.

use blitz_test_harness::Harness;
use blitz_traits::events::{
    BlitzPointerId, BlitzScrollEvent, BlitzWheelDelta, BlitzWheelEvent, BlitzWheelPhase,
    BlitzWheelSource,
};
use dioxus_core::prelude::*;
use dioxus_native_dom::{NativeFocusData, NativeScrollData, NativeTouchData, NativeWheelData};

fn app() -> Element {
    let mut seen = use_signal(String::new);
    rsx! {
        div {
            id: "scroller",
            style: "width: 100px; height: 100px",
            onwheel: move |event| {
                let data = event.data();
                let native = data.downcast::<NativeWheelData>().unwrap();
                seen.set(format!("{:?} {:?}", native.phase(), native.source()));
            },
        }
        div { id: "seen", "{seen}" }
    }
}

#[test]
fn an_onwheel_handler_reads_the_phase_and_source() {
    let mut harness = Harness::from_component(app);
    harness.wheel_gesture_at(
        50.0,
        50.0,
        0.0,
        -10.0,
        BlitzWheelPhase::Started,
        BlitzWheelSource::Finger,
    );
    assert_eq!(harness.text_content("#seen"), "Started Finger");
}

#[test]
fn native_event_data_can_be_constructed_outside_the_crate() {
    let harness = Harness::from_html(r#"<div id="target"></div>"#);
    let base = harness.base();
    let target = base.query_selector("#target").unwrap().unwrap();
    let mut pointer = base
        .get_node(target)
        .unwrap()
        .synthetic_click_event_data(Default::default());
    pointer.id = BlitzPointerId::finger(5);

    let wheel = WheelData::new(NativeWheelData::new(BlitzWheelEvent {
        delta: BlitzWheelDelta::Pixels(0.0, -10.0),
        phase: BlitzWheelPhase::Started,
        source: BlitzWheelSource::Finger,
        coords: pointer.coords,
        buttons: Default::default(),
        mods: Default::default(),
        element: Default::default(),
    }));
    let native_wheel = wheel.downcast::<NativeWheelData>().unwrap();
    assert_eq!(native_wheel.phase(), BlitzWheelPhase::Started);
    assert_eq!(native_wheel.source(), BlitzWheelSource::Finger);

    let scroll = ScrollData::new(NativeScrollData::new(BlitzScrollEvent {
        scroll_top: 30.0,
        scroll_left: 0.0,
        scroll_width: 100,
        scroll_height: 200,
        client_width: 100,
        client_height: 100,
    }));
    assert_eq!(scroll.scroll_top(), 30.0);

    let touch = TouchData::new(NativeTouchData::new(pointer));
    assert_eq!(touch.touches_changed().len(), 1);

    let focus = FocusData::new(NativeFocusData);
    assert!(focus.downcast::<NativeFocusData>().is_some());
}
