//! `KeyboardEvent` layer — key data under `UIEvent`.

use blitz_traits::events::BlitzKeyEvent;
use boa_engine::class::ClassBuilder;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::{
    Constructed, ExtendLayer, Extended, Super, instance_getter, js_fn_ptr, with_own,
};

use super::ui_event::UIEventLayer;

/// `KeyboardEvent` own block: the key, code and key-repeat state.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct KeyboardEventLayer {
    #[unsafe_ignore_trace]
    pub key: String,
    #[unsafe_ignore_trace]
    pub code: String,
    #[unsafe_ignore_trace]
    pub location: u32,
    #[unsafe_ignore_trace]
    pub repeat: bool,
    #[unsafe_ignore_trace]
    pub is_composing: bool,
    #[unsafe_ignore_trace]
    pub ctrl_key: bool,
    #[unsafe_ignore_trace]
    pub shift_key: bool,
    #[unsafe_ignore_trace]
    pub alt_key: bool,
    #[unsafe_ignore_trace]
    pub meta_key: bool,
}

impl KeyboardEventLayer {
    pub(crate) fn from_blitz(e: &BlitzKeyEvent) -> Self {
        Self {
            key: e.key.to_string(),
            code: e.code.to_string(),
            location: e.location as u32,
            repeat: e.is_auto_repeating,
            is_composing: e.is_composing,
            ctrl_key: e.modifiers.contains(keyboard_types::Modifiers::CONTROL),
            shift_key: e.modifiers.contains(keyboard_types::Modifiers::SHIFT),
            alt_key: e.modifiers.contains(keyboard_types::Modifiers::ALT),
            meta_key: e.modifiers.contains(keyboard_types::Modifiers::META),
        }
    }
}

pub(crate) type KeyboardEvent = Extended<KeyboardEventLayer>;

impl ExtendLayer for KeyboardEventLayer {
    type Parent = UIEventLayer;
    const CLASS_NAME: &'static str = "KeyboardEvent";

    fn build(
        args: &[JsValue],
        ctx: &mut Context,
        sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        let done = sup.call(args, ctx)?;
        Ok(Constructed::new(done, KeyboardEventLayer::default()))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_getter!(class, "key", js_fn_ptr!(key_getter, &realm), attr);
        instance_getter!(class, "code", js_fn_ptr!(code_getter, &realm), attr);
        instance_getter!(class, "location", js_fn_ptr!(location_getter, &realm), attr);
        instance_getter!(class, "repeat", js_fn_ptr!(repeat_getter, &realm), attr);
        instance_getter!(
            class,
            "isComposing",
            js_fn_ptr!(is_composing_getter, &realm),
            attr
        );
        instance_getter!(class, "ctrlKey", js_fn_ptr!(ctrl_key_getter, &realm), attr);
        instance_getter!(
            class,
            "shiftKey",
            js_fn_ptr!(shift_key_getter, &realm),
            attr
        );
        instance_getter!(class, "altKey", js_fn_ptr!(alt_key_getter, &realm), attr);
        instance_getter!(class, "metaKey", js_fn_ptr!(meta_key_getter, &realm), attr);

        Ok(())
    }
}

/// Register the `KeyboardEvent` class and wire up the
/// `KeyboardEvent -> UIEvent` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<KeyboardEvent>()?;
    crate::shared::link_prototype::<KeyboardEvent>(context)?;
    Ok(())
}

fn this_keyboard_layer(this: &JsValue) -> JsResult<KeyboardEventLayer> {
    let obj = this
        .as_object()
        .ok_or_else(|| crate::shared::native_error!(typ, "not a KeyboardEvent object"))?;
    with_own::<KeyboardEventLayer, _>(&obj, KeyboardEventLayer::clone)
}

fn key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(crate::dom::js_str(&this_keyboard_layer(this)?.key))
}

fn code_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(crate::dom::js_str(&this_keyboard_layer(this)?.code))
}

fn location_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_keyboard_layer(this)?.location))
}

fn repeat_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_keyboard_layer(this)?.repeat))
}

fn is_composing_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_keyboard_layer(this)?.is_composing))
}

fn ctrl_key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_keyboard_layer(this)?.ctrl_key))
}

fn shift_key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_keyboard_layer(this)?.shift_key))
}

fn alt_key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_keyboard_layer(this)?.alt_key))
}

fn meta_key_getter(this: &JsValue, _: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(this_keyboard_layer(this)?.meta_key))
}
