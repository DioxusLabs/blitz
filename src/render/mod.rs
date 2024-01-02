use std::cell::RefCell;

use crate::{styling::NodeData, util::StyloGradient};
use crate::{util::GradientSlice, Document};
use html5ever::tendril::{fmt::UTF8, Tendril};
use style::{
    properties::{style_structs::Outline, ComputedValues},
    values::{
        computed::{
            Angle, AngleOrPercentage, CSSPixelLength, LengthPercentage, LineDirection, Percentage,
        },
        generics::{
            color::Color as StyloColor,
            image::{EndingShape, GenericGradient, GenericGradientItem, GenericImage},
            position::GenericPosition,
            NonNegative,
        },
        specified::{position::VerticalPositionKeyword, BorderStyle, OutlineStyle},
    },
    OwnedSlice,
};
use taffy::prelude::Layout;
use vello::{
    kurbo::Shape,
    peniko::{self, Fill},
};
use vello::{
    kurbo::{Affine, Point, Vec2},
    peniko::Color,
};

use self::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::util::ToVelloColor;
use vello::SceneBuilder;

mod multicolor_rounded_rect;

impl Document {
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
        // Need to do research on how we can cache most of the bezpaths - there's gonna be a lot of encoding between frames.
        // Might be able to cache resources deeper in vello.
        //
        // Implemented (completely):
        //  - nothing is completely done, vello is limiting all the styles we can implement (performantly)
        //
        // Implemented (partially):
        //  - background, border, font, margin, outline, padding,
        //
        // Not Implemented:
        //  - list, position, table, text, ui,
        //  - custom_properties, writing_mode, rules, visited_style, flags,  box_, column, counters, effects,
        //  - inherited_box, inherited_table, inherited_text, inherited_ui,
        use markup5ever_rcdom::NodeData;

        let element = &self.dom.nodes[node];

        match &element.node.data {
            NodeData::Element { name, .. } => {
                // skip head nodes/script nodes
                // these are handled elsewhere...
                match name.local.as_ref() {
                    "style" | "head" | "script" => {
                        println!("early returning...");
                        return;
                    }
                    _ => {}
                }
            }
            _ => return,
        }

        let cx = self.element_cx(element, location);

        cx.stroke_effects(scene);
        cx.stroke_outline(scene);
        cx.stroke_frame(scene);
        cx.stroke_border(scene);

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
        // dbg!(&contents.borrow(), transform, font_size);

