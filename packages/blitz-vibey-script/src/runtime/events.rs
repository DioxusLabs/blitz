//! Event dispatch, window listeners, and uncaught exception reporting.

use blitz_dom::NodeId;
use blitz_traits::events::{DomEvent, DomEventData, EventState};
use boa_engine::object::JsObject;
use boa_engine::{Context, JsError, JsResult, JsString, JsValue, js_string};

use super::ScriptRuntime;
use crate::dom::event::{EventRef, create_event, create_event_for_dom_event};
use crate::dom::{dom_ctx, js_str, node_wrapper};
use crate::state::{DomCtx, Listener};

/// Report an unhandled JavaScript error: record it in the runtime state for the
/// embedder to collect via
/// [`ScriptDocument::take_js_errors`](crate::ScriptDocument::take_js_errors),
/// then fire an `error` event at the window (the HTML "report an exception"
/// steps) so that `window.onerror` / `addEventListener("error")` handlers
/// observe it.
pub(super) fn report_js_error(ctx: &DomCtx, context: &mut Context, what: &str, error: &JsError) {
    #[cfg(feature = "tracing")]
    tracing::error!("Uncaught JS error in {what}: {error}");
    let already_dispatching = {
        let mut state = ctx.state.borrow_mut();
        state.record_error(format!("Uncaught JS error in {what}: {error}"));
        std::mem::replace(&mut state.dispatching_error_event, true)
    };
    if already_dispatching {
        return;
    }
    dispatch_error_event(ctx, context, error);
    ctx.state.borrow_mut().dispatching_error_event = false;
}

/// Fire an `ErrorEvent`-shaped `error` event at the window for an uncaught
/// exception. Exceptions thrown by the handlers themselves are recorded (via
/// [`report_js_error`]) but do not fire further `error` events.
fn dispatch_error_event(ctx: &DomCtx, context: &mut Context, error: &JsError) {
    let listeners: Vec<Listener> = {
        let mut state = ctx.state.borrow_mut();
        match state.window_listeners.get_mut("error") {
            Some(listeners) => {
                let cloned = listeners.clone();
                listeners.retain(|l| !l.once);
                cloned
            }
            None => Vec::new(),
        }
    };
    let onerror = context
        .global_object()
        .get(js_string!("onerror"), context)
        .ok()
        .and_then(|handler| handler.as_object().filter(|h| h.is_callable()));
    if listeners.is_empty() && onerror.is_none() {
        return;
    }

    let global: JsValue = context.global_object().into();
    let event_obj = create_event(ctx, "error", false, true, &global, context);
    crate::dom::define_value(&event_obj, "currentTarget", global.clone(), context);
    let error_value = error
        .clone()
        .into_opaque(context)
        .unwrap_or_else(|_| js_str(&error.to_string()));
    let message = error_value
        .as_object()
        .and_then(|obj| obj.get(js_string!("message"), context).ok())
        .filter(|message| message.is_string())
        .unwrap_or_else(|| js_str(&error.to_string()));
    crate::dom::define_value(&event_obj, "message", message.clone(), context);
    crate::dom::define_value(&event_obj, "filename", js_str(""), context);
    crate::dom::define_value(&event_obj, "lineno", JsValue::from(0), context);
    crate::dom::define_value(&event_obj, "colno", JsValue::from(0), context);
    crate::dom::define_value(&event_obj, "error", error_value.clone(), context);

    for listener in listeners {
        if let Err(error) = listener
            .callback
            .call(&global, &[event_obj.clone().into()], context)
        {
            report_js_error(ctx, context, "error event listener", &error);
        }
    }
    // `window.onerror(message, source, lineno, colno, error)`
    if let Some(handler) = onerror {
        let args = [
            message,
            js_str(""),
            JsValue::from(0),
            JsValue::from(0),
            error_value,
        ];
        if let Err(error) = handler.call(&global, &args, context) {
            report_js_error(ctx, context, "error event listener", &error);
        }
    }
}

