//! The script runtime: owns the Boa [`Context`], registers the DOM globals and
//! dispatches events / timers into JavaScript.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use blitz_dom::{BaseDocument, NodeId};
use blitz_traits::events::{DomEvent, DomEventData, EventState};
use boa_engine::builtins::promise::PromiseState;
use boa_engine::module::{Module, ModuleLoader, ModuleRequest, Referrer};
use boa_engine::object::{JsObject, ObjectInitializer};
use boa_engine::property::Attribute;
use boa_engine::value::JsValue;
use boa_engine::{
    Context, JsError, JsNativeError, JsResult, JsString, NativeFunction, Source, js_string,
};
use boa_runtime::Console;
use boa_runtime::console::DefaultLogger;
use url::Url;
use web_time::{Duration, Instant};

use crate::dom::event::{EventRef, create_event, create_event_for_dom_event};
use crate::dom::{
    NodeRef, dom_ctx, node_id_of_value, node_wrapper, to_rust_string, wrap_style_object,
};
use crate::fetch::ScriptFetcher;
use crate::state::{DomCtx, Listener, ReadyState};

/// JS bootstrap for APIs that are easiest to define in JS
const BOOTSTRAP_JS: &str = r#"
(function () {
    if (typeof globalThis.queueMicrotask !== "function") {
        globalThis.queueMicrotask = function (callback) {
            Promise.resolve().then(callback);
        };
    }

    if (typeof globalThis.DOMException !== "function") {
        const DOM_EXCEPTION_CODES = {
            IndexSizeError: 1,
            HierarchyRequestError: 3,
            WrongDocumentError: 4,
            InvalidCharacterError: 5,
            NoModificationAllowedError: 7,
            NotFoundError: 8,
            NotSupportedError: 9,
            InUseAttributeError: 10,
            InvalidStateError: 11,
            SyntaxError: 12,
            InvalidModificationError: 13,
            NamespaceError: 14,
            InvalidAccessError: 15,
            TypeMismatchError: 17,
            SecurityError: 18,
            NetworkError: 19,
            AbortError: 20,
            URLMismatchError: 21,
            QuotaExceededError: 22,
            TimeoutError: 23,
            InvalidNodeTypeError: 24,
            DataCloneError: 25,
        };
        globalThis.DOMException = class DOMException extends Error {
            constructor(message = "", name = "Error") {
                super(message);
                this.name = String(name);
            }
            get code() {
                return DOM_EXCEPTION_CODES[this.name] ?? 0;
            }
        };
        // Legacy code constants (on both the interface object and the prototype)
        const LEGACY_CODE_CONSTANTS = {
            INDEX_SIZE_ERR: 1,
            DOMSTRING_SIZE_ERR: 2,
            HIERARCHY_REQUEST_ERR: 3,
            WRONG_DOCUMENT_ERR: 4,
            INVALID_CHARACTER_ERR: 5,
            NO_DATA_ALLOWED_ERR: 6,
            NO_MODIFICATION_ALLOWED_ERR: 7,
            NOT_FOUND_ERR: 8,
            NOT_SUPPORTED_ERR: 9,
            INUSE_ATTRIBUTE_ERR: 10,
            INVALID_STATE_ERR: 11,
            SYNTAX_ERR: 12,
            INVALID_MODIFICATION_ERR: 13,
            NAMESPACE_ERR: 14,
            INVALID_ACCESS_ERR: 15,
            VALIDATION_ERR: 16,
            TYPE_MISMATCH_ERR: 17,
            SECURITY_ERR: 18,
            NETWORK_ERR: 19,
            ABORT_ERR: 20,
            URL_MISMATCH_ERR: 21,
            QUOTA_EXCEEDED_ERR: 22,
            TIMEOUT_ERR: 23,
            INVALID_NODE_TYPE_ERR: 24,
            DATA_CLONE_ERR: 25,
        };
        for (const [name, code] of Object.entries(LEGACY_CODE_CONSTANTS)) {
            DOMException[name] = code;
            DOMException.prototype[name] = code;
        }
    }

    // A Proxy wrapper for CSSStyleDeclaration objects which maps camelCase
    // property access (e.g. `style.gridTemplateColumns`) onto
    // `getPropertyValue`/`setProperty` calls with kebab-case property names.
    const KEBAB_OVERRIDES = { cssFloat: "float" };
    const toKebab = (prop) => {
        const override = KEBAB_OVERRIDES[prop];
        if (override) return override;
        const kebab = prop.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
        // Vendor-prefixed IDL attributes (webkitTransform, msFlex, mozUserSelect)
        // map to dashed prefixes (-webkit-transform, ...) per CSSOM. Uppercase
        // first letters (MozUserSelect) already gain the leading dash above.
        return /^(webkit|moz|ms)-/.test(kebab) ? "-" + kebab : kebab;
    };
    // camelCase (`gridTemplateColumns`) or kebab-case (`grid-template-columns`,
    // via indexed access) property names
    const isCssPropName = (prop) =>
        typeof prop === "string" && /^-?[a-zA-Z][a-zA-Z0-9-]*$/.test(prop);
    globalThis.__blitz_wrap_style = function (native) {
        return new Proxy(native, {
            get(target, prop) {
                if (isCssPropName(prop) && !(prop in target)) {
                    return target.getPropertyValue(toKebab(prop));
                }
                const value = Reflect.get(target, prop, target);
                return typeof value === "function" ? value.bind(target) : value;
            },
            set(target, prop, value) {
                if (isCssPropName(prop) && !(prop in target)) {
                    target.setProperty(
                        toKebab(prop),
                        value === null || value === undefined ? "" : String(value)
                    );
                    return true;
                }
                return Reflect.set(target, prop, value, target);
            },
            // `'propertyName' in style` reports whether the engine supports the
            // property (used by WPT's computed-value test helpers)
            has(target, prop) {
                if (Reflect.has(target, prop)) return true;
                return (
                    isCssPropName(prop) && __blitz_css_property_supported(toKebab(prop))
                );
            },
        });
    };

    // DOM interface objects (`Node`, `Element`, ...) wired up to the native
    // wrapper prototypes so that constants and `instanceof` checks work.
    const makeInterface = (name, proto) => {
        const iface = function () {
            throw new TypeError("Illegal constructor");
        };
        Object.defineProperty(iface, "name", { value: name, configurable: true });
        iface.prototype = proto;
        Object.defineProperty(proto, "constructor", {
            value: iface,
            writable: true,
            configurable: true,
        });
        return iface;
    };

    const documentProto = Object.getPrototypeOf(document);
    const nodeProto = Object.getPrototypeOf(documentProto);
    globalThis.Node = makeInterface("Node", nodeProto);
    globalThis.Document = makeInterface("Document", documentProto);
    globalThis.HTMLDocument = globalThis.Document;
    if (document.documentElement) {
        const elementProto = Object.getPrototypeOf(document.documentElement);
        globalThis.Element = makeInterface("Element", elementProto);
        globalThis.HTMLElement = globalThis.Element;

        // `classList` (DOMTokenList), backed by the `class` attribute
        Object.defineProperty(elementProto, "classList", {
            configurable: true,
            get() {
                const el = this;
                const classes = () =>
                    (el.getAttribute("class") || "").split(/\s+/).filter(Boolean);
                const write = (list) => el.setAttribute("class", list.join(" "));
                return {
                    add(...names) {
                        const list = classes();
                        for (const name of names.map(String)) {
                            if (!list.includes(name)) list.push(name);
                        }
                        write(list);
                    },
                    remove(...names) {
                        const removed = names.map(String);
                        write(classes().filter((name) => !removed.includes(name)));
                    },
                    toggle(name, force) {
                        name = String(name);
                        const list = classes();
                        const has = list.includes(name);
                        const shouldHave = force !== undefined ? Boolean(force) : !has;
                        if (shouldHave && !has) list.push(name);
                        if (!shouldHave && has) list.splice(list.indexOf(name), 1);
                        write(list);
                        return shouldHave;
                    },
                    contains(name) {
                        return classes().includes(String(name));
                    },
                    item(index) {
                        return classes()[index] ?? null;
                    },
                    get length() {
                        return classes().length;
                    },
                    get value() {
                        return el.getAttribute("class") || "";
                    },
                    toString() {
                        return el.getAttribute("class") || "";
                    },
                    forEach(callback, thisArg) {
                        classes().forEach(callback, thisArg);
                    },
                };
            },
        });

        // `dataset` (DOMStringMap), backed by `data-*` attributes
        Object.defineProperty(elementProto, "dataset", {
            configurable: true,
            get() {
                const el = this;
                const toAttr = (prop) =>
                    "data-" + prop.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
                return new Proxy(Object.create(null), {
                    get(_, prop) {
                        if (typeof prop !== "string") return undefined;
                        const value = el.getAttribute(toAttr(prop));
                        return value === null ? undefined : value;
                    },
                    set(_, prop, value) {
                        if (typeof prop === "string") el.setAttribute(toAttr(prop), String(value));
                        return true;
                    },
                    has(_, prop) {
                        return typeof prop === "string" && el.getAttribute(toAttr(prop)) !== null;
                    },
                    deleteProperty(_, prop) {
                        if (typeof prop === "string") el.removeAttribute(toAttr(prop));
                        return true;
                    },
                });
            },
        });
    }

    // `CSS.escape` (the `CSS` namespace object itself, including `CSS.supports`,
    // is registered natively). Implements the CSSOM serialize-an-identifier
    // algorithm: https://drafts.csswg.org/cssom/#serialize-an-identifier
    if (typeof globalThis.CSS === "object" && typeof CSS.escape !== "function") {
        CSS.escape = function (value) {
            const string = String(value);
            const firstCodeUnit = string.charCodeAt(0);
            if (string.length === 1 && firstCodeUnit === 0x2d) {
                return "\\" + string;
            }
            let result = "";
            for (let index = 0; index < string.length; index++) {
                const codeUnit = string.charCodeAt(index);
                if (codeUnit === 0x0000) {
                    result += "\ufffd";
                } else if (
                    (codeUnit >= 0x0001 && codeUnit <= 0x001f) ||
                    codeUnit === 0x007f ||
                    (index === 0 && codeUnit >= 0x30 && codeUnit <= 0x39) ||
                    (index === 1 &&
                        codeUnit >= 0x30 &&
                        codeUnit <= 0x39 &&
                        firstCodeUnit === 0x2d)
                ) {
                    result += "\\" + codeUnit.toString(16) + " ";
                } else if (
                    codeUnit >= 0x0080 ||
                    codeUnit === 0x2d ||
                    codeUnit === 0x5f ||
                    (codeUnit >= 0x30 && codeUnit <= 0x39) ||
                    (codeUnit >= 0x41 && codeUnit <= 0x5a) ||
                    (codeUnit >= 0x61 && codeUnit <= 0x7a)
                ) {
                    result += string.charAt(index);
                } else {
                    result += "\\" + string.charAt(index);
                }
            }
            return result;
        };
    }

    // `document.fonts` (FontFaceSet) stub: all fonts report as loaded
    const fontFaceSet = {
        status: "loaded",
        size: 0,
        check: () => true,
        load: () => Promise.resolve([]),
        forEach() {},
        addEventListener() {},
        removeEventListener() {},
        dispatchEvent() {
            return true;
        },
    };
    fontFaceSet.ready = Promise.resolve(fontFaceSet);
    document.fonts = fontFaceSet;

    const NODE_CONSTANTS = {
        ELEMENT_NODE: 1,
        ATTRIBUTE_NODE: 2,
        TEXT_NODE: 3,
        CDATA_SECTION_NODE: 4,
        ENTITY_REFERENCE_NODE: 5,
        ENTITY_NODE: 6,
        PROCESSING_INSTRUCTION_NODE: 7,
        COMMENT_NODE: 8,
        DOCUMENT_NODE: 9,
        DOCUMENT_TYPE_NODE: 10,
        DOCUMENT_FRAGMENT_NODE: 11,
        NOTATION_NODE: 12,
    };
    for (const [name, value] of Object.entries(NODE_CONSTANTS)) {
        globalThis.Node[name] = value;
        nodeProto[name] = value;
    }
})();
"#;

