//! The `EventTarget` class — root of the event-target side of the DOM.
//!
//! Listener registration and dispatch state stay in the runtime state
//! (`RuntimeState::node_listeners`, driven by the Rust-side dispatcher); this
//! layer contributes the `addEventListener`/`removeEventListener` interface
//! and serves as the parent layer of `Node`.

use boa_engine::class::ClassBuilder;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::{ExtendLayer, Extended, RootLayer, instance_method, native_fn_ptr};

use crate::dom::to_rust_string;

/// `EventTarget` own block. Listener storage lives in the runtime state keyed
/// by node id; this layer only fixes the interface's position in the chain.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct EventTargetLayer;

pub(crate) type EventTarget = Extended<EventTargetLayer>;

impl ExtendLayer for EventTargetLayer {
    type Parent = RootLayer;
    const CLASS_NAME: &'static str = "EventTarget";
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

        Ok(())
    }
}

/// Register the `EventTarget` class (a root class: no prototype link).
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<EventTarget>()?;
    Ok(())
}

use boa_engine::object::JsObject;

// ── Listener registration ────────────────────────────────────────────
//
// The method bodies are the pre-existing implementations from the `Node`
// prototype, moved here unchanged.

use crate::dom::{dom_ctx, this_node_id};

fn add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let Some(callback) = args.get(1).and_then(|value| value.as_object()) else {
        return Ok(JsValue::undefined());
    };
    if !callback.is_callable() {
        return Ok(JsValue::undefined());
    }

    // Parse options (bool `capture` or `{ capture, once }`)
    let mut capture = false;
    let mut once = false;
    match args.get(2) {
        Some(options) if options.is_object() => {
            let options = options.as_object().unwrap();
            capture = options
                .get(boa_engine::js_string!("capture"), context)?
                .to_boolean();
            once = options
                .get(boa_engine::js_string!("once"), context)?
                .to_boolean();
        }
        Some(options) => capture = options.to_boolean(),
        None => {}
    }

    let event_type = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let mut state = ctx.state.borrow_mut();
    let listeners = state
        .node_listeners
        .entry(node_id)
        .or_default()
        .entry(event_type)
        .or_default();

    // Duplicate listeners (same callback + capture flag) are ignored
    if !listeners
        .iter()
        .any(|l| JsObject::equals(&l.callback, &callback) && l.capture == capture)
    {
        listeners.push(crate::state::Listener {
            callback,
            capture,
            once,
        });
    }

    Ok(JsValue::undefined())
}

fn remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let event_type = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let Some(callback) = args.get(1).and_then(|value| value.as_object()) else {
        return Ok(JsValue::undefined());
    };

    let capture = match args.get(2) {
        Some(options) if options.is_object() => options
            .as_object()
            .unwrap()
            .get(boa_engine::js_string!("capture"), context)?
            .to_boolean(),
        Some(options) => options.to_boolean(),
        None => false,
    };

    let mut state = ctx.state.borrow_mut();
    if let Some(listeners) = state
        .node_listeners
        .get_mut(&node_id)
        .and_then(|map| map.get_mut(&event_type))
    {
        listeners.retain(|l| !(JsObject::equals(&l.callback, &callback) && l.capture == capture));
    }

    Ok(JsValue::undefined())
}
