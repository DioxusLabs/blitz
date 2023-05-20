mod gradient;
mod linear_gradient;
mod radial_gradient;

use dioxus_native_core::prelude::*;
use dioxus_native_core_macro::partial_derive_state;
use lightningcss::properties::background;
use lightningcss::traits::Parse;
use lightningcss::values::color::CssColor;
use shipyard::Component;
use std::sync::Arc;
use taffy::prelude::Layout;
use taffy::prelude::Size;
use vello::kurbo::Affine;
use vello::kurbo::Shape;
use vello::peniko;
use vello::peniko::BrushRef;
use vello::peniko::Color;
use vello::peniko::Extend;
use vello::peniko::Fill;
use vello::SceneBuilder;

use crate::image::ImageContext;
use crate::util::translate_color;

use self::gradient::Gradient;

#[derive(PartialEq, Debug, Default)]
pub(crate) enum Image {
    #[default]
    None,
    Image(Arc<vello::peniko::Image>),
    Gradient(Gradient),
}

impl Image {
    fn try_create(value: lightningcss::values::image::Image, ctx: &SendAnyMap) -> Option<Self> {
        use lightningcss::values::image;
        match value {
            image::Image::None => Some(Self::None),
            image::Image::Url(url) => {
                let image_ctx: &ImageContext = ctx.get().expect("ImageContext not found");
                Some(Self::Image(image_ctx.load_file(url.url.as_ref()).unwrap()))
            }
            image::Image::Gradient(gradient) => Some(Self::Gradient((*gradient).try_into().ok()?)),
            _ => None,
        }
    }

    fn render(
        &self,
        sb: &mut SceneBuilder,
        shape: &impl Shape,
        repeat: Repeat,
        rect: &Size<f32>,
        viewport_size: &Size<u32>,
    ) {
        match self {
            Self::Gradient(gradient) => gradient.render(sb, shape, repeat, rect, viewport_size),
            Self::Image(image) => {
                // Translate the image to the layout's position
                match repeat {
                    Repeat { x: false, y: false } => {
                        sb.fill(
                            Fill::NonZero,
                            Affine::IDENTITY,
                            BrushRef::Image(image),
                            None,
                            &shape,
                        );
                    }
                    _ => {
                        sb.fill(
                            Fill::NonZero,
                            Affine::IDENTITY,
                            BrushRef::Image(&(**image).clone().with_extend(peniko::Extend::Repeat)),
                            None,
                            &shape,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(crate) struct Repeat {
    x: bool,
    y: bool,
}

impl From<background::BackgroundRepeat> for Repeat {
    fn from(repeat: background::BackgroundRepeat) -> Self {
        fn is_repeat(repeat: &background::BackgroundRepeatKeyword) -> bool {
            use background::BackgroundRepeatKeyword::*;
            !matches!(repeat, NoRepeat)
        }

        Repeat {
            x: is_repeat(&repeat.x),
            y: is_repeat(&repeat.y),
        }
    }
}

impl From<Repeat> for Extend {
    fn from(val: Repeat) -> Self {
        match val {
            Repeat { x: false, y: false } => Extend::Repeat,
            _ => Extend::Pad,
        }
    }
}

#[derive(PartialEq, Debug, Component)]
pub(crate) struct Background {
    pub color: Color,
    pub image: Image,
    pub repeat: Repeat,
}

impl Background {
    pub(crate) fn draw_shape(
        &self,
        sb: &mut SceneBuilder,
        shape: &impl Shape,
        rect: &Layout,
        viewport_size: &Size<u32>,
    ) {
        // First draw the background color
        sb.fill(
            peniko::Fill::NonZero,
            Affine::IDENTITY,
            self.color,
            None,
            &shape,
        );

        self.image
            .render(sb, shape, self.repeat, &rect.size, viewport_size)
    }
}

impl Default for Background {
    fn default() -> Self {
        Background {
            color: Color::rgba8(255, 255, 255, 0),
            image: Image::default(),
            repeat: Repeat::default(),
        }
    }
}

#[partial_derive_state]
impl State for Background {
    type ChildDependencies = ();
    type ParentDependencies = ();
    type NodeDependencies = ();

    const NODE_MASK: NodeMaskBuilder<'static> =
        NodeMaskBuilder::new().with_attrs(AttributeMaskBuilder::Some(&[
            "background",
            "background-color",
            "background-image",
            "background-repeat",
        ]));

    fn update<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        _: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        ctx: &SendAnyMap,
    ) -> bool {
        let mut new = Background::default();
        for attr in node_view.attributes().into_iter().flatten() {
            if let Some(attr_value) = attr.value.as_text() {
                match attr.attribute.name.as_str() {
                    "background" => {
                        if let Ok(background) = background::Background::parse_string(attr_value) {
                            new.color = translate_color(&background.color);
                            new.repeat = background.repeat.into();
                            new.image = Image::try_create(background.image, ctx).expect(
                                "attempted to convert a background Blitz does not support yet",
                            );
                        }
                    }
                    "background-color" => {
                        if let Ok(new_color) = CssColor::parse_string(attr_value) {
                            new.color = translate_color(&new_color);
                        }
                    }
                    "background-image" => {
                        if let Ok(image) =
                            lightningcss::values::image::Image::parse_string(attr_value)
                        {
                            new.image = Image::try_create(image, ctx).expect(
                                "attempted to convert a background Blitz does not support yet",
                            );
                        }
                    }
                    "background-repeat" => {
                        if let Ok(repeat) = background::BackgroundRepeat::parse_string(attr_value) {
                            new.repeat = repeat.into();
                        }
                    }

                    _ => {}
                }
            }
        }
        let updated = new != *self;
        *self = new;
        updated
    }

    fn create<'a>(
        node_view: NodeView<()>,
        node: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        parent: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        children: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        context: &SendAnyMap,
    ) -> Self {
        let mut myself = Self::default();
        myself.update(node_view, node, parent, children, context);
        myself
    }
}
