//! Text element (UILabel) implementation
//!
//! Maps `<p>`, `<span>`, `<h1>`-`<h6>`, and other text elements to UILabel.

use std::cell::Cell;

use blitz_dom::Node;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_foundation::MainThreadMarker;
use objc2_foundation::NSString;
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
    // Use blitz-dom's built-in text_content() which recursively
    // collects text from all child nodes
    node.text_content()
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
    let text: Option<String>;

    // Try to get text content from element's inline layout data (computed by Parley)
    if let Some(element_data) = node.element_data() {
        if let Some(inline_layout) = &element_data.inline_layout_data {
            // Use the text from inline layout
            text = Some(inline_layout.text.clone());
        } else {
            // Fallback: extract text content recursively
            let extracted = extract_text_content(node);
            text = if extracted.is_empty() {
                None
            } else {
                Some(extracted)
            };
        }
    } else {
        text = None;
    }

    if let Some(content) = text {
        let ns_text = NSString::from_str(&content);
        unsafe {
            label.setText(Some(&ns_text));

            // Check if Taffy computed a valid height
            // If not, use UIKit's native text measurement as fallback
            // (Parley may not have access to iOS system fonts)
            let current_frame = label.frame();
            if current_frame.size.height <= 0.0 && current_frame.size.width > 0.0 {
                use objc2_foundation::{NSPoint, NSRect, NSSize};

                // Constrain width to Taffy's computed width, let UIKit measure height
                let constrained_frame = NSRect::new(
                    current_frame.origin,
                    NSSize::new(current_frame.size.width, f64::MAX),
                );
                label.setFrame(constrained_frame);
                label.sizeToFit();

                // Restore position and width (sizeToFit may change them)
                let sized_frame = label.frame();
                let final_frame = NSRect::new(
                    NSPoint::new(current_frame.origin.x, current_frame.origin.y),
                    NSSize::new(current_frame.size.width, sized_frame.size.height),
                );
                label.setFrame(final_frame);
            }
        }
    }
}
