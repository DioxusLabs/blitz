use std::cell::RefCell;
use style::properties::ComputedValues;
use style_traits::CssType::COLOR;

use crate::{styling::BlitzNode, viewport::Viewport};
use crate::{styling::NodeData, text::TextContext};
use crate::{styling::RealDom, Document};
use html5ever::{
    tendril::{fmt::UTF8, Tendril},
    QualName,
};
use style::color::AbsoluteColor;
use taffy::prelude::Layout;
use vello::kurbo::{Affine, Point, Rect, RoundedRect, Vec2};
use vello::peniko::{self, Color, Fill, Stroke};
use vello::SceneBuilder;

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

        self.render_element(sb, root_element.id, Point::ZERO);
    }

    /// Renders a node, but is guaranteed that the node is an element
    /// This is because the font_size is calculated from layout resolution and all text is rendered directly here, instead
    /// of a separate text stroking phase.
    ///
    /// In Blitz, text styling gets its attributes from its container element/resolved styles
    /// In other libraries, text gets its attributes from a `text` element - this is not how HTML works.
    ///
    /// Approaching rendering this way guarantees we have all the styles we need when rendering text with not having
    /// to traverse back to the parent for its styles, or needing to pass down styles
    fn render_element(&self, scene: &mut SceneBuilder, node: usize, location: Point) {
        use markup5ever_rcdom::NodeData;

        let element = &self.dom.nodes[node];
        let (layout, pos) = self.node_position(node, location);

        // Todo: different semantics based on the element name
        let NodeData::Element { name, .. } = &element.node.data else {
            panic!("Unexpected node found while traversing element tree during render")
        };

        let style = element.style.borrow();
        let primary = style.styles.primary();

        // All the stuff that HTML cares about:
        // custom_properties,
        // writing_mode,
        // rules,
        // visited_style,
        // flags,
        // background,
        // border,
        // box_,
        // column,
        // counters,
        // effects,
        // font,
        // inherited_box,
        // inherited_table,
        // inherited_text,
        // inherited_ui,
        // list,
        // margin,
        // outline,
        // padding,
        // position,
        // table,
        // text,
        // ui,

        /*
        Need to draw:
        - frame
        - image
        - shadow
        - border
        - outline

        need to respect:
        - margin
        - padding
         */

        let background = primary.get_background();
        let border = primary.get_border();
        let effects = primary.get_effects();
        let font = primary.get_font();
        let t = primary.get_text();
        let outline = primary.get_outline();
        let _outline = primary.get_position();
        let _padding = primary.get_padding();
        let _margin = primary.get_margin();
        let _position = primary.get_position();
        let inherited_text = primary.get_inherited_text();

        //
        // 1. Draw the frame
        //
        let bg_color = background.background_color.clone();
        let left_border_width = border.border_left_width.to_f64_px();
        let top_border_width = border.border_top_width.to_f64_px();
        let right_border_width = border.border_right_width.to_f64_px();
        let bottom_border_width = border.border_bottom_width.to_f64_px();

        let x: f64 = pos.x;
        let y: f64 = pos.y;
        let width: f64 = layout.size.width.into();
        let height: f64 = layout.size.height.into();

        let x_start = x + left_border_width / 2.0;
        let y_start = y + top_border_width / 2.0;
        let x_end = x + width - right_border_width / 2.0;
        let y_end = y + height - bottom_border_width / 2.0;

        // todo: rescale these by zoom
        let radii = (1.0, 1.0, 1.0, 1.0);
        let shape = RoundedRect::new(x_start, y_start, x_end, y_end, radii);

        // todo: handle non-absolute colors
        let bg_color = bg_color.as_absolute().unwrap();

        scene.fill(
            peniko::Fill::NonZero,
            Affine::IDENTITY,
            bg_color.as_vello(),
            None,
            &shape,
        );

        //
        // 2. Draw the image
        //
        if name.local.as_ref() == "image" {
            // try loading the img from cache and painting it
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
        }

        //
        // 3. Draw the border
        //
        //
        // todo: borders can be different colors, thickness, etc *and* have radius
        let stroke = Stroke::new(0.0);
        let border_color = Color::FLORAL_WHITE;
        scene.stroke(&stroke, Affine::IDENTITY, border_color, None, &shape);

        //
        // 4. Draw the outline
        //

        //
        // N. Draw the children
        //

        // Render out children nodes now that we've painted the background, border, shadow, etc
        // I'd rather pre-compute all the text rendering stuff

        // Pull out all the stuff we need to render text
        // We do it here so all the child text can share the same text styling (font size, color, weight, etc) without
        // recomputing for *every* segment

        let font_size = font.font_size.computed_size().px() * self.viewport.scale();
        let text_color = inherited_text.clone_color().as_vello();

        for child in &element.children {
            match &self.dom.nodes[*child].node.data {
                // Rendering text is done here in the iterator
                // The codegen isn't as great but saves us having to do a bunch of work
                NodeData::Text { contents } => {
                    // todo: use the layout to handle clipping of the text
                    let (_layout, pos) = self.node_position(*child, pos);
                    let transform =
                        Affine::translate(pos.to_vec2() + Vec2::new(0.0, font_size as f64));

                    self.text_context.add(
                        scene,
                        None,
                        font_size,
                        Some(text_color),
                        transform,
                        &contents.borrow(),
                    )
                }

                // Rendering elements is simple, just recurse
                NodeData::Element { .. } => self.render_element(scene, *child, pos),

                // Documents/comments/etc not important
                _ => {}
            }
        }
    }

    fn node_position(&self, node: usize, location: Point) -> (&Layout, Point) {
        let layout = self.layout(node);
        let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
        (layout, pos)
    }

    fn layout(&self, child: usize) -> &Layout {
        self.taffy
            .layout((&self.dom.nodes[child]).layout_id.get().unwrap())
            .unwrap()
    }
}

trait ToVelloColor {
    fn as_vello(&self) -> Color;
}

impl ToVelloColor for AbsoluteColor {
    fn as_vello(&self) -> Color {
        Color {
            r: (self.components.0 * 255.0) as u8,
            g: (self.components.1 * 255.0) as u8,
            b: (self.components.2 * 255.0) as u8,
            a: (self.alpha() * 255.0) as u8,
        }
    }
}

fn get_font_size(element: &NodeData) -> f32 {
    use style::values::generics::transform::ToAbsoluteLength;
    let style = element.style.borrow();
    let primary: &style::servo_arc::Arc<ComputedValues> = style.styles.primary();
    primary
        .clone_font_size()
        .computed_size()
        .to_pixel_length(None)
        .unwrap()
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
