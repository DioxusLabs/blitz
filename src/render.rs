use dioxus::native_core::real_dom::NodeType;
use druid_shell::kurbo::Point;
use druid_shell::piet::kurbo::RoundedRect;
use druid_shell::piet::{Color, Piet, RenderContext, Text, TextLayoutBuilder};

use crate::{Dom, DomNode};

pub fn render(dom: &Dom, piet: &mut Piet) {
    println!("rendering");
    render_node(dom, &dom[1], piet);
}

fn render_node(dom: &Dom, node: &DomNode, piet: &mut Piet) {
    let layout = node.up_state.layout.unwrap();
    match &node.node_type {
        NodeType::Text { text } => {
            let text_layout = piet
                .text()
                .new_text_layout(text.clone())
                .text_color(Color::WHITE)
                .build()
                .unwrap();
            let pos = Point::new(layout.location.x as f64, layout.location.y as f64);
            piet.draw_text(&text_layout, pos);
        }
        NodeType::Element { children, .. } => {
            let brush = piet.solid_brush(Color::BLUE);
            piet.stroke(
                RoundedRect::new(
                    layout.location.x.into(),
                    layout.location.y.into(),
                    (layout.location.x + layout.size.width).into(),
                    (layout.location.y + layout.size.height).into(),
                    10.0,
                ),
                &brush,
                5.0,
            );
            for child in children {
                render_node(dom, &dom[*child], piet);
            }
        }
        _ => {}
    }
}