/// Record an unhandled JavaScript error in the runtime state, for the embedder
/// to collect via [`ScriptDocument::take_js_errors`](crate::ScriptDocument::take_js_errors)
fn report_js_error(ctx: &DomCtx, what: &str, error: &boa_engine::JsError) {
    #[cfg(feature = "tracing")]
    tracing::error!("Uncaught JS error in {what}: {error}");
    ctx.state
        .borrow_mut()
        .record_error(format!("Uncaught JS error in {what}: {error}"));
}

/// A [`ModuleLoader`] which fetches ES module imports synchronously via the
/// document's [`ScriptFetcher`], resolving specifiers as URLs (relative to the
/// importing module, or to the document base URL).
struct BlitzModuleLoader {
    fetcher: Rc<RefCell<Box<dyn ScriptFetcher>>>,
    base_url: Option<Url>,
}

impl BlitzModuleLoader {
    fn resolve_specifier(&self, referrer: &Referrer, specifier: &str) -> Option<Url> {
        if let Ok(url) = Url::parse(specifier) {
            return Some(url);
        }
        let referrer_url = referrer
            .path()
            .and_then(|path| path.to_str())
            .and_then(|path| Url::parse(path).ok());
        let base = referrer_url.or_else(|| self.base_url.clone())?;
        base.join(specifier).ok()
    }
}

