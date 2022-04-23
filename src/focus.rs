use dioxus::{
    native_core::{
        node_ref::{AttributeMask, NodeMask, NodeView},
        state::NodeDepState,
    },
    native_core_macro::sorted_str_slice,
};

#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Focus {
    pub focusable: bool,
    pub pass_focus: bool,
}

impl NodeDepState for Focus {
    type Ctx = ();
    type DepState = ();
    const NODE_MASK: NodeMask =
        NodeMask::new_with_attrs(AttributeMask::Static(FOCUS_ATTRIBUTES)).with_listeners();

    fn reduce(&mut self, node: NodeView<'_>, _sibling: &Self::DepState, _: &Self::Ctx) -> bool {
        let new = Focus {
            focusable: node
                .listeners()
                .iter()
                .any(|l| FOCUS_EVENTS.binary_search(&l.event).is_ok()),
            pass_focus: node
                .attributes()
                .next()
                .filter(|a| a.value.trim() == "true")
                .is_none(),
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
const FOCUS_ATTRIBUTES: &[&str] = &sorted_str_slice!(["dioxus-prevent-default"]);
