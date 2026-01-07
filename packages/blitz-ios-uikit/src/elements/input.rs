//! Text input element (UITextField) implementation
//!
//! Maps `<input type="text">`, `<input type="password">`, etc. to UITextField.

use std::cell::Cell;

use blitz_dom::Node;
use markup5ever::local_name;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{MainThreadMarker, NSString};
use objc2_ui_kit::{UIControlEvents, UITextBorderStyle, UITextField, UITextInputTraits, UIView};

use crate::events::{queue_focus_gained, queue_focus_lost, queue_text_changed, EventSender};

// =============================================================================
// BlitzTextField - Custom UITextField with event bridging
// =============================================================================

/// Ivars for BlitzTextField
#[derive(Default)]
pub struct BlitzTextFieldIvars {
    pub node_id: Cell<usize>,
}

define_class!(
    /// A UITextField subclass that bridges input events to blitz-dom.
    #[unsafe(super(UITextField))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlitzTextField"]
    #[ivars = BlitzTextFieldIvars]
    pub struct BlitzTextField;

    unsafe impl NSObjectProtocol for BlitzTextField {}

    impl BlitzTextField {
        /// Called when the text field's text changes.
        #[unsafe(method(handleEditingChanged:))]
        fn handle_editing_changed(&self, _sender: &UITextField) {
            let node_id = self.ivars().node_id.get();

            // Get the current text value
            let value = unsafe {
                self.text()
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            };

            println!("[BlitzTextField] EditingChanged node_id={} value={:?}", node_id, value);
            queue_text_changed(node_id, value);
        }

        /// Called when the text field begins editing (gains focus).
        #[unsafe(method(handleEditingDidBegin:))]
        fn handle_editing_did_begin(&self, _sender: &UITextField) {
            let node_id = self.ivars().node_id.get();
            println!("[BlitzTextField] EditingDidBegin (focus) node_id={}", node_id);
            queue_focus_gained(node_id);
        }

        /// Called when the text field ends editing (loses focus).
        #[unsafe(method(handleEditingDidEnd:))]
        fn handle_editing_did_end(&self, _sender: &UITextField) {
            let node_id = self.ivars().node_id.get();
            println!("[BlitzTextField] EditingDidEnd (blur) node_id={}", node_id);
            queue_focus_lost(node_id);
        }
    }
);

impl BlitzTextField {
    /// Create a new BlitzTextField.
    pub fn new(mtm: MainThreadMarker, node_id: usize) -> Retained<Self> {
        let ivars = BlitzTextFieldIvars {
            node_id: Cell::new(node_id),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let text_field: Retained<Self> = unsafe { msg_send![super(this), init] };

        // Default styling
        unsafe {
            // Add a visible border
            text_field.setBorderStyle(UITextBorderStyle::RoundedRect);
        }

        // Wire up event handlers using target-action pattern
        unsafe {
            // Text changed event
            text_field.addTarget_action_forControlEvents(
                Some(&*text_field),
                sel!(handleEditingChanged:),
                UIControlEvents::EditingChanged,
            );

            // Focus gained event
            text_field.addTarget_action_forControlEvents(
                Some(&*text_field),
                sel!(handleEditingDidBegin:),
                UIControlEvents::EditingDidBegin,
            );

            // Focus lost event
            text_field.addTarget_action_forControlEvents(
                Some(&*text_field),
                sel!(handleEditingDidEnd:),
                UIControlEvents::EditingDidEnd,
            );
        }

        text_field
    }

    /// Get the node ID.
    pub fn node_id(&self) -> usize {
        self.ivars().node_id.get()
    }
}

/// Create a UITextField for an input element.
pub fn create_text_field(
    mtm: MainThreadMarker,
    node: &Node,
    node_id: usize,
    _event_sender: &EventSender,
) -> Retained<UIView> {
    println!("[BlitzTextField] Creating text field for node_id={}", node_id);
    let text_field = BlitzTextField::new(mtm, node_id);

    // Apply initial attributes
    apply_text_field_attributes(&text_field, node);

    // Cast to UIView
    unsafe { Retained::cast(text_field) }
}

/// Update a UITextField with new node data.
pub fn update_text_field(view: &UIView, node: &Node) {
    // SAFETY: We only call this for TextField element types
    let text_field: &UITextField = unsafe { std::mem::transmute(view) };
    apply_text_field_attributes(text_field, node);
}

/// Apply attributes from node to text field.
fn apply_text_field_attributes(text_field: &UITextField, node: &Node) {
    let Some(element_data) = node.element_data() else {
        return;
    };

    // Get input type
    let input_type = element_data
        .attr(local_name!("type"))
        .map(|s| s.to_ascii_lowercase());

    // Set secure text entry for password fields
    unsafe {
        text_field.setSecureTextEntry(input_type.as_deref() == Some("password"));
    }

    // Set placeholder
    if let Some(placeholder) = element_data.attr(local_name!("placeholder")) {
        let ns_placeholder = NSString::from_str(placeholder);
        unsafe { text_field.setPlaceholder(Some(&ns_placeholder)) };
    }

    // Set initial value
    if let Some(value) = element_data.attr(local_name!("value")) {
        let ns_value = NSString::from_str(value);
        unsafe { text_field.setText(Some(&ns_value)) };
    }

    // Set disabled state
    let disabled = element_data.attr(local_name!("disabled")).is_some();
    unsafe { text_field.setEnabled(!disabled) };

    // Set readonly state
    // Note: UITextField doesn't have a direct readonly property,
    // we'd need to use a delegate to prevent editing
    // For now, we treat readonly as disabled
    let readonly = element_data.attr(local_name!("readonly")).is_some();
    if readonly {
        unsafe { text_field.setEnabled(false) };
    }

    // Set keyboard type based on input type
    unsafe {
        let keyboard_type = match input_type.as_deref() {
            Some("email") => objc2_ui_kit::UIKeyboardType::EmailAddress,
            Some("number") => objc2_ui_kit::UIKeyboardType::NumberPad,
            Some("tel") => objc2_ui_kit::UIKeyboardType::PhonePad,
            Some("url") => objc2_ui_kit::UIKeyboardType::URL,
            _ => objc2_ui_kit::UIKeyboardType::Default,
        };
        text_field.setKeyboardType(keyboard_type);
    }
}
