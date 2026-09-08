//! The `Window` class — the global object of the script runtime.
//!
//! The global object is born in the host hooks
//! ([`ScriptHooks::create_global_object`]) from pure layer data via
//! [`HostGlobal::host_global`]; [`register`] then defines the `Window`
//! class, links its prototype to `EventTarget.prototype`, and links the
//! global object into the class's prototype chain. The global object is
//! therefore a live window: `globalThis`, `this` and the global var/let
//! bindings all live on it, and its listeners live in the `EventTarget`
//! parent layer's own block like every other event target's.

use boa_engine::class::ClassBuilder;
use boa_engine::context::HostHooks;
use boa_engine::context::intrinsics::Intrinsics;
use boa_engine::gc::GcRefCell;
use boa_engine::object::JsObject;
use boa_engine::object::ObjectInitializer;
use boa_engine::property::Attribute;
use boa_engine::{Context, JsResult, JsString, JsValue, js_string};
use boa_gc::{Finalize, Trace};
use url::Url;

use crate::dom::{dom_ctx, node_wrapper, to_rust_string};
use crate::events::EventTargetLayer;
use crate::events::base::event_target::define_on_event_attributes;
use crate::shared::{
    ExtendLayer, Extended, HostGlobal, LayerChain, instance_getter, instance_method,
    instance_property, js_fn_ptr, layer_chain, native_fn_ptr, set_own_block, with_own_mut,
};

/// Event handler IDL attribute types defined on `Window.prototype`: the
/// window-reflecting body element set (mirrored by `<body>`/`<frameset>` in
/// `dom/body.rs`) plus the bubbling interaction events the window receives
/// when they bubble out of the document.
const WINDOW_EVENT_TYPES: &[&str] = &[
    // WindowEventHandlers + the global handlers reflected from `<body>`
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
    // Bubbling interaction events
    "click",
    "dblclick",
    "contextmenu",
    "mousedown",
    "mouseup",
    "mousemove",
    "mouseover",
    "mouseout",
    "pointerdown",
    "pointerup",
    "pointermove",
    "pointercancel",
    "pointerover",
    "pointerout",
    "touchstart",
    "touchmove",
    "touchend",
    "touchcancel",
    "keydown",
    "keyup",
    "keypress",
    "input",
    "change",
    "focusin",
    "focusout",
    "submit",
    "wheel",
];

/// `Window` own block. The chain data is pure at host-creation time: the
/// runtime's base URL drives the `location` object, and the `location` /
/// `navigator` objects ([SameObject]) are created in the current realm on
/// first access and cached here.
#[derive(Debug, Default, Clone, Trace, Finalize, boa_engine::JsData)]
pub(crate) struct WindowLayer {
    #[unsafe_ignore_trace]
    pub base_url: Option<Url>,
    pub location: GcRefCell<Option<JsObject>>,
    pub navigator: GcRefCell<Option<JsObject>>,
}

pub(crate) type Window = Extended<WindowLayer>;

impl ExtendLayer for WindowLayer {
    type Parent = EventTargetLayer;
    const CLASS_NAME: &'static str = "Window";

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        define_on_event_attributes(class, WINDOW_EVENT_TYPES);

        // Timers
        instance_method!(
            class,
            "setTimeout",
            2,
            native_fn_ptr!(crate::runtime::set_timeout)
        );
        instance_method!(
            class,
            "clearTimeout",
            1,
            native_fn_ptr!(crate::runtime::clear_timer)
        );
        instance_method!(
            class,
            "setInterval",
            2,
            native_fn_ptr!(crate::runtime::set_interval)
        );
        instance_method!(
            class,
            "clearInterval",
            1,
            native_fn_ptr!(crate::runtime::clear_timer)
        );
        instance_method!(
            class,
            "requestAnimationFrame",
            1,
            native_fn_ptr!(crate::runtime::request_animation_frame)
        );
        instance_method!(
            class,
            "cancelAnimationFrame",
            1,
            native_fn_ptr!(crate::runtime::clear_timer)
        );

