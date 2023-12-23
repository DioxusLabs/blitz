use std::cell::RefCell;
use style::properties::ComputedValues;

use crate::text::TextContext;
use crate::{styling::BlitzNode, viewport::Viewport};
use crate::{styling::RealDom, Document};
use html5ever::{
    tendril::{fmt::UTF8, Tendril},
    QualName,
};
use style::color::AbsoluteColor;
use taffy::prelude::Layout;
use taffy::prelude::Size as LayoutSize;
use taffy::TaffyTree;
use vello::peniko;
use vello::peniko::{Color, Fill, Stroke};
use vello::SceneBuilder;
use vello::{
    kurbo::{Affine, Point, Rect, RoundedRect, Vec2},
    Scene,
};

const FOCUS_BORDER_WIDTH: f64 = 6.0;

impl Document {
    /// Render to any scene!
    pub(crate) fn render_internal(&self, sb: &mut SceneBuilder) {
        let root_element = &self.dom.root_element();

        // We by default render a white background for the window. T
        // his is just the default stylesheet in action
        sb.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::WHITE,
            None,
            &root_element.bounds(&self.taffy),
        );

        self.render_node(sb, root_element.id, Point::ZERO, self.viewport.font_size);
    }

    fn render_node(
        &self,
        scene_builder: &mut SceneBuilder,
        node: usize,
        location: Point,
        font_size: f32,
    ) {
        use markup5ever_rcdom::NodeData;

        let element = &self.dom.nodes[node];
        let layout = self.taffy.layout(element.layout_id.get().unwrap()).unwrap();
        let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);

        match &element.node.data {
            NodeData::Text { contents } => {
                // uhhh we need the font size but I dont think text nodes have their fontsize ready?
                self.stroke_text(scene_builder, pos, contents, font_size)
            }
            NodeData::Element { name, .. } => {
                //
                self.stroke_element(name, pos, layout, element, scene_builder)
            }
            NodeData::Document
            | NodeData::Doctype { .. }
            | NodeData::Comment { .. }
            | NodeData::ProcessingInstruction { .. } => todo!(),
        }
    }

    fn stroke_text(
        &self,
        scene_builder: &mut SceneBuilder<'_>,
        pos: Point,
        contents: &RefCell<Tendril<UTF8>>,
        font_size: f32,
    ) {
        // let text_color = translate_color(&node.get::<ForgroundColor>().unwrap().0);

        let font_size = font_size * self.viewport.hidpi_scale;
        let text_color = Color::BLACK;
        let transform = Affine::translate(pos.to_vec2() + Vec2::new(0.0, font_size as f64));

        self.text_context.add(
            scene_builder,
            None,
            font_size,
            Some(text_color),
            transform,
            &contents.borrow(),
        )
    }

    /// Draw an HTML element.
    ///
    /// Will need to render special elements differently....
    fn stroke_element(
        &self,
        name: &QualName,
        pos: Point,
        layout: &Layout,
        element: &crate::styling::NodeData,
        scene_builder: &mut SceneBuilder<'_>,
    ) {
        // 1. Stroke the background

        // 2. Stroke the

        let style = element.style.borrow();
        let primary: &style::servo_arc::Arc<ComputedValues> = style.styles.primary();

        let x: f64 = pos.x;
        let y: f64 = pos.y;
        let width: f64 = layout.size.width.into();
        let height: f64 = layout.size.height.into();

        let background = primary.get_background();
        let bg_color = background.background_color.clone();

        let border = primary.get_border();

        let left_border_width = border.border_left_width.to_f64_px();
        let top_border_width = border.border_top_width.to_f64_px();
        let right_border_width = border.border_right_width.to_f64_px();
        let bottom_border_width = border.border_bottom_width.to_f64_px();

        let x_start = x + left_border_width / 2.0;
        let y_start = y + top_border_width / 2.0;
        let x_end = x + width - right_border_width / 2.0;
        let y_end = y + height - bottom_border_width / 2.0;

        // todo: rescale these by zoom
        let radii = (1.0, 1.0, 1.0, 1.0);
        let shape = RoundedRect::new(x_start, y_start, x_end, y_end, radii);

        let bg_color = bg_color.as_absolute().unwrap();

        // todo: opacity
        let color = Color {
            r: (bg_color.components.0 * 255.0) as u8,
            g: (bg_color.components.1 * 255.0) as u8,
            b: (bg_color.components.2 * 255.0) as u8,
            a: 255,
        };

        scene_builder.fill(peniko::Fill::NonZero, Affine::IDENTITY, color, None, &shape);

        // todo: need more color points
        let stroke = Stroke::new(0.0);
        scene_builder.stroke(&stroke, Affine::IDENTITY, color, None, &shape);

        // manually cascade this into children since text might not have it when rendering
        use style::values::generics::transform::ToAbsoluteLength;
        let font_size = primary
            .clone_font_size()
            .computed_size()
            .to_pixel_length(None)
            .unwrap();

        for id in &element.children {
            self.render_node(scene_builder, *id, pos, font_size);
        }
    }
}

fn convert_servo_color(color: &AbsoluteColor) -> Color {
    fn components_to_u8(val: f32) -> u8 {
        (val * 255.0) as _
    }

    // todo: opacity
    let r = components_to_u8(color.components.0);
    let g = components_to_u8(color.components.1);
    let b = components_to_u8(color.components.2);
    let a = 255;

    let color = Color { r, g, b, a };
    color
}

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

// Stroke background
// Stroke border
// Stroke focused
