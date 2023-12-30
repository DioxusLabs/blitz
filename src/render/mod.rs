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
//
// Need to draw:
// - frame
// - image
// - shadow
// - outline
// - border
// - list discs

use std::cell::RefCell;

use crate::Document;
use crate::{styling::NodeData, util::StyloGradient};
use html5ever::tendril::{fmt::UTF8, Tendril};
use style::color::AbsoluteColor;
use style::{
    properties::{
        style_structs::{Background, Border, Font, InheritedText, Outline},
        ComputedValues,
    },
    values::{
        computed::{CSSPixelLength, Percentage},
        generics::image::{GenericGradient, GenericImage},
        specified::{BorderStyle, OutlineStyle},
    },
};
use taffy::prelude::Layout;
use vello::{
    kurbo::{Affine, Point, Rect, RoundedRect, Stroke, Vec2},
    peniko::{self, Color},
};
use vello::{
    kurbo::{Arc, BezPath, Dashes, PathEl, PathSegIter, RoundedRectRadii, Shape},
    peniko::Fill,
};

use self::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::util::ToVelloColor;
use vello::SceneBuilder;

mod multicolor_rounded_rect;

impl Document {
    /// Render to any scene!
    pub(crate) fn render_internal(&self, scene: &mut SceneBuilder) {
        let root = &self.dom.root_element();

        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::WHITE,
            None,
            &root.bounds(&self.taffy),
        );

        self.render_element(scene, root.id, Point::ZERO);
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

        let cx = self.element_cx(node, location);

        cx.stroke_outline(scene);
        cx.stroke_frame(scene);
        cx.stroke_border(scene);
        cx.stroke_effects(scene);