        // Viewport dimensions
        instance_getter!(
            class,
            "innerWidth",
            js_fn_ptr!(crate::runtime::inner_width, &realm),
            attr
        );
        instance_getter!(
            class,
            "innerHeight",
            js_fn_ptr!(crate::runtime::inner_height, &realm),
            attr
        );
        instance_getter!(
            class,
            "outerWidth",
            js_fn_ptr!(crate::runtime::inner_width, &realm),
            attr
        );
        instance_getter!(
            class,
            "outerHeight",
            js_fn_ptr!(crate::runtime::inner_height, &realm),
            attr
        );
        instance_getter!(
            class,
            "devicePixelRatio",
            js_fn_ptr!(crate::runtime::device_pixel_ratio, &realm),
            attr
        );

        // Viewport scrolling
        instance_getter!(
            class,
            "scrollX",
            js_fn_ptr!(crate::runtime::scroll_x, &realm),
            attr
        );
        instance_getter!(
            class,
            "scrollY",
            js_fn_ptr!(crate::runtime::scroll_y, &realm),
            attr
        );
        instance_getter!(
            class,
            "pageXOffset",
            js_fn_ptr!(crate::runtime::scroll_x, &realm),
            attr
        );
        instance_getter!(
            class,
            "pageYOffset",
            js_fn_ptr!(crate::runtime::scroll_y, &realm),
            attr
        );
        instance_method!(
            class,
            "scroll",
            2,
            native_fn_ptr!(crate::runtime::window_scroll_to)
        );
        instance_method!(
            class,
            "scrollTo",
            2,
            native_fn_ptr!(crate::runtime::window_scroll_to)
        );
        instance_method!(
            class,
            "scrollBy",
            2,
            native_fn_ptr!(crate::runtime::window_scroll_by)
        );

        instance_method!(
            class,
            "getComputedStyle",
            1,
            native_fn_ptr!(crate::runtime::get_computed_style)
        );

        // The window's own document
        instance_getter!(class, "document", js_fn_ptr!(window_document, &realm), attr);

        // The window's `location` and `navigator` ([SameObject]): getters
        // backed by the window's own block, which caches the objects after
        // their first access in the current realm.
        instance_getter!(class, "location", js_fn_ptr!(window_location, &realm), attr);
        instance_getter!(
            class,
            "navigator",
            js_fn_ptr!(window_navigator, &realm),
            attr
        );

        // The window itself and its (single-frame) aliases. There is only
        // ever a single frame, so `parent` and `top` refer to the window
        // itself; `opener` is null.
        instance_getter!(class, "window", js_fn_ptr!(global_this_value, &realm), attr);
        instance_getter!(class, "self", js_fn_ptr!(global_this_value, &realm), attr);
        instance_getter!(class, "parent", js_fn_ptr!(global_this_value, &realm), attr);
        instance_getter!(class, "top", js_fn_ptr!(global_this_value, &realm), attr);
        instance_property!(class, "opener", JsValue::null(), attr);

        // The `CSS` namespace object. `CSS.escape` is defined in the JS
        // bootstrap.
        let css_namespace = boa_engine::object::ObjectInitializer::new(class.context())
            .function(
                boa_engine::NativeFunction::from_fn_ptr(crate::runtime::css_supports),
                js_string!("supports"),
                1,
            )
            .build();
        instance_property!(class, "CSS", JsValue::from(css_namespace), attr);

        // Runtime internals: the embedder message channel (see
        // `ScriptDocument::take_messages`) and the CSS property support
        // check used by the style Proxy's `has` trap.
        instance_method!(
            class,
            "__blitz_send_message",
            1,
            native_fn_ptr!(send_message)
        );
        instance_method!(
            class,
            "__blitz_css_property_supported",
            1,
            native_fn_ptr!(css_property_supported)
        );

        Ok(())
    }
}

/// The host-global path: the realm's global object is born from this chain
/// while the realm is still being built. The chain is `EventTarget ->
/// Window`; the walk fills the parent slot, then this layer's.
impl HostGlobal for WindowLayer {
    fn host_fill(object: &JsObject, chain: LayerChain<Self>) -> JsResult<()> {
        set_own_block::<EventTargetLayer>(object, chain.parent.own)?;
        set_own_block::<Self>(object, chain.own)
    }
}

fn window_document(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let root_id = ctx.doc.borrow().root_node().id;
    Ok(node_wrapper(&ctx, root_id, context).into())
}