impl ModuleLoader for BlitzModuleLoader {
    async fn load_imported_module(
        self: Rc<Self>,
        referrer: Referrer,
        request: ModuleRequest,
        context: &RefCell<&mut Context>,
    ) -> JsResult<Module> {
        let specifier = request.specifier().to_std_string_escaped();
        let url = self
            .resolve_specifier(&referrer, &specifier)
            .ok_or_else(|| {
                JsError::from(
                    JsNativeError::typ()
                        .with_message(format!("could not resolve module specifier {specifier:?}")),
                )
            })?;
        let code = self.fetcher.borrow().fetch(&url).map_err(|error| {
            JsError::from(
                JsNativeError::typ().with_message(format!("failed to fetch module {url}: {error}")),
            )
        })?;
        let source = Source::from_reader(code.as_bytes(), Some(Path::new(url.as_str())));
        Module::parse(source, None, &mut context.borrow_mut())
    }
}

pub(crate) struct ScriptRuntime {
    pub context: Context,
    pub ctx: DomCtx,
}

impl ScriptRuntime {
    pub fn new(
        doc: Rc<RefCell<BaseDocument>>,
        base_url: Option<&Url>,
        fetcher: Rc<RefCell<Box<dyn ScriptFetcher>>>,
    ) -> Self {
        let module_loader = Rc::new(BlitzModuleLoader {
            fetcher,
            base_url: base_url.cloned(),
        });
        let mut context = Context::builder()
            .module_loader(module_loader)
            .build()
            .expect("failed to build JS context");
        let ctx = DomCtx::new(doc);
        context.insert_data(ctx.clone());

        Console::register_with_logger(DefaultLogger, &mut context)
            .expect("failed to register console");

        // Register boa_runtime's web-API extensions: atob/btoa, TextEncoder/
        // TextDecoder, structuredClone, queueMicrotask and URL.
        //
        // Deliberately NOT registered:
        // - TimeoutExtension: blitz-script has its own setTimeout/setInterval/
        //   requestAnimationFrame implementation integrated with the document's
        //   event loop and timer thread
        // - FetchExtension/AbortControllerExtension: fetch should go through
        //   the embedder's net provider, not an internal HTTP client
        boa_runtime::register_extensions(
            (
                boa_runtime::extensions::Base64Extension,
                boa_runtime::extensions::EncodingExtension,
                boa_runtime::extensions::StructuredCloneExtension,
                boa_runtime::extensions::MicrotaskExtension,
                boa_runtime::extensions::UrlExtension,
            ),
            None,
            &mut context,
        )
        .expect("failed to register boa_runtime extensions");

        crate::dom::init_protos(&ctx, &mut context);

        // `document`
        let root_id = ctx.doc.borrow().root_node().id;
        let document_wrapper = node_wrapper(&ctx, root_id, &mut context);
        register_global(&mut context, "document", document_wrapper.into());

        // `window` and friends (aliases for the global object)
        let global: JsValue = context.global_object().into();
        register_global(&mut context, "window", global.clone());
        register_global(&mut context, "self", global.clone());
        // There is only ever a single frame, so `parent` and `top` refer to the
        // window itself and `opener` is null
        register_global(&mut context, "parent", global.clone());
        register_global(&mut context, "top", global);
        register_global(&mut context, "opener", JsValue::null());

        // `location`
        let location = build_location(base_url, &mut context);
        register_global(&mut context, "location", location);

        // `navigator`
        let navigator = ObjectInitializer::new(&mut context)
            .property(
                js_string!("userAgent"),
                js_string!("Mozilla/5.0 (compatible; Blitz)"),
                Attribute::all(),
            )
            .build();
        register_global(&mut context, "navigator", navigator.into());

        // Timers and window event listeners
        register_global_fn(&mut context, "setTimeout", 2, set_timeout);
        register_global_fn(&mut context, "clearTimeout", 1, clear_timer);
        register_global_fn(&mut context, "setInterval", 2, set_interval);
        register_global_fn(&mut context, "clearInterval", 1, clear_timer);
        register_global_fn(
            &mut context,
            "requestAnimationFrame",
            1,
            request_animation_frame,
        );
        register_global_fn(&mut context, "cancelAnimationFrame", 1, clear_timer);
        register_global_fn(
            &mut context,
            "addEventListener",
            2,
            window_add_event_listener,
        );
        register_global_fn(
            &mut context,
            "removeEventListener",
            2,
            window_remove_event_listener,
        );

        // Embedder message channel (see `ScriptDocument::take_messages`)
        register_global_fn(&mut context, "__blitz_send_message", 1, send_message);

        // CSS property support check, used by the style Proxy's `has` trap
        register_global_fn(
            &mut context,
            "__blitz_css_property_supported",
            1,
            css_property_supported,
        );

        // `getComputedStyle`
        register_global_fn(&mut context, "getComputedStyle", 1, get_computed_style);

        // Viewport dimensions
        register_global_accessor(&mut context, "innerWidth", inner_width);
        register_global_accessor(&mut context, "innerHeight", inner_height);
        register_global_accessor(&mut context, "outerWidth", inner_width);
        register_global_accessor(&mut context, "outerHeight", inner_height);
        register_global_accessor(&mut context, "devicePixelRatio", device_pixel_ratio);

        // The `CSS` namespace object. `CSS.escape` is defined in the JS bootstrap.
        let css_namespace = ObjectInitializer::new(&mut context)
            .function(
                NativeFunction::from_fn_ptr(css_supports),
                js_string!("supports"),
                1,
            )
            .build();
        register_global(&mut context, "CSS", css_namespace.into());

        let mut runtime = Self { context, ctx };

        // Small JS bootstrap for APIs that are easiest to define in JS
        runtime.eval_internal(BOOTSTRAP_JS, "<blitz-bootstrap>");

        runtime
    }

