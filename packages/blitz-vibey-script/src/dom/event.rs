//! JS `Event` class dispatched to script event listeners.
//!
//! Core configuration (`type`, `target`, `bubbles`, `cancelable`, ...) and the
//! dispatch flags set by `preventDefault` / `stopPropagation` live in the
//! `EventLayer` own block. Type-specific fields (pointer coordinates, key
//! names, ...) are attached as plain properties on the instance at creation
//! time (see `create_event_for_dom_event`).

use std::cell::Cell;

use blitz_traits::events::{BlitzKeyEvent, BlitzPointerEvent, BlitzWheelDelta, DomEventData};
use boa_engine::class::ClassBuilder;
use boa_engine::gc::GcRefCell;
use boa_engine::object::JsObject;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};
use keyboard_types::Modifiers;

use crate::shared::{
    Constructed, ExtendLayer, Extended, RootLayer, Super, from_chain, instance_getter,
    instance_method, js_fn_ptr, native_fn_ptr, with_own, with_own_mut,
};
use crate::state::DomCtx;

use super::{define_value, js_str, to_rust_string};

/// `Event` own block: configuration + dispatch flags.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub(crate) struct EventLayer {
    #[unsafe_ignore_trace]
    pub type_: String,
    pub target: JsValue,
    /// Updated by the dispatcher as the event propagates.
    pub current_target: GcRefCell<JsValue>,
    #[unsafe_ignore_trace]
    pub bubbles: bool,
    #[unsafe_ignore_trace]
    pub cancelable: bool,
    #[unsafe_ignore_trace]
    pub prevented: Cell<bool>,
    #[unsafe_ignore_trace]
    pub stopped: Cell<bool>,
    #[unsafe_ignore_trace]
    pub stopped_immediate: Cell<bool>,
    #[unsafe_ignore_trace]
    pub time_stamp: f64,
}

impl EventLayer {
    pub fn new(type_: String, bubbles: bool, cancelable: bool, target: JsValue) -> Self {
        let time_stamp = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Self {
            type_,
            target,
            current_target: GcRefCell::new(JsValue::null()),
            bubbles,
            cancelable,
            prevented: Cell::new(false),
            stopped: Cell::new(false),
            stopped_immediate: Cell::new(false),
            time_stamp,
        }
    }
}

pub(crate) type Event = Extended<EventLayer>;

impl ExtendLayer for EventLayer {
    type Parent = RootLayer;
    const CLASS_NAME: &'static str = "Event";

    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, RootLayer>,
    ) -> JsResult<Constructed<Self>> {
        let type_ = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
        let (mut bubbles, mut cancelable) = (false, false);
        if let Some(init) = args.get(1).and_then(|value| value.as_object()) {
            bubbles = init
                .get(boa_engine::js_string!("bubbles"), ctx)?
                .to_boolean();
            cancelable = init
                .get(boa_engine::js_string!("cancelable"), ctx)?
                .to_boolean();
        }
        let done = sup.call(&[], ctx)?;
        Ok(Constructed::new(
            done,
            EventLayer::new(type_, bubbles, cancelable, JsValue::null()),
        ))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_getter!(class, "type", js_fn_ptr!(type_getter, &realm), attr);
        instance_getter!(class, "target", js_fn_ptr!(target_getter, &realm), attr);
        instance_getter!(class, "srcElement", js_fn_ptr!(target_getter, &realm), attr);
        instance_getter!(
            class,
            "currentTarget",
            js_fn_ptr!(current_target_getter, &realm),
            attr
        );
        instance_getter!(class, "bubbles", js_fn_ptr!(bubbles_getter, &realm), attr);
        instance_getter!(
            class,
            "cancelable",
            js_fn_ptr!(cancelable_getter, &realm),
            attr
        );
        instance_getter!(class, "composed", js_fn_ptr!(composed_getter, &realm), attr);
        instance_getter!(
            class,
            "isTrusted",
            js_fn_ptr!(is_trusted_getter, &realm),
            attr
        );
        instance_getter!(
            class,
            "eventPhase",
            js_fn_ptr!(event_phase_getter, &realm),
            attr
        );
        instance_getter!(
            class,
            "timeStamp",
            js_fn_ptr!(time_stamp_getter, &realm),
            attr
        );
        instance_getter!(
            class,
            "defaultPrevented",
            js_fn_ptr!(default_prevented, &realm),
            attr
        );

        instance_method!(class, "preventDefault", 0, native_fn_ptr!(prevent_default));
        instance_method!(
            class,
            "stopPropagation",
            0,
            native_fn_ptr!(stop_propagation)
        );
        instance_method!(
            class,
            "stopImmediatePropagation",
            0,
            native_fn_ptr!(stop_immediate_propagation)
        );
        // React's synthetic keyboard/mouse events call
        // `nativeEvent.getModifierState(...)`; without it, dispatching a
        // key/mouse event into React throws "not a callable function". Reads
        // back the event's own `ctrlKey`/`shiftKey`/`altKey`/`metaKey` fields.
        instance_method!(
            class,
            "getModifierState",
            1,
            native_fn_ptr!(get_modifier_state)
        );

        Ok(())
    }
}

