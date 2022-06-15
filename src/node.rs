use crate::layout::StretchLayout;
use dioxus::core as dioxus_core;
use dioxus::native_core as dioxus_native_core;
use dioxus::native_core_macro::sorted_str_slice;
use dioxus::{native_core::state::*, native_core_macro::State};

#[derive(Clone, PartialEq, Default, State, Debug)]
pub(crate) struct BlitzNodeState {
    #[child_dep_state(layout, Rc<RefCell<Stretch>>)]
    pub layout: StretchLayout,
    #[state]
    pub style: crate::style::Style,
    #[node_dep_state()]
    pub focus: crate::focus::Focus,
    pub focused: bool,
    #[node_dep_state()]
    pub prevent_default: PreventDefault,
}

#[derive(PartialEq, Debug, Clone)]
pub(crate) enum PreventDefault {
    Focus,
    KeyPress,
    KeyRelease,
    KeyDown,
    KeyUp,
    MouseDown,
    Click,
    MouseEnter,
    MouseLeave,
    MouseOut,
    Unknown,
    MouseOver,
    ContextMenu,
    Wheel,
    MouseUp,
}

impl Default for PreventDefault {
    fn default() -> Self {
        PreventDefault::Unknown
    }
}

impl NodeDepState for PreventDefault {
    type Ctx = ();

    type DepState = ();

    const NODE_MASK: dioxus_native_core::node_ref::NodeMask =
        dioxus_native_core::node_ref::NodeMask::new_with_attrs(
            dioxus_native_core::node_ref::AttributeMask::Static(&sorted_str_slice!([
                "dioxus-prevent-default"
            ])),
        );

    fn reduce(
        &mut self,
        node: dioxus_native_core::node_ref::NodeView,
        _sibling: &Self::DepState,
        _ctx: &Self::Ctx,
    ) -> bool {
        let new = match node
            .attributes()
            .find(|a| a.name == "dioxus-prevent-default")
            .and_then(|a| a.value.as_text())
        {
            Some("onfocus") => PreventDefault::Focus,
            Some("onkeypress") => PreventDefault::KeyPress,
            Some("onkeyrelease") => PreventDefault::KeyRelease,
            Some("onkeydown") => PreventDefault::KeyDown,
            Some("onkeyup") => PreventDefault::KeyUp,
            Some("onclick") => PreventDefault::Click,
            Some("onmousedown") => PreventDefault::MouseDown,
            Some("onmouseup") => PreventDefault::MouseUp,
            Some("onmouseenter") => PreventDefault::MouseEnter,
            Some("onmouseover") => PreventDefault::MouseOver,
            Some("onmouseleave") => PreventDefault::MouseLeave,
            Some("onmouseout") => PreventDefault::MouseOut,
            Some("onwheel") => PreventDefault::Wheel,
            Some("oncontextmenu") => PreventDefault::ContextMenu,
            _ => return false,
        };
        if new == *self {
            false
        } else {
            *self = new;
            true
        }
    }
}
