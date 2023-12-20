use markup5ever_rcdom::RcDom;
use style::properties::style_structs::Border;
//
// use dioxus_native_core::prelude::*;
use taffy::prelude::Layout;
use taffy::prelude::Size;
use taffy::Taffy;
use tao::dpi::PhysicalSize;
use vello::kurbo::{Affine, Point, Rect, RoundedRect, Vec2};
use vello::peniko;
use vello::peniko::{Color, Fill, Stroke};
use vello::SceneBuilder;

use crate::BlitzNode;
// use crate::focus::Focused;
// use crate::image::LoadedImage;
// use crate::layout::TaffyLayout;
// use crate::style::Background;
// use crate::style::Border;
// use crate::style::ForgroundColor;
use crate::text::FontSize;
use crate::text::TextContext;
use crate::text::DEFAULT_FONT_SIZE;
// use crate::util::Resolve;
// use crate::util::{translate_color, Axis};
use crate::RealDom;

const FOCUS_BORDER_WIDTH: f64 = 6.0;

pub(crate) fn render(
    dom: &RealDom,
    taffy: &Taffy,
    text_context: &mut TextContext,
    scene_builder: &mut SceneBuilder,
    window_size: PhysicalSize<u32>,
) {
    let root = dom.root_element();

    scene_builder.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::WHITE,
        None,
        &root.bounds(taffy),
    );

    let viewport_size = Size {
        width: window_size.width,
        height: window_size.height,
    };

    render_node(
        root,
        taffy,
        text_context,
        scene_builder,
        Point::ZERO,
        &viewport_size,
    );
}

