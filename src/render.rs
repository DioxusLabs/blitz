use dioxus::core::ElementId;
use dioxus::native_core::real_dom::NodeType;
use piet_wgpu::kurbo::{Point, Rect, RoundedRect, Vec2};
use piet_wgpu::{Color, Piet, RenderContext, Text, TextLayoutBuilder};
use taffy::prelude::Size;
use tao::dpi::PhysicalSize;

use crate::util::{translate_color, Axis, Resolve};
use crate::{Dom, DomNode};

const FOCUS_BORDER_WIDTH: f64 = 6.0;

pub(crate) fn render(dom: &Dom, piet: &mut Piet, window_size: PhysicalSize<u32>) {
    let root = &dom[1];
    let root_layout = root.state.layout.layout.unwrap();
    let background_brush = piet.solid_brush(Color::WHITE);
    piet.fill(
        &Rect {
            x0: root_layout.location.x.into(),
            y0: root_layout.location.y.into(),
            x1: (root_layout.location.x + root_layout.size.width).into(),
            y1: (root_layout.location.y + root_layout.size.height).into(),
        },
        &background_brush,
    );
    let viewport_size = Size {
        width: window_size.width,
        height: window_size.height,
    };
    render_node(dom, root, piet, Point::ZERO, &viewport_size);
    match piet.finish() {
        Ok(()) => {}
        Err(e) => {
            println!("{}", e);
        }
    }
}

fn render_node(
    dom: &Dom,
    node: &DomNode,
    piet: &mut Piet,
    location: Point,
    viewport_size: &Size<u32>,
) {
    let style = &node.state.style;
    let layout = node.state.layout.layout.unwrap();
    let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
    match &node.node_type {
        NodeType::Text { text } => {
            let text_layout = piet
                .text()
                .new_text_layout(text.clone())
                .text_color(translate_color(&style.color.0))
                .build()
                .unwrap();
            piet.draw_text(&text_layout, pos);
        }
        NodeType::Element { children, .. } => {
            let shape = get_shape(node, viewport_size, pos);
            let fill_brush = piet.solid_brush(translate_color(&style.bg_color.0));
            if node.state.focused {
                let stroke_brush = piet.solid_brush(Color::rgb(1.0, 1.0, 1.0));
                piet.stroke(&shape, &stroke_brush, FOCUS_BORDER_WIDTH / 2.0);
                let mut smaller_rect = shape.rect();
                smaller_rect.x0 += FOCUS_BORDER_WIDTH / 2.0;
                smaller_rect.x1 -= FOCUS_BORDER_WIDTH / 2.0;
                smaller_rect.y0 += FOCUS_BORDER_WIDTH / 2.0;
                smaller_rect.y1 -= FOCUS_BORDER_WIDTH / 2.0;
                let smaller_shape = RoundedRect::from_rect(smaller_rect, shape.radii());
                let stroke_brush = piet.solid_brush(Color::rgb(0.0, 0.0, 0.0));
                piet.stroke(&smaller_shape, &stroke_brush, FOCUS_BORDER_WIDTH / 2.0);
                piet.fill(&smaller_shape, &fill_brush);
            } else {
                let stroke_brush = piet.solid_brush(translate_color(&style.border.colors.top));
                piet.stroke(
                    &shape,
                    &stroke_brush,
                    style.border.width.top.resolve(
                        Axis::Min,
                        &node.state.layout.layout.unwrap().size,
                        viewport_size,
                    ),
                );
                piet.fill(&shape, &fill_brush);
            };

            for child in children {
                render_node(dom, &dom[*child], piet, pos, viewport_size);
            }
        }
        _ => {}
    }
}

pub(crate) fn get_shape(node: &DomNode, viewport_size: &Size<u32>, location: Point) -> RoundedRect {
    let layout = node.state.layout.layout.unwrap();
    let style = &node.state.style;

    let axis = Axis::Min;
    let rect = layout.size;
    let x: f64 = location.x;
    let y: f64 = location.y;
    let width: f64 = layout.size.width.into();
    let height: f64 = layout.size.height.into();
    let left_border_width = if node.state.focused {
        FOCUS_BORDER_WIDTH
    } else {
        style.border.width.left.resolve(axis, &rect, viewport_size)
    };
    let right_border_width = if node.state.focused {
        FOCUS_BORDER_WIDTH
    } else {
        style.border.width.right.resolve(axis, &rect, viewport_size)
    };
    let top_border_width = if node.state.focused {
        FOCUS_BORDER_WIDTH
    } else {
        style.border.width.top.resolve(axis, &rect, viewport_size)
    };
    let bottom_border_width = if node.state.focused {
        FOCUS_BORDER_WIDTH
    } else {
        style
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
            style
                .border
                .radius
                .top_left
                .0
                .resolve(axis, &rect, viewport_size),
            style
                .border
                .radius
                .top_right
                .0
                .resolve(axis, &rect, viewport_size),
            style
                .border
                .radius
                .bottom_right
                .0
                .resolve(axis, &rect, viewport_size),
            style
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
    let mut current = node;
    while let Some(parent_id) = current.parent {
        // the root element is positioned at (0, 0)
        if parent_id == ElementId(0) {
            break;
        }
        let parent = &dom[parent_id];
        current = parent;
        let parent_layout = parent.state.layout.layout.unwrap();
        node_layout.x += parent_layout.location.x;
        node_layout.y += parent_layout.location.y;
    }
    Point::new(node_layout.x as f64, node_layout.y as f64)
}
