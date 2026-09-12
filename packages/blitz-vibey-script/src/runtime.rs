//! The script runtime: owns the Boa [`Context`], registers the DOM globals and
//! dispatches events / timers into JavaScript.

use std::cell::RefCell;
use std::collections::HashMap;
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
use boa_engine::{Finalize, Trace};
use boa_runtime::Console;
use boa_runtime::console::{ConsoleState, Logger};
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
    // `window.history`: an in-memory History implementation, sufficient for
    // SPA routers (e.g. React Router): `state`, `pushState`/`replaceState`
    // (which update `location`) and no-op `go`/`back`/`forward`. There is no
    // real session history, so `popstate` events are never fired.
    if (typeof globalThis.history === "undefined") {
        const updateLocationFromUrl = (url) => {
            try {
                const resolved = new URL(String(url), location.href);
                location.href = resolved.href;
                location.protocol = resolved.protocol;
                location.host = resolved.host;
                location.hostname = resolved.hostname;
                location.pathname = resolved.pathname;
                location.search = resolved.search;
                location.hash = resolved.hash;
            } catch (e) {
                // Unresolvable URL (e.g. no base): leave location unchanged
            }
        };
        let historyState = null;
        globalThis.history = {
            length: 1,
            scrollRestoration: "auto",
            get state() {
                return historyState;
            },
            pushState(state, _unused, url) {
                historyState = state;
                if (url !== undefined && url !== null) updateLocationFromUrl(url);
            },
            replaceState(state, _unused, url) {
                historyState = state;
                if (url !== undefined && url !== null) updateLocationFromUrl(url);
            },
            go() {},
            back() {},
            forward() {},
        };
    }

    if (typeof globalThis.queueMicrotask !== "function") {
        globalThis.queueMicrotask = function (callback) {
            Promise.resolve().then(callback);
        };
    }

    // `fetch()`: a minimal Fetch API on top of the synchronous `__blitz_fetch_sync`
    // native (which goes through the document's ScriptFetcher). Bodies are
    // text-only; `Headers` is a simple case-insensitive map.
    if (typeof globalThis.fetch !== "function") {
        class Headers {
            #map = new Map();
            constructor(init) {
                if (init instanceof Headers) {
                    for (const [k, v] of init) this.append(k, v);
                } else if (Array.isArray(init)) {
                    for (const [k, v] of init) this.append(k, v);
                } else if (init && typeof init === "object") {
                    for (const k of Object.keys(init)) this.append(k, init[k]);
                }
            }
            append(name, value) {
                const key = String(name).toLowerCase();
                const existing = this.#map.get(key);
                this.#map.set(key, existing === undefined ? String(value) : existing + ", " + value);
            }
            set(name, value) { this.#map.set(String(name).toLowerCase(), String(value)); }
            get(name) { return this.#map.get(String(name).toLowerCase()) ?? null; }
            has(name) { return this.#map.has(String(name).toLowerCase()); }
            delete(name) { this.#map.delete(String(name).toLowerCase()); }
            forEach(cb, thisArg) { for (const [k, v] of this) cb.call(thisArg, v, k, this); }
            *entries() { yield* [...this.#map.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1)); }
            *keys() { for (const [k] of this.entries()) yield k; }
            *values() { for (const [, v] of this.entries()) yield v; }
            [Symbol.iterator]() { return this.entries(); }
        }

        class Response {
            #body;
            #bodyUsed = false;
            constructor(body = null, init = {}) {
                this.#body = body === null || body === undefined ? "" : String(body);
                const status = init.status === undefined ? 200 : Number(init.status);
                if (!Number.isInteger(status) || status < 200 || status > 599) {
                    throw new RangeError("The status provided (" + status + ") is outside the range [200, 599].");
                }
                Object.defineProperties(this, {
                    status: { value: status, enumerable: true },
                    statusText: { value: init.statusText === undefined ? "" : String(init.statusText), enumerable: true },
                    headers: { value: new Headers(init.headers), enumerable: true },
                    url: { value: init.url === undefined ? "" : String(init.url), enumerable: true },
                    type: { value: "basic", enumerable: true },
                    redirected: { value: false, enumerable: true },
                });
            }
            get ok() { return this.status >= 200 && this.status <= 299; }
            get bodyUsed() { return this.#bodyUsed; }
            #consume() {
                if (this.#bodyUsed) {
                    return Promise.reject(new TypeError("body stream already read"));
                }
                this.#bodyUsed = true;
                return Promise.resolve(this.#body);
            }
            text() { return this.#consume(); }
            json() { return this.#consume().then((text) => JSON.parse(text)); }
            arrayBuffer() { return this.#consume().then((text) => new TextEncoder().encode(text).buffer); }
            clone() {
                if (this.#bodyUsed) throw new TypeError("Response body is already used");
                return new Response(this.#body, {
                    status: this.status, statusText: this.statusText, headers: this.headers, url: this.url,
                });
            }
        }

        globalThis.Headers = Headers;
        globalThis.Response = Response;
        globalThis.fetch = function fetch(input, init) {
            return new Promise((resolve, reject) => {
                let url;
                try {
                    url = input && typeof input === "object" && "url" in input ? String(input.url) : String(input);
                } catch (e) {
                    reject(e);
                    return;
                }
                const method = String(init?.method ?? "GET").toUpperCase();
                if (method !== "GET" && method !== "HEAD") {
                    reject(new TypeError("fetch: only GET and HEAD requests are supported"));
                    return;
                }
                try {
                    const [status, resolvedUrl, text] = __blitz_fetch_sync(url);
                    resolve(new Response(method === "HEAD" ? "" : text, {
                        status,
                        statusText: status === 200 ? "OK" : status === 404 ? "Not Found" : "",
                        url: resolvedUrl,
                    }));
                } catch (e) {
                    reject(e);
                }
            });
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

    // Stub constructors for interfaces referenced by `instanceof` probes
    // (e.g. React probes `x instanceof HTMLInputElement`); without them such
    // probes throw "right-hand side of 'instanceof' is not an object". All
    // blitz-vibey-script elements share a single prototype, so tag-specific
    // interfaces cannot be truthfully modelled: these always answer false.
    // `Window`: the global object's interface (`window instanceof Window`).
    // Feature-detection code (and WPT's idlharness) uses `'Window' in self`
    // to tell a window global from a worker or ShadowRealm global.
    if (typeof globalThis.Window === "undefined") {
        const windowProto = Object.create(Object.getPrototypeOf(globalThis));
        Object.setPrototypeOf(globalThis, windowProto);
        globalThis.Window = makeInterface("Window", windowProto);
    }

    for (const name of [
        "EventTarget", "CharacterData", "Text", "Comment", "DocumentFragment",
        "HTMLInputElement", "HTMLTextAreaElement", "HTMLSelectElement",
        "HTMLButtonElement", "HTMLAnchorElement", "HTMLIFrameElement",
        "HTMLImageElement", "SVGElement",
        "Event", "CustomEvent", "UIEvent", "MouseEvent", "PointerEvent",
        "KeyboardEvent", "InputEvent", "FocusEvent",
    ]) {
        if (typeof globalThis[name] === "undefined") {
            globalThis[name] = makeInterface(name, {});
        }
    }
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

    // CSSOM stylesheet API (`document.styleSheets`, `element.sheet`,
    // `CSSStyleSheet`, `CSSRuleList`, `CSSRule` subclasses), backed by the
    // `__blitz_sheet_*` natives. Stylesheets are keyed by their owner node and
    // rules by a path of indices through nested rule lists.
    {
        const illegal = () => {
            throw new TypeError("Illegal constructor");
        };
        // Internal (constructor-bypassing) construction flag
        let constructing = false;
        const construct = (cls, init) => {
            constructing = true;
            try {
                const obj = new cls();
                init(obj);
                return obj;
            } finally {
                constructing = false;
            }
        };
        const internals = new WeakMap();
        const data = (obj) => {
            const d = internals.get(obj);
            if (!d) throw new TypeError("Illegal invocation");
            return d;
        };

        // Array-like list objects (`StyleSheetList`, `CSSRuleList`, `MediaList`)
        // support indexed access via a Proxy
        const indexedProxy = (target) =>
            new Proxy(target, {
                // Getters and methods run against the target (not the proxy):
                // their internal data is keyed by the target object
                get(t, prop) {
                    if (typeof prop === "string" && /^\d+$/.test(prop)) {
                        const item = t.item(Number(prop));
                        return item === null ? undefined : item;
                    }
                    const value = Reflect.get(t, prop, t);
                    return typeof value === "function" ? value.bind(t) : value;
                },
                set(t, prop, value) {
                    return Reflect.set(t, prop, value, t);
                },
                has(t, prop) {
                    if (typeof prop === "string" && /^\d+$/.test(prop)) {
                        return Number(prop) < t.length;
                    }
                    return Reflect.has(t, prop);
                },
                ownKeys(t) {
                    const keys = [];
                    for (let i = 0; i < t.length; i++) keys.push(String(i));
                    return keys.concat(Reflect.ownKeys(t));
                },
                getOwnPropertyDescriptor(t, prop) {
                    if (typeof prop === "string" && /^\d+$/.test(prop) && Number(prop) < t.length) {
                        return { value: t.item(Number(prop)), enumerable: true, configurable: true, writable: false };
                    }
                    return Reflect.getOwnPropertyDescriptor(t, prop);
                },
            });
        const defineIterable = (proto) => {
            proto[Symbol.iterator] = function* () {
                for (let i = 0; i < this.length; i++) yield this.item(i);
            };
            proto.forEach = function (callback, thisArg) {
                for (let i = 0; i < this.length; i++) callback.call(thisArg, this.item(i), i, this);
            };
        };

        class MediaList {
            constructor() {
                if (!constructing) illegal();
            }
            get mediaText() {
                return data(this).text;
            }
            get length() {
                return this.mediaText === "" ? 0 : this.mediaText.split(",").length;
            }
            item(index) {
                if (this.mediaText === "") return null;
                return this.mediaText.split(",").map((s) => s.trim())[index] ?? null;
            }
            toString() {
                return this.mediaText;
            }
        }
        defineIterable(MediaList.prototype);
        const makeMediaList = (text) =>
            indexedProxy(construct(MediaList, (m) => internals.set(m, { text })));

        class StyleSheet {
            constructor() {
                if (!constructing) illegal();
            }
            get type() {
                return "text/css";
            }
            get ownerNode() {
                return data(this).owner;
            }
            get href() {
                const owner = data(this).owner;
                return owner.localName === "link" ? owner.getAttribute("href") : null;
            }
            get title() {
                return data(this).owner.getAttribute("title");
            }
            get media() {
                return makeMediaList(data(this).owner.getAttribute("media") || "");
            }
            get parentStyleSheet() {
                return null;
            }
            get ownerRule() {
                return null;
            }
            get disabled() {
                return false;
            }
        }

        // `CSSStyleDeclaration` for the declarations of a rule (`CSSStyleRule.style`, ...)
        class RuleStyleDeclaration {
            constructor() {
                if (!constructing) illegal();
            }
            get cssText() {
                const d = data(this);
                return __blitz_sheet_style_css_text(d.owner, d.path);
            }
            set cssText(value) {
                const d = data(this);
                __blitz_sheet_style_set_css_text(d.owner, d.path, String(value));
                touchSheet(data(d.rule).sheet);
            }
            get length() {
                const d = data(this);
                return __blitz_sheet_style_property_names(d.owner, d.path).length;
            }
            item(index) {
                const d = data(this);
                return __blitz_sheet_style_property_names(d.owner, d.path)[index] ?? "";
            }
            getPropertyValue(name) {
                const d = data(this);
                return __blitz_sheet_style_get(d.owner, d.path, String(name))[0];
            }
            getPropertyPriority(name) {
                const d = data(this);
                return __blitz_sheet_style_get(d.owner, d.path, String(name))[1] ? "important" : "";
            }
            setProperty(name, value, priority) {
                const d = data(this);
                value = value === null || value === undefined ? "" : String(value);
                const important = String(priority ?? "").toLowerCase() === "important";
                __blitz_sheet_style_set(d.owner, d.path, String(name), value, important);
                touchSheet(data(d.rule).sheet);
            }
            removeProperty(name) {
                const d = data(this);
                const removed = __blitz_sheet_style_remove(d.owner, d.path, String(name));
                touchSheet(data(d.rule).sheet);
                return removed;
            }
            get parentRule() {
                return data(this).rule;
            }
        }
        defineIterable(RuleStyleDeclaration.prototype);
        const makeRuleStyle = (owner, path, rule) =>
            __blitz_wrap_style(
                indexedProxy(
                    construct(RuleStyleDeclaration, (s) => internals.set(s, { owner, path, rule }))
                )
            );

        class CSSRuleList {
            constructor() {
                if (!constructing) illegal();
            }
            get length() {
                const d = data(this);
                return Math.max(0, __blitz_sheet_rule_count(d.owner, d.path));
            }
            item(index) {
                const d = data(this);
                index = Number(index);
                if (!(index >= 0 && index < this.length)) return null;
                return ruleAt(d.sheet, d.owner, d.path.concat(index), d.parentRule);
            }
        }
        defineIterable(CSSRuleList.prototype);
        const makeRuleList = (sheet, owner, path, parentRule) =>
            indexedProxy(
                construct(CSSRuleList, (l) => internals.set(l, { sheet, owner, path, parentRule }))
            );

        const CSS_RULE_TYPES = {
            STYLE_RULE: 1,
            CHARSET_RULE: 2,
            IMPORT_RULE: 3,
            MEDIA_RULE: 4,
            FONT_FACE_RULE: 5,
            PAGE_RULE: 6,
            KEYFRAMES_RULE: 7,
            KEYFRAME_RULE: 8,
            MARGIN_RULE: 9,
            NAMESPACE_RULE: 10,
            COUNTER_STYLE_RULE: 11,
            SUPPORTS_RULE: 12,
            DOCUMENT_RULE: 13,
            FONT_FEATURE_VALUES_RULE: 14,
            VIEWPORT_RULE: 15,
            REGION_STYLE_RULE: 16,
        };

        class CSSRule {
            constructor() {
                if (!constructing) illegal();
            }
            get type() {
                return data(this).info().type;
            }
            get cssText() {
                return data(this).info().cssText;
            }
            set cssText(_) {}
            get parentRule() {
                return data(this).parentRule;
            }
            get parentStyleSheet() {
                return data(this).sheet;
            }
        }
        for (const [name, value] of Object.entries(CSS_RULE_TYPES)) {
            CSSRule[name] = value;
            CSSRule.prototype[name] = value;
        }

        const attrGetter = (name, convert) =>
            function () {
                const value = data(this).info().attrs[name];
                return convert ? convert(value) : value;
            };
        const defineAttrs = (cls, attrs) => {
            for (const [name, convert] of Object.entries(attrs)) {
                Object.defineProperty(cls.prototype, name, {
                    get: attrGetter(name, convert),
                    configurable: true,
                    enumerable: true,
                });
            }
        };
        const cssRulesGetter = function () {
            const d = data(this);
            return d.rules || (d.rules = makeRuleList(d.sheet, d.owner, d.path, this));
        };
        const groupingMethods = {
            get cssRules() {
                return cssRulesGetter.call(this);
            },
            insertRule(rule, index) {
                const d = data(this);
                index = index === undefined ? 0 : Number(index) >>> 0;
                const result = __blitz_sheet_insert_rule(d.owner, d.path, String(rule), index);
                d.sheet && invalidateRules(d.sheet);
                return result;
            },
            deleteRule(index) {
                const d = data(this);
                __blitz_sheet_delete_rule(d.owner, d.path, Number(index) >>> 0);
                d.sheet && invalidateRules(d.sheet);
            },
        };
        const styleGetter = {
            get style() {
                const d = data(this);
                return d.style || (d.style = makeRuleStyle(d.owner, d.path, this));
            },
        };

        const ruleClasses = {};
        const defineRuleClass = (name, base, mixins, attrs) => {
            const cls = class extends base {
                constructor() {
                    super();
                }
            };
            Object.defineProperty(cls, "name", { value: name, configurable: true });
            for (const mixin of mixins) {
                Object.defineProperties(cls.prototype, Object.getOwnPropertyDescriptors(mixin));
            }
            if (attrs) defineAttrs(cls, attrs);
            ruleClasses[name] = cls;
            globalThis[name] = cls;
            return cls;
        };
        const str = (v) => (v === undefined ? "" : v);
        const CSSGroupingRule = defineRuleClass("CSSGroupingRule", CSSRule, [groupingMethods]);
        const CSSConditionRule = defineRuleClass("CSSConditionRule", CSSGroupingRule, [], {
            conditionText: str,
        });
        defineRuleClass("CSSStyleRule", CSSGroupingRule, [styleGetter], { selectorText: str });
        defineRuleClass("CSSNestedDeclarations", CSSRule, [styleGetter]);
        defineRuleClass("CSSMediaRule", CSSConditionRule, [
            {
                get media() {
                    return makeMediaList(this.conditionText);
                },
            },
        ]);
        defineRuleClass("CSSSupportsRule", CSSConditionRule, []);
        defineRuleClass("CSSContainerRule", CSSConditionRule, [], {
            containerName: () => "",
            containerQuery: str,
        });
        defineRuleClass("CSSMozDocumentRule", CSSConditionRule, []);
        defineRuleClass("CSSLayerBlockRule", CSSGroupingRule, [], { name: str });
        defineRuleClass("CSSLayerStatementRule", CSSRule, [], {
            nameList: (v) => (v ? v.split(",").map((s) => s.trim()) : []),
        });
        defineRuleClass("CSSScopeRule", CSSGroupingRule, []);
        defineRuleClass("CSSStartingStyleRule", CSSGroupingRule, []);
        defineRuleClass("CSSPageRule", CSSGroupingRule, [styleGetter], { selectorText: str });
        defineRuleClass("CSSMarginRule", CSSRule, [styleGetter], { name: str });
        defineRuleClass("CSSImportRule", CSSRule, [], {
            href: str,
            media: (v) => makeMediaList(str(v)),
            styleSheet: () => null,
            layerName: () => null,
            supportsText: () => null,
        });
        defineRuleClass("CSSNamespaceRule", CSSRule, [], { namespaceURI: str, prefix: str });
        defineRuleClass("CSSFontFaceRule", CSSRule, [styleGetter]);
        defineRuleClass("CSSFontFeatureValuesRule", CSSRule, [], { fontFamily: str });
        defineRuleClass("CSSFontPaletteValuesRule", CSSRule, [], {
            name: str,
            fontFamily: str,
            basePalette: str,
            overrideColors: str,
        });
        defineRuleClass("CSSCounterStyleRule", CSSRule, [], { name: str });
        defineRuleClass("CSSPropertyRule", CSSRule, [], {
            name: str,
            syntax: str,
            inherits: (v) => v === "true",
            initialValue: (v) => (v ? v : null),
        });
        defineRuleClass("CSSPositionTryRule", CSSRule, [styleGetter], { name: str });
        defineRuleClass("CSSViewTransitionRule", CSSRule, []);
        defineRuleClass("CSSAppearanceBaseRule", CSSGroupingRule, []);
        defineRuleClass("CSSCustomMediaRule", CSSRule, []);
        defineRuleClass("CSSKeyframeRule", CSSRule, [styleGetter], { keyText: str });
        defineRuleClass(
            "CSSKeyframesRule",
            CSSRule,
            [
                {
                    get cssRules() {
                        return cssRulesGetter.call(this);
                    },
                    get length() {
                        return this.cssRules.length;
                    },
                    appendRule(rule) {
                        const d = data(this);
                        __blitz_sheet_insert_rule(d.owner, d.path, String(rule), 0);
                        d.sheet && invalidateRules(d.sheet);
                    },
                    deleteRule(select) {
                        const index = findKeyframeIndex(this, select);
                        if (index === -1) return;
                        const d = data(this);
                        __blitz_sheet_delete_rule(d.owner, d.path, index);
                        d.sheet && invalidateRules(d.sheet);
                    },
                    findRule(select) {
                        const index = findKeyframeIndex(this, select);
                        return index === -1 ? null : this.cssRules.item(index);
                    },
                },
            ],
            { name: str }
        );
        // Match a keyframe selector string the way `findRule`/`deleteRule` do
        // (normalising `from`/`to` and whitespace), returning the last match
        const findKeyframeIndex = (keyframesRule, select) => {
            const normalise = (s) =>
                String(s)
                    .split(",")
                    .map((k) => k.trim().toLowerCase())
                    .map((k) => (k === "from" ? "0%" : k === "to" ? "100%" : k))
                    .join(", ");
            const wanted = normalise(select);
            const rules = keyframesRule.cssRules;
            for (let i = rules.length - 1; i >= 0; i--) {
                if (normalise(rules.item(i).keyText) === wanted) return i;
            }
            return -1;
        };

        // Rule wrapper objects are cached per stylesheet by path so that repeated
        // `cssRules[i]` accesses return the same object; the cache is dropped
        // whenever the rule list is mutated (indices shift).
        const ruleCaches = new WeakMap();
        // Per-sheet mutation counter, so that rule wrappers re-fetch their
        // (serialized) info from the native side only when it may have changed.
        // The document-wide native counter is folded in so that replacing a
        // node's stylesheet (e.g. editing a `<style>`'s text) is detected too.
        const generations = new WeakMap();
        const sheetGeneration = (sheet) => (sheet && generations.get(sheet)) || 0;
        const generation = (sheet) =>
            __blitz_stylesheet_generation() + ":" + sheetGeneration(sheet);
        const touchSheet = (sheet) => {
            if (sheet) generations.set(sheet, sheetGeneration(sheet) + 1);
        };
        const invalidateRules = (sheet) => {
            ruleCaches.delete(sheet);
            touchSheet(sheet);
        };
        const ruleAt = (sheet, owner, path, parentRule) => {
            const key = path.join("/");
            let cache = sheet && ruleCaches.get(sheet);
            if (cache && cache.generation !== generation(sheet)) {
                ruleCaches.delete(sheet);
                cache = undefined;
            }
            if (cache && cache.has(key)) return cache.get(key);
            let info = __blitz_sheet_rule_info(owner, path);
            if (info === null) return null;
            let infoGeneration = generation(sheet);
            const cls = ruleClasses[info.interface] || CSSRule;
            const rule = construct(cls, (r) =>
                internals.set(r, {
                    sheet,
                    owner,
                    path,
                    parentRule,
                    info: () => {
                        const current = generation(sheet);
                        if (current !== infoGeneration) {
                            info = __blitz_sheet_rule_info(owner, path) || info;
                            infoGeneration = current;
                        }
                        return info;
                    },
                })
            );
            if (sheet) {
                if (!cache) {
                    cache = new Map();
                    cache.generation = infoGeneration;
                    ruleCaches.set(sheet, cache);
                }
                cache.set(key, rule);
            }
            return rule;
        };

        class CSSStyleSheet extends StyleSheet {
            constructor() {
                super();
            }
            get cssRules() {
                const d = data(this);
                return d.rules || (d.rules = makeRuleList(this, d.owner, [], null));
            }
            get rules() {
                return this.cssRules;
            }
            insertRule(rule, index) {
                const d = data(this);
                index = index === undefined ? 0 : Number(index) >>> 0;
                const result = __blitz_sheet_insert_rule(d.owner, [], String(rule), index);
                invalidateRules(this);
                return result;
            }
            deleteRule(index) {
                const d = data(this);
                __blitz_sheet_delete_rule(d.owner, [], Number(index) >>> 0);
                invalidateRules(this);
            }
            addRule(selector, block, index) {
                const rule = (selector ?? "undefined") + " {" + (block ?? "undefined") + "}";
                this.insertRule(rule, index === undefined ? this.cssRules.length : index);
                return -1;
            }
            removeRule(index) {
                this.deleteRule(index ?? 0);
            }
        }

        class StyleSheetList {
            constructor() {
                if (!constructing) illegal();
            }
            get length() {
                return data(this).sheets.length;
            }
            item(index) {
                return data(this).sheets[index] ?? null;
            }
        }
        defineIterable(StyleSheetList.prototype);

        // One `CSSStyleSheet` object per owner node (node wrappers are cached,
        // so identity is stable)
        const sheets = new WeakMap();
        globalThis.__blitz_sheet_for_node = (node) => {
            if (!__blitz_node_has_stylesheet(node)) return null;
            let sheet = sheets.get(node);
            if (!sheet) {
                sheet = construct(CSSStyleSheet, (s) => internals.set(s, { owner: node }));
                sheets.set(node, sheet);
            }
            return sheet;
        };
        globalThis.__blitz_style_sheets = () => {
            const list = __blitz_stylesheet_owner_nodes().map(__blitz_sheet_for_node);
            return indexedProxy(construct(StyleSheetList, (l) => internals.set(l, { sheets: list })));
        };

        globalThis.StyleSheet = StyleSheet;
        globalThis.CSSStyleSheet = CSSStyleSheet;
        globalThis.StyleSheetList = StyleSheetList;
        globalThis.CSSRuleList = CSSRuleList;
        globalThis.CSSRule = CSSRule;
        globalThis.MediaList = MediaList;
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
    /// Modules already fetched and parsed, keyed by resolved URL. Serving
    /// repeat imports from here breaks import cycles (e.g. `a.js` importing
    /// `b.js` which imports `a.js`), which would otherwise load forever.
    modules: RefCell<HashMap<Url, Module>>,
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

    fn get(&self, url: &Url) -> Option<Module> {
        self.modules.borrow().get(url).cloned()
    }

    fn insert(&self, url: Url, module: Module) {
        self.modules.borrow_mut().insert(url, module);
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
        if let Some(module) = self.get(&url) {
            return Ok(module);
        }
        let code = self.fetcher.borrow().fetch(&url).map_err(|error| {
            JsError::from(
                JsNativeError::typ().with_message(format!("failed to fetch module {url}: {error}")),
            )
        })?;
        let source = Source::from_reader(code.as_bytes(), Some(Path::new(url.as_str())));
        let module = Module::parse(source, None, &mut context.borrow_mut())?;
        self.insert(url, module.clone());
        Ok(module)
    }
}

/// Console logger which routes all console output to the `log` crate at
/// `debug` level (target `js_console`), keeping stdout/stderr clean.
#[derive(Debug, Trace, Finalize)]
struct LogCrateLogger;

impl Logger for LogCrateLogger {
    fn log(&self, msg: String, _state: &ConsoleState, _context: &mut Context) -> JsResult<()> {
        log::debug!(target: "js_console", "{msg}");
        Ok(())
    }

    fn info(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }

    fn warn(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }

    fn error(&self, msg: String, state: &ConsoleState, context: &mut Context) -> JsResult<()> {
        self.log(msg, state, context)
    }
}

pub(crate) struct ScriptRuntime {
    pub context: Context,
    pub ctx: DomCtx,
    module_loader: Rc<BlitzModuleLoader>,
}

impl ScriptRuntime {
    pub fn new(
        doc: Rc<RefCell<BaseDocument>>,
        base_url: Option<&Url>,
        fetcher: Rc<RefCell<Box<dyn ScriptFetcher>>>,
    ) -> Self {
        let module_loader = Rc::new(BlitzModuleLoader {
            fetcher: Rc::clone(&fetcher),
            base_url: base_url.cloned(),
            modules: RefCell::new(HashMap::new()),
        });
        let ctx = DomCtx::new(doc);
        {
            let mut state = ctx.state.borrow_mut();
            state.base_url = base_url.cloned();
            state.fetcher = Some(fetcher);
        }
        // Share the runtime's clock with boa so that `Date` observes the same
        // (possibly virtual) time as timers
        let clock = ctx.state.borrow().clock.clone();
        let mut context = Context::builder()
            .module_loader(module_loader.clone())
            .clock(Rc::new(crate::clock::BoaClockAdapter::new(clock)))
            .build()
            .expect("failed to build JS context");
        context.insert_data(ctx.clone());

        Console::register_with_logger(LogCrateLogger, &mut context)
            .expect("failed to register console");

        // Register boa_runtime's web-API extensions: atob/btoa, TextEncoder/
        // TextDecoder, structuredClone, queueMicrotask and URL.
        //
        // Deliberately NOT registered:
        // - TimeoutExtension: blitz-vibey-script has its own setTimeout/setInterval/
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

        // Synchronous resource fetch backing the bootstrap's `fetch()`
        register_global_fn(&mut context, "__blitz_fetch_sync", 1, fetch_sync);

        // CSS property support check, used by the style Proxy's `has` trap
        register_global_fn(
            &mut context,
            "__blitz_css_property_supported",
            1,
            css_property_supported,
        );

        // `getComputedStyle`
        register_global_fn(&mut context, "getComputedStyle", 1, get_computed_style);

        // CSSOM stylesheet natives (`__blitz_sheet_*`), used by the bootstrap's
        // `CSSStyleSheet` / `CSSRule` implementation
        crate::dom::stylesheet::register(&mut context);

        // Viewport dimensions
        register_global_accessor(&mut context, "innerWidth", inner_width);
        register_global_accessor(&mut context, "innerHeight", inner_height);
        register_global_accessor(&mut context, "outerWidth", inner_width);
        register_global_accessor(&mut context, "outerHeight", inner_height);
        register_global_accessor(&mut context, "devicePixelRatio", device_pixel_ratio);

        // Viewport scrolling
        register_global_accessor(&mut context, "scrollX", scroll_x);
        register_global_accessor(&mut context, "scrollY", scroll_y);
        register_global_accessor(&mut context, "pageXOffset", scroll_x);
        register_global_accessor(&mut context, "pageYOffset", scroll_y);
        register_global_fn(&mut context, "scroll", 2, window_scroll_to);
        register_global_fn(&mut context, "scrollTo", 2, window_scroll_to);
        register_global_fn(&mut context, "scrollBy", 2, window_scroll_by);

        // The `CSS` namespace object. `CSS.escape` is defined in the JS bootstrap.
        let css_namespace = ObjectInitializer::new(&mut context)
            .function(
                NativeFunction::from_fn_ptr(css_supports),
                js_string!("supports"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(css_register_property),
                js_string!("registerProperty"),
                1,
            )
            .build();
        register_global(&mut context, "CSS", css_namespace.into());

        let mut runtime = Self {
            context,
            ctx,
            module_loader,
        };

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
        // Register the module so that imports resolving to this URL (including
        // circular ones) reuse it rather than re-fetching
        if let Some(url) = url
            && self.module_loader.get(url).is_none()
        {
            self.module_loader.insert(url.clone(), module.clone());
        }

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
    let mut state = ctx.state.borrow_mut();
    let now = state.clock.now();
    let id = state.timers.add(now, delay, None, callback, rest);
    Ok(JsValue::from(id as f64))
}

fn set_interval(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some((callback, delay, rest)) = timer_args(args, context)? else {
        return Ok(JsValue::from(0));
    };
    let mut state = ctx.state.borrow_mut();
    let now = state.clock.now();
    let id = state.timers.add(now, delay, Some(delay), callback, rest);
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

// === Viewport scrolling ===

fn scroll_x(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    Ok(JsValue::from(ctx.doc.borrow().viewport_scroll().x))
}

fn scroll_y(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    Ok(JsValue::from(ctx.doc.borrow().viewport_scroll().y))
}

/// `window.scrollTo`/`window.scroll`: scroll the viewport, which blitz-dom
/// models as a programmatic scroll of the root element.
fn window_scroll_to(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let parsed = crate::dom::element::parse_scroll_to_args(args, context)?;
    let ctx = dom_ctx(context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let Some(root_id) = doc.try_root_element().map(|root| root.id) else {
        return Ok(JsValue::undefined());
    };
    let current = doc.viewport_scroll();
    doc.scroll_to(
        root_id,
        parsed.left.unwrap_or(current.x),
        parsed.top.unwrap_or(current.y),
        parsed.behavior,
    );
    Ok(JsValue::undefined())
}

fn window_scroll_by(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let parsed = crate::dom::element::parse_scroll_to_args(args, context)?;
    let ctx = dom_ctx(context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let Some(root_id) = doc.try_root_element().map(|root| root.id) else {
        return Ok(JsValue::undefined());
    };
    doc.scroll_by(
        root_id,
        parsed.left.unwrap_or(0.0),
        parsed.top.unwrap_or(0.0),
        parsed.behavior,
    );
    Ok(JsValue::undefined())
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

/// `CSS.registerProperty({ name, syntax = "*", inherits, initialValue })`
/// <https://drafts.css-houdini.org/css-properties-values-api-1/#the-registerproperty-function>
fn css_register_property(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    use blitz_dom::RegisterCustomPropertyResult as Result_;

    let ctx = dom_ctx(context)?;
    let Some(descriptor) = args.first().and_then(JsValue::as_object) else {
        return Err(JsNativeError::typ()
            .with_message("CSS.registerProperty: argument must be a PropertyDefinition dictionary")
            .into());
    };
    let get = |key: &str, context: &mut Context| -> JsResult<Option<String>> {
        let value = descriptor.get(js_string!(key), context)?;
        if value.is_undefined() {
            return Ok(None);
        }
        Ok(Some(to_rust_string(&value, context)?))
    };
    let Some(name) = get("name", context)? else {
        return Err(JsNativeError::typ()
            .with_message("CSS.registerProperty: 'name' is required")
            .into());
    };
    let inherits = descriptor.get(js_string!("inherits"), context)?;
    if inherits.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("CSS.registerProperty: 'inherits' is required")
            .into());
    }
    let inherits = inherits.to_boolean();
    let syntax = get("syntax", context)?.unwrap_or_else(|| "*".to_string());
    let initial_value = get("initialValue", context)?;

    let result = ctx.doc.borrow_mut().register_custom_property(
        &name,
        &syntax,
        inherits,
        initial_value.as_deref(),
    );
    let error = match result {
        Result_::SuccessfullyRegistered => return Ok(JsValue::undefined()),
        Result_::InvalidName => JsNativeError::syntax().with_message(format!(
            "CSS.registerProperty: '{name}' is not a valid custom property name"
        )),
        Result_::AlreadyRegistered => JsNativeError::error().with_message(format!(
            "CSS.registerProperty: '{name}' is already registered"
        )),
        Result_::InvalidSyntax => JsNativeError::syntax()
            .with_message(format!("CSS.registerProperty: invalid syntax '{syntax}'")),
        Result_::NoInitialValue => JsNativeError::syntax().with_message(
            "CSS.registerProperty: 'initialValue' is required for non-universal syntax",
        ),
        Result_::InvalidInitialValue | Result_::InitialValueNotComputationallyIndependent => {
            JsNativeError::syntax().with_message("CSS.registerProperty: invalid 'initialValue'")
        }
    };
    Err(error.into())
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

/// `__blitz_fetch_sync(url)`: resolve `url` against the document base URL and
/// fetch it via the document's [`ScriptFetcher`]. Returns `[status, url, text]`:
/// a missing resource yields a 404 (like an HTTP server would), any other
/// failure throws a `TypeError` (a network error, in `fetch()` terms).
fn fetch_sync(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    use boa_engine::object::builtins::JsArray;

    let ctx = dom_ctx(context)?;
    let input = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_string(context)?
        .to_std_string_lossy();

    let (base_url, fetcher) = {
        let state = ctx.state.borrow();
        (state.base_url.clone(), state.fetcher.clone())
    };
    let url = match &base_url {
        Some(base) => base.join(&input),
        None => Url::parse(&input),
    }
    .map_err(|_| JsNativeError::typ().with_message(format!("Failed to parse URL from {input}")))?;
    let fetcher =
        fetcher.ok_or_else(|| JsNativeError::typ().with_message("fetch is unavailable"))?;

    let (status, text) = match fetcher.borrow().fetch(&url) {
        Ok(text) => (200, text),
        Err(crate::fetch::FetchError::Io(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            (404, String::new())
        }
        Err(error) => {
            return Err(JsNativeError::typ()
                .with_message(format!("Failed to fetch {url}: {error}"))
                .into());
        }
    };

    Ok(JsArray::from_iter(
        [
            JsValue::from(status),
            JsString::from(url.as_str()).into(),
            JsString::from(text).into(),
        ],
        context,
    )
    .into())
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
