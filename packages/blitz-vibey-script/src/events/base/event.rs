//! JS `Event` class dispatched to script event listeners.
//!
//! Configuration fields (`type`, `bubbles`, `cancelable`, `timeStamp`) are
//! immutable and live flat in the `EventLayer` own block. Mutable dispatch
//! state (`target`, `currentTarget`, `eventPhase`, the flags set by
//! `preventDefault` / `stopPropagation`) lives in the `GcRefCell<EventState>`
//! block, written by the event methods and by the dispatch driver.
//!
//! `target` / `currentTarget` are held as [`DispatchTarget`] values: the
//! dispatch side never materializes node wrappers up front — a lazy callable
//! wraps the node only when the JS side first reads the getter (cached after
//! first resolution). Type-specific data lives in the derived layers
//! (`UIEvent`, `MouseEvent`, `KeyboardEvent`, ...); the creation helpers below
//! pick the right chain per `DomEventData` variant.

use blitz_traits::events::DomEventData;
use boa_engine::class::ClassBuilder;
use boa_engine::gc::GcRefCell;
use boa_engine::object::JsObject;
use boa_engine::object::builtins::JsFunction;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::dom::{js_str, to_rust_string};
use crate::events::{
    input_event::{InputEvent, InputEventLayer},
    keyboard_event::{KeyboardEvent, KeyboardEventLayer},
    mouse_event::{MouseEvent, MouseEventLayer},
    pointer_event::{PointerEvent, PointerEventLayer},
    ui_event::UIEventLayer,
    wheel_event::{WheelEvent, WheelEventLayer},
};
use crate::shared::{
    Constructed, ExtendLayer, Extended, RootLayer, Super, from_chain, instance_getter,
    instance_method, js_fn_ptr, native_fn_ptr, with_own,
};

// ── Dispatch target ──────────────────────────────────────────────────

/// A reference to the event's `target` / `currentTarget`.
///
/// The target is not always a node (it can be the window), and node wrappers
/// are built lazily — so the reference is held either as a direct value, or
/// as a `JsFunction` that produces it on first read (cached afterwards).
#[derive(Default, Clone, Debug, Trace, Finalize, JsData)]
pub(crate) enum DispatchTarget {
    /// No target assigned.
    #[default]
    None,
    /// A directly held value.
    Direct(JsValue),
    /// Lazily produces the target; the result is cached after first resolve.
    Callable {
        callable: JsFunction,
        cached: Option<JsValue>,
    },
}

impl DispatchTarget {
    pub(crate) fn from_value(value: JsValue) -> Self {
        Self::Direct(value)
    }

    pub(crate) fn from_callable(callable: JsFunction) -> Self {
        Self::Callable {
            callable,
            cached: None,
        }
    }

    /// Produce the target (`null` when unset). Resolving a `Callable` caches
    /// its result, so every read of `event.target` yields the same wrapper.
    pub(crate) fn resolve(&mut self, context: &mut Context) -> JsResult<JsValue> {
        match self {
            Self::None => Ok(JsValue::null()),
            Self::Direct(value) => Ok(value.clone()),
            Self::Callable { callable, cached } => {
                if let Some(value) = cached {
                    return Ok(value.clone());
                }
                let value = callable.call(&JsValue::null(), &[], context)?;
                *cached = Some(value.clone());
                Ok(value)
            }
        }
    }
}

// ── Mutable dispatch state ───────────────────────────────────────────

/// Per-event dispatch state, written by the event methods and by the
/// dispatch driver.
#[derive(Default, Clone, Debug, Trace, Finalize, JsData)]
pub(crate) struct EventState {
    pub target: DispatchTarget,
    pub current_target: DispatchTarget,
    pub phase: u8,
    pub canceled: bool,
    pub stop_propagation: bool,
    pub stop_immediate: bool,
    pub dispatching: bool,
}

/// Dispatch phases, as reported through `event.eventPhase`.
pub(crate) const NONE_PHASE: u8 = 0;
pub(crate) const CAPTURING_PHASE: u8 = 1;
pub(crate) const AT_TARGET_PHASE: u8 = 2;
pub(crate) const BUBBLING_PHASE: u8 = 3;

// ── Layer ────────────────────────────────────────────────────────────

/// `Event` own block: configuration fields + the dispatch state block.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub(crate) struct EventLayer {
    #[unsafe_ignore_trace]
    pub type_: String,
    #[unsafe_ignore_trace]
    pub bubbles: bool,
    #[unsafe_ignore_trace]
    pub cancelable: bool,
    #[unsafe_ignore_trace]
    pub time_stamp: f64,
    pub state: GcRefCell<EventState>,
}

