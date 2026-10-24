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
    // Declarations exposing `length`/`item()` (e.g. `getComputedStyle()`) also
    // support indexed access (`style[0]`) and iteration.
    const isIndex = (prop) => typeof prop === "string" && /^\d+$/.test(prop);
    const isIndexed = (target) => typeof target.item === "function";
    const styleIterator = function* () {
        for (let i = 0; i < this.length; i++) yield this.item(i);
    };
    globalThis.__blitz_wrap_style = function (native) {
        return new Proxy(native, {
            get(target, prop) {
                if (isIndexed(target)) {
                    if (isIndex(prop)) {
                        return Number(prop) < target.length ? target.item(Number(prop)) : undefined;
                    }
                    if (prop === Symbol.iterator && !(prop in target)) {
                        return styleIterator.bind(target);
                    }
                }
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
                if (isIndexed(target)) {
                    if (isIndex(prop)) return Number(prop) < target.length;
                    if (prop === Symbol.iterator) return true;
                }
                return (
                    isCssPropName(prop) && __blitz_css_property_supported(toKebab(prop))
                );
            },
            ownKeys(target) {
                const keys = [];
                if (isIndexed(target)) {
                    for (let i = 0; i < target.length; i++) keys.push(String(i));
                }
                for (const key of Reflect.ownKeys(target)) {
                    if (!keys.includes(key)) keys.push(key);
                }
                return keys;
            },
            getOwnPropertyDescriptor(target, prop) {
                if (isIndexed(target) && isIndex(prop) && Number(prop) < target.length) {
                    return {
                        value: target.item(Number(prop)),
                        enumerable: true,
                        configurable: true,
                        writable: false,
                    };
                }
                return Reflect.getOwnPropertyDescriptor(target, prop);
            },
        });
    };
