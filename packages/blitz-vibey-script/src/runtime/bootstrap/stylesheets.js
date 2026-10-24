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
                return __blitz_sheet_disabled(data(this).owner);
            }
            set disabled(value) {
                __blitz_sheet_set_disabled(data(this).owner, !!value);
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
        Object.defineProperty(ruleClasses.CSSStyleRule.prototype, "selectorText", {
            get: attrGetter("selectorText", str),
            set(value) {
                const d = data(this);
                __blitz_sheet_set_selector_text(d.owner, d.path, String(value));
                touchSheet(d.sheet);
            },
            configurable: true,
            enumerable: true,
        });
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
