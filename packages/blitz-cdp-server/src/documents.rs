//! Helpers for enumerating documents and their (recursive) sub-documents,
//! for embedders whose documents can host sub-documents (e.g. tabs or
//! iframe-like elements): each sub-document is an inspectable target of its
//! own.

use blitz_dom::BaseDocument;

/// Collect the id of a document and (recursively) all of its sub-documents
pub fn collect_document_ids(doc: &BaseDocument, ids: &mut Vec<usize>) {
    ids.push(doc.id());
    for node_id in doc.sub_document_node_ids() {
        if let Some(sub_doc) = doc.get_node(node_id).and_then(|node| node.subdoc()) {
            collect_document_ids(&sub_doc.inner(), ids);
        }
    }
}

/// Run `cb` against the document with the given id, searching the document
/// itself and (recursively) its sub-documents. Returns whether it was found.
pub fn with_document_in(
    doc: &mut BaseDocument,
    id: usize,
    cb: &mut dyn FnMut(&mut BaseDocument),
) -> bool {
    if doc.id() == id {
        cb(doc);
        return true;
    }
    for node_id in doc.sub_document_node_ids() {
        if let Some(sub_doc) = doc.get_node_mut(node_id).and_then(|node| node.subdoc_mut())
            && with_document_in(&mut sub_doc.inner_mut(), id, cb)
        {
            return true;
        }
    }
    false
}

/// Find the document with the element picker active — the document itself or
/// (recursively) one of its sub-documents — translating the given
/// document-local page coordinates into the found document's coordinate
/// space (by the sub-document host element's position and scroll offset,
/// mirroring how pointer events are forwarded to sub-documents)
pub fn find_picking_document(doc: &BaseDocument, x: f32, y: f32) -> Option<(usize, f32, f32)> {
    if doc.devtools().element_picker {
        return Some((doc.id(), x, y));
    }
    for node_id in doc.sub_document_node_ids() {
        let Some(node) = doc.get_node(node_id) else {
            continue;
        };
        let pos = node.absolute_position(0.0, 0.0);
        let Some(sub_doc) = node.subdoc() else {
            continue;
        };
        let inner = sub_doc.inner();
        let scroll = inner.viewport_scroll();
        let sub_x = x - pos.x + scroll.x as f32;
        let sub_y = y - pos.y + scroll.y as f32;
        if let Some(found) = find_picking_document(&inner, sub_x, sub_y) {
            return Some(found);
        }
    }
    None
}