        self.text_context.add(
            scene,
            None,
            *font_size,
            Some(*text_color),
            transform,
            &contents.borrow(),
        )
    }

    fn element_cx<'a>(&'a self, element: &'a NodeData, location: Point) -> ElementCx<'a> {
        let style = element.style.borrow().styles.primary().clone();

        let (layout, pos) = self.node_position(element.id, location);
        let scale = self.viewport.scale_f64();

        // todo: maybe cache this so we don't need to constantly be figuring it out
        // It is quite a bit of math to calculate during render/traverse
        // Also! we can cache the bezpaths themselves, saving us a bunch of work
        let frame = ElementFrame::new(&style, layout, scale);

        let inherited_text = style.get_inherited_text();
        let font = style.get_font();
        let font_size = font.font_size.computed_size().px() * scale as f32;
        let text_color = inherited_text.clone_color().as_vello();

        // the bezpaths for every element are (potentially) cached (not yet, tbd)
        // By performing the transform, we prevent the cache from becoming invalid when the page shifts around
        let transform = Affine::translate((pos.x, pos.y));

        ElementCx {
            frame,
            scale,
            style,
            layout,
            pos,
            element,
            font_size,
            text_color,
            transform,
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
    transform: Affine,
}

impl ElementCx<'_> {
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

    fn draw_gradient_frame(&self, scene: &mut SceneBuilder, gradient: &StyloGradient) {
        match gradient {
            // https://developer.mozilla.org/en-US/docs/Web/CSS/gradient/linear-gradient
            GenericGradient::Linear {
                direction,
                items,
                repeating,
                compat_mode,
            } => self.draw_linear_gradient(scene, direction, items),
            GenericGradient::Radial {
                shape,
                position,
                items,
                repeating,
                compat_mode,
            } => self.draw_radial_gradient(scene, shape, position, items, *repeating),
            GenericGradient::Conic {
                angle,
                position,
                items,
                repeating,
            } => self.draw_conic_gradient(scene, angle, position, items, *repeating),
        };
    }

    fn draw_linear_gradient(
        &self,
        scene: &mut SceneBuilder<'_>,
        direction: &LineDirection,
        items: &GradientSlice,
    ) {
        let bb = self.frame.outer_rect.bounding_box();

        let shape = self.frame.frame();
        let center = bb.center();
        let rect = self.frame.inner_rect;
        let (start, end) = match direction {
            LineDirection::Angle(angle) => {
                let start = Point::new(
                    self.frame.inner_rect.x0 + rect.width() / 2.0,
                    self.frame.inner_rect.y0,
                );
                let end = Point::new(
                    self.frame.inner_rect.x0 + rect.width() / 2.0,
                    self.frame.inner_rect.y1,
                );

                // rotate the lind around the center
                let line = Affine::rotate_about(-angle.radians64(), center)
                    * vello::kurbo::Line::new(start, end);

                (line.p0, line.p1)
            }
            LineDirection::Horizontal(_) => todo!(),
            LineDirection::Vertical(ore) => {
                let start = Point::new(
                    self.frame.inner_rect.x0 + rect.width() / 2.0,
                    self.frame.inner_rect.y0,
                );
                let end = Point::new(
                    self.frame.inner_rect.x0 + rect.width() / 2.0,
                    self.frame.inner_rect.y1,
                );
                match ore {
                    VerticalPositionKeyword::Top => (end, start),
                    VerticalPositionKeyword::Bottom => (start, end),
                }
            }
            LineDirection::Corner(_, _) => todo!(),
        };
        let mut gradient = peniko::Gradient {
            kind: peniko::GradientKind::Linear { start, end },
            extend: Default::default(),
            stops: Default::default(),
        };
        for (idx, item) in items.iter().enumerate() {
            match item {
                GenericGradientItem::SimpleColorStop(stop) => {
                    let step = 1.0 / (items.len() as f32 - 1.0);
                    let offset = step * idx as f32;
                    let color = stop.as_vello();
                    gradient.stops.push(peniko::ColorStop { color, offset });
                }
                GenericGradientItem::ComplexColorStop { color, position } => todo!(),
                GenericGradientItem::InterpolationHint(_) => todo!(),
            }
        }
        let brush = peniko::BrushRef::Gradient(&gradient);
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, brush, None, &shape);
    }

    fn draw_image_frame(&self, scene: &mut SceneBuilder) {}

    fn draw_solid_frame(&self, scene: &mut SceneBuilder) {
        let background = self.style.get_background();

        // todo: handle non-absolute colors
        let bg_color = background.background_color.clone();
        let bg_color = bg_color.as_absolute().unwrap();
        let shape = self.frame.frame();

        // Fill the color
        scene.fill(
            Fill::NonZero,
            self.transform,
            bg_color.as_vello(),
            None,
            &shape,
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
    /// ❌ groove - Defines a 3D grooved border.
    /// ❌ ridge - Defines a 3D ridged border.
    /// ❌ inset - Defines a 3D inset border.
    /// ❌ outset - Defines a 3D outset border.
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
    /// [Border](https://www.w3schools.com/css/css_border.asp)
    ///
    /// The following values are allowed:
    /// - ❌ dotted: Defines a dotted border
    /// - ❌ dashed: Defines a dashed border
    /// - ✅ solid: Defines a solid border
    /// - ❌ double: Defines a double border
    /// - ❌ groove: Defines a 3D grooved border*
    /// - ❌ ridge: Defines a 3D ridged border*
    /// - ❌ inset: Defines a 3D inset border*
    /// - ❌ outset: Defines a 3D outset border*
    /// - ✅ none: Defines no border
    /// - ✅ hidden: Defines a hidden border
    ///
    /// [*] The effect depends on the border-color value
    fn stroke_border_edge(&self, sb: &mut SceneBuilder, edge: Edge) {
        let border = self.style.get_border();
        let path = self.frame.border(edge);

        let color = match edge {
            Edge::Top => border.border_top_color.as_vello(),
            Edge::Right => border.border_right_color.as_vello(),
            Edge::Bottom => border.border_bottom_color.as_vello(),
            Edge::Left => border.border_left_color.as_vello(),
        };

        sb.fill(Fill::NonZero, self.transform, color, None, &path);
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

        scene.fill(Fill::NonZero, self.transform, color, None, &path);
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
        // also: if focused, draw a focus ring
        //
        //             let stroke_color = Color::rgb(1.0, 1.0, 1.0);
        //             let stroke = Stroke::new(FOCUS_BORDER_WIDTH as f32 / 2.0);
        //             scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
        //             let smaller_rect = shape.rect().inset(-FOCUS_BORDER_WIDTH / 2.0);
        //             let smaller_shape = RoundedRect::from_rect(smaller_rect, shape.radii());
        //             let stroke_color = Color::rgb(0.0, 0.0, 0.0);
        //             scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
        //             background.draw_shape(scene_builder, &smaller_shape, layout, viewport_size);
        let effects = self.style.get_effects();
    }

    fn stroke_box_shadow(&self, scene: &mut SceneBuilder<'_>) {
        let effects = self.style.get_effects();
    }

    fn draw_radial_gradient(
        &self,
        scene: &mut SceneBuilder<'_>,
        shape: &EndingShape<NonNegative<CSSPixelLength>, NonNegative<LengthPercentage>>,
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        items: &OwnedSlice<GenericGradientItem<StyloColor<Percentage>, LengthPercentage>>,
        repeating: bool,
    ) {
        todo!()
    }

    fn draw_conic_gradient(
        &self,
        scene: &mut SceneBuilder<'_>,
        angle: &Angle,
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        items: &OwnedSlice<GenericGradientItem<StyloColor<Percentage>, AngleOrPercentage>>,
        repeating: bool,
    ) {
        todo!()
    }
}
