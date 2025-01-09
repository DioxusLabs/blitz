use crate::BaseDocument;

pub(crate) fn handle_hover(doc: &mut BaseDocument, _target: usize, x: f32, y: f32) {
    if let Some(node) = doc.get_node_mut(_target) {
        // Toggle hover state on the node
        node.hover();

        doc.set_focus_to(_target);

        doc.set_hover_to(x, y);
    }
}
