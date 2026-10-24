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