impl EventLayer {
    pub(crate) fn new(type_: String, bubbles: bool, cancelable: bool) -> Self {
        let time_stamp = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Self {
            type_,
            bubbles,
            cancelable,
            time_stamp,
            state: GcRefCell::new(EventState::default()),
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
            EventLayer::new(type_, bubbles, cancelable),
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

/// Read from the event's dispatch state block.
#[inline]
pub(crate) fn with_state<R>(event: &JsObject, f: impl FnOnce(&EventState) -> R) -> JsResult<R> {
    with_event(event, |e| f(&e.state.borrow()))
}

/// Write to the event's dispatch state block.
#[inline]
pub(crate) fn with_state_mut<R>(
    event: &JsObject,
    f: impl FnOnce(&mut EventState) -> R,
) -> JsResult<R> {
    with_event(event, |e| f(&mut e.state.borrow_mut()))
}

/// Read a dispatch flag from the event's state (false if `event` is not an
/// `Event`).
pub(crate) fn event_flag(event: &JsObject, f: impl FnOnce(&EventState) -> bool) -> bool {
    with_state(event, f).unwrap_or(false)
}

// ── Accessor implementations ─────────────────────────────────────────

fn this_event(this: &JsValue) -> JsResult<JsObject> {
    this.as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not an Event object"))
}

fn type_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    with_event(&this_event(this)?, |e| js_str(&e.type_))
}

fn target_getter(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    with_state_mut(&this_event(this)?, |st| st.target.resolve(context))?
}

fn current_target_getter(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    with_state_mut(&this_event(this)?, |st| st.current_target.resolve(context))?
}

fn bubbles_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    with_event(&this_event(this)?, |e| JsValue::from(e.bubbles))
}

fn cancelable_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    with_event(&this_event(this)?, |e| JsValue::from(e.cancelable))
}

fn composed_getter(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(false))
}

fn is_trusted_getter(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(true))
}

fn event_phase_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    with_state(&this_event(this)?, |st| JsValue::from(st.phase))
}

fn time_stamp_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    with_event(&this_event(this)?, |e| JsValue::from(e.time_stamp))
}

fn default_prevented(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    with_state(&this_event(this)?, |st| JsValue::from(st.canceled))
}

fn prevent_default(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this_event(this)?;
    let cancelable = with_event(&obj, |e| e.cancelable)?;
    if cancelable {
        with_state_mut(&obj, |st| st.canceled = true)?;
    }
    Ok(JsValue::undefined())
}

fn stop_propagation(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    with_state_mut(&this_event(this)?, |st| st.stop_propagation = true)?;
    Ok(JsValue::undefined())
}

fn stop_immediate_propagation(
    this: &JsValue,
    _: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    with_state_mut(&this_event(this)?, |st| {
        st.stop_propagation = true;
        st.stop_immediate = true;
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

/// Create a bare JS `Event` object with the standard `Event` fields
pub(crate) fn create_event(
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    context: &mut Context,
) -> JsObject {
    from_chain!(
        (Event, context),
        EventLayer::new(event_type.to_string(), bubbles, cancelable),
    )
    .expect("failed to create JS event")
}

/// Create a JS event object for a Blitz [`DomEventData`], building the chain
/// that matches the event's interface (`WheelEvent`, `KeyboardEvent`, ...) and
/// populating the derived layers' own blocks
pub(crate) fn create_event_for_dom_event(
    data: &DomEventData,
    bubbles: bool,
    cancelable: bool,
    context: &mut Context,
) -> JsObject {
    let layer = EventLayer::new(data.name().to_string(), bubbles, cancelable);

    match data {
        // `MouseEvent`/`PointerEvent`-family events carry the same coordinate
        // and modifier data; touches ride the PointerEvent interface here
        // (there is no separate touch interface).
        DomEventData::PointerMove(pointer)
        | DomEventData::PointerDown(pointer)
        | DomEventData::PointerUp(pointer)
        | DomEventData::PointerCancel(pointer)
        | DomEventData::PointerEnter(pointer)
        | DomEventData::PointerLeave(pointer)
        | DomEventData::PointerOver(pointer)
        | DomEventData::PointerOut(pointer)
        | DomEventData::TouchStart(pointer)
        | DomEventData::TouchMove(pointer)
        | DomEventData::TouchEnd(pointer)
        | DomEventData::TouchCancel(pointer) => from_chain!(
            (PointerEvent, context),
            layer,
            UIEventLayer { detail: 1 },
            MouseEventLayer::from_pointer(pointer),
            PointerEventLayer,
        )
        .expect("failed to create JS PointerEvent"),

        DomEventData::MouseMove(pointer)
        | DomEventData::MouseDown(pointer)
        | DomEventData::MouseUp(pointer)
        | DomEventData::MouseEnter(pointer)
        | DomEventData::MouseLeave(pointer)
        | DomEventData::MouseOver(pointer)
        | DomEventData::MouseOut(pointer)
        | DomEventData::Click(pointer)
        | DomEventData::ContextMenu(pointer)
        | DomEventData::DoubleClick(pointer) => from_chain!(
            (MouseEvent, context),
            layer,
            UIEventLayer { detail: 1 },
            MouseEventLayer::from_pointer(pointer),
        )
        .expect("failed to create JS MouseEvent"),

        DomEventData::Wheel(wheel) => from_chain!(
            (WheelEvent, context),
            layer,
            UIEventLayer { detail: 0 },
            MouseEventLayer::from_wheel(wheel),
            WheelEventLayer::from_blitz(wheel),
        )
        .expect("failed to create JS WheelEvent"),

        DomEventData::KeyPress(key) | DomEventData::KeyDown(key) | DomEventData::KeyUp(key) => {
            from_chain!(
                (KeyboardEvent, context),
                layer,
                UIEventLayer { detail: 0 },
                KeyboardEventLayer::from_blitz(key),
            )
            .expect("failed to create JS KeyboardEvent")
        }

        DomEventData::Input(input) => from_chain!(
            (InputEvent, context),
            layer,
            UIEventLayer { detail: 0 },
            InputEventLayer::from_blitz(input),
        )
        .expect("failed to create JS InputEvent"),

        _ => create_event(data.name(), bubbles, cancelable, context),
    }
}
