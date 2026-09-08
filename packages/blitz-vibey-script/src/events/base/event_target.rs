//! The `EventTarget` class — root of the event-target side of the DOM.
//!
//! Registered listeners live in the layer's own block (a `GcRefCell<Vec<..>>`
//! so the list can be mutated from within a dispatched callback), which makes
//! `new EventTarget()` instances first-class targets alongside DOM node
//! wrappers. The Rust-side dispatch driver invokes
//! [`EventTargetLayer::invoke_listeners`] on a receiver's own block; the
//! three-phase chain walk lives there.

use boa_engine::class::ClassBuilder;
use boa_engine::gc::GcRefCell;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsObject, JsResult, JsValue, Trace};

use crate::dom::{dom_ctx, dom_exception, this_node_id, to_rust_string};
use crate::events::base::event::{
    AT_TARGET_PHASE, DispatchTarget, EventLayer, EventState, NONE_PHASE, event_flag, with_event,
    with_state, with_state_mut,
};
use crate::runtime::dispatch_event_on_chain;
use crate::shared::{
    Constructed, ExtendLayer, Extended, RootLayer, Super, instance_accessor, instance_method,
    js_copy_closure_with_captures, native_error, native_fn_ptr, with_own,
};
use crate::state::DomCtx;

/// The callable backing one registered listener, in its registration shape.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub(crate) enum ListenerCallback {
    /// `addEventListener(type, fn)`.
    Function(JsObject),
    /// `addEventListener(type, listenerObject)` — invoked through the
    /// object's `handleEvent`, with the listener object as `this`.
    /// `handleEvent` is read off the object at call time, so a replacement
    /// takes effect.
    HandlerObject(JsObject),
    /// The `on<event>` attribute handler, always a function. `this` is the
    /// current receiver, like `Function`.
    AttributeFunction(JsObject),
}

impl ListenerCallback {
    /// The callable to invoke for this shape. A handler object's
    /// `handleEvent` is resolved live; a missing/non-callable handle yields
    /// a `TypeError` (reported as a listener error by the invoker).
    fn callable(&self, context: &mut Context) -> JsResult<JsObject> {
        match self {
            Self::Function(callable) | Self::AttributeFunction(callable) => Ok(callable.clone()),
            Self::HandlerObject(obj) => Ok(obj
                .get(boa_engine::js_string!("handleEvent"), context)?
                .as_object()
                .filter(|handle| handle.is_callable())
                .ok_or_else(|| {
                    native_error!(typ, "listener object has no callable `handleEvent`")
                })?),
        }
    }

    /// The `this` value for the invocation. `None` means the current
    /// receiver (the event's `currentTarget`).
    fn this_value(&self) -> Option<JsValue> {
        match self {
            Self::HandlerObject(obj) => Some(obj.clone().into()),
            _ => None,
        }
    }

    /// Whether this is an `on<event>` attribute handler.
    fn is_attribute(&self) -> bool {
        matches!(self, Self::AttributeFunction(_))
    }

    /// Strict-equality identity for duplicate/removal checks: two callbacks
    /// match when they share the shape and the underlying JS value
    /// (`Function`/`AttributeFunction` compare the function, `HandlerObject`
    /// compares the listener object).
    fn same_registration(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Function(a), Self::Function(b)) => JsObject::equals(a, b),
            (Self::HandlerObject(a), Self::HandlerObject(b)) => JsObject::equals(a, b),
            _ => false,
        }
    }
}

/// One registered listener.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub(crate) struct ListenerEntry {
    #[unsafe_ignore_trace]
    pub event_type: String,
    pub callback: ListenerCallback,
    #[unsafe_ignore_trace]
    pub capture: bool,
    #[unsafe_ignore_trace]
    pub once: bool,
}

/// `EventTarget` own block: the listeners registered on this target.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct EventTargetLayer {
    pub listeners: GcRefCell<Vec<ListenerEntry>>,
}

pub(crate) type EventTarget = Extended<EventTargetLayer>;

