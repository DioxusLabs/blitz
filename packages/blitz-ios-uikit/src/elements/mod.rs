//! Element mapping from DOM nodes to UIKit views
//!
//! This module handles creating the appropriate UIKit view type for each DOM element.

mod button;
mod checkbox;
mod container;
mod image;
mod input;
pub mod text;

use blitz_dom::Node;
use markup5ever::local_name;
use objc2::rc::Retained;
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::UIView;

use crate::events::EventSender;

pub use button::BlitzButton;
pub use checkbox::BlitzSwitch;
pub use container::BlitzView;
pub use input::BlitzTextField;
pub use text::BlitzLabel;

/// Categories of UIKit views we create
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementType {
    /// Generic container (UIView) - for div, section, article, etc.
    Container,
    /// Text content (UILabel) - for p, span, h1-h6, text nodes
    Text,
    /// Button (UIButton) - for button elements
    Button,
    /// Text input (UITextField) - for input[type=text], input[type=password], etc.
    TextField,
    /// Toggle switch (UISwitch) - for input[type=checkbox]
    Switch,
    /// Image (UIImageView) - for img elements
    ImageView,
    /// Scroll container (UIScrollView) - for overflow: scroll/auto
    ScrollView,
}

/// Determine the appropriate UIKit view type for a DOM node.
///
/// Returns `None` for nodes that shouldn't create a view (e.g., text nodes
/// are handled by their parent element).
pub fn element_type_for_node(node: &Node) -> Option<ElementType> {
    let element_data = node.element_data()?;
    let tag = &element_data.name.local;

    // Check for input types first
    if *tag == local_name!("input") {
        let input_type = element_data
            .attr(local_name!("type"))
            .map(|s| s.to_ascii_lowercase());

        return match input_type.as_deref() {
            Some("checkbox") | Some("radio") => Some(ElementType::Switch),
            Some("text") | Some("password") | Some("email") | Some("number") | Some("tel")
            | Some("url") | Some("search") | None => Some(ElementType::TextField),
            // Hidden inputs don't create views
            Some("hidden") => None,
            // Default to text field for unknown types
            _ => Some(ElementType::TextField),
        };
    }

    // Check for other specific elements
    match tag.as_ref() {
        "button" => Some(ElementType::Button),
        "img" => Some(ElementType::ImageView),
        "textarea" => Some(ElementType::TextField),

        // Text elements - these will contain text content
        "p" | "span" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "label" | "a" | "strong" | "em"
        | "b" | "i" | "u" | "s" | "small" | "mark" | "del" | "ins" | "sub" | "sup" | "code"
        | "pre" | "blockquote" | "q" | "cite" | "abbr" | "time" | "var" | "samp" | "kbd" => {
            Some(ElementType::Text)
        }

        // Elements that shouldn't render
        "script" | "style" | "template" | "head" | "meta" | "link" | "title" | "base"
        | "noscript" => None,

        // Check if element is scrollable (would need computed styles)
        // For now, just treat as container - we'll enhance this later
        _ => {
            // Check if this container is an inline root with actual text content
            // Anonymous inline boxes (wrapping buttons, etc) may be inline roots
            // but have empty text - render those as containers, not labels
            if node.flags.is_inline_root() {
                if let Some(inline_layout) = &element_data.inline_layout_data {
                    // Only render as Text if there's non-whitespace text content
                    if !inline_layout.text.trim().is_empty() {
                        return Some(ElementType::Text);
                    }
                }
            }
            // Default to container for all other elements
            Some(ElementType::Container)
        }
    }
}

/// Create a UIView for the given element type.
pub fn create_view(
    mtm: MainThreadMarker,
    node: &Node,
    element_type: ElementType,
    node_id: usize,
    event_sender: &EventSender,
) -> Retained<UIView> {
    match element_type {
        ElementType::Container => container::create_container(mtm, node_id, event_sender),
        ElementType::Text => text::create_label(mtm, node, node_id),
        ElementType::Button => button::create_button(mtm, node, node_id, event_sender),
        ElementType::TextField => input::create_text_field(mtm, node, node_id, event_sender),
        ElementType::Switch => checkbox::create_switch(mtm, node, node_id, event_sender),
        ElementType::ImageView => image::create_image_view(mtm, node, node_id),
        ElementType::ScrollView => {
            // For now, create a regular container - we'll add scroll support later
            container::create_container(mtm, node_id, event_sender)
        }
    }
}

/// Update an existing view with new node data.
pub fn update_view(view: &UIView, node: &Node, element_type: ElementType, node_id: usize) {
    match element_type {
        ElementType::Text => text::update_label(view, node),
        ElementType::Button => button::update_button(view, node),
        ElementType::TextField => input::update_text_field(view, node),
        ElementType::Switch => checkbox::update_switch(view, node),
        ElementType::ImageView => image::update_image_view(view, node),
        ElementType::Container | ElementType::ScrollView => {
            // Containers don't have content to update
        }
    }
}
