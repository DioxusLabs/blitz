//! Scroll view element (UIScrollView) implementation
//!
//! Used for scrollable containers (body, overflow: scroll/auto).
//! Handles keyboard avoidance and dismissal.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSSize};
use objc2_ui_kit::{
    UIGestureRecognizer, UIScrollView, UIScrollViewKeyboardDismissMode, UITapGestureRecognizer,
    UIView,
};

use crate::events::EventSender;

// =============================================================================
// BlitzScrollView - Custom UIScrollView with keyboard handling
// =============================================================================

/// Ivars for BlitzScrollView
#[derive(Default)]
pub struct BlitzScrollViewIvars {
    /// The blitz-dom node ID this view represents
    pub node_id: Cell<usize>,
}

define_class!(
    /// A UIScrollView subclass for scrollable containers.
    #[unsafe(super(UIScrollView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlitzScrollView"]
    #[ivars = BlitzScrollViewIvars]
    pub struct BlitzScrollView;

    unsafe impl NSObjectProtocol for BlitzScrollView {}

    impl BlitzScrollView {
        /// Handle tap gesture to dismiss keyboard
        #[unsafe(method(handleTapToDismissKeyboard:))]
        fn handle_tap_to_dismiss_keyboard(&self, _gesture: &UITapGestureRecognizer) {
            // End editing on the entire view hierarchy, which dismisses the keyboard
            unsafe {
                self.endEditing(true);
            }
        }
    }
);

impl BlitzScrollView {
    /// Create a new BlitzScrollView.
    pub fn new(mtm: MainThreadMarker, node_id: usize) -> Retained<Self> {
        let ivars = BlitzScrollViewIvars {
            node_id: Cell::new(node_id),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let scroll_view: Retained<Self> = unsafe { msg_send![super(this), init] };

        // Enable scrolling
        unsafe {
            scroll_view.setScrollEnabled(true);
            scroll_view.setBounces(true);
            scroll_view.setShowsVerticalScrollIndicator(true);
            scroll_view.setShowsHorizontalScrollIndicator(false);

            // Dismiss keyboard when dragging
            scroll_view.setKeyboardDismissMode(UIScrollViewKeyboardDismissMode::Interactive);

            // Enable automatic keyboard avoidance
            // This tells iOS to automatically adjust content insets for safe areas
            scroll_view.setContentInsetAdjustmentBehavior(
                objc2_ui_kit::UIScrollViewContentInsetAdjustmentBehavior::Always,
            );

            // Ensure scroll indicators also adjust
            scroll_view.setAutomaticallyAdjustsScrollIndicatorInsets(true);
        }

        // Add tap gesture recognizer to dismiss keyboard when tapping outside inputs
        unsafe {
            let tap_gesture =
                UITapGestureRecognizer::initWithTarget_action(
                    mtm.alloc(),
                    Some(&*scroll_view),
                    Some(sel!(handleTapToDismissKeyboard:)),
                );

            // Don't cancel touches in view - allow buttons/inputs to still work
            tap_gesture.setCancelsTouchesInView(false);

            scroll_view.addGestureRecognizer(&tap_gesture);
        }

        scroll_view
    }

    /// Get the node ID.
    pub fn node_id(&self) -> usize {
        self.ivars().node_id.get()
    }

    /// Set the content size for scrolling.
    pub fn set_content_size(&self, width: f64, height: f64) {
        unsafe {
            self.setContentSize(NSSize::new(width, height));
        }
    }
}

/// Create a scroll view for a scrollable container.
pub fn create_scroll_view(
    mtm: MainThreadMarker,
    node_id: usize,
    _event_sender: &EventSender,
) -> Retained<UIView> {
    println!("[BlitzScrollView] Creating scroll view for node_id={}", node_id);

    let scroll_view = BlitzScrollView::new(mtm, node_id);

    // Cast to UIView
    unsafe { Retained::cast(scroll_view) }
}

/// Update scroll view content size based on children's layout.
pub fn update_scroll_view_content_size(view: &UIView, content_width: f64, content_height: f64) {
    // SAFETY: We only call this for scroll view types
    let scroll_view: &BlitzScrollView = unsafe { std::mem::transmute(view) };
    scroll_view.set_content_size(content_width, content_height);
}