        for child in &cx.element.children {
            match &self.dom.nodes[*child].node.data {
                NodeData::Element { .. } => self.render_element(scene, *child, cx.pos),
                NodeData::Text { contents } => self.render_text(scene, child, &cx, contents),
                NodeData::Document => {}
                NodeData::Doctype { .. } => {}
                NodeData::Comment { .. } => {}
                NodeData::ProcessingInstruction { .. } => {}
            }
        }
    }

    fn render_text(
        &self,
        scene: &mut SceneBuilder<'_>,
        child: &usize,
        parent: &ElementCx,
        contents: &RefCell<Tendril<UTF8>>,
    ) {
        let ElementCx {
            font_size,
            text_color,
            ..
        } = parent;

        let (_layout, pos) = self.node_position(*child, parent.pos);

        let transform = Affine::translate(pos.to_vec2() + Vec2::new(0.0, *font_size as f64));

        self.text_context.add(
            scene,
            None,
            *font_size,
            Some(*text_color),
            transform,
            &contents.borrow(),
        )
    }

    fn element_cx(&self, node: usize, location: Point) -> ElementCx {
        let element = &self.dom.nodes[node];

        let style = element.style.borrow().styles.primary().clone();

        let (layout, pos) = self.node_position(node, location);
        let scale = self.viewport.scale_f64();

        // todo: maybe cache this so we don't need to constantly be figuring it out
        // It is quite a bit of math to calculate during render/traverse
        let frame = ElementFrame::new(&style, layout, pos, scale);

        let inherited_text = style.get_inherited_text();
        let font = style.get_font();
        let font_size = font.font_size.computed_size().px() * scale as f32;
        let text_color = inherited_text.clone_color().as_vello();

        ElementCx {
            frame,
            scale,
            style,
            layout,
            pos,
            element,
            font_size,
            text_color,
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

/// A context of loaded and hot data to draw the element from
struct ElementCx<'a> {
    frame: ElementFrame,
    style: style::servo_arc::Arc<ComputedValues>,
    layout: &'a Layout,
    pos: Point,
    scale: f64,
    element: &'a NodeData,
    font_size: f32,
    text_color: Color,
}

impl<'a> ElementCx<'a> {
    fn stroke_frame(&self, scene: &mut SceneBuilder) {
        use GenericImage::*;

        for segment in &self.style.get_background().background_image.0 {
            match segment {
                None => self.draw_solid_frame(scene),
                Gradient(gradient) => self.draw_gradient_frame(scene, gradient),
                Url(_) => todo!("Implement background drawing for Image::Url"),
                Rect(_) => todo!("Implement background drawing for Image::Rect"),
                PaintWorklet(_) => todo!("Implement background drawing for Image::PaintWorklet"),
                CrossFade(_) => todo!("Implement background drawing for Image::CrossFade"),
                ImageSet(_) => todo!("Implement background drawing for Image::ImageSet"),
            }
        }
    }

    fn draw_gradient_frame(&self, scene: &mut SceneBuilder, gradient: &Box<StyloGradient>) {
        // let bb = shape.bounding_box();
        // let starting_point_offset = gradient.center_offset(*rect);
        // let ending_point_offset =
        //     Point::new(-starting_point_offset.x, -starting_point_offset.y);
        // let center = bb.center();
        // let start = Point::new(
        //     center.x + starting_point_offset.x,
        //     center.y + starting_point_offset.y,
        // );
        // let end = Point::new(
        //     center.x + ending_point_offset.x,
        //     center.y + ending_point_offset.y,
        // );

        // let kind = peniko::GradientKind::Linear { start, end };

        // let gradient = peniko::Gradient {
        //     kind,
        //     extend,
        //     stops: (*stops).clone(),
        // };

        // let brush = peniko::BrushRef::Gradient(&gradient);

        // sb.fill(peniko::Fill::NonZero, Affine::IDENTITY, brush, None, shape)
    }

    fn draw_image_frame(&self, scene: &mut SceneBuilder) {}

    fn draw_solid_frame(&self, scene: &mut SceneBuilder) {
        let background = self.style.get_background();

        // todo: handle non-absolute colors
        let bg_color = background.background_color.clone();
        let bg_color = bg_color.as_absolute().unwrap();

        // Fill the color
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            bg_color.as_vello(),
            None,
            &self.frame.rect,
        );
    }

    /// Stroke a border
    ///
    /// The border-style property specifies what kind of border to display.
    ///
    /// The following values are allowed:
    /// ❌ dotted - Defines a dotted border
    /// ❌ dashed - Defines a dashed border
    /// ✅ solid - Defines a solid border
    /// ❌ double - Defines a double border
    /// ❌ groove - Defines a 3D grooved border. The effect depends on the border-color value
    /// ❌ ridge - Defines a 3D ridged border. The effect depends on the border-color value
    /// ❌ inset - Defines a 3D inset border. The effect depends on the border-color value
    /// ❌ outset - Defines a 3D outset border. The effect depends on the border-color value
    /// ✅ none - Defines no border
    /// ✅ hidden - Defines a hidden border
    ///
    /// The border-style property can have from one to four values (for the top border, right border, bottom border, and the left border).
    fn stroke_border(&self, sb: &mut SceneBuilder) {
        for edge in [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
            self.stroke_border_edge(sb, edge);
        }
    }

    /// The border-style property specifies what kind of border to display.
    ///
    /// The following values are allowed:
    /// ❌ dotted - Defines a dotted border
    /// ❌ dashed - Defines a dashed border
    /// ✅ solid - Defines a solid border
    /// ❌ double - Defines a double border
    /// ❌ groove - Defines a 3D grooved border. The effect depends on the border-color value
    /// ❌ ridge - Defines a 3D ridged border. The effect depends on the border-color value
    /// ❌ inset - Defines a 3D inset border. The effect depends on the border-color value
    /// ❌ outset - Defines a 3D outset border. The effect depends on the border-color value
    /// ✅ none - Defines no border
    /// ✅ hidden - Defines a hidden border
    fn stroke_border_edge(&self, sb: &mut SceneBuilder, edge: Edge) {
        let border = self.style.get_border();
        let path = self.frame.border(edge);

        let color = match edge {
            Edge::Top => border.border_top_color.as_vello(),
            Edge::Right => border.border_right_color.as_vello(),
            Edge::Bottom => border.border_bottom_color.as_vello(),
            Edge::Left => border.border_left_color.as_vello(),
        };

        sb.fill(Fill::NonZero, Affine::IDENTITY, color, None, &path);
    }

    /// ❌ dotted - Defines a dotted border
    /// ❌ dashed - Defines a dashed border
    /// ✅ solid - Defines a solid border
    /// ❌ double - Defines a double border
    /// ❌ groove - Defines a 3D grooved border. The effect depends on the border-color value
    /// ❌ ridge - Defines a 3D ridged border. The effect depends on the border-color value
    /// ❌ inset - Defines a 3D inset border. The effect depends on the border-color value
    /// ❌ outset - Defines a 3D outset border. The effect depends on the border-color value
    /// ✅ none - Defines no border
    /// ✅ hidden - Defines a hidden border
    fn stroke_outline(&self, scene: &mut SceneBuilder) {
        let Outline {
            outline_color,
            outline_style,
            ..
        } = self.style.get_outline();

        let color = outline_color
            .as_absolute()
            .map(ToVelloColor::as_vello)
            .unwrap_or_default();

        let style = match outline_style {
            OutlineStyle::Auto => return,
            OutlineStyle::BorderStyle(BorderStyle::Hidden) => return,
            OutlineStyle::BorderStyle(BorderStyle::None) => return,
            OutlineStyle::BorderStyle(style) => style,
        };

        let path = match style {
            BorderStyle::None | BorderStyle::Hidden => return,
            BorderStyle::Solid => self.frame.outline(),
            BorderStyle::Inset => todo!(),
            BorderStyle::Groove => todo!(),
            BorderStyle::Outset => todo!(),
            BorderStyle::Ridge => todo!(),
            BorderStyle::Dotted => todo!(),
            BorderStyle::Dashed => todo!(),
            BorderStyle::Double => todo!(),
        };

        scene.fill(Fill::NonZero, Affine::IDENTITY, color, None, &path);
    }

    /// Applies filters to a final frame
    ///
    /// Notably, I don't think we can do this here since vello needs to run this as a pass
    ///
    /// ❌ opacity: The opacity computed value.
    /// ❌ box_shadow: The box-shadow computed value.
    /// ❌ clip: The clip computed value.
    /// ❌ filter: The filter computed value.
    /// ❌ mix_blend_mode: The mix-blend-mode computed value.
    fn stroke_effects(&self, scene: &mut SceneBuilder<'_>) {
        let effects = self.style.get_effects();
    }
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
