//! `WheelEvent` layer — wheel deltas under `MouseEvent`.

use blitz_traits::events::{BlitzWheelDelta, BlitzWheelEvent};
use boa_engine::class::ClassBuilder;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::{
    Constructed, ExtendLayer, Extended, Super, instance_getter, js_fn_ptr, with_own,
};

use super::mouse_event::MouseEventLayer;

/// `WheelEvent` own block: the wheel delta and its unit mode.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct WheelEventLayer {
    #[unsafe_ignore_trace]
    pub delta_x: f64,
    #[unsafe_ignore_trace]
    pub delta_y: f64,
    #[unsafe_ignore_trace]
    pub delta_mode: u32,
}

impl WheelEventLayer {
    /// Deltas from a Blitz wheel event (`Lines` maps to mode 1, `Pixels` to 0;
    /// `deltaZ` is always 0).
    pub(crate) fn from_blitz(e: &BlitzWheelEvent) -> Self {
        let (delta_x, delta_y, delta_mode) = match &e.delta {
            BlitzWheelDelta::Lines(x, y) => (*x, *y, 1),
            BlitzWheelDelta::Pixels(x, y) => (*x, *y, 0),
        };
        Self {
            delta_x,
            delta_y,
            delta_mode,
        }
    }
}

pub(crate) type WheelEvent = Extended<WheelEventLayer>;

impl ExtendLayer for WheelEventLayer {
    type Parent = MouseEventLayer;
    const CLASS_NAME: &'static str = "WheelEvent";

    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        let done = sup.call(args, ctx)?;
        Ok(Constructed::new(done, WheelEventLayer::default()))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_getter!(class, "deltaX", js_fn_ptr!(delta_x_getter, &realm), attr);
        instance_getter!(class, "deltaY", js_fn_ptr!(delta_y_getter, &realm), attr);
        instance_getter!(class, "deltaZ", js_fn_ptr!(delta_z_getter, &realm), attr);
        instance_getter!(
            class,
            "deltaMode",
            js_fn_ptr!(delta_mode_getter, &realm),
            attr
        );

        Ok(())
    }
}

/// Register the `WheelEvent` class and wire up the
/// `WheelEvent -> MouseEvent` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<WheelEvent>()?;
    crate::shared::link_prototype::<WheelEvent>(context)?;
    Ok(())
}

fn this_wheel_layer(this: &JsValue) -> JsResult<WheelEventLayer> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not a WheelEvent object"))?;
    with_own::<WheelEventLayer, _>(&obj, WheelEventLayer::clone)
}

fn delta_x_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_wheel_layer(this)?.delta_x))
}

fn delta_y_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_wheel_layer(this)?.delta_y))
}

fn delta_z_getter(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(0.0))
}

fn delta_mode_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_wheel_layer(this)?.delta_mode))
}
