//! `PointerEvent` layer — a pointer under `MouseEvent`.
//!
//! Pointer events carry the same coordinate and modifier data as mouse
//! events (touch events ride this interface too), so the layer fixes only
//! the class identity.

use boa_engine::class::ClassBuilder;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::{Constructed, ExtendLayer, Extended, Super};

use super::mouse_event::MouseEventLayer;

/// `PointerEvent` own block.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct PointerEventLayer;

pub(crate) type PointerEvent = Extended<PointerEventLayer>;

impl ExtendLayer for PointerEventLayer {
    type Parent = MouseEventLayer;
    const CLASS_NAME: &'static str = "PointerEvent";

    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        let done = sup.call(args, ctx)?;
        Ok(Constructed::new(done, PointerEventLayer))
    }

    fn define_members(_class: &mut ClassBuilder<'_>) -> JsResult<()> {
        Ok(())
    }
}

/// Register the `PointerEvent` class and wire up the
/// `PointerEvent -> MouseEvent` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<PointerEvent>()?;
    crate::shared::link_prototype::<PointerEvent>(context)?;
    Ok(())
}
