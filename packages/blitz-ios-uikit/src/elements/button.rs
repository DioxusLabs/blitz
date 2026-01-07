//! Button element (UIButton) implementation
//!
//! Maps `<button>` elements to UIButton with tap event handling.

use std::cell::Cell;

use blitz_dom::Node;
use markup5ever::local_name;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{MainThreadMarker, NSString};
use objc2_ui_kit::{UIButton, UIButtonConfiguration, UIControlEvents, UIView};

use crate::events::EventSender;

// =============================================================================
// BlitzButton - Custom UIButton with event bridging
// =============================================================================

/// Ivars for BlitzButton
#[derive(Default)]
pub struct BlitzButtonIvars {
    pub node_id: Cell<usize>,
}

define_class!(
    /// A UIButton subclass that bridges tap events to blitz-dom.
    #[unsafe(super(UIButton))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlitzButton"]
    #[ivars = BlitzButtonIvars]
    pub struct BlitzButton;

    unsafe impl NSObjectProtocol for BlitzButton {}

    impl BlitzButton {
        #[unsafe(method(handleTouchUpInside:))]
        fn handle_touch_up_inside(&self, _sender: &UIButton) {
            let node_id = self.ivars().node_id.get();

            #[cfg(debug_assertions)]
            println!("[BlitzButton] tap event for node_id={}", node_id);

            // TODO: Send click event via EventSender
            // This will be wired up when we implement the full event system
        }
    }
);

impl BlitzButton {
    /// Create a new BlitzButton.
    pub fn new(mtm: MainThreadMarker, node_id: usize) -> Retained<Self> {
        let ivars = BlitzButtonIvars {
            node_id: Cell::new(node_id),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let button: Retained<Self> = unsafe { msg_send![super(this), init] };

        // Add target-action for touch up inside
        unsafe {
            button.addTarget_action_forControlEvents(
                Some(&*button),
                sel!(handleTouchUpInside:),
                UIControlEvents::TouchUpInside,
            );
        }

        button
    }

    /// Get the node ID.
    pub fn node_id(&self) -> usize {
        self.ivars().node_id.get()
    }
}

/// Extract button title from node.
fn get_button_title(node: &Node) -> Option<String> {
    // First check for a value attribute
    if let Some(element_data) = node.element_data() {
        if let Some(value) = element_data.attr(local_name!("value")) {
            return Some(value.to_string());
        }
    }

    // Otherwise, try to get text content from inline layout
    if let Some(element_data) = node.element_data() {
        if let Some(inline_layout) = &element_data.inline_layout_data {
            let text = inline_layout.text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }

    None
}

/// Create a UIButton for a button element.
pub fn create_button(
    mtm: MainThreadMarker,
    node: &Node,
    node_id: usize,
    _event_sender: &EventSender,
) -> Retained<UIView> {
    let button = BlitzButton::new(mtm, node_id);

    // Set initial title
    if let Some(title) = get_button_title(node) {
        let ns_title = NSString::from_str(&title);
        unsafe {
            button.setTitle_forState(Some(&ns_title), objc2_ui_kit::UIControlState::Normal);
        }
    }

    // Check if disabled
    if let Some(element_data) = node.element_data() {
        if element_data.attr(local_name!("disabled")).is_some() {
            unsafe { button.setEnabled(false) };
        }
    }

    // Cast to UIView
    unsafe { Retained::cast(button) }
}

/// Update a UIButton with new node data.
pub fn update_button(view: &UIView, node: &Node) {
    // SAFETY: We only call this for Button element types
    let button: &UIButton = unsafe { std::mem::transmute(view) };

    // Update title
    if let Some(title) = get_button_title(node) {
        let ns_title = NSString::from_str(&title);
        unsafe {
            button.setTitle_forState(Some(&ns_title), objc2_ui_kit::UIControlState::Normal);
        }
    }

    // Update enabled state
    if let Some(element_data) = node.element_data() {
        let disabled = element_data.attr(local_name!("disabled")).is_some();
        unsafe { button.setEnabled(!disabled) };
    }
}
