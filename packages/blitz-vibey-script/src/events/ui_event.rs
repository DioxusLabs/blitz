//! `UIEvent` layer — parent of Mouse/Wheel/Keyboard/Input.

use boa_engine::class::ClassBuilder;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::with_own;
use crate::shared::{Constructed, ExtendLayer, Extended, Super, instance_getter, js_fn_ptr};
use boa_engine::property::Attribute;

use super::base::event::EventLayer;

/// `UIEvent` own block: the view-derived detail counter.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct UIEventLayer {
    #[unsafe_ignore_trace]
    pub detail: u32,
}

pub(crate) type UIEvent = Extended<UIEventLayer>;

impl ExtendLayer for UIEventLayer {
    type Parent = EventLayer;
    const CLASS_NAME: &'static str = "UIEvent";

    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        let done = sup.call(args, ctx)?;
        Ok(Constructed::new(done, UIEventLayer { detail: 0 }))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_getter!(class, "detail", js_fn_ptr!(detail_getter, &realm), attr);

        Ok(())
    }
}

/// Register the `UIEvent` class and wire up the `UIEvent -> Event` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<UIEvent>()?;
    crate::shared::link_prototype::<UIEvent>(context)?;
    Ok(())
}

fn detail_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not a UIEvent object"))?;
    with_own::<UIEventLayer, _>(&obj, |e| JsValue::from(e.detail))
}
