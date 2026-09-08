//! `MouseEvent` layer — parent of Pointer/Wheel.
//!
//! Owns the coordinate, button and modifier fields shared by every
//! pointer-ish event (`MouseEvent`, `PointerEvent`, touch events).

use blitz_traits::events::{BlitzPointerEvent, BlitzWheelEvent};
use boa_engine::class::ClassBuilder;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::{
    Constructed, ExtendLayer, Extended, Super, instance_getter, js_fn_ptr, with_own,
};

use super::ui_event::UIEventLayer;

/// `MouseEvent` own block: coordinates, buttons and modifiers.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct MouseEventLayer {
    #[unsafe_ignore_trace]
    pub screen_x: f64,
    #[unsafe_ignore_trace]
    pub screen_y: f64,
    #[unsafe_ignore_trace]
    pub client_x: f64,
    #[unsafe_ignore_trace]
    pub client_y: f64,
    #[unsafe_ignore_trace]
    pub page_x: f64,
    #[unsafe_ignore_trace]
    pub page_y: f64,
    #[unsafe_ignore_trace]
    pub offset_x: f64,
    #[unsafe_ignore_trace]
    pub offset_y: f64,
    #[unsafe_ignore_trace]
    pub button: u8,
    #[unsafe_ignore_trace]
    pub buttons: u16,
    #[unsafe_ignore_trace]
    pub ctrl_key: bool,
    #[unsafe_ignore_trace]
    pub shift_key: bool,
    #[unsafe_ignore_trace]
    pub alt_key: bool,
    #[unsafe_ignore_trace]
    pub meta_key: bool,
}

impl MouseEventLayer {
    /// Static coords + modifiers from a pointer-ish event.
    pub(crate) fn from_pointer(e: &BlitzPointerEvent) -> Self {
        Self {
            screen_x: e.screen_x() as f64,
            screen_y: e.screen_y() as f64,
            client_x: e.client_x() as f64,
            client_y: e.client_y() as f64,
            page_x: e.page_x() as f64,
            page_y: e.page_y() as f64,
            offset_x: e.element_x() as f64,
            offset_y: e.element_y() as f64,
            button: e.button as u8,
            buttons: e.buttons.bits() as u16,
            ctrl_key: e.mods.contains(keyboard_types::Modifiers::CONTROL),
            shift_key: e.mods.contains(keyboard_types::Modifiers::SHIFT),
            alt_key: e.mods.contains(keyboard_types::Modifiers::ALT),
            meta_key: e.mods.contains(keyboard_types::Modifiers::META),
        }
    }

    /// Static coords + modifiers from a wheel event (the `Mouse` fields of
    /// `Wheel`; the wheel button is reported as 0, matching the previous
    /// instance-property behavior).
    pub(crate) fn from_wheel(e: &BlitzWheelEvent) -> Self {
        Self {
            screen_x: e.coords.screen_x as f64,
            screen_y: e.coords.screen_y as f64,
            client_x: e.coords.client_x as f64,
            client_y: e.coords.client_y as f64,
            page_x: 0.0,
            page_y: 0.0,
            offset_x: 0.0,
            offset_y: 0.0,
            button: 0,
            buttons: e.buttons.bits() as u16,
            ctrl_key: e.mods.contains(keyboard_types::Modifiers::CONTROL),
            shift_key: e.mods.contains(keyboard_types::Modifiers::SHIFT),
            alt_key: e.mods.contains(keyboard_types::Modifiers::ALT),
            meta_key: e.mods.contains(keyboard_types::Modifiers::META),
        }
    }
}

pub(crate) type MouseEvent = Extended<MouseEventLayer>;

impl ExtendLayer for MouseEventLayer {
    type Parent = UIEventLayer;
    const CLASS_NAME: &'static str = "MouseEvent";

    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        let done = sup.call(args, ctx)?;
        Ok(Constructed::new(done, MouseEventLayer::default()))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_getter!(class, "screenX", js_fn_ptr!(screen_x_getter, &realm), attr);
        instance_getter!(class, "screenY", js_fn_ptr!(screen_y_getter, &realm), attr);
        instance_getter!(class, "clientX", js_fn_ptr!(client_x_getter, &realm), attr);
        instance_getter!(class, "clientY", js_fn_ptr!(client_y_getter, &realm), attr);
        instance_getter!(class, "x", js_fn_ptr!(client_x_getter, &realm), attr);
        instance_getter!(class, "y", js_fn_ptr!(client_y_getter, &realm), attr);
        instance_getter!(class, "pageX", js_fn_ptr!(page_x_getter, &realm), attr);
        instance_getter!(class, "pageY", js_fn_ptr!(page_y_getter, &realm), attr);
        instance_getter!(class, "offsetX", js_fn_ptr!(offset_x_getter, &realm), attr);
        instance_getter!(class, "offsetY", js_fn_ptr!(offset_y_getter, &realm), attr);
        instance_getter!(class, "button", js_fn_ptr!(button_getter, &realm), attr);
        instance_getter!(class, "buttons", js_fn_ptr!(buttons_getter, &realm), attr);
        instance_getter!(class, "ctrlKey", js_fn_ptr!(ctrl_key_getter, &realm), attr);
        instance_getter!(
            class,
            "shiftKey",
            js_fn_ptr!(shift_key_getter, &realm),
            attr
        );
        instance_getter!(class, "altKey", js_fn_ptr!(alt_key_getter, &realm), attr);
        instance_getter!(class, "metaKey", js_fn_ptr!(meta_key_getter, &realm), attr);

        Ok(())
    }
}

/// Register the `MouseEvent` class and wire up the
/// `MouseEvent -> UIEvent` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<MouseEvent>()?;
    crate::shared::link_prototype::<MouseEvent>(context)?;
    Ok(())
}

fn this_mouse_layer(this: &JsValue) -> JsResult<MouseEventLayer> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not a MouseEvent object"))?;
    with_own::<MouseEventLayer, _>(&obj, MouseEventLayer::clone)
}

fn screen_x_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.screen_x))
}

fn screen_y_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.screen_y))
}

fn client_x_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.client_x))
}

fn client_y_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.client_y))
}

fn page_x_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.page_x))
}

fn page_y_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.page_y))
}

fn offset_x_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.offset_x))
}

fn offset_y_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.offset_y))
}

fn button_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.button))
}

fn buttons_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.buttons))
}

fn ctrl_key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.ctrl_key))
}

fn shift_key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.shift_key))
}

fn alt_key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.alt_key))
}

fn meta_key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_mouse_layer(this)?.meta_key))
}
