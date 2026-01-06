//! DOM tree to UIKit view hierarchy synchronization
//!
//! This module handles walking the DOM tree and creating/updating/removing
//! UIKit views to match the current DOM state.

use blitz_dom::Node;
use blitz_dom::node::NodeData;
use objc2_foundation::NSPoint;
use objc2_ui_kit::{UILabel, UIView};
use style::values::computed::Display as StyloDisplay;

use crate::elements::{ElementType, create_view, element_type_for_node, update_view};
use crate::style::{apply_layout, apply_text_styles, apply_visual_styles};
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
        // Start syncing from root
        sync_node(
            renderer,
            root_element_id,
            &root_view,
            NSPoint::new(0.0, 0.0),
            scale,
            generation,
        );
    }

    // Clean up stale views
    cleanup_stale_views(renderer, generation);
}

/// Sync a single node and its children.
fn sync_node(
    renderer: &mut UIKitRenderer,
    node_id: usize,
    parent_view: &UIView,
    parent_offset: NSPoint,
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

        // Recursively sync children
        sync_children(
            renderer,
            node_id,
            parent_view,
            parent_offset,
            scale,
            generation,
        );
        return;
    };

    // Get or create view
    let mtm = renderer.main_thread_marker();
    let event_sender = renderer.event_sender().clone();

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

    // Apply layout and styles
    apply_layout(&view, node, parent_offset, scale);
    apply_visual_styles(&view, node, scale);

    // Apply text styles if this is a text element
    if element_type == ElementType::Text {
        // SAFETY: Text elements are UILabels
        let label: &UILabel = unsafe { std::mem::transmute(&*view) };
        apply_text_styles(label, node, scale);
    }

    // Calculate child offset
    let layout = node.final_layout;
    let child_offset = NSPoint::new(
        parent_offset.x + layout.location.x as f64 * scale,
        parent_offset.y + layout.location.y as f64 * scale,
    );

    drop(doc);

    // Sync children
    sync_children(renderer, node_id, &view, child_offset, scale, generation);
}

/// Sync children of a node.
fn sync_children(
    renderer: &mut UIKitRenderer,
    parent_node_id: usize,
    parent_view: &UIView,
    parent_offset: NSPoint,
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
            parent_offset,
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
