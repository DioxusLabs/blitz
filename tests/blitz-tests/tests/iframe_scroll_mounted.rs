//! An `<iframe>`'s MountedData reads and scrolls the frame's document, whose
//! viewport is what scrolls, not the iframe element's own overflow.

use blitz_test_harness::Harness;
use dioxus::html::geometry::PixelsVector2D;
use dioxus::prelude::*;
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

thread_local! {
    static FRAME: RefCell<Option<Rc<MountedData>>> = const { RefCell::new(None) };
}

/// A 100px-tall frame showing a 1000px-tall document: 900px to scroll.
fn app() -> Element {
    rsx! {
        iframe {
            style: "border: 0; width: 100px; height: 100px; display: block",
            srcdoc: "<body style='margin:0'><div style='height:1000px'></div></body>",
            onmounted: |event| FRAME.with(|frame| *frame.borrow_mut() = Some(event.data())),
        }
    }
}

/// Runs a MountedData future, which completes without waiting.
fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("MountedData future did not complete"),
    }
}

fn frame() -> Rc<MountedData> {
    FRAME
        .with(|frame| frame.borrow().clone())
        .expect("iframe not mounted")
}

#[test]
fn an_iframes_scroll_size_is_its_documents() {
    let _harness = Harness::from_component(app);
    let size = ready(frame().get_scroll_size()).unwrap();
    assert_eq!(size.height, 900.0);
}

#[test]
fn scrolling_an_iframe_scrolls_its_document() {
    let mut harness = Harness::from_component(app);
    ready(frame().scroll(PixelsVector2D::new(0.0, 300.0), ScrollBehavior::Instant)).unwrap();
    harness.pump();
    let offset = ready(frame().get_scroll_offset()).unwrap();
    assert_eq!(offset.y, 300.0);
}
