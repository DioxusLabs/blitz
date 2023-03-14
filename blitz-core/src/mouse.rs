use dioxus_native_core::prelude::*;
use dioxus_native_core_macro::partial_derive_state;
use once_cell::sync::Lazy;
use rustc_hash::FxHashSet;
use shipyard::Component;
use taffy::{prelude::Size, Taffy};
use vello::kurbo::{Point, Shape};

use crate::{
    layout::TaffyLayout,
    render::{get_abs_pos, get_shape},
    RealDom,
};

pub(crate) fn get_hovered(
    taffy: &Taffy,
    dom: &RealDom,
    viewport_size: &Size<u32>,
    mouse_pos: Point,
) -> Option<NodeId> {
    let mut hovered: Option<NodeId> = None;
    dom.traverse_depth_first(|node| {
        if node.get::<MouseEffected>().unwrap().0
            && check_hovered(taffy, node, viewport_size, mouse_pos)
        {
            let new_id = node.id();
            if let Some(id) = hovered {
                if node.height() > dom.get(id).unwrap().height() {
                    hovered = Some(new_id);
                }
            } else {
                hovered = Some(new_id);
            }
        }
    });
    hovered
}

pub(crate) fn check_hovered(
    taffy: &Taffy,
    node: NodeRef,
    viewport_size: &Size<u32>,
    mouse_pos: Point,
) -> bool {
    let taffy_node = node.get::<TaffyLayout>().unwrap().node.unwrap();
    let node_layout = taffy.layout(taffy_node).unwrap();
    get_shape(
        node_layout,
        node,
        viewport_size,
        get_abs_pos(*node_layout, taffy, node),
    )
    .contains(mouse_pos)
}

#[derive(Debug, Default, PartialEq, Clone, Component)]
pub(crate) struct MouseEffected(bool);

#[partial_derive_state]
impl State for MouseEffected {
    type ChildDependencies = ();
    type ParentDependencies = ();
    type NodeDependencies = ();
    const NODE_MASK: NodeMaskBuilder<'static> = NodeMaskBuilder::new().with_listeners();

    fn update<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        _: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: &SendAnyMap,
    ) -> bool {
        let new = Self(
            node_view
                .listeners()
                .into_iter()
                .flatten()
                .any(|event| MOUSE_EVENTS.contains(&event)),
        );
        if *self != new {
            *self = new;
            true
        } else {
            false
        }
    }

    fn create<'a>(
        node_view: NodeView<()>,
        node: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        parent: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        children: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        context: &SendAnyMap,
    ) -> Self {
        let mut myself = Self::default();
        myself.update(node_view, node, parent, children, context);
        myself
    }
}

static MOUSE_EVENTS: Lazy<FxHashSet<&'static str>> = Lazy::new(|| {
    [
        "hover",
        "mouseleave",
        "mouseenter",
        "click",
        "mouseup",
        "mouseclick",
        "mouseover",
    ]
    .into_iter()
    .collect()
});
