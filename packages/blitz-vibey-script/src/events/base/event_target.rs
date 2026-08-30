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
use boa_engine::{Context, Finalize, JsData, JsObject, JsResult, JsValue, Trace};

use crate::dom::{dom_ctx, to_rust_string};
use crate::events::base::event::{
    AT_TARGET_PHASE, DispatchTarget, EventLayer, EventState, NONE_PHASE, with_state, with_state_mut,
};
use crate::shared::{
    Constructed, ExtendLayer, Extended, RootLayer, Super, instance_method, native_fn_ptr, with_own,
};
use crate::state::DomCtx;

/// One registered listener.
#[derive(Debug, Clone, Trace, Finalize, JsData)]
pub(crate) struct ListenerEntry {
    #[unsafe_ignore_trace]
    pub event_type: String,
    pub callback: JsObject,
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
        .ok_or_else(|| crate::shared::native_error!(typ, "not an EventTarget object"))
}

/// Extract the callback argument (second position), requiring a callable.
fn callback_arg(args: &[JsValue]) -> Option<JsObject> {
    args.get(1)
        .and_then(|value| value.as_object())
        .filter(|callback| callback.is_callable())
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

fn add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let obj = this_target(this)?;
    let event_type = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let Some(callback) = callback_arg(args) else {
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
        // Duplicate listeners (same callback + capture flag) are ignored
        if !listeners.iter().any(|l| {
            l.event_type == event_type
                && l.capture == capture
                && JsObject::equals(&l.callback, &callback)
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

fn remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let obj = this_target(this)?;
    let event_type = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let Some(callback) = callback_arg(args) else {
        return Ok(JsValue::undefined());
    };
    let capture = capture_option(args, context)?;

    with_own::<EventTargetLayer, _>(&obj, |target| {
        target.listeners.borrow_mut().retain(|l| {
            !(l.event_type == event_type
                && l.capture == capture
                && JsObject::equals(&l.callback, &callback))
        });
    })?;

    Ok(JsValue::undefined())
}

impl EventTargetLayer {
    /// Invoke this target's listeners matching `event_type` and the capture
    /// flag, in registration order. The event's `currentTarget` supplies the
    /// invocation `this`, and its state is read back for
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
        // Snapshot the matching callbacks and drop fired `once` entries up
        // front, so callbacks never run while the list is borrowed and
        // re-entrant registration is safe.
        let callbacks: Vec<JsObject> = {
            let mut listeners = self.listeners.borrow_mut();
            let matching: Vec<JsObject> = listeners
                .iter()
                .filter(|l| l.event_type == event_type && l.capture == capture)
                .map(|l| l.callback.clone())
                .collect();
            listeners.retain(|l| !(l.once && l.event_type == event_type && l.capture == capture));
            matching
        };

        let current_target = with_state(event_obj, |st| st.current_target.clone())
            .ok()
            .and_then(|mut target| target.resolve(context).ok())
            .unwrap_or_else(JsValue::undefined);

        let mut called = false;
        for callback in callbacks {
            called = true;
            if let Err(error) = callback.call(&current_target, &[event_obj.clone().into()], context)
            {
                crate::runtime::report_js_error(ctx, "event listener", &error);
            }
            if with_state(event_obj, |st| st.stop_immediate).unwrap_or(false) {
                return (true, called);
            }
        }

        (
            with_state(event_obj, |st| st.stop_propagation).unwrap_or(false),
            called,
        )
    }
}

fn dispatch_event(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let obj = this_target(this)?;
    let ctx = dom_ctx(context)?;
    let Some(event_obj) = args.first().and_then(|value| value.as_object()) else {
        return Err(crate::shared::native_error!(
            typ,
            "dispatchEvent: argument is not an Event"
        ));
    };
    let event_type = with_own::<EventLayer, _>(&event_obj, |e| e.type_.clone())?;

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
        target.invoke_listeners(&event_obj, &event_type, false, &ctx, context);
        target.invoke_listeners(&event_obj, &event_type, true, &ctx, context);
    })?;

    with_state_mut(&event_obj, |st| {
        st.current_target = DispatchTarget::None;
        st.phase = NONE_PHASE;
        st.dispatching = false;
    })?;

    Ok(JsValue::from(!with_state(&event_obj, |st| st.canceled)?))
}
