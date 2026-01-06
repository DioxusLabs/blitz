//! Text element (UILabel) implementation
//!
//! Maps `<p>`, `<span>`, `<h1>`-`<h6>`, and other text elements to UILabel.

use std::cell::Cell;

use blitz_dom::Node;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSString};
use objc2_ui_kit::{UILabel, UIView};

// =============================================================================
// BlitzLabel - Custom UILabel for text content
// =============================================================================

/// Ivars for BlitzLabel
#[derive(Default)]
pub struct BlitzLabelIvars {
    pub node_id: Cell<usize>,
}

define_class!(
    /// A UILabel subclass that tracks its blitz-dom node ID.
    #[unsafe(super(UILabel))]
    #[thread_kind = MainThreadOnly]
    #[name = "BlitzLabel"]
    #[ivars = BlitzLabelIvars]
    pub struct BlitzLabel;

    unsafe impl NSObjectProtocol for BlitzLabel {}
);

impl BlitzLabel {
    /// Create a new BlitzLabel.
    pub fn new(mtm: MainThreadMarker, node_id: usize) -> Retained<Self> {
        let ivars = BlitzLabelIvars {
            node_id: Cell::new(node_id),
        };
        let this = mtm.alloc::<Self>().set_ivars(ivars);
        let label: Retained<Self> = unsafe { msg_send![super(this), init] };

        // Default settings for text rendering
        unsafe {
            // Allow multiple lines
            label.setNumberOfLines(0);
            // Line break mode
            label.setLineBreakMode(objc2_ui_kit::NSLineBreakMode::ByWordWrapping);
        }

        label
    }

    /// Get the node ID.
    pub fn node_id(&self) -> usize {
        self.ivars().node_id.get()
    }
}

/// Extract text content from a DOM node and its children.
fn extract_text_content(node: &Node) -> String {
    let mut text = String::new();
    collect_text_recursive(node, &mut text);
    text
}

/// Recursively collect text from a node and its children.
fn collect_text_recursive(node: &Node, output: &mut String) {
    // If this is a text node, add its content
    if let Some(text_data) = node.text_data() {
        output.push_str(&text_data.content);
        return;
    }

    // Otherwise, recurse into children
    // Note: We need access to the document to get children
    // For now, we'll just handle direct text content
    // The full implementation would need to walk layout_children
}

/// Create a UILabel for a text element.
pub fn create_label(mtm: MainThreadMarker, node: &Node, node_id: usize) -> Retained<UIView> {
    let label = BlitzLabel::new(mtm, node_id);

    // Set initial text content
    update_label_content(&label, node);

    // Cast to UIView
    unsafe { Retained::cast(label) }
}

/// Update a UILabel with new node data.
pub fn update_label(view: &UIView, node: &Node) {
    // Cast back to UILabel
    // SAFETY: We only call this for Text element types which are UILabels
    let label: &UILabel = unsafe { std::mem::transmute(view) };
    update_label_content(label, node);
}

/// Update label content from node.
fn update_label_content(label: &UILabel, node: &Node) {
    // Try to get text content from element's inline layout data
    if let Some(element_data) = node.element_data() {
        if let Some(inline_layout) = &element_data.inline_layout_data {
            // Use the text from inline layout
            let text = &inline_layout.text;
            let ns_text = NSString::from_str(text);
            unsafe { label.setText(Some(&ns_text)) };
            return;
        }
    }

    // Fallback: extract text content recursively
    let text = extract_text_content(node);
    if !text.is_empty() {
        let ns_text = NSString::from_str(&text);
        unsafe { label.setText(Some(&ns_text)) };
    }
}
