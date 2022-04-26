use dioxus::core as dioxus_core;
use dioxus::native_core as dioxus_native_core;
use dioxus::native_core::node_ref::AttributeMask;
use dioxus::native_core::node_ref::NodeMask;
use dioxus::native_core::node_ref::NodeView;
use dioxus::native_core::state::*;
use dioxus::native_core_macro::State;

use crate::focus;
use crate::layout::StretchLayout;
use crate::mouse;
use crate::style;

#[derive(Clone, PartialEq, Default, State, Debug)]
pub(crate) struct BlitzNodeState {
    #[child_dep_state(layout, Rc<RefCell<Stretch>>)]
    pub(crate) layout: StretchLayout,
    #[state]
    pub(crate) style: style::Style,
    #[node_dep_state()]
    pub(crate) focus: focus::Focus,
    #[node_dep_state()]
    pub(crate) mouse_effected: mouse::MouseEffected,
    #[node_dep_state()]
    pub(crate) event_prevented: EventPrevented,
    pub(crate) focused: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum EventPrevented {
    OnKeyDown,
    OnKeyUp,
    OnKeyPress,
    OnMouseDown,
    OnMouseUp,
    OnMouseEnter,
    OnMouseLeave,
    OnClick,
    OnDoubleClick,
    None,
}

impl Default for EventPrevented {
    fn default() -> Self {
        EventPrevented::None
    }
}

impl NodeDepState for EventPrevented {
    type Ctx = ();
    type DepState = ();
    const NODE_MASK: NodeMask =
        NodeMask::new_with_attrs(AttributeMask::Static(&["dioxus-prevent-default"]));

    fn reduce(&mut self, node: NodeView<'_>, _sibling: &Self::DepState, _: &Self::Ctx) -> bool {
        if let Some(prevent_default) = node
            .attributes()
            .find(|a| a.name == "dioxus-prevent-default")
        {
            match prevent_default.value {
                "onkeydown" => *self = EventPrevented::OnKeyDown,
                "onkeyup" => *self = EventPrevented::OnKeyUp,
                "onkeypress" => *self = EventPrevented::OnKeyPress,
                "onmousedown" => *self = EventPrevented::OnMouseDown,
                "onmouseup" => *self = EventPrevented::OnMouseUp,
                "onmouseenter" => *self = EventPrevented::OnMouseEnter,
                "onmouseleave" => *self = EventPrevented::OnMouseLeave,
                "onclick" => *self = EventPrevented::OnClick,
                "ondblclick" => *self = EventPrevented::OnDoubleClick,
                _ => todo!(),
            }
            true
        } else {
            false
        }
    }
}