    /// Evaluate a script, logging (but not propagating) any uncaught errors,
    /// then drain the microtask queue.
    pub fn eval(&mut self, code: &str, description: &str) {
        self.eval_internal(code, description);
        self.run_jobs(description);
    }

    fn eval_internal(&mut self, code: &str, description: &str) {
        if let Err(error) = self.context.eval(Source::from_bytes(code)) {
            report_js_error(&self.ctx, description, &error);
        }
    }

    /// Run pending promise jobs (microtasks)
    pub fn run_jobs(&mut self, description: &str) {
        if let Err(error) = self.context.run_jobs() {
            report_js_error(&self.ctx, description, &error);
        }
    }

    /// Evaluate an ES module script: parse it, load its imports (via the module
    /// loader), then link and evaluate it. Uncaught errors are logged (but not
    /// propagated), matching [`eval`](Self::eval).
    pub fn eval_module(&mut self, code: &str, url: Option<&Url>) {
        let description = url
            .map(Url::as_str)
            .unwrap_or("<inline module>")
            .to_string();
        let path = url.map(|url| Path::new(url.as_str()));
        let source = Source::from_reader(code.as_bytes(), path);

        let module = match Module::parse(source, None, &mut self.context) {
            Ok(module) => module,
            Err(error) => {
                report_js_error(&self.ctx, &description, &error);
                return;
            }
        };

        let promise = module.load_link_evaluate(&mut self.context);
        self.run_jobs(&description);
        if let PromiseState::Rejected(reason) = promise.state() {
            report_js_error(&self.ctx, &description, &JsError::from_opaque(reason));
        }
    }

