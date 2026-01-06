//! DOM tree to UIKit view hierarchy synchronization
//!
//! This module handles walking the DOM tree and creating/updating/removing
//! UIKit views to match the current DOM state.

use blitz_dom::Node;
use blitz_dom::node::NodeData;
use objc2_foundation::NSPoint;
use objc2_ui_kit::{UIButton, UILabel, UIView};
use style::values::computed::Display as StyloDisplay;

use crate::elements::{ElementType, create_view, element_type_for_node, update_view};
use crate::style::{apply_button_styles, apply_layout, apply_text_styles, apply_visual_styles};
use crate::{UIKitRenderer, ViewEntry};

/// Synchronize the UIKit view hierarchy with the DOM tree.
pub fn sync_tree(renderer: &mut UIKitRenderer) {
    // Get the next generation counter
    let generation = renderer.next_generation();

    // Borrow the document
    let doc = renderer.doc();
    let doc = doc.borrow();

    // Get the root element (typically <html> or the body)
    // let Some(root_element_id) = doc.root_element().map(|el| el.id) else {
    let root_element_id = doc.root_element().id;

    // Get renderer state
    let root_view = renderer.root_view().to_owned();
    let mtm = renderer.main_thread_marker();
    let scale = renderer.scale();
    let event_sender = renderer.event_sender().clone();

    // Drop the doc borrow before mutable operations
    drop(doc);

    {
        // Start syncing from root with zero frame offset
        sync_node(
            renderer,
            root_element_id,
            &root_view,
            NSPoint::new(0.0, 0.0), // frame_offset: no offset for root
            scale,
            generation,
        );
    }

    // Clean up stale views
    cleanup_stale_views(renderer, generation);
}

