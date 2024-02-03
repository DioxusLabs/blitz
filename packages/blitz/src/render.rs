mod multicolor_rounded_rect;

// So many imports
use self::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::{
    devtools::Devtools,
    fontcache::FontCache,
    imagecache::ImageCache,
    text::TextContext,
    util::{GradientSlice, StyloGradient, ToVelloColor},
    viewport::Viewport,
};
use blitz_dom::{Document, Node};
use html5ever::local_name;
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
    kurbo::{Affine, Point, Rect, Shape, Stroke, Vec2},
    peniko::{self, Color, Fill},
    util::{RenderContext, RenderSurface},
    AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions, Scene, SceneBuilder,
};

pub struct Renderer {
    pub dom: Document,

    /// The actual viewport of the page that we're getting a glimpse of.
    /// We need this since the part of the page that's being viewed might not be the page in its entirety.
    /// This will let us opt of rendering some stuff
    pub(crate) viewport: Viewport,

    /// Our drawing kit, not necessarily tied to a surface
    pub(crate) renderer: VelloRenderer,

    pub(crate) surface: RenderSurface,

    pub(crate) render_context: RenderContext,

    /// Our text stencil to be used with vello
    pub(crate) text_context: TextContext,

    /// Our image cache
    pub(crate) images: ImageCache,

    /// A storage of fonts to load in and out.
    /// Whenever we encounter new fonts during parsing + mutations, this will become populated
    pub(crate) fonts: FontCache,

    pub devtools: Devtools,
}

impl Renderer {
    pub async fn from_window<W>(window: W, dom: Document, viewport: Viewport) -> Self
    where
        W: raw_window_handle::HasRawWindowHandle + raw_window_handle::HasRawDisplayHandle,
    {
        // 1. Set up renderer-specific stuff
        // We build an independent viewport which can be dynamically set later
        // The intention here is to split the rendering pipeline away from tao/windowing for rendering to images

        // 2. Set up Vello specific stuff
        let mut render_context = RenderContext::new().unwrap();
        let surface = render_context
            .create_surface(&window, viewport.window_size.0, viewport.window_size.1)
            .await
            .expect("Error creating surface");

        let options = RendererOptions {
            surface_format: Some(surface.config.format),
            antialiasing_support: AaSupport::all(),
            use_cpu: false,
        };

        let renderer =
            VelloRenderer::new(&render_context.devices[surface.dev_id].device, options).unwrap();

        // 5. Build helpers for things like event handlers, hit testing
        Self {
            viewport,
            render_context,
            renderer,
            surface,
            dom,
            text_context: Default::default(),
            images: Default::default(),
            fonts: Default::default(),
            devtools: Default::default(),
        }
    }

    pub fn zoom(&mut self, zoom: f32) {
        *self.viewport.zoom_mut() += zoom;
        self.kick_viewport()
    }

    // Adjust the viewport
    pub fn set_size(&mut self, physical_size: (u32, u32)) {
        self.viewport.window_size = physical_size;
        self.kick_viewport()
    }

    pub fn kick_viewport(&mut self) {
        let (width, height) = self.viewport.window_size;

        if width > 0 && height > 0 {
            self.dom
                .set_stylist_device(dbg!(self.viewport.make_device()));
            dbg!(&self.viewport);
            self.render_context
                .resize_surface(&mut self.surface, width, height);
        }
    }