impl ExtendLayer for EventTargetLayer {
    type Parent = RootLayer;
    const CLASS_NAME: &'static str = "EventTarget";

    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, RootLayer>,
    ) -> JsResult<Constructed<Self>> {
        // `new EventTarget()` is standard: an inert target that scripts can
        // register listeners on and dispatch events to.
        let done = sup.call(args, ctx)?;
        Ok(Constructed::new(
            done,
            EventTargetLayer {
                listeners: GcRefCell::default(),
            },
        ))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        instance_method!(
            class,
            "addEventListener",
            2,
            native_fn_ptr!(add_event_listener)
        );
        instance_method!(
            class,
            "removeEventListener",
            2,
            native_fn_ptr!(remove_event_listener)
        );
        instance_method!(class, "dispatchEvent", 1, native_fn_ptr!(dispatch_event));

        Ok(())
    }
}

/// Register the `EventTarget` class (a root class: no prototype link).
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<EventTarget>()?;
    Ok(())
}

// ── Listener management ──────────────────────────────────────────────

fn this_target(this: &JsValue) -> JsResult<JsObject> {
    this.as_object()
        .ok_or_else(|| native_error!(typ, "not an EventTarget object"))
}

/// Parse the callback argument (second position) into its registration
/// shape: a callable function, or an event-listener object with a callable
/// `handleEvent`. Any other value registers nothing.
fn parse_listener_callback(
    value: Option<&JsValue>,
    context: &mut Context,
) -> Option<ListenerCallback> {
    let obj = value?.as_object()?;
    if obj.is_callable() {
        return Some(ListenerCallback::Function(obj));
    }
    let has_callable_handle = obj
        .get(boa_engine::js_string!("handleEvent"), context)
        .ok()?
        .as_object()
        .filter(|handle| handle.is_callable())
        .is_some();
    has_callable_handle.then_some(ListenerCallback::HandlerObject(obj))
}

/// Parse the `options` argument: a `capture` boolean or an options object.
fn capture_option(args: &[JsValue], context: &mut Context) -> JsResult<bool> {
    match args.get(2) {
        Some(options) if options.is_object() => {
            let options = options.as_object().unwrap();
            Ok(options
                .get(boa_engine::js_string!("capture"), context)?
                .to_boolean())
        }
        Some(options) => Ok(options.to_boolean()),
        None => Ok(false),
    }
}

pub(crate) fn add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // WebIDL semantics: an interface operation called with no receiver
    // (or detached) receives `undefined` as `this`; it binds the global
    // this — the realm's `globalThis`, i.e. the window.
    let this = if this.is_null_or_undefined() {
        context.global_object().clone().into()
    } else {
        this.clone()
    };
    let obj = this_target(&this)?;
    let event_type = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let Some(callback) = parse_listener_callback(args.get(1), context) else {
        return Ok(JsValue::undefined());
    };
    let capture = capture_option(args, context)?;
    // `once` is only read off the options object (the boolean form has none).
    let once = args
        .get(2)
        .and_then(|options| options.as_object())
        .and_then(|options| {
            options
                .get(boa_engine::js_string!("once"), context)
                .ok()
                .map(|value| value.to_boolean())
        })
        .unwrap_or(false);

    with_own::<EventTargetLayer, _>(&obj, |target| {
        let mut listeners = target.listeners.borrow_mut();
        // Duplicate listeners (same callback shape + capture flag) are ignored
        if !listeners.iter().any(|l| {
            l.event_type == event_type
                && l.capture == capture
                && l.callback.same_registration(&callback)
        }) {
            listeners.push(ListenerEntry {
                event_type,
                callback,
                capture,
                once,
            });
        }
    })?;

    Ok(JsValue::undefined())
}

pub(crate) fn remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    // Same global-this binding as `add_event_listener`.
    let this = if this.is_null_or_undefined() {
        context.global_object().clone().into()
    } else {
        this.clone()
    };
    let obj = this_target(&this)?;
    let event_type = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let Some(callback) = parse_listener_callback(args.get(1), context) else {
        return Ok(JsValue::undefined());
    };
    let capture = capture_option(args, context)?;

    with_own::<EventTargetLayer, _>(&obj, |target| {
        target.listeners.borrow_mut().retain(|l| {
            !(l.event_type == event_type
                && l.capture == capture
                && l.callback.same_registration(&callback))
        });
    })?;

    Ok(JsValue::undefined())
}

