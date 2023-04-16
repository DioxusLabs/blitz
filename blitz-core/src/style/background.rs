use dioxus_native_core::prelude::*;
use dioxus_native_core_macro::partial_derive_state;
use lightningcss::properties::background;
use lightningcss::traits::Parse;
use lightningcss::values::color::CssColor;
use lightningcss::values::gradient;
use lightningcss::values::gradient::GradientItem;
use lightningcss::values::length::LengthPercentage;
use lightningcss::values::length::LengthValue;
use lightningcss::values::position::HorizontalPositionKeyword;
use lightningcss::values::position::VerticalPositionKeyword;
use shipyard::Component;
use smallvec::SmallVec;
use std::f64::consts::PI;
use std::sync::Arc;
use std::sync::Mutex;
use taffy::prelude::Layout;
use taffy::prelude::Size;
use vello::kurbo::Affine;
use vello::kurbo::Point;
use vello::kurbo::Shape;
use vello::peniko;
use vello::peniko::BrushRef;
use vello::peniko::Color;
use vello::peniko::Extend;
use vello::peniko::Fill;
use vello::SceneBuilder;

use crate::image::ImageContext;
use crate::render::with_mask;
use crate::util::map_dimension_percentage;
use crate::util::translate_color;
use crate::util::AngleExt;
use crate::util::Resolve;

#[derive(Clone, PartialEq, Debug)]
enum GradientType {
    Linear { direction_rad: f64 },
    Radial,
    Conic,
}