/// `Window.prototype.location`: the [SameObject] location is created in
/// the current realm from the window's base URL on first access and
/// cached in the own block.
fn window_location(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| boa_engine::JsNativeError::typ().with_message("`this` is not a Window"))?;
    with_own_mut::<WindowLayer, _>(&obj, |w| {
        let cached = w.location.borrow().clone();
        let location = match cached {
            Some(location) => location,
            None => {
                let location = build_location(w.base_url.as_ref(), context);
                *w.location.borrow_mut() = Some(location.clone());
                location
            }
        };
        location.into()
    })
}

/// `Window.prototype.navigator`, same [SameObject] pattern.
fn window_navigator(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let obj = this
        .as_object()
        .ok_or_else(|| boa_engine::JsNativeError::typ().with_message("`this` is not a Window"))?;
    with_own_mut::<WindowLayer, _>(&obj, |w| {
        let cached = w.navigator.borrow().clone();
        let navigator = match cached {
            Some(navigator) => navigator,
            None => {
                let navigator = build_navigator(context);
                *w.navigator.borrow_mut() = Some(navigator.clone());
                navigator
            }
        };
        navigator.into()
    })
}

fn global_this_value(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    Ok(context.global_object().clone().into())
}

/// The embedder message channel (`ScriptDocument::take_messages` drains it).
fn send_message(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let message = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_string(context)?
        .to_std_string_lossy();
    ctx.state.borrow_mut().outbound_messages.push(message);
    Ok(JsValue::undefined())
}

/// CSS property support check, used by the style Proxy's `has` trap.
fn css_property_supported(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    Ok(JsValue::from(blitz_dom::css_property_is_supported(&name)))
}

/// Host hooks for the script runtime: the root realm's global object is
/// born from the `Window` chain's pure layer data — the base URL is the
/// host configuration carried here.
pub(crate) struct ScriptHooks {
    pub base_url: Option<Url>,
}

impl HostHooks for ScriptHooks {
    fn create_global_object(&self, intrinsics: &Intrinsics) -> JsObject {
        WindowLayer::host_global(
            layer_chain!(
                EventTargetLayer::default(),
                WindowLayer {
                    base_url: self.base_url.clone(),
                    location: GcRefCell::new(None),
                    navigator: GcRefCell::new(None),
                },
            ),
            intrinsics,
        )
        .expect("failed to create the window global object")
    }
}

/// Register the `Window` class, wire up the
/// `Window -> EventTarget` prototype chain, and link the global object
/// (born in the host hooks) into it.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<Window>()?;
    crate::shared::link_prototype::<Window>(context)?;

    // A host-born global cannot carry its class prototype at birth (it
    // does not exist at realm-creation time); link it now.
    let prototype = context
        .get_global_class::<Window>()
        .expect("the Window class was just registered")
        .prototype();
    context.global_object().set_prototype(Some(prototype));
    Ok(())
}

/// Build the `location` object for the document's base URL.
fn build_location(base_url: Option<&Url>, context: &mut Context) -> JsObject {
    let (href, protocol, host, pathname, search, hash, origin) = match base_url {
        Some(url) => (
            url.to_string(),
            format!("{}:", url.scheme()),
            url.host_str().unwrap_or_default().to_string(),
            url.path().to_string(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default(),
            url.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
            url.origin().ascii_serialization(),
        ),
        None => (
            "about:blank".to_string(),
            "about:".to_string(),
            String::new(),
            "blank".to_string(),
            String::new(),
            String::new(),
            "null".to_string(),
        ),
    };
    ObjectInitializer::new(context)
        .property(js_string!("href"), JsString::from(href), Attribute::all())
        .property(
            js_string!("protocol"),
            JsString::from(protocol),
            Attribute::all(),
        )
        .property(
            js_string!("host"),
            JsString::from(host.clone()),
            Attribute::all(),
        )
        .property(
            js_string!("hostname"),
            JsString::from(host),
            Attribute::all(),
        )
        .property(
            js_string!("pathname"),
            JsString::from(pathname),
            Attribute::all(),
        )
        .property(
            js_string!("search"),
            JsString::from(search),
            Attribute::all(),
        )
        .property(js_string!("hash"), JsString::from(hash), Attribute::all())
        .property(
            js_string!("origin"),
            JsString::from(origin),
            Attribute::all(),
        )
        .build()
}

/// Build the `navigator` object.
fn build_navigator(context: &mut Context) -> JsObject {
    ObjectInitializer::new(context)
        .property(
            js_string!("userAgent"),
            js_string!("Mozilla/5.0 (compatible; Blitz)"),
            Attribute::all(),
        )
        .build()
}