impl EventTargetLayer {
    /// The registered `on<event>` attribute listener for `event_type`, if any.
    pub(crate) fn attribute_listener(
        &self,
        event_type: &str,
        context: &mut Context,
    ) -> Option<JsObject> {
        self.listeners
            .borrow()
            .iter()
            .find(|l| l.event_type == event_type && l.callback.is_attribute())
            .map(|l| l.callback.callable(context))
            .transpose()
            .ok()
            .flatten()
    }

    /// Replace the `on<event>` attribute listener for `event_type`: any
    /// previous attribute listener goes away and `handler` (always a
    /// callable) is registered in the attribute shape.
    pub(crate) fn set_attribute_listener(&self, event_type: &str, handler: JsObject) {
        let mut listeners = self.listeners.borrow_mut();
        listeners.retain(|l| !(l.event_type == event_type && l.callback.is_attribute()));
        listeners.push(ListenerEntry {
            event_type: event_type.to_string(),
            callback: ListenerCallback::AttributeFunction(handler),
            capture: false,
            once: false,
        });
    }

    /// Remove the `on<event>` attribute listener for `event_type` (the
    /// attribute was assigned a non-callable value).
    pub(crate) fn remove_attribute_listener(&self, event_type: &str) {
        self.listeners
            .borrow_mut()
            .retain(|l| !(l.event_type == event_type && l.callback.is_attribute()));
    }

    /// Invoke this target's listeners matching `event_type` and the capture
    /// flag, in registration order. The event's `currentTarget` supplies the
    /// invocation `this` (except for `handleEvent` objects, which receive
    /// themselves), and the event state is read back for
    /// `stopPropagation` / `stopImmediatePropagation`. Returns
    /// `(propagation_stopped, any_listener_called)`.
    pub(crate) fn invoke_listeners(
        &self,
        event_obj: &JsObject,
        event_type: &str,
        capture: bool,
        ctx: &DomCtx,
        context: &mut Context,
    ) -> (bool, bool) {
        // Snapshot the matching callbacks, so callbacks never run while the
        // list is borrowed and re-entrant registration stays safe. `once`
        // entries are removed right before their call, so a listener halted
        // by `stopImmediatePropagation` stays registered.
        let callbacks: Vec<(ListenerCallback, bool)> = {
            let listeners = self.listeners.borrow();
            listeners
                .iter()
                .filter(|l| l.event_type == event_type && l.capture == capture)
                .map(|l| (l.callback.clone(), l.once))
                .collect()
        };

        let current_target = with_state(event_obj, |st| st.current_target.clone())
            .ok()
            .and_then(|mut target| target.resolve(context).ok())
            .unwrap_or_else(JsValue::undefined);

        let mut called = false;
        for (callback, once) in callbacks {
            // A previously-run listener may have stopped the dispatch; the
            // remaining ones (including un-fired `once` entries) must not run.
            if with_state(event_obj, |st| st.stop_immediate).unwrap_or(false) {
                return (true, called);
            }
            called = true;
            if once {
                self.listeners.borrow_mut().retain(|l| {
                    !(l.once
                        && l.event_type == event_type
                        && l.capture == capture
                        && l.callback.same_registration(&callback))
                });
            }
            let this_value = callback
                .this_value()
                .unwrap_or_else(|| current_target.clone());
            let invocation = callback.callable(context).and_then(|callable| {
                callable.call(&this_value, &[event_obj.clone().into()], context)
            });
            if let Err(error) = invocation {
                crate::runtime::report_js_error(ctx, "event listener", &error);
            }
        }

        (
            with_state(event_obj, |st| st.stop_propagation).unwrap_or(false),
            called,
        )
    }
}