    /// The deadline of the soonest pending timer (if any)
    pub fn next_timer_deadline(&self) -> Option<Instant> {
        self.ctx.state.borrow().timers.next_deadline()
    }

    /// Set the value exposed as `document.readyState`
    pub fn set_ready_state(&mut self, ready_state: ReadyState) {
        self.ctx.state.borrow_mut().ready_state = ready_state;
    }

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

    /// HTML named element access: expose elements with an `id` attribute as
    /// properties of the global (`window`) object, without overriding existing
    /// globals. Should be called before evaluating scripts so that newly
    /// parsed/created ids are visible.
    pub fn sync_named_element_globals(&mut self) {
        use boa_engine::property::{PropertyDescriptor, PropertyKey};

        let ids: Vec<(String, NodeId)> = {
            let doc = self.ctx.doc.borrow();
            let mut ids = Vec::new();
            let mut stack = vec![doc.root_node().id];
            while let Some(node_id) = stack.pop() {
                let Some(node) = doc.get_node(node_id) else {
                    continue;
                };
                if let Some(id) = node
                    .element_data()
                    .and_then(|element| element.attr(blitz_dom::local_name!("id")))
                {
                    if !id.is_empty() {
                        ids.push((id.to_string(), node_id));
                    }
                }
                stack.extend(node.children.iter().rev().copied());
            }
            ids
        };

        let global = self.context.global_object();
        for (id, node_id) in ids {
            let key = PropertyKey::from(JsString::from(id));
            let already_defined = global
                .has_own_property(key.clone(), &mut self.context)
                .unwrap_or(true);
            if already_defined {
                continue;
            }
            let wrapper = node_wrapper(&self.ctx, node_id, &mut self.context);
            let _ = global.define_property_or_throw(
                key,
                PropertyDescriptor::builder()
                    .value(wrapper)
                    .writable(true)
                    .enumerable(false)
                    .configurable(true)
                    .build(),
                &mut self.context,
            );
        }
    }

