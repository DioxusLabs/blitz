//! Timer bindings and delivery of due callbacks.

use boa_engine::object::JsObject;
use boa_engine::{Context, JsResult, JsValue};
use web_time::{Duration, Instant};

use super::{ScriptRuntime, events::report_js_error};
use crate::dom::dom_ctx;

impl ScriptRuntime {
    /// The deadline of the soonest pending timer (if any)
    pub fn next_timer_deadline(&self) -> Option<Instant> {
        self.ctx.state.borrow().timers.next_deadline()
    }

    /// Run all timers that are currently due. Returns `true` if any JavaScript was run.
    pub fn run_due_timers(&mut self) -> bool {
        let due = {
            let mut state = self.ctx.state.borrow_mut();
            let now = state.clock.now();
            state.timers.take_due(now)
        };
        if due.is_empty() {
            return false;
        }
        for timer in due {
            if let Err(error) =
                timer
                    .callback
                    .call(&JsValue::undefined(), &timer.args, &mut self.context)
            {
                report_js_error(&self.ctx, &mut self.context, "timer callback", &error);
            }
        }
        self.run_jobs("timer microtasks");
        true
    }
}

fn timer_args(
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<Option<(JsObject, Duration, Vec<JsValue>)>> {
    let Some(callback) = args
        .first()
        .and_then(|value| value.as_object())
        .filter(|obj| obj.is_callable())
    else {
        return Ok(None);
    };
    let delay_ms = match args.get(1) {
        Some(value) => value.to_number(context)?,
        None => 0.0,
    };
    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 {
        delay_ms
    } else {
        0.0
    };
    let rest: Vec<JsValue> = args.iter().skip(2).cloned().collect();
    Ok(Some((
        callback,
        Duration::from_secs_f64(delay_ms / 1000.0),
        rest,
    )))
}

pub(super) fn set_timeout(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some((callback, delay, rest)) = timer_args(args, context)? else {
        return Ok(JsValue::from(0));
    };
    let mut state = ctx.state.borrow_mut();
    let now = state.clock.now();
    let id = state.timers.add(now, delay, None, callback, rest);
    Ok(JsValue::from(id as f64))
}

pub(super) fn set_interval(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some((callback, delay, rest)) = timer_args(args, context)? else {
        return Ok(JsValue::from(0));
    };
    let mut state = ctx.state.borrow_mut();
    let now = state.clock.now();
    let id = state.timers.add(now, delay, Some(delay), callback, rest);
    Ok(JsValue::from(id as f64))
}

pub(super) fn request_animation_frame(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some(callback) = args
        .first()
        .and_then(|value| value.as_object())
        .filter(|obj| obj.is_callable())
    else {
        return Ok(JsValue::from(0));
    };
    // Approximate the next frame as ~16ms away
    let timestamp = JsValue::from(16.0);
    let mut state = ctx.state.borrow_mut();
    let now = state.clock.now();
    let id = state.timers.add(
        now,
        Duration::from_millis(16),
        None,
        callback,
        vec![timestamp],
    );
    Ok(JsValue::from(id as f64))
}

pub(super) fn clear_timer(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let id = match args.first() {
        Some(value) => value.to_number(context)?,
        None => return Ok(JsValue::undefined()),
    };
    if id.is_finite() && id >= 0.0 {
        ctx.state.borrow_mut().timers.remove(id as u64);
    }
    Ok(JsValue::undefined())
}
