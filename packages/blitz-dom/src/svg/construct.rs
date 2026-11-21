//! Turning a root `<svg>` element into an `SvgContext` fragment.
//!
//! This happens in two steps, because of when each piece of information
//! becomes available:
//!
//!  1. [`mark_svg_root`], called from `layout::construct` while it walks
//!     the HTML tree (before Taffy runs), installs a placeholder
//!     `SvgContext` and registers the node so step 2 can find it without a
//!     document-wide scan.
//!  2. [`rebuild_svg_fragments`], called once per layout pass *after*
//!     Taffy layout completes, rebuilds the `SvgContext` for every
//!     registered root that was damaged since the last rebuild. It has to
//!     run this late because `viewBox`/percentage geometry resolve against
//!     the root's content-box size, which Taffy is the one that computes.

use std::sync::Arc;

use blitz_traits::node_id::NodeId;

use crate::BaseDocument;
use crate::layout::damage::{CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC, CONSTRUCT_SVG};
use crate::node::SpecialElementData;

use super::SvgContext;

/// Install `node_id` as a root `<svg>` fragment. Idempotent safe to call
/// on a node that's already an `SvgRoot`.
pub(crate) fn mark_svg_root(doc: &mut BaseDocument, node_id: NodeId) {
    // The subtree below an `SvgRoot` never gets an HTML box:
    // clear whatever HTML box-construction damage its children picked up,
    // so the box-construction pass doesn't try to build them.
    doc.iter_subtree_mut(node_id, |id: NodeId, doc: &mut BaseDocument| {
        doc.nodes[id].remove_damage(CONSTRUCT_BOX | CONSTRUCT_DESCENDENT | CONSTRUCT_FC);
    });

    if let Some(element_data) = doc
        .get_node_mut(node_id)
        .and_then(|node| node.element_data_mut())
    {
        if !matches!(element_data.special_data, SpecialElementData::SvgRoot(_)) {
            element_data.special_data = SpecialElementData::SvgRoot(Arc::new(SvgContext {
                root: node_id,
                viewport: kurbo::Size::ZERO,
            }));
        }
        // An `SvgRoot` paints through `svg::hit_test`/paint (follow-up),
        // never through the ordinary inline-text layout path.
        element_data.take_inline_layout();
    }

    doc.svg_root_nodes.insert(node_id);
    doc.nodes[node_id].insert_damage(CONSTRUCT_SVG);
}

/// Rebuild the `SvgContext` for every registered root `<svg>` whose
/// fragment was invalidated (`CONSTRUCT_SVG` damage) since the last layout
/// pass. Call once per layout, after Taffy has assigned every node its
/// final content-box size.
pub fn rebuild_svg_fragments(doc: &mut BaseDocument) {
    let root_ids: Vec<NodeId> = doc.svg_root_nodes.iter().copied().collect();

    for node_id in root_ids {
        let Some(node) = doc.get_node(node_id) else {
            doc.svg_root_nodes.remove(&node_id);
            continue;
        };
        let Some(element_data) = node.element_data() else {
            doc.svg_root_nodes.remove(&node_id);
            continue;
        };
        if !matches!(element_data.special_data, SpecialElementData::SvgRoot(_)) {
            doc.svg_root_nodes.remove(&node_id);
            continue;
        }
        if !node
            .damage()
            .is_some_and(|damage| damage.contains(CONSTRUCT_SVG))
        {
            continue;
        }

        let layout = node.final_layout();
        let content_width = (layout.size.width
            - layout.border.left
            - layout.border.right
            - layout.padding.left
            - layout.padding.right)
            .max(0.0);
        let content_height = (layout.size.height
            - layout.border.top
            - layout.border.bottom
            - layout.padding.top
            - layout.padding.bottom)
            .max(0.0);
        let viewport = kurbo::Size::new(content_width as f64, content_height as f64);

        let ctx = Arc::new(SvgContext {
            root: node_id,
            viewport,
        });
        doc.get_node_mut(node_id)
            .unwrap()
            .element_data_mut()
            .unwrap()
            .special_data = SpecialElementData::SvgRoot(ctx);
        doc.nodes[node_id].remove_damage(CONSTRUCT_SVG);
    }
}