    /// Draw the current tree to current render surface
    /// Eventually we'll want the surface itself to be passed into the render function, along with things like the viewport
    ///
    /// This assumes styles are resolved and layout is complete.
    /// Make sure you do those before trying to render
    pub fn render(&mut self, scene: &mut Scene) {
        // Simply render the document (the root element (note that this is not the same as the root node)))
        self.render_element(
            &mut SceneBuilder::for_scene(scene),
            self.dom.root_element().id,
            Point::ZERO,
        );

        let surface_texture = self
            .surface
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");

        let device = &self.render_context.devices[self.surface.dev_id];

        let render_params = RenderParams {
            base_color: Color::WHITE,
            width: self.surface.config.width,
            height: self.surface.config.height,
            antialiasing_method: vello::AaConfig::Msaa16,
        };

        self.renderer
            .render_to_surface(
                &device.device,
                &device.queue,
                &scene,
                &surface_texture,
                &render_params,
            )
            .expect("failed to render to surface");

        surface_texture.present();
        device.device.poll(wgpu::Maintain::Wait);
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
        //  - nothing is completely done:
        //  - vello is limiting all the styles we can implement (performantly)
        //  - servo is missing a number of features (like space-evenly justify)
        //
        // Implemented (partially):
        //  - background, border, font, margin, outline, padding,
        //
        // Not Implemented:
        //  - list, position, table, text, ui,
        //  - custom_properties, writing_mode, rules, visited_style, flags,  box_, column, counters, effects,
        //  - inherited_box, inherited_table, inherited_text, inherited_ui,
        use markup5ever_rcdom::NodeData;

        let element = &self.dom.tree()[node];

        // Early return if the element is hidden
        if matches!(element.style.display, taffy::prelude::Display::None) {
            return;
        }

        let NodeData::Element { name, attrs, .. } = &element.node.data else {
            return;
        };

        // Only draw elements with a style
        if element.data.borrow().styles.get_primary().is_none() {
            return;
        }

        // Hide hidden things...
        // todo: move this to state on the element itself
        if let Some(attr) = attrs
            .borrow()
            .iter()
            .find(|attr| attr.name.local == local_name!("hidden"))
        {
            if attr.value.as_ref() == "true" || attr.value.as_ref() == "" {
                return;
            }
        }

        // Hide inputs with type=hidden
        // Can this just be css?
        if name.local == local_name!("input") {
            if let Some(attr) = attrs
                .borrow()
                .iter()
                .find(|attr| attr.name.local == local_name!("type"))
            {
                if attr.value.as_ref() == "hidden" {
                    return;
                }
            }
        }

        let cx = self.element_cx(element, location);
        cx.stroke_effects(scene);
        cx.stroke_outline(scene);
        cx.stroke_frame(scene);
        cx.stroke_border(scene);
        cx.stroke_devtools(scene);

        for child in &cx.element.children {
            match &self.dom.tree()[*child].node.data {
                NodeData::Element { .. } => self.render_element(scene, *child, cx.pos),
                NodeData::Text { contents } => {
                    let (_layout, pos) = self.node_position(*child, cx.pos);
                    cx.stroke_text(scene, &self.text_context, contents.borrow().as_ref(), pos)
                }
                NodeData::Document => {}
                NodeData::Doctype { .. } => {}
                NodeData::Comment { .. } => {}
                NodeData::ProcessingInstruction { .. } => {}
            }
        }
    }

    fn element_cx<'a>(&'a self, element: &'a Node, location: Point) -> ElementCx<'a> {
        let style = element.data.borrow().styles.primary().clone();

        let (layout, pos) = self.node_position(element.id, location);
        let scale = self.viewport.scale_f64();

        let inherited_text = style.get_inherited_text();
        let font = style.get_font();
        let font_size = font.font_size.computed_size().px() as f32;
        let text_color = inherited_text.clone_color().as_vello();

        // the bezpaths for every element are (potentially) cached (not yet, tbd)
        // By performing the transform, we prevent the cache from becoming invalid when the page shifts around
        let transform = Affine::translate((pos.x * scale, pos.y * scale));

        // todo: maybe cache this so we don't need to constantly be figuring it out
        // It is quite a bit of math to calculate during render/traverse
        // Also! we can cache the bezpaths themselves, saving us a bunch of work
        let frame = ElementFrame::new(&style, &layout, scale);

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
            devtools: &self.devtools,
        }
    }

    fn node_position(&self, node: usize, location: Point) -> (Layout, Point) {
        let layout = self.layout(node);
        let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
        (layout, pos)
    }

    fn layout(&self, child: usize) -> Layout {
        self.dom.tree()[child].unrounded_layout
        // self.dom.tree()[child].final_layout
    }
}

