use std::f64::consts::PI;
use std::vec::IntoIter;

use dioxus::native_core::real_dom::NodeType;
use piet_wgpu::kurbo::{Point, Rect, Shape, Vec2};
use piet_wgpu::{kurbo, Color, Piet, RenderContext, Text, TextLayoutBuilder};
use stretch2::prelude::Size;

use crate::util::{resolve_measure, translate_color, Axis};
use crate::{Dom, DomNode};

pub(crate) fn render(dom: &Dom, piet: &mut Piet) {
    let root = &dom[1];
    let root_layout = root.state.layout.layout.unwrap();
    let background_brush = piet.solid_brush(Color::GRAY);
    piet.fill(
        &Rect {
            x0: root_layout.location.x.into(),
            y0: root_layout.location.y.into(),
            x1: (root_layout.location.x + root_layout.size.width).into(),
            y1: (root_layout.location.y + root_layout.size.height).into(),
        },
        &background_brush,
    );
    render_node(dom, &root, piet);
    match piet.finish() {
        Ok(()) => {}
        Err(e) => {
            println!("{}", e);
        }
    }
}

fn render_node(dom: &Dom, node: &DomNode, piet: &mut Piet) {
    let style = &node.state.style;
    let layout = node.state.layout.layout.unwrap();
    match &node.node_type {
        NodeType::Text { text } => {
            let text_layout = piet
                .text()
                .new_text_layout(text.clone())
                .text_color(translate_color(&style.color.0))
                .build()
                .unwrap();
            let pos = Point::new(layout.location.x as f64, layout.location.y as f64);
            piet.draw_text(&text_layout, pos);
        }
        NodeType::Element { children, .. } => {
            let stroke_brush = piet.solid_brush(translate_color(&style.border.colors.0));
            let shape = RoundedCornerRectangle {
                x0: layout.location.x.into(),
                y0: layout.location.y.into(),
                x1: (layout.location.x + layout.size.width).into(),
                y1: (layout.location.y + layout.size.height).into(),
                top_left_radius: resolve_measure(
                    &style.border.radius.top_left.0,
                    Axis::Min,
                    &layout.size,
                    &Size {
                        width: 100.0,
                        height: 100.0,
                    },
                ),
                top_right_radius: resolve_measure(
                    &style.border.radius.top_right.0,
                    Axis::Min,
                    &layout.size,
                    &Size {
                        width: 100.0,
                        height: 100.0,
                    },
                ),
                bottom_right_radius: resolve_measure(
                    &style.border.radius.bottom_right.0,
                    Axis::Min,
                    &layout.size,
                    &Size {
                        width: 100.0,
                        height: 100.0,
                    },
                ),
                bottom_left_radius: resolve_measure(
                    &style.border.radius.bottom_left.0,
                    Axis::Min,
                    &layout.size,
                    &Size {
                        width: 100.0,
                        height: 100.0,
                    },
                ),
            };
            piet.stroke(&shape, &stroke_brush, 5.0);
            let fill_brush = piet.solid_brush(translate_color(&style.bg_color.0));
            piet.fill(&shape, &fill_brush);
            for child in children {
                render_node(dom, &dom[*child], piet);
            }
        }
        _ => {}
    }
}

/// A rectangle with rounded corners with different radii.
struct RoundedCornerRectangle {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    top_left_radius: f64,
    top_right_radius: f64,
    bottom_left_radius: f64,
    bottom_right_radius: f64,
}

impl RoundedCornerRectangle {
    fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    fn radii(&self) -> [f64; 4] {
        [
            self.top_left_radius,
            self.top_right_radius,
            self.bottom_left_radius,
            self.bottom_right_radius,
        ]
    }
}

impl Shape for RoundedCornerRectangle {
    type PathElementsIter = IntoIter<kurbo::PathEl>;

    fn path_elements(&self, tolerance: f64) -> Self::PathElementsIter {
        use kurbo::PathEl::*;

        let mut paths = Vec::new();
        paths.push(MoveTo(Point::new(self.x0, self.y0 + self.top_left_radius)));

        paths.extend(
            kurbo::Arc {
                center: Point::new(
                    self.x0 + self.top_left_radius,
                    self.y0 + self.top_left_radius,
                ),
                radii: Vec2::new(self.top_left_radius, self.top_left_radius),
                start_angle: PI * 1.0,
                sweep_angle: PI / 2.0,
                x_rotation: 0.0,
            }
            .append_iter(tolerance),
        );
        paths.push(LineTo(Point::new(self.x1 - self.top_right_radius, self.y0)));
        paths.extend(
            kurbo::Arc {
                center: Point::new(
                    self.x1 - self.top_right_radius,
                    self.y0 + self.top_right_radius,
                ),
                radii: Vec2::new(self.top_right_radius, self.top_right_radius),
                start_angle: 1.5 * PI,
                sweep_angle: PI / 2.0,
                x_rotation: 0.0,
            }
            .append_iter(tolerance),
        );
        paths.push(LineTo(Point::new(
            self.x1,
            self.y1 - self.bottom_right_radius,
        )));
        paths.extend(
            kurbo::Arc {
                center: Point::new(
                    self.x1 - self.bottom_right_radius,
                    self.y1 - self.bottom_right_radius,
                ),
                radii: Vec2::new(self.bottom_right_radius, self.bottom_right_radius),
                start_angle: PI * 0.0,
                sweep_angle: PI / 2.0,
                x_rotation: 0.0,
            }
            .append_iter(tolerance),
        );
        paths.push(LineTo(Point::new(
            self.x0 + self.bottom_left_radius,
            self.y1,
        )));
        paths.extend(
            kurbo::Arc {
                center: Point::new(
                    self.x0 + self.bottom_left_radius,
                    self.y1 - self.bottom_left_radius,
                ),
                radii: Vec2::new(self.bottom_left_radius, self.bottom_left_radius),
                start_angle: PI * 0.5,
                sweep_angle: PI / 2.0,
                x_rotation: 0.0,
            }
            .append_iter(tolerance),
        );
        paths.push(ClosePath);

        paths.into_iter()
    }

    fn area(&self) -> f64 {
        self.width() * self.height() - self.radii().iter().map(|r| r * r).sum::<f64>() * PI / 4.0
    }

    fn perimeter(&self, _accuracy: f64) -> f64 {
        2.0 * (self.width() + self.height()) + (0.5 * PI - 2.0) * self.radii().iter().sum::<f64>()
    }

    fn winding(&self, _pt: Point) -> i32 {
        todo!()
    }

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x0, self.y0, self.x1, self.y1)
    }
}
