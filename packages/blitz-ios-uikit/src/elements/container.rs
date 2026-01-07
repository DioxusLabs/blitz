//! Container element (UIView) implementation
//!
//! Maps `<div>`, `<section>`, `<article>`, and other container elements to UIView.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSet, NSSize};
use objc2_ui_kit::{UIEvent, UITouch, UIView};

use crate::events::EventSender;

// =============================================================================
// BlitzView - Custom UIView with touch handling
// =============================================================================

/// Ivars for BlitzView
#[derive(Default)]
pub struct BlitzViewIvars {
    /// The blitz-dom node ID this view represents
    pub node_id: Cell<usize>,
}

define_class!(
    /// A UIView subclass that tracks touch events and bridges them to blitz-dom.
    #[unsafe(super(UIView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlitzView"]
    #[ivars = BlitzViewIvars]
    pub struct BlitzView;

    unsafe impl NSObjectProtocol for BlitzView {}

    impl BlitzView {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            // Get the first touch
            if let Some(touch) = touches.iter().next() {
                let location = unsafe { touch.locationInView(Some(self)) };
                let node_id = self.ivars().node_id.get();

                // TODO: Send mouse down event via EventSender
                #[cfg(debug_assertions)]
                println!(
                    "[BlitzView] touchesBegan node_id={} at ({}, {})",
                    node_id, location.x, location.y
                );
            }
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            if let Some(touch) = touches.iter().next() {
                let _location = unsafe { touch.locationInView(Some(self)) };
                let _node_id = self.ivars().node_id.get();

                // TODO: Send mouse move event
            }
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            if let Some(touch) = touches.iter().next() {
                let location = unsafe { touch.locationInView(Some(self)) };
                let node_id = self.ivars().node_id.get();

                // TODO: Send mouse up + click events
                #[cfg(debug_assertions)]
                println!(
                    "[BlitzView] touchesEnded node_id={} at ({}, {})",
                    node_id, location.x, location.y
                );
            }
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, _touches: &NSSet<UITouch>, _event: Option<&UIEvent>) {
            // Touch was cancelled (e.g., system gesture took over)
        }
    }
);

impl BlitzView {
    /// Create a new BlitzView with the given frame.
    pub fn new(mtm: MainThreadMarker, frame: NSRect, node_id: usize) -> Retained<Self> {
        let ivars = BlitzViewIvars {
            node_id: Cell::new(node_id),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };

        // Enable user interaction by default
        unsafe { view.setUserInteractionEnabled(true) };

        // Don't clip by default (CSS overflow: visible is the default)
        // Clipping will be enabled by apply_visual_styles for overflow: hidden/scroll/auto
        unsafe { view.setClipsToBounds(false) };

        view
    }

    /// Get the node ID this view represents.
    pub fn node_id(&self) -> usize {
        self.ivars().node_id.get()
    }

    /// Set the node ID.
    pub fn set_node_id(&self, node_id: usize) {
        self.ivars().node_id.set(node_id);
    }
}

/// Create a container view for a DOM node.
pub fn create_container(
    mtm: MainThreadMarker,
    node_id: usize,
    _event_sender: &EventSender,
) -> Retained<UIView> {
    let frame = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(0.0, 0.0),
    );
    let view = BlitzView::new(mtm, frame, node_id);

    // Cast to UIView
    // SAFETY: BlitzView inherits from UIView
    unsafe { Retained::cast(view) }
}
