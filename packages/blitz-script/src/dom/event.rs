//! JS `Event` objects dispatched to script event listeners.

use std::cell::Cell;

use blitz_traits::events::{BlitzKeyEvent, BlitzPointerEvent, BlitzWheelDelta, DomEventData};
use boa_engine::object::JsObject;
use boa_engine::value::JsValue;
use boa_engine::{Context, Finalize, JsData, JsResult, Trace};
use keyboard_types::Modifiers;

use super::{define_accessor, define_method, define_value, js_str};
use crate::state::DomCtx;

/// Native data attached to JS `Event` objects. Tracks the flags set by
/// `preventDefault` / `stopPropagation` so they can be read back after dispatch.
#[derive(Default, Trace, Finalize, JsData)]
pub(crate) struct EventRef {
    #[unsafe_ignore_trace]
    pub prevented: Cell<bool>,
    #[unsafe_ignore_trace]
    pub stopped: Cell<bool>,
    #[unsafe_ignore_trace]
    pub stopped_immediate: Cell<bool>,
}

pub(crate) fn init_event_proto(proto: &JsObject, context: &mut Context) {
    define_method(proto, "preventDefault", 0, prevent_default, context);
    define_method(proto, "stopPropagation", 0, stop_propagation, context);
    define_method(
        proto,
        "stopImmediatePropagation",
        0,
        stop_immediate_propagation,
        context,
    );
    define_accessor(
        proto,
        "defaultPrevented",
        Some(default_prevented),
        None,
        context,
    );
}

fn event_ref<T>(this: &JsValue, f: impl FnOnce(&EventRef) -> T) -> Option<T> {
    this.as_object()
        .and_then(|obj| obj.downcast_ref::<EventRef>().map(|event| f(&event)))
}

fn prevent_default(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    event_ref(this, |event| event.prevented.set(true));
    Ok(JsValue::undefined())
}

fn stop_propagation(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    event_ref(this, |event| event.stopped.set(true));
    Ok(JsValue::undefined())
}

fn stop_immediate_propagation(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    event_ref(this, |event| {
        event.stopped.set(true);
        event.stopped_immediate.set(true);
    });
    Ok(JsValue::undefined())
}

fn default_prevented(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(
        event_ref(this, |event| event.prevented.get()).unwrap_or(false),
    ))
}

/// Create a JS event object with the standard `Event` fields
pub(crate) fn create_event(
    ctx: &DomCtx,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    target: &JsValue,
    context: &mut Context,
) -> JsObject {
    let proto = ctx.state.borrow().protos().event.clone();
    let event = JsObject::from_proto_and_data(Some(proto), EventRef::default());
    define_value(&event, "type", js_str(event_type), context);
    define_value(&event, "target", target.clone(), context);
    define_value(&event, "srcElement", target.clone(), context);
    define_value(&event, "currentTarget", JsValue::null(), context);
    define_value(&event, "bubbles", JsValue::from(bubbles), context);
    define_value(&event, "cancelable", JsValue::from(cancelable), context);
    define_value(&event, "composed", JsValue::from(false), context);
    define_value(&event, "isTrusted", JsValue::from(true), context);
    define_value(&event, "eventPhase", JsValue::from(2), context);
    let timestamp = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    define_value(&event, "timeStamp", JsValue::from(timestamp), context);
    event
}

fn add_modifiers(event: &JsObject, mods: Modifiers, context: &mut Context) {
    define_value(
        event,
        "ctrlKey",
        JsValue::from(mods.contains(Modifiers::CONTROL)),
        context,
    );
    define_value(
        event,
        "shiftKey",
        JsValue::from(mods.contains(Modifiers::SHIFT)),
        context,
    );
    define_value(
        event,
        "altKey",
        JsValue::from(mods.contains(Modifiers::ALT)),
        context,
    );
    define_value(
        event,
        "metaKey",
        JsValue::from(mods.contains(Modifiers::META)),
        context,
    );
}