fn render_node(
    node: BlitzNode,
    taffy: &Taffy,
    text_context: &mut TextContext,
    scene_builder: &mut SceneBuilder,
    location: Point,
    viewport_size: &Size<u32>,
) {
    use markup5ever_rcdom::NodeData;

    let element = node.data();
    let layout = taffy.layout(element.layout_id.get().unwrap()).unwrap();

    let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);

    match &element.node.data {
        NodeData::Text { contents } => {
            // let text_color = translate_color(&node.get::<ForgroundColor>().unwrap().0);
            // let font_size = if let Some(font_size) = node.get::<FontSize>() {
            //     font_size.0
            // } else {
            //     DEFAULT_FONT_SIZE
            // };

            let font_size = DEFAULT_FONT_SIZE * 4.0;
            let text_color = Color::BLACK;
            let contents = contents.borrow();
            let text = contents.as_ref();

            println!("text: {text} {layout:?}");

            text_context.add(
                scene_builder,
                None,
                font_size,
                Some(text_color),
                Affine::translate(pos.to_vec2() + Vec2::new(0.0, font_size as f64)),
                text,
            )
        }
        NodeData::Element {
            name,
            attrs,
            template_contents,
            mathml_annotation_xml_integration_point,
        } => {
            // let shape = get_shape(layout, node, viewport_size, pos);

            println!("element: {name:?} {layout:?}");

            let rect = layout.size;
            let x: f64 = pos.x;
            let y: f64 = pos.y;
            let width: f64 = layout.size.width.into();
            let height: f64 = layout.size.height.into();

            let style = element.style.borrow();
            let primary = style.styles.primary();
            let background = primary.get_background();
            let bg_color = background.background_color.clone();

            let Border {
                border_top_color,
                border_top_style,
                border_top_width,
                border_right_color,
                border_right_style,
                border_right_width,
                border_bottom_color,
                border_bottom_style,
                border_bottom_width,
                border_left_color,
                border_left_style,
                border_left_width,
                border_top_left_radius,
                border_top_right_radius,
                border_bottom_right_radius,
                border_bottom_left_radius,
                border_image_source,
                border_image_outset,
                border_image_repeat,
                border_image_width,
                border_image_slice,
            } = primary.get_border();

            let left_border_width = border_left_width.to_f64_px();
            let top_border_width = border_top_width.to_f64_px();
            let right_border_width = border_right_width.to_f64_px();
            let bottom_border_width = border_bottom_width.to_f64_px();

            // The stroke is drawn on the outside of the border, so we need to offset the rect by the border width for each side.
            let x_start = x + left_border_width / 2.0;
            let y_start = y + top_border_width / 2.0;
            let x_end = x + width - right_border_width / 2.0;
            let y_end = y + height - bottom_border_width / 2.0;

            let shape = RoundedRect::new(
                x_start,
                y_start,
                x_end,
                y_end,
                (
                    1.0, 1.0, 1.0,
                    1.0, // border.radius.top_left.0.resolve(axis, &rect, viewport_size),
                        // border
                        //     .radius
                        //     .top_right
                        //     .0
                        //     .resolve(axis, &rect, viewport_size),
                        // border
                        //     .radius
                        //     .bottom_right
                        //     .0
                        //     .resolve(axis, &rect, viewport_size),
                        // border
                        //     .radius
                        //     .bottom_left
                        //     .0
                        //     .resolve(axis, &rect, viewport_size),
                ),
            );

            // bg_color
            let as_abs = bg_color.as_absolute().unwrap();

            println!("stroking element {bg_color:?}");

            fn components_to_u8(val: f32) -> u8 {
                (val * 256.0) as _
            }

            let r = components_to_u8(as_abs.components.0);
            let g = components_to_u8(as_abs.components.1);
            let b = components_to_u8(as_abs.components.2);
            let a = 255;
            let color = Color { r, g, b, a };

            scene_builder.fill(peniko::Fill::NonZero, Affine::IDENTITY, color, None, &shape);

            // let stroke_color = translate_color(&node.get::<Border>().unwrap().colors.top);
            let stroke = Stroke::new(10.0);
            scene_builder.stroke(&stroke, Affine::IDENTITY, color, None, &shape);
            // background.draw_shape(scene_builder, &shape, layout, viewport_size);

            // draw the rect, descend to its children with the position updated
            for id in &element.children {
                let child = node.with(*id);
                // let mut loc = location.clone();

                // loc.y += 5.0;

                render_node(
                    child,
                    taffy,
                    text_context,
                    scene_builder,
                    pos,
                    viewport_size,
                );
            }
        }
        NodeData::Document => todo!(),
        NodeData::Doctype {
            name,
            public_id,
            system_id,
        } => todo!(),
        NodeData::Comment { contents } => todo!(),
        NodeData::ProcessingInstruction { target, contents } => todo!(),
    }

    // let taffy_node = node.get::<TaffyLayout>().unwrap().node.unwrap();
    // let layout = taffy.layout(taffy_node).unwrap();
    // let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
    // match &*node.node_type() {
    //     NodeType::Text(TextNode { text, .. }) => {
    //         let text_color = translate_color(&node.get::<ForgroundColor>().unwrap().0);
    //         let font_size = if let Some(font_size) = node.get::<FontSize>() {
    //             font_size.0
    //         } else {
    //             DEFAULT_FONT_SIZE
    //         };
    //         text_context.add(
    //             scene_builder,
    //             None,
    //             font_size,
    //             Some(text_color),
    //             Affine::translate(pos.to_vec2() + Vec2::new(0.0, font_size as f64)),
    //             text,
    //         )
    //     }
    //     NodeType::Element(_) => {
    //         let shape = get_shape(layout, node, viewport_size, pos);

    //         let background = node.get::<Background>().unwrap();
    //         if node.get::<Focused>().filter(|focused| focused.0).is_some() {
    //             let stroke_color = Color::rgb(1.0, 1.0, 1.0);
    //             let stroke = Stroke::new(FOCUS_BORDER_WIDTH as f32 / 2.0);
    //             scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
    //             let smaller_rect = shape.rect().inset(-FOCUS_BORDER_WIDTH / 2.0);
    //             let smaller_shape = RoundedRect::from_rect(smaller_rect, shape.radii());
    //             let stroke_color = Color::rgb(0.0, 0.0, 0.0);
    //             scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
    //             background.draw_shape(scene_builder, &smaller_shape, layout, viewport_size);
    //         } else {
    //             let stroke_color = translate_color(&node.get::<Border>().unwrap().colors.top);
    //             let stroke = Stroke::new(node.get::<Border>().unwrap().width.top.resolve(
    //                 Axis::Min,
    //                 &layout.size,
    //                 viewport_size,
    //             ) as f32);
    //             scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
    //             background.draw_shape(scene_builder, &shape, layout, viewport_size);
    //         };

    //         if let Some(image) = node
    //             .get::<LoadedImage>()
    //             .as_ref()
    //             .and_then(|image| image.0.as_ref())
    //         {
    //             // Scale the image to fit the layout
    //             let image_width = image.width as f64;
    //             let image_height = image.height as f64;
    //             let scale = Affine::scale_non_uniform(
    //                 layout.size.width as f64 / image_width,
    //                 layout.size.height as f64 / image_height,
    //             );

    //             // Translate the image to the layout's position
    //             let translate = Affine::translate(pos.to_vec2());

    //             scene_builder.draw_image(image, translate * scale);
    //         }

    //         for child in node.children() {
    //             render_node(
    //                 taffy,
    //                 child,
    //                 text_context,
    //                 scene_builder,
    //                 pos,
    //                 viewport_size,
    //             );
    //         }
    //     }
    //     _ => {}
    // }
}