/// Sync a single node and its children.
///
/// # Arguments
/// * `frame_offset` - Additional offset to apply when setting this node's frame.
///   This accumulates positions from skipped anonymous blocks. For normal views,
///   children receive (0,0) since their frames are relative to the new view.
fn sync_node(
    renderer: &mut UIKitRenderer,
    node_id: usize,
    parent_view: &UIView,
    frame_offset: NSPoint,
    scale: f64,
    generation: u64,
) {
    let doc = renderer.doc();
    let doc = doc.borrow();

    let Some(node) = doc.get_node(node_id) else {
        return;
    };

    // Skip nodes that shouldn't render
    if should_skip_node(node) {
        drop(doc);
        // Remove view if it exists
        if let Some(entry) = renderer.remove_view(node_id) {
            unsafe { entry.view.removeFromSuperview() };
        }
        return;
    }

    // Determine element type
    let Some(element_type) = element_type_for_node(node) else {
        // This node type doesn't create a view (e.g., text nodes)
        // Text content is handled by the parent element
        drop(doc);

        // Recursively sync children (pass same frame_offset since no view was created)
        sync_children(
            renderer,
            node_id,
            parent_view,
            frame_offset,
            scale,
            generation,
        );
        return;
    };

    // Get or create view
    let mtm = renderer.main_thread_marker();
    let event_sender = renderer.event_sender().clone();

    // Check if this is an anonymous block that we should skip.
    // Anonymous blocks are created by Stylo for layout purposes but don't render
    // their children correctly when mapped to UIKit views.
    // We skip creating a view for them and sync children directly to parent.
    let is_anonymous = node.is_anonymous();

    // Skip anonymous blocks - sync their children directly to parent
    if is_anonymous && element_type == ElementType::Container {
        // Accumulate this skipped node's position into frame_offset
        let layout = node.final_layout;
        let accumulated_offset = NSPoint::new(
            frame_offset.x + layout.location.x as f64,
            frame_offset.y + layout.location.y as f64,
        );

        drop(doc);

        // Sync children directly to the parent view, with accumulated offset
        sync_children(renderer, node_id, parent_view, accumulated_offset, scale, generation);
        return;
    }

    let view = match renderer.view_map_mut().get_mut(&node_id) {
        Some(entry) if entry.element_type == element_type => {
            // Update existing view
            entry.generation = generation;
            update_view(&entry.view, node, element_type, node_id);
            entry.view.clone()
        }
        Some(entry) => {
            // Type changed - need to recreate
            let old_view = entry.view.clone();
            drop(doc);

            unsafe { old_view.removeFromSuperview() };

            let doc = renderer.doc();
            let doc = doc.borrow();
            let node = doc.get_node(node_id).unwrap();

            let new_view = create_view(mtm, node, element_type, node_id, &event_sender);
            unsafe { parent_view.addSubview(&new_view) };

            renderer.insert_view(
                node_id,
                ViewEntry {
                    view: new_view.clone(),
                    element_type,
                    generation,
                },
            );

            new_view
        }
        None => {
            // Create new view
            let new_view = create_view(mtm, node, element_type, node_id, &event_sender);
            unsafe { parent_view.addSubview(&new_view) };

            drop(doc);

            renderer.insert_view(
                node_id,
                ViewEntry {
                    view: new_view.clone(),
                    element_type,
                    generation,
                },
            );

            new_view
        }
    };

    // Re-borrow document for style application
    let doc = renderer.doc();
    let doc = doc.borrow();
    let node = doc.get_node(node_id).unwrap();

    // Apply layout (with frame_offset from any skipped ancestors) and styles
    apply_layout(&view, node, frame_offset, scale);
    apply_visual_styles(&view, node, scale);

    // Apply text styles and content if this is a text element
    if element_type == ElementType::Text {
        // SAFETY: Text elements are UILabels
        let label: &UILabel = unsafe { std::mem::transmute(&*view) };
        apply_text_styles(label, node, scale);

        // Update text content
        crate::elements::text::update_label(&view, node);
    }

    // Apply button styles (title color, background, font)
    if element_type == ElementType::Button {
        // SAFETY: Button elements are UIButtons
        let button: &UIButton = unsafe { std::mem::transmute(&*view) };
        apply_button_styles(button, node, scale);
    }

    drop(doc);

    // Sync children with zero frame_offset since they're relative to this new view
    sync_children(renderer, node_id, &view, NSPoint::new(0.0, 0.0), scale, generation);
}

/// Sync children of a node.
fn sync_children(
    renderer: &mut UIKitRenderer,
    parent_node_id: usize,
    parent_view: &UIView,
    frame_offset: NSPoint,
    scale: f64,
    generation: u64,
) {
    let doc = renderer.doc();
    let doc = doc.borrow();

    let Some(node) = doc.get_node(parent_node_id) else {
        return;
    };

    // Get layout children (includes anonymous boxes)
    let children: Vec<usize> = node
        .layout_children
        .borrow()
        .as_ref()
        .map(|c| c.clone())
        .unwrap_or_default();

    drop(doc);

    // Sync each child
    for child_id in children {
        sync_node(
            renderer,
            child_id,
            parent_view,
            frame_offset,
            scale,
            generation,
        );
    }
}

/// Check if a node should be skipped during rendering.
fn should_skip_node(node: &Node) -> bool {
    // Skip display: none
    if let Some(display) = node.display_style() {
        if display == StyloDisplay::None {
            return true;
        }
    }

    // Skip certain node types
    matches!(node.data, NodeData::Comment | NodeData::Document)
}

/// Remove views that are no longer in the DOM.
fn cleanup_stale_views(renderer: &mut UIKitRenderer, current_generation: u64) {
    let stale_ids: Vec<usize> = renderer
        .view_map_mut()
        .iter()
        .filter(|(_, entry)| entry.generation != current_generation)
        .map(|(&id, _)| id)
        .collect();

    for id in stale_ids {
        if let Some(entry) = renderer.remove_view(id) {
            unsafe { entry.view.removeFromSuperview() };
        }
    }
}