impl ScriptRuntime {
    /// Install the `<body onload="...">` attribute (if any) as the window's
    /// `load` event handler, per the HTML spec (event handler attributes on
    /// `<body>` apply to the window). Handlers assigned to `window.onload` by
    /// scripts take precedence.
    pub fn install_body_onload_attribute(&mut self) {
        let code: Option<String> = {
            let doc = self.ctx.doc.borrow();
            let mut stack = vec![doc.root_node().id];
            let mut code = None;
            while let Some(node_id) = stack.pop() {
                let Some(node) = doc.get_node(node_id) else {
                    continue;
                };
                if let Some(element) = node.element_data() {
                    if element.name.local == blitz_dom::local_name!("body") {
                        code = element
                            .attr(blitz_dom::local_name!("onload"))
                            .map(str::to_string);
                        break;
                    }
                }
                stack.extend(node.children.iter().rev().copied());
            }
            code
        };
        let Some(code) = code else { return };

        // Don't override a handler assigned via `window.onload = ...`
        let already_set = self
            .context
            .global_object()
            .get(js_string!("onload"), &mut self.context)
            .ok()
            .and_then(|value| value.as_object())
            .is_some_and(|obj| obj.is_callable());
        if already_set {
            return;
        }

        let script = format!("window.onload = function (event) {{\n{code}\n}};");
        self.eval_internal(&script, "<body onload attribute>");
    }

    /// Dispatch a Blitz DOM event to JavaScript event listeners registered on
    /// the nodes in `chain` (which is ordered target-first).
    ///
    /// Returns `true` if any listener was invoked.
    pub fn dispatch_dom_event(
        &mut self,
        chain: &[NodeId],
        event: &DomEvent,
        event_state: &mut EventState,
    ) -> bool {
        let name = event.name().to_string();
        let mut any_called = self.dispatch_event_inner(
            chain,
            &name,
            event.bubbles,
            |ctx, target, context| {
                create_event_for_dom_event(
                    ctx,
                    &event.data,
                    event.bubbles,
                    event.cancelable,
                    target,
                    context,
                )
            },
            event_state,
        );

        // Browsers fire a `change` event after `input` events on checkbox/radio
        // inputs. Blitz only generates `input` events, so synthesise the `change`
        // event here.
        if matches!(event.data, DomEventData::Input(_))
            && self.target_is_checkbox_or_radio(event.target)
        {
            let mut change_state = EventState::default();
            any_called |= self.dispatch_event_inner(
                chain,
                "change",
                true,
                |ctx, target, context| create_event(ctx, "change", true, false, target, context),
                &mut change_state,
            );
            if change_state.redraw_is_requested() {
                event_state.request_redraw();
            }
        }

        if any_called {
            self.run_jobs("event microtasks");
        }

        any_called
    }

    fn target_is_checkbox_or_radio(&self, node_id: NodeId) -> bool {
        let doc = self.ctx.doc.borrow();
        doc.get_node(node_id)
            .and_then(|node| node.element_data())
            .is_some_and(|element| {
                element.name.local == blitz_dom::local_name!("input")
                    && matches!(
                        element.attr(blitz_dom::local_name!("type")),
                        Some("checkbox") | Some("radio")
                    )
            })
    }