    /// Run all timers that are currently due. Returns `true` if any JavaScript was run.
    pub fn run_due_timers(&mut self) -> bool {
        let due = self.ctx.state.borrow_mut().timers.take_due(Instant::now());
        if due.is_empty() {
            return false;
        }
        for timer in due {
            if let Err(error) =
                timer
                    .callback
                    .call(&JsValue::undefined(), &timer.args, &mut self.context)
            {
                report_js_error(&self.ctx, "timer callback", &error);
            }
        }
        self.run_jobs("timer microtasks");
        true
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
                    report_js_error(&ctx, "event listener", &error);
                }
                if event_ref(&event_obj, &|event| event.stopped_immediate.get()) {
                    break 'chain;
                }
            }

            if !bubbles || event_ref(&event_obj, &|event| event.stopped.get()) {
                break;
            }
        }

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
                        report_js_error(&ctx, "event listener", &error);
                    }
                    if event_ref(&event_obj, &|event| event.stopped_immediate.get()) {
                        break;
                    }
                }
            }
        }

        crate::dom::define_value(&event_obj, "currentTarget", JsValue::null(), context);

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
                report_js_error(&ctx, "event listener", &error);
            }
        }

        // `window.onload = ...` style handler
        let on_name = JsString::from(format!("on{name}"));
        if let Ok(handler) = context.global_object().get(on_name, context) {
            if let Some(handler) = handler.as_object() {
                if handler.is_callable() {
                    any_called = true;
                    if let Err(error) = handler.call(&global, &[event_obj.into()], context) {
                        report_js_error(&ctx, "event listener", &error);
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

fn register_global(context: &mut Context, name: &str, value: JsValue) {
    context
        .register_global_property(
            JsString::from(name),
            value,
            Attribute::WRITABLE.union(Attribute::CONFIGURABLE),
        )
        .expect("failed to register global");
}

fn register_global_accessor(
    context: &mut Context,
    name: &str,
    getter: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
) {
    use boa_engine::object::FunctionObjectBuilder;
    use boa_engine::property::{PropertyDescriptor, PropertyKey};

    let getter_fn =
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(getter))
            .name(JsString::from(format!("get {name}")))
            .length(0)
            .build();
    context
        .global_object()
        .define_property_or_throw(
            PropertyKey::from(JsString::from(name)),
            PropertyDescriptor::builder()
                .get(getter_fn)
                .enumerable(false)
                .configurable(true)
                .build(),
            context,
        )
        .expect("failed to register global accessor");
}

fn register_global_fn(
    context: &mut Context,
    name: &str,
    length: usize,
    body: fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
) {
    context
        .register_global_callable(
            JsString::from(name),
            length,
            NativeFunction::from_fn_ptr(body),
        )
        .expect("failed to register global function");
}

fn build_location(base_url: Option<&Url>, context: &mut Context) -> JsValue {
    let (href, protocol, host, pathname, search, hash) = match base_url {
        Some(url) => (
            url.to_string(),
            format!("{}:", url.scheme()),
            url.host_str().unwrap_or_default().to_string(),
            url.path().to_string(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default(),
            url.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
        ),
        None => (
            "about:blank".to_string(),
            "about:".to_string(),
            String::new(),
            "blank".to_string(),
            String::new(),
            String::new(),
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
        .build()
        .into()
}

// === Timer + window listener native functions ===

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

fn set_timeout(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some((callback, delay, rest)) = timer_args(args, context)? else {
        return Ok(JsValue::from(0));
    };
    let id = ctx
        .state
        .borrow_mut()
        .timers
        .add(delay, None, callback, rest);
    Ok(JsValue::from(id as f64))
}

fn set_interval(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some((callback, delay, rest)) = timer_args(args, context)? else {
        return Ok(JsValue::from(0));
    };
    let id = ctx
        .state
        .borrow_mut()
        .timers
        .add(delay, Some(delay), callback, rest);
    Ok(JsValue::from(id as f64))
}

fn request_animation_frame(
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
    let id = ctx.state.borrow_mut().timers.add(
        Duration::from_millis(16),
        None,
        callback,
        vec![timestamp],
    );
    Ok(JsValue::from(id as f64))
}

// === Viewport dimensions ===

fn inner_width(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let doc = ctx.doc.borrow();
    let viewport = doc.viewport();
    Ok(JsValue::from(
        viewport.window_size.0 as f64 / viewport.scale() as f64,
    ))
}

fn inner_height(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let doc = ctx.doc.borrow();
    let viewport = doc.viewport();
    Ok(JsValue::from(
        viewport.window_size.1 as f64 / viewport.scale() as f64,
    ))
}

fn device_pixel_ratio(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let scale = ctx.doc.borrow().viewport().scale();
    Ok(JsValue::from(scale as f64))
}

fn css_property_supported(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    Ok(JsValue::from(blitz_dom::css_property_is_supported(&name)))
}

/// `CSS.supports()`: the two-argument form checks a property/value declaration,
/// the one-argument form evaluates a `@supports` condition
fn css_supports(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let supported = if args.len() >= 2 {
        let property = to_rust_string(&args[0], context)?;
        let value = to_rust_string(&args[1], context)?;
        ctx.doc.borrow().css_declaration_is_valid(&property, &value)
    } else {
        let condition = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
        ctx.doc.borrow().css_supports_condition(&condition)
    };
    Ok(JsValue::from(supported))
}

fn get_computed_style(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some(node_id) = args.first().and_then(node_id_of_value) else {
        return Err(boa_engine::JsNativeError::typ()
            .with_message("getComputedStyle: argument is not an Element")
            .into());
    };
    let proto = ctx.state.borrow().protos().computed_style.clone();
    let obj = JsObject::from_proto_and_data(Some(proto), NodeRef { node_id });
    Ok(wrap_style_object(obj, context))
}

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

fn clear_timer(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
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

fn window_add_event_listener(
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

fn window_remove_event_listener(
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