fn dispatch_event(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    // Same global-this binding as the listener operations.
    let this = if this.is_null_or_undefined() {
        context.global_object().clone().into()
    } else {
        this.clone()
    };
    let obj = this_target(&this)?;
    let ctx = dom_ctx(context)?;
    let Some(event_obj) = args.first().and_then(|value| value.as_object()) else {
        return Err(native_error!(
            typ,
            "dispatchEvent: argument is not an Event"
        ));
    };
    let event_type = with_own::<EventLayer, _>(&event_obj, |e| e.type_.clone())?;
    let bubbles = with_event(&event_obj, |e| e.bubbles)?;

    // An event that is already being dispatched cannot be dispatched again.
    let dispatching = with_state(&event_obj, |st| st.dispatching)?;
    if dispatching {
        return Err(dom_exception(
            context,
            "InvalidStateError",
            "Failed to execute 'dispatchEvent' on 'EventTarget': the event is already being dispatched",
        ));
    }

    // A DOM node target walks the full capture/target/bubble chain over the
    // node's DOM ancestors; a plain `EventTarget` dispatches to itself only.
    if let Ok(node_id) = this_node_id(&this) {
        let chain = {
            let doc = ctx.doc.borrow();
            doc.node_chain(node_id)
        };
        with_state_mut(&event_obj, |st| {
            st.dispatching = true;
            st.target = DispatchTarget::from_value(obj.clone().into());
        })?;
        let mut event_state = blitz_traits::events::EventState::default();
        dispatch_event_on_chain(
            &ctx,
            &chain,
            &event_type,
            bubbles,
            &event_obj,
            &mut event_state,
            context,
        );
        return Ok(JsValue::from(!with_state(&event_obj, |st| st.canceled)?));
    }

    // A synthetic single-target dispatch: `target` and `currentTarget` are
    // this object, phase is AT_TARGET, and both listener flavors fire.
    with_state_mut(&event_obj, |st| {
        *st = EventState {
            target: DispatchTarget::from_value(obj.clone().into()),
            current_target: DispatchTarget::from_value(obj.clone().into()),
            phase: AT_TARGET_PHASE,
            dispatching: true,
            ..EventState::default()
        };
    })?;

    with_own::<EventTargetLayer, _>(&obj, |target| {
        // Capture-registered listeners run before the non-capture ones.
        target.invoke_listeners(&event_obj, &event_type, true, &ctx, context);
        // stopImmediatePropagation ends the dispatch; plain stopPropagation
        // leaves this target's remaining listeners in place.
        if !event_flag(&event_obj, |st| st.stop_immediate) {
            target.invoke_listeners(&event_obj, &event_type, false, &ctx, context);
        }
    })?;

    with_state_mut(&event_obj, |st| {
        st.current_target = DispatchTarget::None;
        st.phase = NONE_PHASE;
        st.dispatching = false;
    })?;

    Ok(JsValue::from(!with_state(&event_obj, |st| st.canceled)?))
}

// ── `on<event>` IDL-style attributes ─────────────────────────────────

/// Define the `on<event>` IDL-style attributes for `types` on the class
/// prototype: assigning a callable registers it as the event's attribute
/// listener (replacing any previous one), assigning anything else removes
/// it. The getter reflects the registered handler. Each class defines its
/// own set of event types (`Node`, `Window`), per the HTML spec's handler
/// mixins.
pub(crate) fn define_on_event_attributes(
    class: &mut ClassBuilder<'_>,
    types: &'static [&'static str],
) {
    let realm = class.context().realm().clone();
    for event_type in types.iter().copied() {
        let getter = js_copy_closure_with_captures!(
            |this, _args, event_type: &&'static str, context| {
                let obj = this
                    .as_object()
                    .ok_or_else(|| native_error!(typ, "not an event target"))?;
                let handler = with_own::<EventTargetLayer, _>(&obj, |target| {
                    target.attribute_listener(event_type, context)
                })?;
                Ok(handler.map(JsValue::from).unwrap_or_else(JsValue::null))
            },
            event_type,
            &realm
        );
        let setter = js_copy_closure_with_captures!(
            |this, args, event_type: &&'static str, _context| {
                let obj = this
                    .as_object()
                    .ok_or_else(|| native_error!(typ, "not an event target"))?;
                let handler = args.first().and_then(|value| value.as_object());
                with_own::<EventTargetLayer, _>(&obj, |target| {
                    match handler.filter(|candidate| candidate.is_callable()) {
                        Some(handler) => target.set_attribute_listener(event_type, handler),
                        None => target.remove_attribute_listener(event_type),
                    }
                })?;
                Ok(JsValue::undefined())
            },
            event_type,
            &realm
        );
        instance_accessor!(
            class,
            format!("on{event_type}"),
            getter,
            setter,
            Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE
        );
    }
}