impl<'a> From<&'a gradient::LinearGradient> for GradientType {
    fn from(l: &'a gradient::LinearGradient) -> Self {
        GradientType::Linear {
            direction_rad: match &l.direction {
                gradient::LineDirection::Angle(angle) => angle.to_radians().into(),
                gradient::LineDirection::Horizontal(HorizontalPositionKeyword::Left) => PI * 1.5,
                gradient::LineDirection::Horizontal(HorizontalPositionKeyword::Right) => PI * 0.5,
                gradient::LineDirection::Vertical(VerticalPositionKeyword::Top) => 0.0,
                gradient::LineDirection::Vertical(VerticalPositionKeyword::Bottom) => PI * 1.0,
                _ => todo!(),
            },
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
struct ColorStop {
    color: Color,
    position: Option<LengthPercentage>,
}

impl TryFrom<&GradientItem<LengthPercentage>> for ColorStop {
    type Error = ();

    fn try_from(item: &GradientItem<LengthPercentage>) -> Result<Self, Self::Error> {
        match item {
            GradientItem::ColorStop(color_stop) => Ok(ColorStop {
                color: translate_color(&color_stop.color),
                position: color_stop.position.clone(),
            }),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Gradient {
    gradient_type: GradientType,
    stops: Vec<ColorStop>,
    // The last size and resolved positions and stops. This is used cache the last resolved position and stops so that we don't have to resolve them again if they are the same.
    last_resolved: Mutex<Option<GradientCache>>,
    repeating: bool,
}

impl TryFrom<gradient::Gradient> for Gradient {
    type Error = ();

    fn try_from(gradient: gradient::Gradient) -> Result<Self, Self::Error> {
        use gradient::Gradient::*;
        let (gradient_type, repeating) = match &gradient {
            Linear(liniar) => (liniar.into(), false),
            RepeatingLinear(liniar) => (liniar.into(), true),
            Radial(_) => (GradientType::Radial, false),
            RepeatingRadial(_) => (GradientType::Radial, true),
            Conic(_) => (GradientType::Conic, false),
            RepeatingConic(_) => (GradientType::Conic, true),
            _ => return Err(()),
        };
        let stops = match gradient {
            Linear(linear) | RepeatingLinear(linear) => linear.items,
            Radial(radial) | RepeatingRadial(radial) => radial.items,
            Conic(conic) | RepeatingConic(conic) => conic
                .items
                .into_iter()
                .map(|item| match item {
                    GradientItem::ColorStop(stop) => GradientItem::ColorStop(gradient::ColorStop {
                        color: stop.color,
                        position: stop.position.map(|percentage| {
                            map_dimension_percentage(percentage, |angle| {
                                LengthValue::Px(angle.to_turn_percentage())
                            })
                        }),
                    }),
                    GradientItem::Hint(pos) => {
                        GradientItem::Hint(map_dimension_percentage(pos, |angle| {
                            LengthValue::Px(angle.to_turn_percentage())
                        }))
                    }
                })
                .collect(),
            _ => return Err(()),
        };

        let stops = stops
            .into_iter()
            .map(|stop| ColorStop::try_from(&stop))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Gradient {
            gradient_type,
            stops,
            last_resolved: Default::default(),
            repeating,
        })
    }
}

#[derive(Clone, Debug)]
struct GradientCache {
    rect: Size<f32>,
    viewport_size: Size<u32>,
    result: Arc<peniko::ColorStops>,
}

impl PartialEq for Gradient {
    fn eq(&self, other: &Self) -> bool {
        self.gradient_type == other.gradient_type && self.stops == other.stops
    }
}

impl Gradient {
    /// Resolve all missing positions. If a position is missing, it is in between the last resolved position and the next resolved position.
    pub(crate) fn resolve_stops(
        &self,
        rect: &Size<f32>,
        viewport_size: &Size<u32>,
    ) -> Arc<peniko::ColorStops> {
        let mut last_resolved = self.last_resolved.lock().unwrap();
        let resolved = last_resolved
            .as_ref()
            .filter(|last_resolved| {
                last_resolved.rect == *rect && last_resolved.viewport_size == *viewport_size
            })
            .is_some();
        if resolved {
            last_resolved.as_ref().unwrap().result.clone()
        } else {
            let mut resolved = smallvec::SmallVec::default();

            let mut last_resolved_position = None;
            let mut unresolved_positions: Vec<Color> = Vec::new();

            let mut resolve_pos =
                |position: Option<f32>,
                 resolved: &mut SmallVec<[peniko::ColorStop; 4]>,
                 unresolved_positions: &mut Vec<Color>| {
                    let resolved_position = position.unwrap_or(1.);
                    let total_unresolved = unresolved_positions.len();
                    for (i, unresolved_stop) in unresolved_positions.iter_mut().enumerate() {
                        let last_pos = last_resolved_position.unwrap_or_default();

                        let exclude_last_pos = last_resolved_position.is_some();
                        let exclude_next_pos = position.is_some();

                        let offset = last_pos
                            + (((i + exclude_last_pos as usize) as f32)
                                / (total_unresolved + (exclude_next_pos as usize) * 2 - 1) as f32)
                                * (resolved_position - last_pos);

                        resolved.push(peniko::ColorStop {
                            color: *unresolved_stop,
                            offset,
                        });
                    }
                    unresolved_positions.clear();
                    last_resolved_position = Some(resolved_position);
                    resolved_position
                };

            for stop in &self.stops {
                match &stop.position {
                    Some(position) => {
                        let position =
                            position.resolve(crate::util::Axis::X, rect, viewport_size) as f32;
                        resolve_pos(Some(position), &mut resolved, &mut unresolved_positions);
                        resolved.push(peniko::ColorStop {
                            color: stop.color,
                            offset: position,
                        });
                    }
                    None => {
                        unresolved_positions.push(stop.color);
                    }
                }
            }

            // resolve any remaining positions
            resolve_pos(None, &mut resolved, &mut unresolved_positions);

            *last_resolved = Some(GradientCache {
                rect: *rect,
                viewport_size: *viewport_size,
                result: Arc::new(resolved),
            });

            last_resolved.as_ref().unwrap().result.clone()
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
        let stops = self.resolve_stops(rect, viewport_size);
        let extend = if self.repeating {
            Extend::Repeat
        } else {
            repeat.into()
        };
        match &self.gradient_type {
            GradientType::Linear { direction_rad } => {
                // Rotating the gradient with a point th
                let bb = shape.bounding_box();
                let starting_point_offset = angle_to_center_offset(*direction_rad, *rect);
                let ending_point_offset =
                    Point::new(-starting_point_offset.x, -starting_point_offset.y);
                let center = bb.center();
                let start = Point::new(
                    center.x + starting_point_offset.x,
                    center.y + starting_point_offset.y,
                );
                let end = Point::new(
                    center.x + ending_point_offset.x,
                    center.y + ending_point_offset.y,
                );

                let kind = peniko::GradientKind::Linear { start, end };

                let gradient = peniko::Gradient {
                    kind,
                    extend,
                    stops: (*stops).clone(),
                };

                let brush = peniko::BrushRef::Gradient(&gradient);

                sb.fill(peniko::Fill::NonZero, Affine::IDENTITY, brush, None, shape)
            }
            GradientType::Radial => todo!(),
            GradientType::Conic => todo!(),
        }
    }
}

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

        with_mask(sb, shape, |sb| {
            self.image
                .render(sb, shape, self.repeat, &rect.size, viewport_size)
        })
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

// https://developer.mozilla.org/en-US/docs/Web/CSS/gradient/linear-gradient#composition_of_a_linear_gradient
// Graphed visualization: https://www.desmos.com/calculator/7vfcr5kczy
fn half_length_q1(width: f64, height: f64, angle: f64) -> f64 {
    ((height / width).atan() - angle).cos() * (width.powi(2) + height.powi(2)).sqrt()
}

fn angle_to_center_offset(full_angle: f64, size: Size<f32>) -> Point {
    let x = size.width as f64 / 2.;
    let y = size.height as f64 / 2.;
    let full_angle = full_angle % (2. * PI);
    let angle = full_angle % (PI / 2.);
    // Q1
    if (0.0..PI / 2.).contains(&full_angle) {
        let length = half_length_q1(x, y, angle);
        (angle.cos() * length, angle.sin() * length).into()
    }
    // Q2
    else if ((PI / 2.)..PI).contains(&full_angle) {
        let length = half_length_q1(y, x, angle);
        (-angle.sin() * length, angle.cos() * length).into()
    }
    // Q3
    else if (PI..3. * PI / 2.).contains(&full_angle) {
        let length = half_length_q1(x, y, angle);
        (-angle.cos() * length, -angle.sin() * length).into()
    }
    // Q4
    else {
        let length = half_length_q1(y, x, angle);
        (angle.sin() * length, -angle.cos() * length).into()
    }
}

#[test]
fn gradient_offset() {
    // Check that when the angle points dirrectly to a midpoint of a side the offset is correct
    assert_eq!(
        angle_to_center_offset(
            0.0,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(50., 0.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            PI / 2.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(0., 50.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            PI,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(-50., 0.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            3. * PI / 2.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(0., -50.).round()
    );

    // Check that when the angle points to a corner or midpoint of a side the offset is correct
    assert_eq!(
        angle_to_center_offset(
            PI / 4.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(50., 50.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            3. * PI / 4.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(-50., 50.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            5. * PI / 4.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(-50., -50.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            7. * PI / 4.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(50., -50.).round()
    );
}
