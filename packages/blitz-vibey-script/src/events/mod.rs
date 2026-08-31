//! Event class layers: the WinterTC event hierarchy.
//!
//! `Event` is the root; `UIEvent` adds `detail` and parents the
//! `MouseEvent`/`KeyboardEvent`/`InputEvent` families, with `PointerEvent` and
//! `WheelEvent` under `MouseEvent` (touch events ride `PointerEvent`).

pub(crate) mod base;
pub(crate) mod input_event;
pub(crate) mod keyboard_event;
pub(crate) mod mouse_event;
pub(crate) mod pointer_event;
pub(crate) mod ui_event;
pub(crate) mod wheel_event;

pub(crate) use base::event::{
    AT_TARGET_PHASE, BUBBLING_PHASE, CAPTURING_PHASE, DispatchTarget, NONE_PHASE, create_event,
    create_event_for_dom_event, event_flag, with_state_mut,
};
pub(crate) use base::event_target::EventTargetLayer;

/// Register all event classes into the given context and wire up their
/// prototype chains.
pub(crate) fn register(context: &mut boa_engine::Context) -> boa_engine::JsResult<()> {
    base::event::register(context)?;
    base::event_target::register(context)?;
    ui_event::register(context)?;
    mouse_event::register(context)?;
    pointer_event::register(context)?;
    wheel_event::register(context)?;
    keyboard_event::register(context)?;
    input_event::register(context)?;
    Ok(())
}