/// Register the `Event` class (a root class: no prototype link).
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<Event>()?;
    Ok(())
}

// ── Access helpers ───────────────────────────────────────────────────

/// Read the event's own block.
#[inline]
pub(crate) fn with_event<R>(event: &JsObject, f: impl FnOnce(&EventLayer) -> R) -> JsResult<R> {
    with_own::<EventLayer, R>(event, f)
}

/// Write to the event's own block in place.
#[inline]
pub(crate) fn with_event_mut<R>(
    event: &JsObject,
    f: impl FnOnce(&mut EventLayer) -> R,
) -> JsResult<R> {
    with_own_mut::<EventLayer, R>(event, f)
}

/// Read a dispatch flag from the event (false if `event` is not an `Event`).
pub(crate) fn event_flag(event: &JsObject, f: impl FnOnce(&EventLayer) -> bool) -> bool {
    with_event(event, f).unwrap_or(false)
}

/// Update `currentTarget` on the event's own block (used by the dispatcher).
pub(crate) fn set_current_target(event: &JsObject, target: JsValue) -> JsResult<()> {
    with_event_mut(event, |e| *e.current_target.borrow_mut() = target)
}

// ── Accessor implementations ─────────────────────────────────────────

fn event_layer(this: &JsValue) -> JsResult<()> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not an Event object"))?;
    with_own::<EventLayer, _>(&obj, |_| ()).map(|_| ())
}

fn type_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    event_layer(this)?;
    let obj = this.as_object().unwrap();
    with_event(&obj, |e| js_str(&e.type_))
}

fn target_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    event_layer(this)?;
    let obj = this.as_object().unwrap();
    with_event(&obj, |e| e.target.clone())
}

fn current_target_getter(
    this: &JsValue,
    _: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    event_layer(this)?;
    let obj = this.as_object().unwrap();
    with_event(&obj, |e| e.current_target.borrow().clone())
}

fn bubbles_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    event_layer(this)?;
    let obj = this.as_object().unwrap();
    with_event(&obj, |e| JsValue::from(e.bubbles))
}

fn cancelable_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    event_layer(this)?;
    let obj = this.as_object().unwrap();
    with_event(&obj, |e| JsValue::from(e.cancelable))
}

fn composed_getter(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(false))
}

fn is_trusted_getter(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(true))
}

fn event_phase_getter(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(2))
}

fn time_stamp_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    event_layer(this)?;
    let obj = this.as_object().unwrap();
    with_event(&obj, |e| JsValue::from(e.time_stamp))
}

fn default_prevented(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    event_layer(this)?;
    let obj = this.as_object().unwrap();
    Ok(JsValue::from(event_flag(&obj, |e| e.prevented.get())))
}

fn prevent_default(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not an Event object"))?;
    with_event_mut(&obj, |e| {
        e.prevented.set(true);
    })?;
    Ok(JsValue::undefined())
}

fn stop_propagation(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not an Event object"))?;
    with_event_mut(&obj, |e| e.stopped.set(true))?;
    Ok(JsValue::undefined())
}

fn stop_immediate_propagation(
    this: &JsValue,
    _: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not an Event object"))?;
    with_event_mut(&obj, |e| {
        e.stopped.set(true);
        e.stopped_immediate.set(true);
    })?;
    Ok(JsValue::undefined())
}

fn get_modifier_state(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let key = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let prop = match key.as_str() {
        "Control" => "ctrlKey",
        "Shift" => "shiftKey",
        "Alt" => "altKey",
        "Meta" => "metaKey",
        _ => return Ok(JsValue::from(false)),
    };
    let state = this
        .as_object()
        .and_then(|obj| obj.get(boa_engine::JsString::from(prop), context).ok())
        .map(|value| value.to_boolean())
        .unwrap_or(false);
    Ok(JsValue::from(state))
}

// ── Construction ─────────────────────────────────────────────────────

/// Create a JS event object with the standard `Event` fields
pub(crate) fn create_event(
    _ctx: &DomCtx,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    target: &JsValue,
    context: &mut Context,
) -> JsObject {
    from_chain!(
        (Event, context)
        EventLayer::new(event_type.to_string(), bubbles, cancelable, target.clone()),
    )
    .expect("failed to create JS event")
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
