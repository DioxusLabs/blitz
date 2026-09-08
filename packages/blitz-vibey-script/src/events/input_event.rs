//! `InputEvent` layer — text-input data under `UIEvent`.

use blitz_traits::events::BlitzInputEvent;
use boa_engine::class::ClassBuilder;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::dom::js_str;
use crate::shared::{
    Constructed, ExtendLayer, Extended, Super, instance_getter, js_fn_ptr, with_own,
};

use super::ui_event::UIEventLayer;

/// `InputEvent` own block: the inserted text and its input type.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct InputEventLayer {
    #[unsafe_ignore_trace]
    pub data: String,
    #[unsafe_ignore_trace]
    pub input_type: String,
}

impl InputEventLayer {
    /// Blitz text input events only carry the inserted value; the input type
    /// is reported as `"insertText"`, matching the previous behavior.
    pub(crate) fn from_blitz(e: &BlitzInputEvent) -> Self {
        Self {
            data: e.value.clone(),
            input_type: "insertText".to_string(),
        }
    }
}

pub(crate) type InputEvent = Extended<InputEventLayer>;

impl ExtendLayer for InputEventLayer {
    type Parent = UIEventLayer;
    const CLASS_NAME: &'static str = "InputEvent";

    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        let done = sup.call(args, ctx)?;
        Ok(Constructed::new(done, InputEventLayer::default()))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_getter!(class, "data", js_fn_ptr!(data_getter, &realm), attr);
        instance_getter!(
            class,
            "inputType",
            js_fn_ptr!(input_type_getter, &realm),
            attr
        );

        Ok(())
    }
}

/// Register the `InputEvent` class and wire up the
/// `InputEvent -> UIEvent` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<InputEvent>()?;
    crate::shared::link_prototype::<InputEvent>(context)?;
    Ok(())
}

fn this_input_layer(this: &JsValue) -> JsResult<InputEventLayer> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not an InputEvent object"))?;
    with_own::<InputEventLayer, _>(&obj, InputEventLayer::clone)
}

fn data_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(js_str(&this_input_layer(this)?.data))
}

fn input_type_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(js_str(&this_input_layer(this)?.input_type))
}
