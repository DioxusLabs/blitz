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

    // `URLSearchParams`: a query string's name-value pairs, in order, as
    // `application/x-www-form-urlencoded` parses them. Tests read their
    // variant's query through it.
    if (typeof globalThis.URLSearchParams !== "function") {
        const decode = (s) => {
            try {
                return decodeURIComponent(s.replace(/\+/g, " "));
            } catch (e) {
                return s;
            }
        };
        const encode = (s) => encodeURIComponent(s).replace(/%20/g, "+");
        class URLSearchParams {
            #pairs = [];
            constructor(init) {
                if (init instanceof URLSearchParams) {
                    for (const [k, v] of init) this.append(k, v);
                } else if (Array.isArray(init)) {
                    for (const [k, v] of init) this.append(k, v);
                } else if (init && typeof init === "object") {
                    for (const k of Object.keys(init)) this.append(k, init[k]);
                } else if (init !== undefined && init !== null) {
                    let query = String(init);
                    if (query.startsWith("?")) query = query.slice(1);
                    for (const part of query.split("&")) {
                        if (part === "") continue;
                        const at = part.indexOf("=");
                        const name = at < 0 ? part : part.slice(0, at);
                        const value = at < 0 ? "" : part.slice(at + 1);
                        this.#pairs.push([decode(name), decode(value)]);
                    }
                }
            }
            get size() {
                return this.#pairs.length;
            }
            append(name, value) {
                this.#pairs.push([String(name), String(value)]);
            }
            delete(name) {
                name = String(name);
                this.#pairs = this.#pairs.filter(([k]) => k !== name);
            }
            get(name) {
                name = String(name);
                const found = this.#pairs.find(([k]) => k === name);
                return found ? found[1] : null;
            }
            getAll(name) {
                name = String(name);
                return this.#pairs.filter(([k]) => k === name).map(([, v]) => v);
            }
            has(name) {
                name = String(name);
                return this.#pairs.some(([k]) => k === name);
            }
            set(name, value) {
                name = String(name);
                const at = this.#pairs.findIndex(([k]) => k === name);
                if (at < 0) {
                    this.append(name, value);
                } else {
                    this.#pairs[at] = [name, String(value)];
                    this.#pairs = this.#pairs.filter(([k], i) => k !== name || i === at);
                }
            }
            forEach(callback, thisArg) {
                for (const [k, v] of this.#pairs) callback.call(thisArg, v, k, this);
            }
            keys() {
                return this.#pairs.map(([k]) => k)[Symbol.iterator]();
            }
            values() {
                return this.#pairs.map(([, v]) => v)[Symbol.iterator]();
            }
            entries() {
                return this.#pairs.map((pair) => [pair[0], pair[1]])[Symbol.iterator]();
            }
            [Symbol.iterator]() {
                return this.entries();
            }
            toString() {
                return this.#pairs.map(([k, v]) => encode(k) + "=" + encode(v)).join("&");
            }
        }
        globalThis.URLSearchParams = URLSearchParams;
    }

    if (typeof globalThis.queueMicrotask !== "function") {
        globalThis.queueMicrotask = function (callback) {
            Promise.resolve().then(callback);
        };
    }
