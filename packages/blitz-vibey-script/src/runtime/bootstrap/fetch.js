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