    /// Dispatch an event named `name` along `chain`, using `make_event` to lazily
    /// construct the JS event object. Returns `true` if any listener was invoked.
    fn dispatch_event_inner(
        &mut self,
        chain: &[NodeId],
        name: &str,
        bubbles: bool,
        make_event: impl FnOnce(&DomCtx, &JsValue, &mut Context) -> JsObject,
        event_state: &mut EventState,
    ) -> bool {
        let ctx = self.ctx.clone();
        let context = &mut self.context;
        let on_name = JsString::from(format!("on{name}"));

        // Fast path: bail if no listener of this type could possibly be registered
        let may_have_listeners = {
            let state = ctx.state.borrow();
            let registry_hit = chain.iter().any(|node_id| {
                state
                    .node_listeners
                    .get(node_id)
                    .and_then(|map| map.get(name))
                    .is_some_and(|listeners| !listeners.is_empty())
            }) || state
                .window_listeners
                .get(name)
                .is_some_and(|listeners| !listeners.is_empty());
            // `on<event>` handlers can only exist on nodes that script has touched
            // (i.e. nodes with a cached wrapper)
            let wrapper_hit = chain
                .iter()
                .any(|node_id| state.node_wrappers.contains_key(node_id));
            registry_hit || wrapper_hit
        };
        if !may_have_listeners {
            return false;
        }

        let target: JsValue = node_wrapper(&ctx, chain[0], context).into();
        let event_obj = make_event(&ctx, &target, context);

        // `window.event`: the event being dispatched is readable from the global
        // object for the duration of the dispatch, for listeners written against
        // the old IE-style API (`window.event.keyCode`) rather than the argument.
        let window = context.global_object();
        let previous_event = window
            .get(JsString::from("event"), context)
            .unwrap_or(JsValue::undefined());
        crate::dom::define_value(&window, "event", event_obj.clone().into(), context);
        let event_ref = |event_obj: &JsObject, f: &dyn Fn(&EventRef) -> bool| -> bool {
            event_obj
                .downcast_ref::<EventRef>()
                .map(|event| f(&event))
                .unwrap_or(false)
        };

        let mut any_called = false;

        'chain: for &node_id in chain {
            // Gather listeners for this node: `addEventListener` listeners plus
            // an `on<event>` property handler (if any)
            let mut callbacks: Vec<JsObject> = Vec::new();
            {
                let mut state = ctx.state.borrow_mut();
                if let Some(listeners) = state
                    .node_listeners
                    .get_mut(&node_id)
                    .and_then(|map| map.get_mut(name))
                {
                    callbacks.extend(listeners.iter().map(|l| l.callback.clone()));
                    // `once` listeners are removed at dispatch time
                    listeners.retain(|l| !l.once);
                }
            }
            let wrapper = ctx.state.borrow().node_wrappers.get(&node_id).cloned();
            if let Some(wrapper) = wrapper {
                if let Ok(handler) = wrapper.get(on_name.clone(), context) {
                    if let Some(handler) = handler.as_object() {
                        if handler.is_callable() {
                            callbacks.push(handler);
                        }
                    }
                }
            }

            if callbacks.is_empty() {
                if !bubbles {
                    break;
                }
                continue;
            }

            let current_target: JsValue = node_wrapper(&ctx, node_id, context).into();
            crate::dom::define_value(&event_obj, "currentTarget", current_target.clone(), context);

            for callback in callbacks {
                any_called = true;
                if let Err(error) =
                    callback.call(&current_target, &[event_obj.clone().into()], context)
                {
                    report_js_error(&ctx, context, "event listener", &error);
                }
                if event_ref(&event_obj, &|event| event.stopped_immediate.get()) {
                    break 'chain;
                }
            }

            if !bubbles || event_ref(&event_obj, &|event| event.stopped.get()) {
                break;
            }
        }

        let restore_event = |context: &mut Context| {
            let window = context.global_object();
            crate::dom::define_value(&window, "event", previous_event.clone(), context);
        };

        // Window-level listeners
        if bubbles && !event_ref(&event_obj, &|event| event.stopped.get()) {
            let listeners: Vec<Listener> = {
                let mut state = ctx.state.borrow_mut();
                match state.window_listeners.get_mut(name) {
                    Some(listeners) => {
                        let cloned = listeners.clone();
                        listeners.retain(|l| !l.once);
                        cloned
                    }
                    None => Vec::new(),
                }
            };
            if !listeners.is_empty() {
                let global: JsValue = context.global_object().into();
                crate::dom::define_value(&event_obj, "currentTarget", global.clone(), context);
                for listener in listeners {
                    any_called = true;
                    if let Err(error) =
                        listener
                            .callback
                            .call(&global, &[event_obj.clone().into()], context)
                    {
                        report_js_error(&ctx, context, "event listener", &error);
                    }
                    if event_ref(&event_obj, &|event| event.stopped_immediate.get()) {
                        break;
                    }
                }
            }
        }

