use dioxus_native_core::state::*;
use dioxus_native_core_macro::State;

use crate::focus::Focus;
use crate::layout::StretchLayout;
use crate::mouse::MouseEffected;
use crate::style::{BackgroundColor, Border, ForgroundColor};
use dioxus_native_core_macro::sorted_str_slice;

#[derive(Clone, PartialEq, Default, State, Debug)]
pub(crate) struct BlitzNodeState {
    #[node_dep_state()]
    pub(crate) mouse_effected: MouseEffected,
    #[child_dep_state(layout, Arc<Mutex<Taffy>>)]
    pub layout: StretchLayout,
    #[parent_dep_state(color)]
    pub color: ForgroundColor,
    #[node_dep_state()]
    pub bg_color: BackgroundColor,
    #[node_dep_state()]
    pub border: Border,
    #[node_dep_state()]
    pub focus: Focus,
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
    type DepState = ();
    type Ctx = ();

    const NODE_MASK: dioxus_native_core::node_ref::NodeMask =
        dioxus_native_core::node_ref::NodeMask::new_with_attrs(
            dioxus_native_core::node_ref::AttributeMask::Static(&sorted_str_slice!([
                "dioxus-prevent-default"
            ])),
        );

    fn reduce(
        &mut self,
        node: dioxus_native_core::node_ref::NodeView,
        _sibling: (),
        _ctx: &Self::Ctx,
    ) -> bool {
        let new = match node
            .attributes()
            .into_iter()
            .flatten()
            .find(|a| a.attribute.name == "dioxus-prevent-default")
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
