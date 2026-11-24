//! A Dioxus Native app reads a wheel event's gesture phase and source by
//! downcasting its data to `NativeWheelData`.

use blitz_test_harness::Harness;
use blitz_traits::events::{BlitzWheelPhase, BlitzWheelSource};
use dioxus::prelude::*;
use dioxus_native_dom::NativeWheelData;

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