// pub(crate) fn get_shape(
//     layout: &Layout,
//     node: BlitzNode,
//     viewport_size: &Size<u32>,
//     location: Point,
// ) -> RoundedRect {
//     // use vello::glyph::fello::raw::tables::base::Axis;

//     let axis = Axis::Min;
//     let rect = layout.size;
//     let x: f64 = location.x;
//     let y: f64 = location.y;
//     let width: f64 = layout.size.width.into();
//     let height: f64 = layout.size.height.into();
//     let focused = false;
//     // let border = taffy::prelude::
//     // let border: &Border = &node.get().unwrap();
//     // let focused = node.get::<Focused>().filter(|focused| focused.0).is_some();
//     // let focused = node.get::<Focused>().filter(|focused| focused.0).is_some();
//     let left_border_width = if focused {
//         FOCUS_BORDER_WIDTH
//     } else {
//         border.width.left.resolve(axis, &rect, viewport_size)
//     };
//     let right_border_width = if focused {
//         FOCUS_BORDER_WIDTH
//     } else {
//         border.width.right.resolve(axis, &rect, viewport_size)
//     };
//     let top_border_width = if focused {
//         FOCUS_BORDER_WIDTH
//     } else {
//         border.width.top.resolve(axis, &rect, viewport_size)
//     };
//     let bottom_border_width = if focused {
//         FOCUS_BORDER_WIDTH
//     } else {
//         border.width.bottom.resolve(axis, &rect, viewport_size)
//     };

//     // The stroke is drawn on the outside of the border, so we need to offset the rect by the border width for each side.
//     let x_start = x + left_border_width / 2.0;
//     let y_start = y + top_border_width / 2.0;
//     let x_end = x + width - right_border_width / 2.0;
//     let y_end = y + height - bottom_border_width / 2.0;

//     RoundedRect::new(
//         x_start,
//         y_start,
//         x_end,
//         y_end,
//         (
//             border.radius.top_left.0.resolve(axis, &rect, viewport_size),
//             border
//                 .radius
//                 .top_right
//                 .0
//                 .resolve(axis, &rect, viewport_size),
//             border
//                 .radius
//                 .bottom_right
//                 .0
//                 .resolve(axis, &rect, viewport_size),
//             border
//                 .radius
//                 .bottom_left
//                 .0
//                 .resolve(axis, &rect, viewport_size),
//         ),
//     )
// }

// pub(crate) fn get_abs_pos(layout: Layout, taffy: &Taffy, node: NodeRef) -> Point {
//     let mut node_layout = layout.location;
//     let mut current = node.id();
//     while let Some(parent) = node.real_dom().get(current).unwrap().parent() {
//         let parent_id = parent.id();
//         // the root element is positioned at (0, 0)
//         if parent_id == node.real_dom().root_id() {
//             break;
//         }
//         current = parent_id;
//         let taffy_node = parent.get::<TaffyLayout>().unwrap().node.unwrap();
//         let parent_layout = taffy.layout(taffy_node).unwrap();
//         node_layout.x += parent_layout.location.x;
//         node_layout.y += parent_layout.location.y;
//     }
//     Point::new(node_layout.x as f64, node_layout.y as f64)
// }
