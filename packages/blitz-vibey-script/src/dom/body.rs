//! The `HTMLBodyElement` / `HTMLFrameSetElement` layer: contributes the
//! window-reflecting event handler accessors to the prototype chain of
//! `<body>` / `<frameset>` wrappers, per the HTML spec (the
//! "window-reflecting body element event handler set" plus the
//! `WindowEventHandlers` mixin).

use boa_engine::class::ClassBuilder;
use boa_engine::property::Attribute;
use boa_engine::{Context, JsData, JsResult, JsString, JsValue};
use boa_gc::{Finalize, Trace};

use crate::shared::{ExtendLayer, Extended, instance_accessor, js_copy_closure_with_captures};

use super::element::ElementLayer;

/// Event handler IDL attributes on `<body>` / `<frameset>` elements that are
/// aliases for the corresponding window event handlers.
const WINDOW_REFLECTING_BODY_EVENTS: &[&str] = &[
    "afterprint",
    "beforeprint",
    "beforeunload",
    "blur",
    "error",
    "focus",
    "hashchange",
    "languagechange",
    "load",
    "message",
    "messageerror",
    "offline",
    "online",
    "pagehide",
    "pageshow",
    "popstate",
    "rejectionhandled",
    "resize",
    "scroll",
    "storage",
    "unhandledrejection",
    "unload",
];

/// `HTMLBodyElement` own block. All data lives in the `Node` layer; this
/// layer only contributes the window-forwarding accessors to the prototype
/// chain.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct BodyLayer;

pub(crate) type Body = Extended<BodyLayer>;

impl ExtendLayer for BodyLayer {
    type Parent = ElementLayer;
    const CLASS_NAME: &'static str = "HTMLBodyElement";

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        for (index, name) in WINDOW_REFLECTING_BODY_EVENTS.iter().enumerate() {
            let getter = js_copy_closure_with_captures!(
                |_this, _args, index: &u16, context| {
                    let name = WINDOW_REFLECTING_BODY_EVENTS[*index as usize];
                    let prop = JsString::from(format!("on{name}"));
                    context.global_object().get(prop, context)
                },
                index as u16,
                &realm
            );
            let setter = js_copy_closure_with_captures!(
                |_this, args, index: &u16, context| {
                    let name = WINDOW_REFLECTING_BODY_EVENTS[*index as usize];
                    let prop = JsString::from(format!("on{name}"));
                    let value = args.first().cloned().unwrap_or_default();
                    context.global_object().set(prop, value, false, context)?;
                    Ok(JsValue::undefined())
                },
                index as u16,
                &realm
            );
            instance_accessor!(
                class,
                format!("on{name}"),
                getter,
                setter,
                Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE
            );
        }
        Ok(())
    }
}

/// Register the `HTMLBodyElement` class and wire up the
/// `HTMLBodyElement -> Element` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<Body>()?;
    crate::shared::link_prototype::<Body>(context)?;
    Ok(())
}
