use dioxus::{
    native_core::{
        node_ref::{NodeMask, NodeView},
        state::NodeDepState,
    },
    native_core_macro::sorted_str_slice,
};

#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Focusable(pub bool);

impl NodeDepState for Focusable {
    type Ctx = ();
    type DepState = ();
    const NODE_MASK: NodeMask = NodeMask::new().with_listeners();

    fn reduce(&mut self, node: NodeView<'_>, _sibling: &Self::DepState, _: &Self::Ctx) -> bool {
        let new = if node
            .listeners()
            .iter()
            .any(|l| FOCUS_EVENTS.binary_search(&l.event).is_ok())
        {
            Focusable(true)
        } else {
            Focusable(false)
        };
        if *self != new {
            *self = new;
            true
        } else {
            false
        }
    }
}

const FOCUS_EVENTS: &[&str] = &sorted_str_slice!(["keydown", "keyup", "keypress"]);
