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