        crate::dom::define_value(&event_obj, "currentTarget", JsValue::null(), context);
        restore_event(context);

        // Feed `preventDefault` / `stopPropagation` back into Blitz
        if event_ref(&event_obj, &|event| event.prevented.get()) {
            event_state.prevent_default();
        }
        if event_ref(&event_obj, &|event| event.stopped.get()) {
            event_state.stop_propagation();
        }
        if any_called {
            event_state.request_redraw();
        }

        any_called
    }

    /// Dispatch a simple event (e.g. `DOMContentLoaded`) targeting the document node
    pub fn dispatch_document_event(&mut self, name: &str) -> bool {
        let root_id = self.ctx.doc.borrow().root_node().id;
        let mut event_state = EventState::default();
        let ran = self.dispatch_event_inner(
            &[root_id],
            name,
            true,
            |ctx, target, context| create_event(ctx, name, true, false, target, context),
            &mut event_state,
        );
        if ran {
            self.run_jobs("event microtasks");
        }
        ran
    }

    /// Dispatch a simple event (e.g. `load`) targeting the window
    pub fn dispatch_window_event(&mut self, name: &str) -> bool {
        let ctx = self.ctx.clone();
        let context = &mut self.context;

        let listeners: Vec<Listener> = {
            let mut state = ctx.state.borrow_mut();
            match state.window_listeners.get_mut(name) {
                Some(listeners) => {
                    let cloned = listeners.clone();
                    listeners.retain(|l| !l.once);
                    cloned
                }
                None => Vec::new(),
            }
        };

        let global: JsValue = context.global_object().into();
        let event_obj = create_event(&ctx, name, false, false, &global, context);
        crate::dom::define_value(&event_obj, "currentTarget", global.clone(), context);

        let mut any_called = false;
        for listener in listeners {
            any_called = true;
            if let Err(error) =
                listener
                    .callback
                    .call(&global, &[event_obj.clone().into()], context)
            {
                report_js_error(&ctx, context, "event listener", &error);
            }
        }

        // `window.onload = ...` style handler
        let on_name = JsString::from(format!("on{name}"));
        if let Ok(handler) = context.global_object().get(on_name, context) {
            if let Some(handler) = handler.as_object() {
                if handler.is_callable() {
                    any_called = true;
                    if let Err(error) = handler.call(&global, &[event_obj.into()], context) {
                        report_js_error(&ctx, context, "event listener", &error);
                    }
                }
            }
        }

        if any_called {
            self.run_jobs("event microtasks");
        }
        any_called
    }
}

pub(super) fn window_add_event_listener(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let event_type =
        crate::dom::to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let Some(callback) = args
        .get(1)
        .and_then(|value| value.as_object())
        .filter(|obj| obj.is_callable())
    else {
        return Ok(JsValue::undefined());
    };

    let mut state = ctx.state.borrow_mut();
    let listeners = state.window_listeners.entry(event_type).or_default();
    if !listeners
        .iter()
        .any(|l| JsObject::equals(&l.callback, &callback))
    {
        listeners.push(Listener {
            callback,
            capture: false,
            once: false,
        });
    }
    Ok(JsValue::undefined())
}

pub(super) fn window_remove_event_listener(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let event_type =
        crate::dom::to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let Some(callback) = args.get(1).and_then(|value| value.as_object()) else {
        return Ok(JsValue::undefined());
    };

    let mut state = ctx.state.borrow_mut();
    if let Some(listeners) = state.window_listeners.get_mut(&event_type) {
        listeners.retain(|l| !JsObject::equals(&l.callback, &callback));
    }
    Ok(JsValue::undefined())
}
