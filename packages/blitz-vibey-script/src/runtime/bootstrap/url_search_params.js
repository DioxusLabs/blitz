    const paramState = new WeakMap();
    const urlParams = new WeakMap();
    const parseParams = globalThis.__blitz_parse_search_params;
    delete globalThis.__blitz_parse_search_params;

    function usv(value) {
        if (typeof value === 'symbol') throw new TypeError('Cannot convert a Symbol to a string');
        let result = '';
        for (const ch of String(value)) {
            const code = ch.codePointAt(0);
            result += code >= 0xD800 && code <= 0xDFFF ? '\uFFFD' : ch;
        }
        return result;
    }
    function state(params) {
        const data = paramState.get(params);
        if (!data) throw new TypeError('Invalid URLSearchParams receiver');
        return data;
    }
    function required(args, count) {
        if (args.length < count) throw new TypeError('Not enough arguments');
    }
    function encodeParam(value) {
        return encodeURIComponent(value).replace(/[!'()~]/g, ch => '%' + ch.charCodeAt(0).toString(16).toUpperCase()).replace(/%20/g, '+');
    }
    function update(data) {
        if (data.url) data.url.search = data.list.map(pair => pair.map(encodeParam).join('=')).join('&');
    }
    class URLSearchParams {
        constructor(init = '') {
            const list = [];
            if (init !== null && (typeof init === 'object' || typeof init === 'function')) {
                const iterator = init[Symbol.iterator];
                if (iterator != null) {
                    if (typeof iterator !== 'function') throw new TypeError('Invalid iterator');
                    for (const pair of init) {
                        if (pair === null || (typeof pair !== 'object' && typeof pair !== 'function')) throw new TypeError('Expected a pair');
                        const items = Array.from(pair);
                        if (items.length !== 2 || typeof pair[Symbol.iterator] !== 'function') throw new TypeError('Expected a pair');
                        list.push(items.map(usv));
                    }
                } else {
                    const record = new Map();
                    for (const key of Reflect.ownKeys(init)) {
                        if (Object.prototype.propertyIsEnumerable.call(init, key)) record.set(usv(key), usv(init[key]));
                    }
                    list.push(...record);
                }
            } else {
                const input = init === null ? '' : usv(init);
                list.push(...parseParams(input.startsWith('?') ? input.slice(1) : input));
            }
            paramState.set(this, { list, url: null });
        }
        get size() { return state(this).list.length; }
        append(name, value) {
            const data = state(this); required(arguments, 2);
            data.list.push([usv(name), usv(value)]); update(data);
        }
        delete(name, value) {
            const data = state(this); required(arguments, 1);
            name = usv(name); if (value !== undefined) value = usv(value);
            data.list = data.list.filter(pair => pair[0] !== name || (value !== undefined && pair[1] !== value));
            update(data);
        }
        get(name) {
            const data = state(this); required(arguments, 1); name = usv(name);
            const pair = data.list.find(pair => pair[0] === name);
            return pair ? pair[1] : null;
        }
        getAll(name) {
            const data = state(this); required(arguments, 1); name = usv(name);
            return data.list.filter(pair => pair[0] === name).map(pair => pair[1]);
        }
        has(name, value) {
            const data = state(this); required(arguments, 1);
            name = usv(name); if (value !== undefined) value = usv(value);
            return data.list.some(pair => pair[0] === name && (value === undefined || pair[1] === value));
        }
        set(name, value) {
            const data = state(this); required(arguments, 2); name = usv(name); value = usv(value);
            let found = false;
            data.list = data.list.filter(pair => {
                if (pair[0] !== name) return true;
                if (found) return false;
                pair[1] = value; found = true; return true;
            });
            if (!found) data.list.push([name, value]);
            update(data);
        }
        sort() {
            const data = state(this);
            data.list.sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0);
            update(data);
        }
        toString() { return state(this).list.map(pair => pair.map(encodeParam).join('=')).join('&'); }
        forEach(callback, thisArg) {
            const data = state(this); required(arguments, 1);
            if (typeof callback !== 'function') throw new TypeError('Expected a callback');
            for (let i = 0; i < data.list.length; i++) callback.call(thisArg, data.list[i][1], data.list[i][0], this);
        }
        entries() { return paramIterator(this, 'entries'); }
        keys() { return paramIterator(this, 'keys'); }
        values() { return paramIterator(this, 'values'); }
    }
    function paramIterator(params, kind) {
        const data = state(params);
        return (function* () {
            for (let i = 0; i < data.list.length; i++) {
                const pair = data.list[i];
                yield kind === 'keys' ? pair[0] : kind === 'values' ? pair[1] : pair.slice();
            }
        })();
    }
    Object.defineProperty(URLSearchParams.prototype, Symbol.iterator, { value: URLSearchParams.prototype.entries, writable: true, configurable: true });
    Object.defineProperty(URLSearchParams.prototype, Symbol.toStringTag, { value: 'URLSearchParams', configurable: true });
    for (const key of Object.getOwnPropertyNames(URLSearchParams.prototype)) {
        if (key !== 'constructor') Object.defineProperty(URLSearchParams.prototype, key, { enumerable: true });
    }
    Object.defineProperty(globalThis, 'URLSearchParams', { value: URLSearchParams, writable: true, configurable: true });

    const urlPrototype = URL.prototype;
    for (const key of ['href', 'search']) {
        const descriptor = Object.getOwnPropertyDescriptor(urlPrototype, key);
        Object.defineProperty(urlPrototype, key, {
            ...descriptor,
            set(value) {
                descriptor.set.call(this, value);
                const params = urlParams.get(this);
                if (params) state(params).list = parseParams(this.search.slice(1));
            }
        });
    }
    Object.defineProperty(urlPrototype, 'searchParams', {
        configurable: true, enumerable: true,
        get() {
            const search = this.search;
            let params = urlParams.get(this);
            if (!params) {
                params = new URLSearchParams(search);
                state(params).url = this;
                urlParams.set(this, params);
            }
            return params;
        }
    });
