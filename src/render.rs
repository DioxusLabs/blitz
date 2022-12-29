use dioxus_native_core::node::NodeType;
use dioxus_native_core::tree::TreeView;
use dioxus_native_core::NodeId;
use taffy::prelude::Size;
use tao::dpi::PhysicalSize;
use vello::kurbo::{Affine, Point, Rect, RoundedRect, Vec2};
use vello::peniko::{Color, Fill, Stroke};
use vello::SceneBuilder;

use crate::text::TextContext;
use crate::util::{translate_color, Axis, Resolve};
use crate::{Dom, DomNode};

const FOCUS_BORDER_WIDTH: f64 = 6.0;

pub(crate) fn render(
    dom: &Dom,
    text_context: &mut TextContext,
    scene_builder: &mut SceneBuilder,
    window_size: PhysicalSize<u32>,
) {
    let root = &dom[NodeId(0)];
    let root_layout = root.state.layout.layout.unwrap();
    let shape = Rect {
        x0: root_layout.location.x.into(),
        y0: root_layout.location.y.into(),
        x1: (root_layout.location.x + root_layout.size.width).into(),
        y1: (root_layout.location.y + root_layout.size.height).into(),
    };
    scene_builder.fill(Fill::NonZero, Affine::IDENTITY, Color::WHITE, None, &shape);
    let viewport_size = Size {
        width: window_size.width,
        height: window_size.height,
    };
    render_node(
        dom,
        root,
        text_context,
        scene_builder,
        Point::ZERO,
        &viewport_size,
    );
}

fn render_node(
    dom: &Dom,
    node: &DomNode,
    text_context: &mut TextContext,
    scene_builder: &mut SceneBuilder,
    location: Point,
    viewport_size: &Size<u32>,
) {
    let state = &node.state;
    let layout = state.layout.layout.unwrap();
    let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
    match &node.node_data.node_type {
        NodeType::Text { text } => {
            let text_color = translate_color(&state.color.0);
            let font_size = 16.0;
            text_context.add(
                scene_builder,
                None,
                font_size,
                Some(text_color),
                Affine::translate(pos.to_vec2() + Vec2::new(0.0, font_size as f64)),
                text,
            )
        }
        NodeType::Element { .. } => {
            let shape = get_shape(node, viewport_size, pos);
            let fill_color = translate_color(&state.bg_color.0);
            if node.state.focused {
                let stroke_color = Color::rgb(1.0, 1.0, 1.0);
                let stroke = Stroke::new(FOCUS_BORDER_WIDTH as f32 / 2.0);
                scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
                let mut smaller_rect = shape.rect();
                smaller_rect.x0 += FOCUS_BORDER_WIDTH / 2.0;
                smaller_rect.x1 -= FOCUS_BORDER_WIDTH / 2.0;
                smaller_rect.y0 += FOCUS_BORDER_WIDTH / 2.0;
                smaller_rect.y1 -= FOCUS_BORDER_WIDTH / 2.0;
                let smaller_shape = RoundedRect::from_rect(smaller_rect, shape.radii());
                let stroke_color = Color::rgb(0.0, 0.0, 0.0);
                scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
                scene_builder.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    fill_color,
                    None,
                    &smaller_shape,
                );
            } else {
                let stroke_color = translate_color(&state.border.colors.top);
                let stroke = Stroke::new(state.border.width.top.resolve(
                    Axis::Min,
                    &node.state.layout.layout.unwrap().size,
                    viewport_size,
                ) as f32);
                scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
                scene_builder.fill(Fill::NonZero, Affine::IDENTITY, fill_color, None, &shape);
            };

            for child in dom.children(node.node_data.node_id).unwrap() {
                render_node(dom, child, text_context, scene_builder, pos, viewport_size);
            }
        }
        _ => {}
    }
}

pub(crate) fn get_shape(node: &DomNode, viewport_size: &Size<u32>, location: Point) -> RoundedRect {
    let state = &node.state;
    let layout = state.layout.layout.unwrap();

    let axis = Axis::Min;
    let rect = layout.size;
    let x: f64 = location.x;
    let y: f64 = location.y;
    let width: f64 = layout.size.width.into();
    let height: f64 = layout.size.height.into();
    let left_border_width = if node.state.focused {
        FOCUS_BORDER_WIDTH
    } else {
        state.border.width.left.resolve(axis, &rect, viewport_size)
    };
    let right_border_width = if node.state.focused {
        FOCUS_BORDER_WIDTH
    } else {
        state.border.width.right.resolve(axis, &rect, viewport_size)
    };
    let top_border_width = if node.state.focused {
        FOCUS_BORDER_WIDTH
    } else {
        state.border.width.top.resolve(axis, &rect, viewport_size)
    };
    let bottom_border_width = if node.state.focused {
        FOCUS_BORDER_WIDTH
    } else {
        state
            .border
            .width
            .bottom
            .resolve(axis, &rect, viewport_size)
    };

    // The stroke is drawn on the outside of the border, so we need to offset the rect by the border width for each side.
    let x_start = x + left_border_width / 2.0;
    let y_start = y + top_border_width / 2.0;
    let x_end = x + width - right_border_width / 2.0;
    let y_end = y + height - bottom_border_width / 2.0;

    RoundedRect::new(
        x_start,
        y_start,
        x_end,
        y_end,
        (
            state
                .border
                .radius
                .top_left
                .0
                .resolve(axis, &rect, viewport_size),
            state
                .border
                .radius
                .top_right
                .0
                .resolve(axis, &rect, viewport_size),
            state
                .border
                .radius
                .bottom_right
                .0
                .resolve(axis, &rect, viewport_size),
            state
                .border
                .radius
                .bottom_left
                .0
                .resolve(axis, &rect, viewport_size),
        ),
    )
}

pub(crate) fn get_abs_pos(node: &DomNode, dom: &Dom) -> Point {
    let mut node_layout = node.state.layout.layout.unwrap().location;
    let mut current = node.node_data.node_id;
    while let Some(parent) = dom.parent(current) {
        let parent_id = parent.node_data.node_id;
        // the root element is positioned at (0, 0)
        if parent_id == NodeId(0) {
            break;
        }
        current = parent.node_data.node_id;
        let parent_layout = parent.state.layout.layout.unwrap();
        node_layout.x += parent_layout.location.x;
        node_layout.y += parent_layout.location.y;
    }
    Point::new(node_layout.x as f64, node_layout.y as f64)
}
