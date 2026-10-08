// The Web Animations interfaces, on top of the native `__blitz_anim` function.
// Animation state lives in the document's animation store; the objects here
// hold an id, the promises and the event listeners.
(function () {
    "use strict";
    const native = globalThis.__blitz_anim;
    if (typeof native !== "function" || typeof document === "undefined") return;

    const animations = new Map(); // id -> Animation
    const ID = Symbol("id");
    let pendingEvents = [];
    let frameRequested = false;

    function domException(name, message) {
        return new DOMException(message, name);
    }

    function newPromise() {
        let resolve, reject;
        const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
        // An aborted promise that nobody observes is not an error
        promise.catch(() => {});
        return { promise, resolve, reject, settled: false };
    }

    function requestFrame() {
        if (frameRequested) return;
        frameRequested = true;
        requestAnimationFrame(() => {});
    }

    function processActions() {
        const actions = native("takeActions");
        for (const [id, kind, currentTime, timelineTime] of actions) {
            const animation = animations.get(id);
            if (!animation) continue;
            const state = animation[STATE];
            switch (kind) {
                case "replaceReady":
                    state.ready = newPromise();
                    break;
                case "resolveReady":
                    if (state.ready.settled) state.ready = newPromise();
                    state.ready.settled = true;
                    state.ready.resolve(animation);
                    break;
                case "abortReady":
                    if (!state.ready.settled) {
                        state.ready.settled = true;
                        state.ready.reject(domException("AbortError", "The animation was cancelled"));
                    }
                    break;
                case "replaceFinished":
                    state.finished = newPromise();
                    break;
                case "resolveFinished":
                    if (!state.finished.settled) {
                        state.finished.settled = true;
                        state.finished.resolve(animation);
                    }
                    break;
                case "abortFinished":
                    if (!state.finished.settled) {
                        state.finished.settled = true;
                        state.finished.reject(domException("AbortError", "The animation was cancelled"));
                    }
                    break;
                case "finishNotification":
                    queueMicrotask(() => call("finishNotification", id));
                    break;
                default:
                    pendingEvents.push({ animation, type: kind, currentTime, timelineTime });
            }
        }
        if (pendingEvents.length || native("needsFrames")) requestFrame();
    }

    function call(op, ...args) {
        try {
            return native(op, ...args);
        } finally {
            processActions();
        }
    }

    // Called by the runtime at the start of each frame, after the timeline advanced
    globalThis.__blitz_animations_frame = function () {
        frameRequested = false;
        processActions();
        const events = pendingEvents;
        pendingEvents = [];
        events.sort((a, b) => {
            const at = a.timelineTime === null ? -Infinity : a.timelineTime;
            const bt = b.timelineTime === null ? -Infinity : b.timelineTime;
            return at - bt || a.animation[ID] - b.animation[ID];
        });
        for (const { animation, type, currentTime, timelineTime } of events) {
            animation.dispatchEvent(new AnimationPlaybackEvent(type, { currentTime, timelineTime }));
        }
        processActions();
    };

    // ---- AnimationPlaybackEvent -------------------------------------------

    class AnimationPlaybackEvent {
        constructor(type, init = {}) {
            if (arguments.length < 1) throw new TypeError("Not enough arguments");
            this.type = String(type);
            this.bubbles = !!init.bubbles;
            this.cancelable = !!init.cancelable;
            this.defaultPrevented = false;
            this.target = null;
            this.currentTarget = null;
            this.eventPhase = 0;
            this.isTrusted = false;
            this.timeStamp = 0;
            this._currentTime = init.currentTime === undefined ? null : init.currentTime;
            this._timelineTime = init.timelineTime === undefined ? null : init.timelineTime;
        }
        get currentTime() { return this._currentTime; }
        get timelineTime() { return this._timelineTime; }
        preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
        stopPropagation() {}
        stopImmediatePropagation() { this._stopped = true; }
    }
    if (typeof Event === "function" && Event.prototype) {
        Object.setPrototypeOf(AnimationPlaybackEvent.prototype, Event.prototype);
    }

    // ---- AnimationEvent / TransitionEvent ----------------------------------

    function styleEventClass(className, nameField) {
        const cls = class {
            constructor(type, init = {}) {
                if (arguments.length < 1) throw new TypeError("Not enough arguments");
                if (init === null || init === undefined) init = {};
                if (typeof init !== "object" && typeof init !== "function") {
                    throw new TypeError("The init dictionary must be an object");
                }
                const elapsedTime = init.elapsedTime === undefined ? 0 : Number(init.elapsedTime);
                if (!Number.isFinite(elapsedTime)) throw new TypeError("elapsedTime must be finite");
                const values = {
                    type: String(type), bubbles: !!init.bubbles, cancelable: !!init.cancelable,
                    composed: !!init.composed, defaultPrevented: false, target: null,
                    currentTarget: null, eventPhase: 0, isTrusted: false, timeStamp: 0,
                };
                for (const key in values) {
                    Object.defineProperty(this, key, { value: values[key], configurable: true });
                }
                this._name = init[nameField] === undefined ? "" : String(init[nameField]);
                this._elapsedTime = elapsedTime;
                this._pseudoElement = init.pseudoElement === undefined ? "" : String(init.pseudoElement);
            }
            get elapsedTime() { return this._elapsedTime; }
            get pseudoElement() { return this._pseudoElement; }
            get [Symbol.toStringTag]() { return className; }
        };
        Object.defineProperty(cls.prototype, nameField, {
            configurable: true,
            enumerable: true,
            get() { return this._name; },
        });
        for (const key of ["elapsedTime", "pseudoElement"]) {
            Object.defineProperty(cls.prototype, key, { enumerable: true });
        }
        Object.defineProperty(cls, "name", { value: className });
        if (typeof Event === "function" && Event.prototype) {
            Object.setPrototypeOf(cls.prototype, Event.prototype);
            Object.setPrototypeOf(cls, Event);
        }
        Object.defineProperty(globalThis, className, {
            value: cls, writable: true, configurable: true, enumerable: false,
        });
    }
    styleEventClass("AnimationEvent", "animationName");
    styleEventClass("TransitionEvent", "propertyName");

    // ---- Timelines ---------------------------------------------------------

    let constructingTimeline = false;
    class AnimationTimeline {
        constructor() {
            if (!constructingTimeline) throw new TypeError("Illegal constructor");
        }
        get currentTime() { return this._currentTime(); }
    }
    class DocumentTimeline extends AnimationTimeline {
        constructor(options = {}) {
            constructingTimeline = true;
            try { super(); } finally { constructingTimeline = false; }
            const originTime = options && options.originTime !== undefined ? Number(options.originTime) : 0;
            if (!Number.isFinite(originTime)) throw new TypeError("originTime must be finite");
            this._originTime = originTime;
        }
        _currentTime() { return native("timelineTime") - this._originTime; }
    }
    const documentTimeline = new DocumentTimeline();

    // ---- AnimationEffect / KeyframeEffect ----------------------------------

    const FILLS = ["none", "forwards", "backwards", "both", "auto"];
    const DIRECTIONS = ["normal", "reverse", "alternate", "alternate-reverse"];
    const COMPOSITES = ["replace", "add", "accumulate"];
    const ITERATION_COMPOSITES = ["replace", "accumulate"];

    function enumValue(value, values, what) {
        value = String(value);
        if (!values.includes(value)) throw new TypeError(`Invalid ${what}: ${value}`);
        return value;
    }

    function defaultTiming() {
        return {
            delay: 0, endDelay: 0, fill: "auto", iterationStart: 0, iterations: 1,
            duration: "auto", direction: "normal", easing: "linear",
        };
    }

    function finiteDouble(value, what) {
        value = Number(value);
        if (!Number.isFinite(value)) throw new TypeError(`${what} must be finite`);
        return value;
    }

    // https://drafts.csswg.org/web-animations-1/#update-the-timing-properties-of-an-animation-effect
    function updateTiming(timing, input) {
        const next = {};
        if (input.delay !== undefined) next.delay = finiteDouble(input.delay, "delay");
        if (input.endDelay !== undefined) next.endDelay = finiteDouble(input.endDelay, "endDelay");
        if (input.fill !== undefined) next.fill = enumValue(input.fill, FILLS, "fill");
        if (input.iterationStart !== undefined) {
            next.iterationStart = finiteDouble(input.iterationStart, "iterationStart");
            if (next.iterationStart < 0) throw new TypeError("iterationStart must not be negative");
        }
        if (input.iterations !== undefined) {
            next.iterations = Number(input.iterations);
            if (Number.isNaN(next.iterations) || next.iterations < 0) {
                throw new TypeError("iterations must not be negative");
            }
        }
        if (input.duration !== undefined) {
            if (typeof input.duration === "string") {
                if (input.duration !== "auto") throw new TypeError("Invalid duration");
                next.duration = "auto";
            } else {
                next.duration = Number(input.duration);
                if (Number.isNaN(next.duration) || next.duration < 0) {
                    throw new TypeError("duration must not be negative");
                }
            }
        }
        if (input.direction !== undefined) {
            next.direction = enumValue(input.direction, DIRECTIONS, "direction");
        }
        if (input.easing !== undefined) {
            next.easing = String(input.easing);
            if (!native("isEasing", null, next.easing)) throw new TypeError(`Invalid easing: ${next.easing}`);
        }
        Object.assign(timing, next);
    }

    class AnimationEffect {
        constructor() {
            if (new.target === AnimationEffect) throw new TypeError("Illegal constructor");
        }
        getTiming() { return Object.assign({}, this._timing); }
        getComputedTiming() {
            const timing = this.getTiming();
            if (timing.fill === "auto") timing.fill = "none";
            if (timing.duration === "auto") timing.duration = 0;
            const animation = this._animation;
            const computed = animation ? native("computedTiming", animation[ID]) : null;
            timing.activeDuration = timing.duration * timing.iterations || 0;
            timing.endTime = Math.max(timing.delay + timing.activeDuration + timing.endDelay, 0);
            timing.localTime = animation ? animation.currentTime : null;
            timing.progress = computed ? computed[2] : null;
            timing.currentIteration = computed ? computed[3] : null;
            return timing;
        }
        updateTiming(timing = {}) {
            updateTiming(this._timing, timing || {});
            this._sync();
        }
    }

    function propertyName(key) {
        if (key === "cssFloat") return "float";
        if (key === "cssOffset") return "offset";
        if (key === "float" || key === "offset") return null;
        if (key.startsWith("--")) return key;
        if (key.includes("-")) return null;
        const name = key.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
        return native("isAnimatable", null, name) ? name : null;
    }

    function ownKeys(object) {
        const keys = [];
        for (const key in object) {
            if (Object.prototype.hasOwnProperty.call(object, key)) keys.push(key);
        }
        return keys.sort();
    }

    function keyframeOffset(value) {
        if (value === null || value === undefined) return null;
        const offset = Number(value);
        if (!Number.isFinite(offset)) throw new TypeError("Invalid keyframe offset");
        return offset;
    }

    function keyframeComposite(value) {
        if (value === undefined || value === null || value === "auto") return "auto";
        return enumValue(value, COMPOSITES.concat("auto"), "composite");
    }

    function toList(value) {
        if (value !== null && typeof value === "object" && typeof value[Symbol.iterator] === "function") {
            return Array.from(value);
        }
        return [value];
    }

    // https://drafts.csswg.org/web-animations-1/#process-a-keyframes-argument
    function processKeyframes(object) {
        if (object === null || object === undefined) return [];
        if (typeof object !== "object") throw new TypeError("Keyframes must be an object");
        let keyframes = [];
        const iterator = object[Symbol.iterator];
        if (iterator !== undefined && iterator !== null) {
            if (typeof iterator !== "function") throw new TypeError("Keyframes are not iterable");
            for (const item of object) {
                if (item !== null && item !== undefined && typeof item !== "object") {
                    throw new TypeError("A keyframe must be an object");
                }
                const source = item || {};
                const keyframe = {
                    offset: keyframeOffset(source.offset),
                    easing: source.easing === undefined ? "linear" : String(source.easing),
                    composite: keyframeComposite(source.composite),
                    properties: [],
                };
                for (const key of ownKeys(source)) {
                    const name = propertyName(key);
                    if (name === null) continue;
                    keyframe.properties.push([key, name, String(source[key])]);
                }
                keyframes.push(keyframe);
            }
            let previous = -Infinity;
            for (const keyframe of keyframes) {
                if (keyframe.offset === null) continue;
                if (keyframe.offset < 0 || keyframe.offset > 1) {
                    throw new TypeError("Keyframe offsets must be between 0 and 1");
                }
                if (keyframe.offset < previous) {
                    throw new TypeError("Keyframe offsets must be in order");
                }
                previous = keyframe.offset;
            }
        } else {
            const offsets = object.offset === undefined ? [] : toList(object.offset).map(keyframeOffset);
            const easings = object.easing === undefined ? [] : toList(object.easing).map(String);
            const composites = object.composite === undefined
                ? [] : toList(object.composite).map(keyframeComposite);
            const properties = [];
            for (const key of ownKeys(object)) {
                const name = propertyName(key);
                if (name === null) continue;
                properties.push([key, name, toList(object[key]).map(String)]);
            }
            const byOffset = new Map();
            for (const [key, name, values] of properties) {
                values.forEach((value, index) => {
                    const offset = values.length === 1 ? 1 : index / (values.length - 1);
                    if (!byOffset.has(offset)) {
                        byOffset.set(offset, {
                            offset: null, computedOffset: offset, easing: "linear",
                            composite: "auto", properties: [],
                        });
                    }
                    byOffset.get(offset).properties.push([key, name, value]);
                });
            }
            keyframes = Array.from(byOffset.values()).sort((a, b) => a.computedOffset - b.computedOffset);
            offsets.forEach((offset, index) => {
                if (index < keyframes.length) keyframes[index].offset = offset;
            });
            let previous = -Infinity;
            for (const keyframe of keyframes) {
                if (keyframe.offset === null) continue;
                if (keyframe.offset < 0 || keyframe.offset > 1) {
                    throw new TypeError("Keyframe offsets must be between 0 and 1");
                }
                if (keyframe.offset < previous) {
                    throw new TypeError("Keyframe offsets must be in order");
                }
                previous = keyframe.offset;
            }
            if (easings.length) {
                keyframes.forEach((keyframe, index) => {
                    keyframe.easing = easings[index % easings.length];
                });
            }
            if (composites.length) {
                keyframes.forEach((keyframe, index) => {
                    keyframe.composite = composites[index % composites.length];
                });
            }
            for (const easing of easings) {
                if (!native("isEasing", null, easing)) throw new TypeError(`Invalid easing: ${easing}`);
            }
        }
        for (const keyframe of keyframes) {
            if (!native("isEasing", null, keyframe.easing)) {
                throw new TypeError(`Invalid easing: ${keyframe.easing}`);
            }
        }
        computeOffsets(keyframes);
        return keyframes;
    }

    // https://drafts.csswg.org/web-animations-1/#compute-missing-keyframe-offsets
    function computeOffsets(keyframes) {
        const count = keyframes.length;
        if (!count) return;
        for (const keyframe of keyframes) keyframe.computedOffset = keyframe.offset;
        if (count > 1 && keyframes[0].computedOffset === null) keyframes[0].computedOffset = 0;
        if (keyframes[count - 1].computedOffset === null) keyframes[count - 1].computedOffset = 1;
        let start = 0;
        for (let index = 1; index < count; index++) {
            if (keyframes[index].computedOffset === null) continue;
            const from = keyframes[start].computedOffset;
            const to = keyframes[index].computedOffset;
            for (let i = start + 1; i < index; i++) {
                keyframes[i].computedOffset = from + ((to - from) * (i - start)) / (index - start);
            }
            start = index;
        }
    }

    class KeyframeEffect extends AnimationEffect {
        constructor(target, keyframes, options) {
            super();
            if (arguments.length === 1 && target instanceof KeyframeEffect) {
                const source = target;
                this._target = source._target;
                this._pseudo = source._pseudo;
                this._timing = Object.assign({}, source._timing);
                this._composite = source._composite;
                this._iterationComposite = source._iterationComposite;
                this._keyframes = source._keyframes.map((keyframe) =>
                    Object.assign({}, keyframe, { properties: keyframe.properties.slice() }));
                this._animation = null;
                return;
            }
            if (arguments.length < 2) throw new TypeError("Not enough arguments");
            if (target !== null && !(target instanceof Element)) {
                throw new TypeError("The target must be an Element or null");
            }
            this._target = target;
            this._pseudo = null;
            this._timing = defaultTiming();
            this._composite = "replace";
            this._iterationComposite = "replace";
            this._animation = null;
            if (typeof options === "object" && options !== null) {
                updateTiming(this._timing, options);
                if (options.composite !== undefined) {
                    this._composite = enumValue(options.composite, COMPOSITES, "composite");
                }
                if (options.iterationComposite !== undefined) {
                    this._iterationComposite =
                        enumValue(options.iterationComposite, ITERATION_COMPOSITES, "iterationComposite");
                }
                if (options.pseudoElement !== undefined && options.pseudoElement !== null) {
                    this._pseudo = checkPseudo(options.pseudoElement);
                }
            } else if (options !== undefined && options !== null) {
                updateTiming(this._timing, { duration: Number(options) });
            }
            this._keyframes = processKeyframes(keyframes);
        }
        get target() { return this._target; }
        set target(target) {
            if (target !== null && !(target instanceof Element)) {
                throw new TypeError("The target must be an Element or null");
            }
            this._target = target;
            this._sync();
        }
        get pseudoElement() { return this._pseudo; }
        set pseudoElement(pseudo) {
            this._pseudo = pseudo === null || pseudo === undefined ? null : checkPseudo(pseudo);
            this._sync();
        }
        get composite() { return this._composite; }
        set composite(composite) {
            if (!COMPOSITES.includes(String(composite))) return;
            this._composite = String(composite);
            this._sync();
        }
        get iterationComposite() { return this._iterationComposite; }
        set iterationComposite(composite) {
            if (!ITERATION_COMPOSITES.includes(String(composite))) return;
            this._iterationComposite = String(composite);
            this._sync();
        }
        getKeyframes() {
            return this._keyframes.map((keyframe) => {
                const result = {
                    offset: keyframe.offset,
                    computedOffset: keyframe.computedOffset,
                    easing: keyframe.easing,
                    composite: keyframe.composite,
                };
                for (const [key, , value] of keyframe.properties) result[key] = value;
                return result;
            });
        }
        setKeyframes(keyframes) {
            this._keyframes = processKeyframes(keyframes);
            this._sync();
        }
        _spec() {
            const timing = this._timing;
            return [
                [
                    timing.delay, timing.endDelay, timing.fill === "auto" ? "none" : timing.fill,
                    timing.iterationStart, timing.iterations,
                    timing.duration === "auto" ? 0 : timing.duration, timing.direction,
                    timing.easing, this._composite, this._iterationComposite,
                ],
                this._keyframes.map((keyframe) => [
                    keyframe.computedOffset, keyframe.easing,
                    keyframe.composite === "auto" ? null : keyframe.composite,
                    keyframe.properties.flatMap(([, name, value]) => [name, value]),
                ]),
            ];
        }
        _sync() {
            const animation = this._animation;
            if (!animation) return;
            call("setEffect", animation[ID], this._target, this._pseudo, this._spec());
        }
    }

    function checkPseudo(pseudo) {
        pseudo = String(pseudo);
        if (!pseudo.startsWith(":")) {
            throw domException("SyntaxError", `Invalid pseudo-element: ${pseudo}`);
        }
        if (pseudo === ":before" || pseudo === ":after") pseudo = ":" + pseudo;
        return pseudo;
    }

    // The effect of a CSS animation or transition. Its keyframes belong to style.
    class StyleEffect extends AnimationEffect {
        constructor(target, pseudo) {
            super();
            this._target = target;
            this._pseudo = pseudo;
            this._animation = null;
        }
        // The timing lives in the animation store, where style updates it
        get _timing() {
            const timing = this._animation && native("effectTiming", this._animation[ID]);
            if (!timing) return defaultTiming();
            const [delay, endDelay, fill, iterationStart, iterations, duration, direction, easing] = timing;
            return { delay, endDelay, fill, iterationStart, iterations, duration, direction, easing };
        }
        updateTiming(input = {}) {
            const timing = this._timing;
            updateTiming(timing, input || {});
            if (!this._animation) return;
            call("setEffectTiming", this._animation[ID], [
                timing.delay, timing.endDelay, timing.fill === "auto" ? "none" : timing.fill,
                timing.iterationStart, timing.iterations,
                timing.duration === "auto" ? 0 : timing.duration, timing.direction, timing.easing,
            ]);
        }
        get target() { return this._target; }
        get pseudoElement() { return this._pseudo; }
        get composite() { return "replace"; }
        get iterationComposite() { return "replace"; }
        getKeyframes() { return []; }
        _sync() {}
    }
    Object.setPrototypeOf(StyleEffect.prototype, KeyframeEffect.prototype);

    // ---- Animation ----------------------------------------------------------

    const STATE = Symbol("state");

    function timeArgument(value) {
        if (value === null || value === undefined) return null;
        const time = Number(value);
        if (!Number.isFinite(time)) throw new TypeError("Times must be finite");
        return time;
    }

    class Animation {
        constructor(effect = null, timeline) {
            if (effect !== null && effect !== undefined && !(effect instanceof AnimationEffect)) {
                throw new TypeError("The effect must be an AnimationEffect or null");
            }
            if (timeline !== undefined && timeline !== null && !(timeline instanceof AnimationTimeline)) {
                throw new TypeError("The timeline must be an AnimationTimeline or null");
            }
            const adopt = Animation._adopt;
            Animation._adopt = undefined;
            this[ID] = adopt === undefined ? native("create") : adopt;
            if (adopt !== undefined) native("retain", adopt);
            const ready = newPromise();
            ready.settled = true;
            ready.resolve(this);
            this[STATE] = {
                ready, finished: newPromise(), listeners: new Map(), handlers: {},
                effect: null, timeline: timeline === undefined ? documentTimeline : timeline,
                id: "",
            };
            animations.set(this[ID], this);
            if (adopt !== undefined) return;
            if (this[STATE].timeline === null) call("setTimeline", this[ID], false);
            if (effect) this.effect = effect;
        }

        get id() { return this[STATE].id; }
        set id(id) { this[STATE].id = String(id); }

        get effect() { return this[STATE].effect; }
        set effect(effect) {
            if (effect !== null && !(effect instanceof AnimationEffect)) {
                throw new TypeError("The effect must be an AnimationEffect or null");
            }
            const state = this[STATE];
            if (effect === state.effect) return;
            if (state.effect) state.effect._animation = null;
            if (effect && effect._animation) effect._animation.effect = null;
            state.effect = effect;
            if (effect instanceof StyleEffect) return;
            if (effect) {
                effect._animation = this;
                call("setEffect", this[ID], effect._target, effect._pseudo, effect._spec());
            } else {
                call("setEffect", this[ID], null, null, null);
            }
        }

        get timeline() { return this[STATE].timeline; }
        set timeline(timeline) {
            if (timeline !== null && !(timeline instanceof AnimationTimeline)) {
                throw new TypeError("The timeline must be an AnimationTimeline or null");
            }
            this[STATE].timeline = timeline;
            call("setTimeline", this[ID], timeline !== null);
        }

        get startTime() { return native("state", this[ID])[1]; }
        set startTime(time) { call("setStartTime", this[ID], timeArgument(time)); }
        get currentTime() { return native("state", this[ID])[0]; }
        set currentTime(time) { call("setCurrentTime", this[ID], timeArgument(time)); }
        get playbackRate() { return native("state", this[ID])[2]; }
        set playbackRate(rate) {
            rate = Number(rate);
            if (!Number.isFinite(rate)) throw new TypeError("playbackRate must be finite");
            call("setPlaybackRate", this[ID], rate);
        }
        get playState() { return native("state", this[ID])[3]; }
        get pending() { return native("state", this[ID])[4]; }
        get replaceState() { return native("replaceState", this[ID]); }
        get ready() { return this[STATE].ready.promise; }
        get finished() { return this[STATE].finished.promise; }

        play() { call("play", this[ID]); }
        pause() { call("pause", this[ID]); }
        finish() { call("finish", this[ID]); }
        cancel() { call("cancel", this[ID]); }
        reverse() { call("reverse", this[ID]); }
        updatePlaybackRate(rate) {
            if (arguments.length < 1) throw new TypeError("Not enough arguments");
            rate = Number(rate);
            if (!Number.isFinite(rate)) throw new TypeError("playbackRate must be finite");
            call("updatePlaybackRate", this[ID], rate);
        }
        persist() { call("persist", this[ID]); }

        addEventListener(type, listener, options) {
            if (listener === null || listener === undefined) return;
            const listeners = this[STATE].listeners;
            if (!listeners.has(type)) listeners.set(type, []);
            const list = listeners.get(type);
            if (list.some((entry) => entry.listener === listener)) return;
            const once = typeof options === "object" && options !== null && !!options.once;
            list.push({ listener, once });
        }
        removeEventListener(type, listener) {
            const list = this[STATE].listeners.get(type);
            if (!list) return;
            const index = list.findIndex((entry) => entry.listener === listener);
            if (index >= 0) list.splice(index, 1);
        }
        dispatchEvent(event) {
            const state = this[STATE];
            try {
                Object.defineProperty(event, "target", { value: this, configurable: true });
                Object.defineProperty(event, "currentTarget", { value: this, configurable: true });
            } catch (e) {}
            const entries = [];
            const handler = state.handlers[event.type];
            if (typeof handler === "function") entries.push({ listener: handler });
            for (const entry of (state.listeners.get(event.type) || []).slice()) entries.push(entry);
            for (const entry of entries) {
                if (entry.once) this.removeEventListener(event.type, entry.listener);
                try {
                    if (typeof entry.listener === "function") entry.listener.call(this, event);
                    else entry.listener.handleEvent(event);
                } catch (error) {
                    if (typeof reportError === "function") reportError(error);
                    else console.error(error);
                }
                if (event._stopped) break;
            }
            return !event.defaultPrevented;
        }
    }
    for (const type of ["finish", "cancel", "remove"]) {
        Object.defineProperty(Animation.prototype, "on" + type, {
            configurable: true,
            enumerable: true,
            get() { return this[STATE].handlers[type] || null; },
            set(handler) { this[STATE].handlers[type] = typeof handler === "function" ? handler : null; },
        });
    }
    if (typeof EventTarget === "function" && EventTarget.prototype) {
        Object.setPrototypeOf(Animation.prototype, EventTarget.prototype);
    }

    class CSSAnimation extends Animation {
        get animationName() { return this[STATE].name; }
    }
    class CSSTransition extends Animation {
        get transitionProperty() { return this[STATE].name; }
    }

    function wrap([id, kind, name, target, pseudo]) {
        let animation = animations.get(id);
        if (animation) return animation;
        Animation._adopt = id;
        animation = kind === "animation" ? new CSSAnimation()
            : kind === "transition" ? new CSSTransition() : new Animation();
        animation[STATE].name = name;
        const effect = new StyleEffect(target, pseudo);
        effect._animation = animation;
        animation[STATE].effect = effect;
        return animation;
    }

    // ---- Animatable / Document ----------------------------------------------

    const elementProto = globalThis.Element && globalThis.Element.prototype;
    if (elementProto) {
        elementProto.animate = function (keyframes, options) {
            const effect = new KeyframeEffect(this, keyframes, options);
            const animation = new Animation(effect, documentTimeline);
            if (typeof options === "object" && options !== null && options.id !== undefined) {
                animation.id = options.id;
            }
            animation.play();
            return animation;
        };
        elementProto.getAnimations = function (options) {
            const subtree = typeof options === "object" && options !== null && !!options.subtree;
            return native("getAnimations", null, this, subtree).map(wrap);
        };
    }
    const documentProto = Object.getPrototypeOf(document);
    Object.defineProperty(documentProto, "timeline", {
        configurable: true,
        enumerable: true,
        get() { return documentTimeline; },
    });
    documentProto.getAnimations = function () {
        return native("getAnimations", null, null, true).map(wrap);
    };

    Object.assign(globalThis, {
        Animation, AnimationEffect, KeyframeEffect, AnimationTimeline, DocumentTimeline,
        AnimationPlaybackEvent, CSSAnimation, CSSTransition,
    });
    for (const name of [
        "Animation", "AnimationEffect", "KeyframeEffect", "AnimationTimeline", "DocumentTimeline",
        "AnimationPlaybackEvent", "CSSAnimation", "CSSTransition",
    ]) {
        Object.defineProperty(globalThis, name, { enumerable: false });
    }
})();