fn add_pointer_fields(event: &JsObject, data: &BlitzPointerEvent, context: &mut Context) {
    define_value(
        event,
        "clientX",
        JsValue::from(data.client_x() as f64),
        context,
    );
    define_value(
        event,
        "clientY",
        JsValue::from(data.client_y() as f64),
        context,
    );
    define_value(event, "x", JsValue::from(data.client_x() as f64), context);
    define_value(event, "y", JsValue::from(data.client_y() as f64), context);
    define_value(event, "pageX", JsValue::from(data.page_x() as f64), context);
    define_value(event, "pageY", JsValue::from(data.page_y() as f64), context);
    define_value(
        event,
        "screenX",
        JsValue::from(data.screen_x() as f64),
        context,
    );
    define_value(
        event,
        "screenY",
        JsValue::from(data.screen_y() as f64),
        context,
    );
    define_value(
        event,
        "offsetX",
        JsValue::from(data.element_x() as f64),
        context,
    );
    define_value(
        event,
        "offsetY",
        JsValue::from(data.element_y() as f64),
        context,
    );
    define_value(event, "button", JsValue::from(data.button as u8), context);
    define_value(
        event,
        "buttons",
        JsValue::from(data.buttons.bits()),
        context,
    );
    define_value(event, "detail", JsValue::from(1), context);
    add_modifiers(event, data.mods, context);
}

fn add_key_fields(event: &JsObject, data: &BlitzKeyEvent, context: &mut Context) {
    define_value(event, "key", js_str(&data.key.to_string()), context);
    define_value(event, "code", js_str(&data.code.to_string()), context);
    define_value(
        event,
        "location",
        JsValue::from(data.location as u32),
        context,
    );
    define_value(
        event,
        "repeat",
        JsValue::from(data.is_auto_repeating),
        context,
    );
    define_value(
        event,
        "isComposing",
        JsValue::from(data.is_composing),
        context,
    );
    add_modifiers(event, data.modifiers, context);
}

/// Create a JS event object for a Blitz [`DomEventData`], populating
/// type-specific fields (mouse coordinates, key names, etc)
pub(crate) fn create_event_for_dom_event(
    ctx: &DomCtx,
    data: &DomEventData,
    bubbles: bool,
    cancelable: bool,
    target: &JsValue,
    context: &mut Context,
) -> JsObject {
    let event = create_event(ctx, data.name(), bubbles, cancelable, target, context);

    match data {
        DomEventData::PointerMove(pointer)
        | DomEventData::PointerDown(pointer)
        | DomEventData::PointerUp(pointer)
        | DomEventData::PointerCancel(pointer)
        | DomEventData::PointerEnter(pointer)
        | DomEventData::PointerLeave(pointer)
        | DomEventData::PointerOver(pointer)
        | DomEventData::PointerOut(pointer)
        | DomEventData::MouseMove(pointer)
        | DomEventData::MouseDown(pointer)
        | DomEventData::MouseUp(pointer)
        | DomEventData::MouseEnter(pointer)
        | DomEventData::MouseLeave(pointer)
        | DomEventData::MouseOver(pointer)
        | DomEventData::MouseOut(pointer)
        | DomEventData::TouchStart(pointer)
        | DomEventData::TouchMove(pointer)
        | DomEventData::TouchEnd(pointer)
        | DomEventData::TouchCancel(pointer)
        | DomEventData::Click(pointer)
        | DomEventData::ContextMenu(pointer)
        | DomEventData::DoubleClick(pointer) => {
            add_pointer_fields(&event, pointer, context);
        }

        DomEventData::KeyPress(key) | DomEventData::KeyDown(key) | DomEventData::KeyUp(key) => {
            add_key_fields(&event, key, context);
        }

        DomEventData::Wheel(wheel) => {
            let (delta_x, delta_y, delta_mode) = match wheel.delta {
                BlitzWheelDelta::Lines(x, y) => (x, y, 1),
                BlitzWheelDelta::Pixels(x, y) => (x, y, 0),
            };
            define_value(&event, "deltaX", JsValue::from(delta_x), context);
            define_value(&event, "deltaY", JsValue::from(delta_y), context);
            define_value(&event, "deltaZ", JsValue::from(0.0), context);
            define_value(&event, "deltaMode", JsValue::from(delta_mode), context);
            add_modifiers(&event, wheel.mods, context);
        }

        DomEventData::Input(input) => {
            define_value(&event, "data", js_str(&input.value), context);
            define_value(&event, "inputType", js_str("insertText"), context);
        }

        _ => {}
    }

    event
}