/// A context of loaded and hot data to draw the element from
struct ElementCx<'a> {
    frame: ElementFrame,
    style: style::servo_arc::Arc<ComputedValues>,
    layout: Layout,
    pos: Point,
    scale: f64,
    element: &'a Node,
    font_size: f32,
    text_color: Color,
    transform: Affine,
    devtools: &'a Devtools,
}

impl ElementCx<'_> {
    fn stroke_text(
        &self,
        scene: &mut SceneBuilder<'_>,
        text_context: &TextContext,
        contents: &str,
        pos: Point,
    ) {
        let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale))
            .then_translate((0.0, self.font_size as f64 * self.scale as f64).into());

        text_context.add(
            scene,
            None,
            self.font_size * self.scale as f32,
            Some(self.text_color),
            transform,
            contents,
        )
    }

    fn stroke_devtools(&self, scene: &mut SceneBuilder) {
        if self.devtools.show_layout {
            let shape = &self.frame.outer_rect;
            let stroke = Stroke::new(self.scale);

            let stroke_color = match self.element.style.display {
                taffy::prelude::Display::Block => Color::rgb(1.0, 0.0, 0.0),
                taffy::prelude::Display::Flex => Color::rgb(0.0, 1.0, 0.0),
                taffy::prelude::Display::Grid => Color::rgb(0.0, 0.0, 1.0),
                taffy::prelude::Display::None => Color::rgb(0.0, 0.0, 1.0),
            };

            scene.stroke(&stroke, self.transform, stroke_color, None, &shape);
        }

        // if self.devtools.show_style {
        //     self.frame.draw_style(scene);
        // }

        // if self.devtools.print_hover {
        //     self.frame.draw_hover(scene);
        // }
    }

    fn stroke_frame(&self, scene: &mut SceneBuilder) {
        use GenericImage::*;

        for segment in &self.style.get_background().background_image.0 {
            match segment {
                None => self.draw_solid_frame(scene),
                Gradient(gradient) => self.draw_gradient_frame(scene, gradient),
                Url(_) => {
                    //
                    // todo!("Implement background drawing for Image::Url")
                    println!("Implement background drawing for Image::Url");
                    // let background = self.style.get_background();

                    // todo: handle non-absolute colors
                    // let bg_color = background.background_color.clone();
                    // let bg_color = bg_color.as_absolute().unwrap();
                    // let bg_color = Color::RED;
                    let shape = self.frame.outer_rect;

                    // Fill the color
                    scene.fill(
                        Fill::NonZero,
                        self.transform,
                        Color::RED,
                        // bg_color.as_vello(),
                        Option::None,
                        &shape,
                    );
                }
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
            LineDirection::Horizontal(_) => unimplemented!(),
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
            LineDirection::Corner(_, _) => unimplemented!(),
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
                GenericGradientItem::ComplexColorStop { color, position } => unimplemented!(),
                GenericGradientItem::InterpolationHint(_) => unimplemented!(),
            }
        }
        let brush = peniko::BrushRef::Gradient(&gradient);
        scene.fill(peniko::Fill::NonZero, self.transform, brush, None, &shape);
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
            BorderStyle::Inset => unimplemented!(),
            BorderStyle::Groove => unimplemented!(),
            BorderStyle::Outset => unimplemented!(),
            BorderStyle::Ridge => unimplemented!(),
            BorderStyle::Dotted => unimplemented!(),
            BorderStyle::Dashed => unimplemented!(),
            BorderStyle::Double => unimplemented!(),
        };

        scene.fill(Fill::NonZero, self.transform, color, None, &path);
    }

    /// Applies filters to a final frame
    ///
    /// Notably, I don't think we can do this here since vello needs to run this as a pass (shadows need to apply everywhere)
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
        unimplemented!()
    }

    fn draw_conic_gradient(
        &self,
        scene: &mut SceneBuilder<'_>,
        angle: &Angle,
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        items: &OwnedSlice<GenericGradientItem<StyloColor<Percentage>, AngleOrPercentage>>,
        repeating: bool,
    ) {
        unimplemented!()
    }
}
