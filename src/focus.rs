use std::num::NonZeroU16;

use dioxus::{
    native_core::{
        node_ref::{AttributeMask, NodeMask, NodeView},
        state::NodeDepState,
    },
    native_core_macro::sorted_str_slice,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Ord)]
pub(crate) enum FocusLevel {
    Unfocusable,
    Focusable,
    Ordered(std::num::NonZeroU16),
}

impl FocusLevel {
    pub fn focusable(&self) -> bool {
        match self {
            FocusLevel::Unfocusable => false,
            FocusLevel::Focusable => true,
            FocusLevel::Ordered(_) => true,
        }
    }
}

impl PartialOrd for FocusLevel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (FocusLevel::Unfocusable, FocusLevel::Unfocusable) => Some(std::cmp::Ordering::Equal),
            (FocusLevel::Unfocusable, FocusLevel::Focusable) => Some(std::cmp::Ordering::Less),
            (FocusLevel::Unfocusable, FocusLevel::Ordered(_)) => Some(std::cmp::Ordering::Less),
            (FocusLevel::Focusable, FocusLevel::Unfocusable) => Some(std::cmp::Ordering::Greater),
            (FocusLevel::Focusable, FocusLevel::Focusable) => Some(std::cmp::Ordering::Equal),
            (FocusLevel::Focusable, FocusLevel::Ordered(_)) => Some(std::cmp::Ordering::Greater),
            (FocusLevel::Ordered(_), FocusLevel::Unfocusable) => Some(std::cmp::Ordering::Greater),
            (FocusLevel::Ordered(_), FocusLevel::Focusable) => Some(std::cmp::Ordering::Less),
            (FocusLevel::Ordered(a), FocusLevel::Ordered(b)) => a.partial_cmp(b),
        }
    }
}

impl Default for FocusLevel {
    fn default() -> Self {
        FocusLevel::Unfocusable
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Focus {
    pub pass_focus: bool,
    pub level: FocusLevel,
}

impl NodeDepState for Focus {
    type Ctx = ();
    type DepState = ();
    const NODE_MASK: NodeMask =
        NodeMask::new_with_attrs(AttributeMask::Static(FOCUS_ATTRIBUTES)).with_listeners();

    fn reduce(&mut self, node: NodeView<'_>, _sibling: &Self::DepState, _: &Self::Ctx) -> bool {
        let new = Focus {
            pass_focus: !node
                .attributes()
                .any(|a| a.name == "dioxus-prevent-default" && a.value.trim() == "true"),
            level: if let Some(a) = node.attributes().find(|a| a.name == "tabindex") {
                if let Ok(index) = a.value.parse::<i32>() {
                    if index < 0 {
                        FocusLevel::Unfocusable
                    } else if index == 0 {
                        FocusLevel::Focusable
                    } else {
                        FocusLevel::Ordered(NonZeroU16::new(index as u16).unwrap())
                    }
                } else {
                    FocusLevel::Unfocusable
                }
            } else {
                if node
                    .listeners()
                    .iter()
                    .any(|l| FOCUS_EVENTS.binary_search(&l.event).is_ok())
                {
                    FocusLevel::Focusable
                } else {
                    FocusLevel::Unfocusable
                }
            },
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
const FOCUS_ATTRIBUTES: &[&str] = &sorted_str_slice!(["dioxus-prevent-default", "tabindex"]);
