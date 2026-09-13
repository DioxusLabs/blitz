// Geometry Interfaces Module Level 1: DOMPoint, DOMRect, DOMRectList,
// DOMQuad and DOMMatrix (https://drafts.csswg.org/geometry/).
//
// Parsing of transform-list strings (`new DOMMatrix("translate(10px)")`,
// `setMatrixValue()`) is delegated to the `__blitz_parse_transform` native,
// which uses stylo's CSS parser.
(function () {
    const internals = new WeakMap();
    const data = (obj) => {
        const d = internals.get(obj);
        if (!d) throw new TypeError("Illegal invocation");
        return d;
    };
    // Internal (constructor-bypassing) construction: `allocate(Cls)` creates an
    // instance of `Cls` with its constructor's argument handling skipped so that
    // internal algorithms can populate the internal slots directly.
    let allocating = false;
    const allocate = (Cls) => {
        allocating = true;
        try {
            return new Cls();
        } finally {
            allocating = false;
        }
    };

    // WebIDL `unrestricted double` conversion
    const toNumber = (value) => {
        if (typeof value === "bigint") {
            throw new TypeError("Cannot convert a BigInt value to a number");
        }
        return Number(value);
    };
    // WebIDL `boolean` conversion
    const toBoolean = (value) => Boolean(value);

    // WebIDL dictionary conversion: `undefined`/`null` are empty dictionaries,
    // any other non-object is a TypeError. Returns the object to read members
    // from (an empty object for the empty dictionary).
    const toDictionary = (value, name) => {
        if (value === undefined || value === null) return {};
        if (typeof value !== "object" && typeof value !== "function") {
            throw new TypeError(`${name}: value can not be converted to a dictionary`);
        }
        return value;
    };
    // Read a dictionary member (`undefined` means "not present")
    const member = (dict, key) => {
        const value = dict[key];
        return value === undefined ? undefined : value;
    };

    // Give a class WebIDL interface semantics: the class string, and enumerable
    // prototype members / static operations
    const defineInterface = (Cls) => {
        Object.defineProperty(Cls.prototype, Symbol.toStringTag, {
            value: Cls.name,
            configurable: true,
        });
        const makeEnumerable = (obj, skip) => {
            for (const key of Object.getOwnPropertyNames(obj)) {
                if (skip.includes(key)) continue;
                const desc = Object.getOwnPropertyDescriptor(obj, key);
                if (!desc.configurable) continue;
                Object.defineProperty(obj, key, { ...desc, enumerable: true });
            }
        };
        makeEnumerable(Cls.prototype, ["constructor"]);
        makeEnumerable(Cls, ["length", "name", "prototype"]);
    };
    const exposeGlobal = (name, value) => {
        Object.defineProperty(globalThis, name, {
            value,
            writable: true,
            enumerable: false,
            configurable: true,
        });
    };
    const domException = (name, message) => new DOMException(message, name);

    const isZero = (v) => v === 0; // true for both 0 and -0

    // === DOMPoint ===

    class DOMPointReadOnly {
        constructor(x = 0, y = 0, z = 0, w = 1) {
            if (allocating) {
                internals.set(this, { x: 0, y: 0, z: 0, w: 1 });
                return;
            }
            internals.set(this, {
                x: toNumber(x),
                y: toNumber(y),
                z: toNumber(z),
                w: toNumber(w),
            });
        }

        static fromPoint(other = {}) {
            return createPointFromDictionary(this, other);
        }

        get x() {
            return data(this).x;
        }
        get y() {
            return data(this).y;
        }
        get z() {
            return data(this).z;
        }
        get w() {
            return data(this).w;
        }

        matrixTransform(matrix = {}) {
            const p = data(this);
            const m = createMatrixFromDictionary(DOMMatrix, matrix, "DOMMatrixInit");
            return transformPoint(p, data(m).m);
        }

        toJSON() {
            const p = data(this);
            return { x: p.x, y: p.y, z: p.z, w: p.w };
        }
    }
    defineInterface(DOMPointReadOnly);

    class DOMPoint extends DOMPointReadOnly {
        constructor(x = 0, y = 0, z = 0, w = 1) {
            super(x, y, z, w);
        }

        static fromPoint(other = {}) {
            return createPointFromDictionary(this, other);
        }

        get x() {
            return data(this).x;
        }
        set x(value) {
            data(this).x = toNumber(value);
        }
        get y() {
            return data(this).y;
        }
        set y(value) {
            data(this).y = toNumber(value);
        }
        get z() {
            return data(this).z;
        }
        set z(value) {
            data(this).z = toNumber(value);
        }
        get w() {
            return data(this).w;
        }
        set w(value) {
            data(this).w = toNumber(value);
        }
    }
    defineInterface(DOMPoint);

    // Convert a DOMPointInit dictionary into plain `{x, y, z, w}` numbers
    const readPointInit = (init, name) => {
        const dict = toDictionary(init, name);
        // Dictionary members are read in lexicographical order
        const w = member(dict, "w");
        const x = member(dict, "x");
        const y = member(dict, "y");
        const z = member(dict, "z");
        return {
            x: x === undefined ? 0 : toNumber(x),
            y: y === undefined ? 0 : toNumber(y),
            z: z === undefined ? 0 : toNumber(z),
            w: w === undefined ? 1 : toNumber(w),
        };
    };
    const makePoint = (Cls, x, y, z, w) => {
        const point = allocate(Cls);
        const p = data(point);
        p.x = x;
        p.y = y;
        p.z = z;
        p.w = w;
        return point;
    };
    const createPointFromDictionary = (Cls, init) => {
        const { x, y, z, w } = readPointInit(init, "DOMPointInit");
        return makePoint(Cls, x, y, z, w);
    };

    // https://drafts.csswg.org/geometry/#transform-a-point-with-a-matrix
    const transformPoint = (p, m) => {
        const { x, y, z, w } = p;
        return makePoint(
            DOMPoint,
            m[0] * x + m[4] * y + m[8] * z + m[12] * w,
            m[1] * x + m[5] * y + m[9] * z + m[13] * w,
            m[2] * x + m[6] * y + m[10] * z + m[14] * w,
            m[3] * x + m[7] * y + m[11] * z + m[15] * w
        );
    };

    // === DOMRect ===

    class DOMRectReadOnly {
        constructor(x = 0, y = 0, width = 0, height = 0) {
            if (allocating) {
                internals.set(this, { x: 0, y: 0, width: 0, height: 0 });
                return;
            }
            internals.set(this, {
                x: toNumber(x),
                y: toNumber(y),
                width: toNumber(width),
                height: toNumber(height),
            });
        }

        static fromRect(other = {}) {
            return createRectFromDictionary(this, other);
        }

        get x() {
            return data(this).x;
        }
        get y() {
            return data(this).y;
        }
        get width() {
            return data(this).width;
        }
        get height() {
            return data(this).height;
        }
        get top() {
            const r = data(this);
            return Math.min(r.y, r.y + r.height);
        }
        get right() {
            const r = data(this);
            return Math.max(r.x, r.x + r.width);
        }
        get bottom() {
            const r = data(this);
            return Math.max(r.y, r.y + r.height);
        }
        get left() {
            const r = data(this);
            return Math.min(r.x, r.x + r.width);
        }

        toJSON() {
            data(this);
            return {
                x: this.x,
                y: this.y,
                width: this.width,
                height: this.height,
                top: this.top,
                right: this.right,
                bottom: this.bottom,
                left: this.left,
            };
        }
    }
    defineInterface(DOMRectReadOnly);

    class DOMRect extends DOMRectReadOnly {
        constructor(x = 0, y = 0, width = 0, height = 0) {
            super(x, y, width, height);
        }

        static fromRect(other = {}) {
            return createRectFromDictionary(this, other);
        }

        get x() {
            return data(this).x;
        }
        set x(value) {
            data(this).x = toNumber(value);
        }
        get y() {
            return data(this).y;
        }
        set y(value) {
            data(this).y = toNumber(value);
        }
        get width() {
            return data(this).width;
        }
        set width(value) {
            data(this).width = toNumber(value);
        }
        get height() {
            return data(this).height;
        }
        set height(value) {
            data(this).height = toNumber(value);
        }
    }
    defineInterface(DOMRect);

    const readRectInit = (init, name) => {
        const dict = toDictionary(init, name);
        const height = member(dict, "height");
        const width = member(dict, "width");
        const x = member(dict, "x");
        const y = member(dict, "y");
        return {
            x: x === undefined ? 0 : toNumber(x),
            y: y === undefined ? 0 : toNumber(y),
            width: width === undefined ? 0 : toNumber(width),
            height: height === undefined ? 0 : toNumber(height),
        };
    };
    const makeRect = (Cls, x, y, width, height) => {
        const rect = allocate(Cls);
        const r = data(rect);
        r.x = x;
        r.y = y;
        r.width = width;
        r.height = height;
        return rect;
    };
    const createRectFromDictionary = (Cls, init) => {
        const { x, y, width, height } = readRectInit(init, "DOMRectInit");
        return makeRect(Cls, x, y, width, height);
    };

    // === DOMRectList ===

    class DOMRectList {
        constructor() {
            if (!allocating) throw new TypeError("Illegal constructor");
            internals.set(this, { rects: [] });
        }
        get length() {
            return data(this).rects.length;
        }
        item(index) {
            if (arguments.length < 1) {
                throw new TypeError("DOMRectList.item: 1 argument required, but only 0 present");
            }
            const rects = data(this).rects;
            index = Number(index) >>> 0;
            return index < rects.length ? rects[index] : null;
        }
    }
    defineInterface(DOMRectList);
    Object.defineProperty(DOMRectList.prototype, Symbol.iterator, {
        value: function* () {
            for (let i = 0; i < this.length; i++) yield this.item(i);
        },
        writable: true,
        configurable: true,
    });
    const isIndex = (prop) => typeof prop === "string" && /^(0|[1-9]\d*)$/.test(prop);
    const rectLists = new WeakSet();
    const makeRectList = (rects) => {
        const list = allocate(DOMRectList);
        data(list).rects = rects;
        const proxy = new Proxy(list, {
            get(target, prop) {
                if (isIndex(prop)) {
                    const item = target.item(Number(prop));
                    return item === null ? undefined : item;
                }
                const value = Reflect.get(target, prop, target);
                return typeof value === "function" ? value.bind(target) : value;
            },
            has(target, prop) {
                if (isIndex(prop)) return Number(prop) < target.length;
                return Reflect.has(target, prop);
            },
            ownKeys(target) {
                const keys = [];
                for (let i = 0; i < target.length; i++) keys.push(String(i));
                return keys.concat(Reflect.ownKeys(target));
            },
            getOwnPropertyDescriptor(target, prop) {
                if (isIndex(prop) && Number(prop) < target.length) {
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
        rectLists.add(proxy);
        return proxy;
    };

    // === DOMQuad ===

    class DOMQuad {
        constructor(p1 = {}, p2 = {}, p3 = {}, p4 = {}) {
            if (allocating) {
                internals.set(this, { p1: null, p2: null, p3: null, p4: null });
                return;
            }
            internals.set(this, {
                p1: createPointFromDictionary(DOMPoint, p1),
                p2: createPointFromDictionary(DOMPoint, p2),
                p3: createPointFromDictionary(DOMPoint, p3),
                p4: createPointFromDictionary(DOMPoint, p4),
            });
        }

        static fromRect(other = {}) {
            const { x, y, width, height } = readRectInit(other, "DOMRectInit");
            const quad = allocate(DOMQuad);
            const q = data(quad);
            q.p1 = makePoint(DOMPoint, x, y, 0, 1);
            q.p2 = makePoint(DOMPoint, x + width, y, 0, 1);
            q.p3 = makePoint(DOMPoint, x + width, y + height, 0, 1);
            q.p4 = makePoint(DOMPoint, x, y + height, 0, 1);
            return quad;
        }

        static fromQuad(other = {}) {
            const dict = toDictionary(other, "DOMQuadInit");
            const p1 = member(dict, "p1");
            const p2 = member(dict, "p2");
            const p3 = member(dict, "p3");
            const p4 = member(dict, "p4");
            const quad = allocate(DOMQuad);
            const q = data(quad);
            q.p1 = createPointFromDictionary(DOMPoint, p1);
            q.p2 = createPointFromDictionary(DOMPoint, p2);
            q.p3 = createPointFromDictionary(DOMPoint, p3);
            q.p4 = createPointFromDictionary(DOMPoint, p4);
            return quad;
        }

        get p1() {
            return data(this).p1;
        }
        get p2() {
            return data(this).p2;
        }
        get p3() {
            return data(this).p3;
        }
        get p4() {
            return data(this).p4;
        }

        getBounds() {
            const q = data(this);
            const a = data(q.p1);
            const b = data(q.p2);
            const c = data(q.p3);
            const d = data(q.p4);
            const left = Math.min(a.x, b.x, c.x, d.x);
            const top = Math.min(a.y, b.y, c.y, d.y);
            const right = Math.max(a.x, b.x, c.x, d.x);
            const bottom = Math.max(a.y, b.y, c.y, d.y);
            return makeRect(DOMRect, left, top, right - left, bottom - top);
        }

        toJSON() {
            const q = data(this);
            return { p1: q.p1, p2: q.p2, p3: q.p3, p4: q.p4 };
        }
    }
    defineInterface(DOMQuad);

    // === DOMMatrix ===
    //
    // The 16 matrix elements are stored in column-major order:
    // index = (column - 1) * 4 + (row - 1), i.e. `m[0]` is m11, `m[1]` is m12,
    // `m[4]` is m21, and so on.

    const IDENTITY = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
    const ELEMENT_NAMES = [
        "m11", "m12", "m13", "m14",
        "m21", "m22", "m23", "m24",
        "m31", "m32", "m33", "m34",
        "m41", "m42", "m43", "m44",
    ];
    // Elements which must be 0 in a 2D matrix, and their index
    const MUST_BE_ZERO_2D = { m13: 2, m14: 3, m23: 6, m24: 7, m31: 8, m32: 9, m34: 11, m43: 14 };
    // Elements which must be 1 in a 2D matrix, and their index
    const MUST_BE_ONE_2D = { m33: 10, m44: 15 };

    // Post-multiply `a` by `b`: returns `a * b`
    const multiplyMatrices = (a, b) => {
        const out = new Array(16);
        for (let col = 0; col < 4; col++) {
            for (let row = 0; row < 4; row++) {
                let sum = 0;
                for (let k = 0; k < 4; k++) {
                    sum += a[k * 4 + row] * b[col * 4 + k];
                }
                out[col * 4 + row] = sum;
            }
        }
        return out;
    };

    const translationMatrix = (tx, ty, tz) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, tx, ty, tz, 1];
    const scaleMatrix = (sx, sy, sz) => [sx, 0, 0, 0, 0, sy, 0, 0, 0, 0, sz, 0, 0, 0, 0, 1];
    const deg2rad = (degrees) => (degrees * Math.PI) / 180;

    // https://drafts.csswg.org/css-transforms-2/#Rotate3dDefined
    const rotationMatrix = (x, y, z, angleDegrees) => {
        const length = Math.sqrt(x * x + y * y + z * z);
        if (length === 0) return IDENTITY.slice();
        if (length !== 1) {
            x /= length;
            y /= length;
            z /= length;
        }
        const half = deg2rad(angleDegrees) / 2;
        const sc = Math.sin(half) * Math.cos(half);
        const sq = Math.sin(half) * Math.sin(half);
        return [
            1 - 2 * (y * y + z * z) * sq,
            2 * (x * y * sq + z * sc),
            2 * (x * z * sq - y * sc),
            0,
            2 * (x * y * sq - z * sc),
            1 - 2 * (x * x + z * z) * sq,
            2 * (y * z * sq + x * sc),
            0,
            2 * (x * z * sq + y * sc),
            2 * (y * z * sq - x * sc),
            1 - 2 * (x * x + y * y) * sq,
            0,
            0,
            0,
            0,
            1,
        ];
    };
    const rotationMatrixZ = (angleDegrees) => {
        const rad = deg2rad(angleDegrees);
        const cos = Math.cos(rad);
        const sin = Math.sin(rad);
        return [cos, sin, 0, 0, -sin, cos, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
    };
    const skewXMatrix = (degrees) => [1, 0, 0, 0, Math.tan(deg2rad(degrees)), 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
    const skewYMatrix = (degrees) => [1, Math.tan(deg2rad(degrees)), 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

    // General 4x4 inverse via the adjugate matrix. Returns `null` if the
    // matrix is not invertible.
    const invertMatrix = (m) => {
        const [
            a00, a01, a02, a03,
            a10, a11, a12, a13,
            a20, a21, a22, a23,
            a30, a31, a32, a33,
        ] = m;

        const b00 = a00 * a11 - a01 * a10;
        const b01 = a00 * a12 - a02 * a10;
        const b02 = a00 * a13 - a03 * a10;
        const b03 = a01 * a12 - a02 * a11;
        const b04 = a01 * a13 - a03 * a11;
        const b05 = a02 * a13 - a03 * a12;
        const b06 = a20 * a31 - a21 * a30;
        const b07 = a20 * a32 - a22 * a30;
        const b08 = a20 * a33 - a23 * a30;
        const b09 = a21 * a32 - a22 * a31;
        const b10 = a21 * a33 - a23 * a31;
        const b11 = a22 * a33 - a23 * a32;

        const det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
        if (det === 0 || !Number.isFinite(det)) return null;

        return [
            (a11 * b11 - a12 * b10 + a13 * b09) / det,
            (a02 * b10 - a01 * b11 - a03 * b09) / det,
            (a31 * b05 - a32 * b04 + a33 * b03) / det,
            (a22 * b04 - a21 * b05 - a23 * b03) / det,
            (a12 * b08 - a10 * b11 - a13 * b07) / det,
            (a00 * b11 - a02 * b08 + a03 * b07) / det,
            (a32 * b02 - a30 * b05 - a33 * b01) / det,
            (a20 * b05 - a22 * b02 + a23 * b01) / det,
            (a10 * b10 - a11 * b08 + a13 * b06) / det,
            (a01 * b08 - a00 * b10 - a03 * b06) / det,
            (a30 * b04 - a31 * b02 + a33 * b00) / det,
            (a21 * b02 - a20 * b04 - a23 * b00) / det,
            (a11 * b07 - a10 * b09 - a12 * b06) / det,
            (a00 * b09 - a01 * b07 + a02 * b06) / det,
            (a31 * b01 - a30 * b03 - a32 * b00) / det,
            (a20 * b03 - a21 * b01 + a22 * b00) / det,
        ];
    };

    // Internal matrix state: `{ m: number[16], is2D: boolean }`
    const makeMatrix = (Cls, m, is2D) => {
        const matrix = allocate(Cls);
        const d = data(matrix);
        d.m = m;
        d.is2D = is2D;
        return matrix;
    };
    const create2DMatrix = (Cls, [m11, m12, m21, m22, m41, m42]) =>
        makeMatrix(Cls, [m11, m12, 0, 0, m21, m22, 0, 0, 0, 0, 1, 0, m41, m42, 0, 1], true);
    const create3DMatrix = (Cls, elements) => makeMatrix(Cls, Array.from(elements), false);

    const matrixFromSequence = (Cls, elements) => {
        if (elements.length === 6) return create2DMatrix(Cls, elements);
        if (elements.length === 16) return create3DMatrix(Cls, elements);
        throw new TypeError(
            `${Cls.name}: sequence must contain 6 or 16 elements, got ${elements.length}`
        );
    };

    // https://drafts.csswg.org/geometry/#parse-a-string-into-an-abstract-matrix
    const matrixFromString = (Cls, string) => {
        if (string === "") string = "matrix(1, 0, 0, 1, 0, 0)";
        const parsed = __blitz_parse_transform(string);
        if (parsed === null) {
            throw domException("SyntaxError", `Failed to parse ${JSON.stringify(string)} as a transform list`);
        }
        const [elements, is2D] = parsed;
        return is2D
            ? create2DMatrix(Cls, [elements[0], elements[1], elements[4], elements[5], elements[12], elements[13]])
            : create3DMatrix(Cls, elements);
    };

    // `(DOMString or sequence<unrestricted double>)` constructor argument
    const matrixFromInit = (Cls, init) => {
        if (init === undefined) return create2DMatrix(Cls, [1, 0, 0, 1, 0, 0]);
        if (init !== null && (typeof init === "object" || typeof init === "function")) {
            const iterator = init[Symbol.iterator];
            if (typeof iterator === "function") {
                const elements = [];
                for (const element of init) elements.push(toNumber(element));
                return matrixFromSequence(Cls, elements);
            }
        }
        return matrixFromString(Cls, String(init));
    };

    // https://drafts.csswg.org/geometry/#matrix-validate-and-fixup
    // Returns the 16 fixed-up elements and the `is2D` flag.
    const validateAndFixupMatrixInit = (init, name) => {
        const dict = toDictionary(init, name);
        // Read members in lexicographical order (a, b, c, d, e, f, is2D, m11, ...)
        const read = {};
        for (const key of ["a", "b", "c", "d", "e", "f"]) {
            const v = member(dict, key);
            if (v !== undefined) read[key] = toNumber(v);
        }
        const is2DValue = member(dict, "is2D");
        let is2D = is2DValue === undefined ? undefined : toBoolean(is2DValue);
        for (const key of ELEMENT_NAMES) {
            const v = member(dict, key);
            if (v !== undefined) read[key] = toNumber(v);
        }

        const sameValueZero = (x, y) => x === y || (Number.isNaN(x) && Number.isNaN(y));
        const aliases = [["a", "m11", 1], ["b", "m12", 0], ["c", "m21", 0], ["d", "m22", 1], ["e", "m41", 0], ["f", "m42", 0]];
        for (const [alias, element] of aliases) {
            if (alias in read && element in read && !sameValueZero(read[alias], read[element])) {
                throw new TypeError(`${name}: ${alias} and ${element} have different values`);
            }
        }
        for (const [alias, element, fallback] of aliases) {
            if (!(element in read)) read[element] = alias in read ? read[alias] : fallback;
        }

        let has3D = false;
        for (const key of Object.keys(MUST_BE_ZERO_2D)) {
            if (key in read && !isZero(read[key])) has3D = true;
        }
        for (const key of Object.keys(MUST_BE_ONE_2D)) {
            if (key in read && read[key] !== 1) has3D = true;
        }
        if (is2D === true && has3D) {
            throw new TypeError(`${name}: is2D is true but the matrix has 3D components`);
        }
        if (is2D === undefined) is2D = !has3D;

        const m = IDENTITY.slice();
        for (let i = 0; i < 16; i++) {
            const key = ELEMENT_NAMES[i];
            if (key in read) m[i] = read[key];
        }
        return { m, is2D };
    };

    const createMatrixFromDictionary = (Cls, init, name) => {
        const { m, is2D } = validateAndFixupMatrixInit(init, name);
        return is2D ? create2DMatrix(Cls, [m[0], m[1], m[4], m[5], m[12], m[13]]) : create3DMatrix(Cls, m);
    };

    const matrixFromTypedArray = (Cls, array, Type) => {
        if (!(array instanceof Type)) {
            throw new TypeError(`${Cls.name}: argument is not a ${Type.name}`);
        }
        return matrixFromSequence(Cls, Array.from(array));
    };

    // Mutable transformation algorithms operating on the internal state `d`
    const postMultiply = (d, other) => {
        d.m = multiplyMatrices(d.m, other);
    };
    const preMultiply = (d, other) => {
        d.m = multiplyMatrices(other, d.m);
    };
    const translateSelf = (d, tx, ty, tz) => {
        postMultiply(d, translationMatrix(tx, ty, tz));
        if (!isZero(tz)) d.is2D = false;
    };
    const scaleSelf = (d, sx, sy, sz, ox, oy, oz) => {
        translateSelf(d, ox, oy, oz);
        postMultiply(d, scaleMatrix(sx, sy, sz));
        translateSelf(d, -ox, -oy, -oz);
        if (sz !== 1) d.is2D = false;
    };
    const scale3dSelf = (d, scale, ox, oy, oz) => {
        translateSelf(d, ox, oy, oz);
        postMultiply(d, scaleMatrix(scale, scale, scale));
        translateSelf(d, -ox, -oy, -oz);
        if (scale !== 1) d.is2D = false;
    };
    const rotateSelf = (d, rotX, rotY, rotZ) => {
        if (rotY === undefined && rotZ === undefined) {
            rotZ = rotX;
            rotX = 0;
            rotY = 0;
        }
        if (rotY === undefined) rotY = 0;
        if (rotZ === undefined) rotZ = 0;
        if (!isZero(rotX) || !isZero(rotY)) d.is2D = false;
        postMultiply(d, rotationMatrix(0, 0, 1, rotZ));
        postMultiply(d, rotationMatrix(0, 1, 0, rotY));
        postMultiply(d, rotationMatrix(1, 0, 0, rotX));
    };
    const rotateFromVectorSelf = (d, x, y) => {
        const angle = isZero(x) && isZero(y) ? 0 : (Math.atan2(y, x) * 180) / Math.PI;
        postMultiply(d, rotationMatrixZ(angle));
    };
    const rotateAxisAngleSelf = (d, x, y, z, angle) => {
        postMultiply(d, rotationMatrix(x, y, z, angle));
        if (!isZero(x) || !isZero(y)) d.is2D = false;
    };
    const skewXSelf = (d, sx) => postMultiply(d, skewXMatrix(sx));
    const skewYSelf = (d, sy) => postMultiply(d, skewYMatrix(sy));
    const invertSelf = (d) => {
        const inverse = invertMatrix(d.m);
        if (inverse === null) {
            d.m = new Array(16).fill(NaN);
            d.is2D = false;
        } else {
            d.m = inverse;
        }
    };
    const multiplySelf = (d, other, name) => {
        const o = validateAndFixupMatrixInit(other, name);
        postMultiply(d, o.m);
        if (!o.is2D) d.is2D = false;
    };
    const preMultiplySelf = (d, other, name) => {
        const o = validateAndFixupMatrixInit(other, name);
        preMultiply(d, o.m);
        if (!o.is2D) d.is2D = false;
    };

    // A mutable copy of `this` matrix, for the immutable methods
    const cloneMatrix = (matrix) => {
        const d = data(matrix);
        return makeMatrix(DOMMatrix, d.m.slice(), d.is2D);
    };

    // The value of the scaleY argument when it is not passed: WebIDL
    // `optional unrestricted double scaleY` without a default
    const optionalNumber = (value) => (value === undefined ? undefined : toNumber(value));

    class DOMMatrixReadOnly {
        constructor(init = undefined) {
            if (allocating) {
                internals.set(this, { m: IDENTITY.slice(), is2D: true });
                return;
            }
            const built = matrixFromInit(new.target, init);
            internals.set(this, data(built));
        }

        static fromMatrix(other = {}) {
            return createMatrixFromDictionary(this, other, "DOMMatrixInit");
        }
        static fromFloat32Array(array32) {
            return matrixFromTypedArray(this, array32, Float32Array);
        }
        static fromFloat64Array(array64) {
            return matrixFromTypedArray(this, array64, Float64Array);
        }

        get a() {
            return data(this).m[0];
        }
        get b() {
            return data(this).m[1];
        }
        get c() {
            return data(this).m[4];
        }
        get d() {
            return data(this).m[5];
        }
        get e() {
            return data(this).m[12];
        }
        get f() {
            return data(this).m[13];
        }

        get is2D() {
            return data(this).is2D;
        }
        get isIdentity() {
            const m = data(this).m;
            for (let i = 0; i < 16; i++) {
                if (m[i] !== IDENTITY[i]) return false;
            }
            return true;
        }

        translate(tx = 0, ty = 0, tz = 0) {
            const result = cloneMatrix(this);
            translateSelf(data(result), toNumber(tx), toNumber(ty), toNumber(tz));
            return result;
        }
        scale(scaleX = 1, scaleY = undefined, scaleZ = 1, originX = 0, originY = 0, originZ = 0) {
            scaleX = toNumber(scaleX);
            scaleY = optionalNumber(scaleY);
            if (scaleY === undefined) scaleY = scaleX;
            const result = cloneMatrix(this);
            scaleSelf(
                data(result),
                scaleX,
                scaleY,
                toNumber(scaleZ),
                toNumber(originX),
                toNumber(originY),
                toNumber(originZ)
            );
            return result;
        }
        scaleNonUniform(scaleX = 1, scaleY = 1) {
            const result = cloneMatrix(this);
            scaleSelf(data(result), toNumber(scaleX), toNumber(scaleY), 1, 0, 0, 0);
            return result;
        }
        scale3d(scale = 1, originX = 0, originY = 0, originZ = 0) {
            const result = cloneMatrix(this);
            scale3dSelf(data(result), toNumber(scale), toNumber(originX), toNumber(originY), toNumber(originZ));
            return result;
        }
        rotate(rotX = 0, rotY = undefined, rotZ = undefined) {
            const result = cloneMatrix(this);
            rotateSelf(data(result), toNumber(rotX), optionalNumber(rotY), optionalNumber(rotZ));
            return result;
        }
        rotateFromVector(x = 0, y = 0) {
            const result = cloneMatrix(this);
            rotateFromVectorSelf(data(result), toNumber(x), toNumber(y));
            return result;
        }
        rotateAxisAngle(x = 0, y = 0, z = 0, angle = 0) {
            const result = cloneMatrix(this);
            rotateAxisAngleSelf(data(result), toNumber(x), toNumber(y), toNumber(z), toNumber(angle));
            return result;
        }
        skewX(sx = 0) {
            const result = cloneMatrix(this);
            skewXSelf(data(result), toNumber(sx));
            return result;
        }
        skewY(sy = 0) {
            const result = cloneMatrix(this);
            skewYSelf(data(result), toNumber(sy));
            return result;
        }
        multiply(other = {}) {
            const result = cloneMatrix(this);
            multiplySelf(data(result), other, "DOMMatrixInit");
            return result;
        }
        flipX() {
            const result = cloneMatrix(this);
            postMultiply(data(result), [-1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
            return result;
        }
        flipY() {
            const result = cloneMatrix(this);
            postMultiply(data(result), [1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
            return result;
        }
        inverse() {
            const result = cloneMatrix(this);
            invertSelf(data(result));
            return result;
        }

        transformPoint(point = {}) {
            const p = readPointInit(point, "DOMPointInit");
            return transformPoint(p, data(this).m);
        }
        toFloat32Array() {
            return new Float32Array(data(this).m);
        }
        toFloat64Array() {
            return new Float64Array(data(this).m);
        }

        toString() {
            const { m, is2D } = data(this);
            if (m.some((v) => !Number.isFinite(v))) {
                throw domException(
                    "InvalidStateError",
                    "DOMMatrix cannot be serialized with NaN or Infinity values"
                );
            }
            if (is2D) {
                return `matrix(${m[0]}, ${m[1]}, ${m[4]}, ${m[5]}, ${m[12]}, ${m[13]})`;
            }
            return `matrix3d(${m.join(", ")})`;
        }
        toJSON() {
            data(this);
            const result = {};
            for (const key of ["a", "b", "c", "d", "e", "f", ...ELEMENT_NAMES, "is2D", "isIdentity"]) {
                result[key] = this[key];
            }
            return result;
        }
    }
    // `m11`..`m44` getters
    // Accessor functions named `get m11` / `set m11` etc., as WebIDL requires
    const namedAccessor = (kind, name, fn) => {
        Object.defineProperty(fn, "name", { value: `${kind} ${name}`, configurable: true });
        return fn;
    };
    ELEMENT_NAMES.forEach((name, index) => {
        Object.defineProperty(DOMMatrixReadOnly.prototype, name, {
            get: namedAccessor("get", name, function () {
                return data(this).m[index];
            }),
            enumerable: true,
            configurable: true,
        });
    });
    defineInterface(DOMMatrixReadOnly);

    class DOMMatrix extends DOMMatrixReadOnly {
        constructor(init = undefined) {
            super(init);
        }

        static fromMatrix(other = {}) {
            return createMatrixFromDictionary(this, other, "DOMMatrixInit");
        }
        static fromFloat32Array(array32) {
            return matrixFromTypedArray(this, array32, Float32Array);
        }
        static fromFloat64Array(array64) {
            return matrixFromTypedArray(this, array64, Float64Array);
        }

        get a() {
            return data(this).m[0];
        }
        set a(value) {
            data(this).m[0] = toNumber(value);
        }
        get b() {
            return data(this).m[1];
        }
        set b(value) {
            data(this).m[1] = toNumber(value);
        }
        get c() {
            return data(this).m[4];
        }
        set c(value) {
            data(this).m[4] = toNumber(value);
        }
        get d() {
            return data(this).m[5];
        }
        set d(value) {
            data(this).m[5] = toNumber(value);
        }
        get e() {
            return data(this).m[12];
        }
        set e(value) {
            data(this).m[12] = toNumber(value);
        }
        get f() {
            return data(this).m[13];
        }
        set f(value) {
            data(this).m[13] = toNumber(value);
        }

        multiplySelf(other = {}) {
            multiplySelf(data(this), other, "DOMMatrixInit");
            return this;
        }
        preMultiplySelf(other = {}) {
            preMultiplySelf(data(this), other, "DOMMatrixInit");
            return this;
        }
        translateSelf(tx = 0, ty = 0, tz = 0) {
            translateSelf(data(this), toNumber(tx), toNumber(ty), toNumber(tz));
            return this;
        }
        scaleSelf(scaleX = 1, scaleY = undefined, scaleZ = 1, originX = 0, originY = 0, originZ = 0) {
            scaleX = toNumber(scaleX);
            scaleY = optionalNumber(scaleY);
            if (scaleY === undefined) scaleY = scaleX;
            scaleSelf(
                data(this),
                scaleX,
                scaleY,
                toNumber(scaleZ),
                toNumber(originX),
                toNumber(originY),
                toNumber(originZ)
            );
            return this;
        }
        scale3dSelf(scale = 1, originX = 0, originY = 0, originZ = 0) {
            scale3dSelf(data(this), toNumber(scale), toNumber(originX), toNumber(originY), toNumber(originZ));
            return this;
        }
        rotateSelf(rotX = 0, rotY = undefined, rotZ = undefined) {
            rotateSelf(data(this), toNumber(rotX), optionalNumber(rotY), optionalNumber(rotZ));
            return this;
        }
        rotateFromVectorSelf(x = 0, y = 0) {
            rotateFromVectorSelf(data(this), toNumber(x), toNumber(y));
            return this;
        }
        rotateAxisAngleSelf(x = 0, y = 0, z = 0, angle = 0) {
            rotateAxisAngleSelf(data(this), toNumber(x), toNumber(y), toNumber(z), toNumber(angle));
            return this;
        }
        skewXSelf(sx = 0) {
            skewXSelf(data(this), toNumber(sx));
            return this;
        }
        skewYSelf(sy = 0) {
            skewYSelf(data(this), toNumber(sy));
            return this;
        }
        invertSelf() {
            invertSelf(data(this));
            return this;
        }
        setMatrixValue(transformList) {
            const d = data(this);
            if (arguments.length < 1) {
                throw new TypeError("DOMMatrix.setMatrixValue: 1 argument required, but only 0 present");
            }
            const parsed = matrixFromString(DOMMatrix, String(transformList));
            const p = data(parsed);
            d.m = p.m;
            d.is2D = p.is2D;
            return this;
        }
    }
    // `m11`..`m44` accessors. Setting a 3D-only element to a non-default
    // value clears `is2D` (which can never be set back to true, except via
    // `setMatrixValue()`).
    ELEMENT_NAMES.forEach((name, index) => {
        Object.defineProperty(DOMMatrix.prototype, name, {
            get: namedAccessor("get", name, function () {
                return data(this).m[index];
            }),
            set: namedAccessor("set", name, function (value) {
                const d = data(this);
                value = toNumber(value);
                d.m[index] = value;
                if (name in MUST_BE_ZERO_2D && !isZero(value)) d.is2D = false;
                if (name in MUST_BE_ONE_2D && value !== 1) d.is2D = false;
            }),
            enumerable: true,
            configurable: true,
        });
    });
    defineInterface(DOMMatrix);

    exposeGlobal("DOMPointReadOnly", DOMPointReadOnly);
    exposeGlobal("DOMPoint", DOMPoint);
    exposeGlobal("DOMRectReadOnly", DOMRectReadOnly);
    exposeGlobal("DOMRect", DOMRect);
    exposeGlobal("DOMRectList", DOMRectList);
    exposeGlobal("DOMQuad", DOMQuad);
    exposeGlobal("DOMMatrixReadOnly", DOMMatrixReadOnly);
    exposeGlobal("DOMMatrix", DOMMatrix);
    // Legacy aliases
    exposeGlobal("SVGPoint", DOMPoint);
    exposeGlobal("SVGRect", DOMRect);
    exposeGlobal("SVGMatrix", DOMMatrix);
    exposeGlobal("WebKitCSSMatrix", DOMMatrix);

    // === Structured serialization ===
    //
    // The native `structuredClone` knows nothing about the geometry classes,
    // so geometry objects are swapped for tagged plain-object placeholders
    // before cloning, and swapped back afterwards (recursing into arrays and
    // plain objects, the only containers geometry objects commonly appear in).
    // The same clone is used by `MessagePort.postMessage()`.

    const GEOMETRY_TAG = "__blitz_geometry__";
    const GEOMETRY_CLASSES = {
        DOMPointReadOnly,
        DOMPoint,
        DOMRectReadOnly,
        DOMRect,
        DOMQuad,
        DOMMatrixReadOnly,
        DOMMatrix,
    };
    const geometryClassName = (value) => {
        for (const Cls of Object.values(GEOMETRY_CLASSES)) {
            if (Object.getPrototypeOf(value) === Cls.prototype) return Cls.name;
        }
        return null;
    };
    const dataCloneError = (what) =>
        domException("DataCloneError", `${what} could not be cloned`);

    const serializeGeometry = (obj, name) => {
        const d = data(obj);
        switch (name) {
            case "DOMPointReadOnly":
            case "DOMPoint":
                return { x: d.x, y: d.y, z: d.z, w: d.w };
            case "DOMRectReadOnly":
            case "DOMRect":
                return { x: d.x, y: d.y, width: d.width, height: d.height };
            case "DOMQuad":
                return {
                    p1: serializeGeometry(d.p1, "DOMPoint"),
                    p2: serializeGeometry(d.p2, "DOMPoint"),
                    p3: serializeGeometry(d.p3, "DOMPoint"),
                    p4: serializeGeometry(d.p4, "DOMPoint"),
                };
            default: {
                // DOMMatrix: 2D matrices only round-trip the 2D elements
                const m = d.is2D ? [d.m[0], d.m[1], d.m[4], d.m[5], d.m[12], d.m[13]] : d.m.slice();
                return { m, is2D: d.is2D };
            }
        }
    };
    const deserializeGeometry = (name, s) => {
        const Cls = GEOMETRY_CLASSES[name];
        switch (name) {
            case "DOMPointReadOnly":
            case "DOMPoint":
                return makePoint(Cls, s.x, s.y, s.z, s.w);
            case "DOMRectReadOnly":
            case "DOMRect":
                return makeRect(Cls, s.x, s.y, s.width, s.height);
            case "DOMQuad": {
                const quad = allocate(DOMQuad);
                const q = data(quad);
                for (const key of ["p1", "p2", "p3", "p4"]) {
                    q[key] = deserializeGeometry("DOMPoint", s[key]);
                }
                return quad;
            }
            default:
                return s.is2D ? create2DMatrix(Cls, s.m) : create3DMatrix(Cls, s.m);
        }
    };

    const isPlainObject = (value) => {
        const proto = Object.getPrototypeOf(value);
        return proto === Object.prototype || proto === null;
    };
    const replaceGeometry = (value, memo) => {
        if (value === null || (typeof value !== "object" && typeof value !== "function")) {
            return value;
        }
        if (memo.has(value)) return memo.get(value);
        if (rectLists.has(value)) throw dataCloneError("DOMRectList object");
        const name = geometryClassName(value);
        if (name !== null) {
            const placeholder = { [GEOMETRY_TAG]: name, value: serializeGeometry(value, name) };
            memo.set(value, placeholder);
            return placeholder;
        }
        if (Array.isArray(value)) {
            const out = new Array(value.length);
            memo.set(value, out);
            for (const key of Object.keys(value)) out[key] = replaceGeometry(value[key], memo);
            return out;
        }
        if (isPlainObject(value)) {
            const out = {};
            memo.set(value, out);
            for (const key of Object.keys(value)) out[key] = replaceGeometry(value[key], memo);
            return out;
        }
        return value;
    };
    const restoreGeometry = (value, memo) => {
        if (value === null || typeof value !== "object") return value;
        if (memo.has(value)) return memo.get(value);
        if (Array.isArray(value)) {
            memo.set(value, value);
            for (const key of Object.keys(value)) value[key] = restoreGeometry(value[key], memo);
            return value;
        }
        if (isPlainObject(value)) {
            if (typeof value[GEOMETRY_TAG] === "string" && value[GEOMETRY_TAG] in GEOMETRY_CLASSES) {
                const restored = deserializeGeometry(value[GEOMETRY_TAG], value.value);
                memo.set(value, restored);
                return restored;
            }
            memo.set(value, value);
            for (const key of Object.keys(value)) value[key] = restoreGeometry(value[key], memo);
            return value;
        }
        return value;
    };

    const nativeStructuredClone = globalThis.structuredClone;
    const structuredClone = function structuredClone(value, options) {
        if (arguments.length < 1) {
            throw new TypeError("structuredClone: 1 argument required, but only 0 present");
        }
        const replaced = replaceGeometry(value, new Map());
        const cloned =
            typeof nativeStructuredClone === "function"
                ? nativeStructuredClone(replaced, options)
                : replaced;
        return restoreGeometry(cloned, new Map());
    };
    exposeGlobal("structuredClone", structuredClone);

    // === MessageChannel / MessagePort ===
    //
    // An in-process channel: messages posted on one port are structured-cloned
    // and delivered asynchronously (as a task) to the entangled port.

    class MessagePort {
        constructor() {
            if (!allocating) throw new TypeError("Illegal constructor");
            internals.set(this, { other: null, onmessage: null, listeners: [], closed: false });
        }
        get onmessage() {
            return data(this).onmessage;
        }
        set onmessage(handler) {
            data(this).onmessage = typeof handler === "function" ? handler : null;
        }
        addEventListener(type, listener) {
            if (type === "message" && typeof listener === "function") {
                data(this).listeners.push(listener);
            }
        }
        removeEventListener(type, listener) {
            if (type !== "message") return;
            const d = data(this);
            d.listeners = d.listeners.filter((l) => l !== listener);
        }
        postMessage(message) {
            const d = data(this);
            const cloned = structuredClone(message);
            const target = d.other;
            if (target === null || d.closed) return;
            setTimeout(() => {
                const t = data(target);
                if (t.closed) return;
                const event = {
                    type: "message",
                    data: cloned,
                    origin: "",
                    lastEventId: "",
                    source: null,
                    ports: [],
                    target,
                    currentTarget: target,
                };
                if (t.onmessage !== null) t.onmessage.call(target, event);
                for (const listener of t.listeners.slice()) listener.call(target, event);
            }, 0);
        }
        start() {}
        close() {
            data(this).closed = true;
        }
    }
    defineInterface(MessagePort);

    class MessageChannel {
        constructor() {
            const port1 = allocate(MessagePort);
            const port2 = allocate(MessagePort);
            data(port1).other = port2;
            data(port2).other = port1;
            internals.set(this, { port1, port2 });
        }
        get port1() {
            return data(this).port1;
        }
        get port2() {
            return data(this).port2;
        }
    }
    defineInterface(MessageChannel);

    if (typeof globalThis.MessageChannel === "undefined") {
        exposeGlobal("MessagePort", MessagePort);
        exposeGlobal("MessageChannel", MessageChannel);
    }

    // `Element.getBoundingClientRect()` / `getClientRects()`: wrap the plain
    // rect objects produced by the natives in `DOMRect` / `DOMRectList`
    if (document.documentElement) {
        const elementProto = Object.getPrototypeOf(document.documentElement);
        const nativeBoundingRect = elementProto.getBoundingClientRect;
        const nativeClientRects = elementProto.getClientRects;
        const toDomRect = (r) => makeRect(DOMRect, r.x, r.y, r.width, r.height);
        Object.defineProperties(elementProto, {
            getBoundingClientRect: {
                value: function getBoundingClientRect() {
                    return toDomRect(nativeBoundingRect.call(this));
                },
                writable: true,
                configurable: true,
            },
            getClientRects: {
                value: function getClientRects() {
                    return makeRectList(nativeClientRects.call(this).map(toDomRect));
                },
                writable: true,
                configurable: true,
            },
        });
    }
})();
