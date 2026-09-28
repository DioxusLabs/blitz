// DOM Standard `Range` (https://dom.spec.whatwg.org/#interface-range) and
// `document.createRange()`.
//
// Ranges are not live: boundary points are not updated when the DOM is mutated.
// The mutation methods (`deleteContents`, `insertNode`, ...) are not implemented.
// Geometry (`getClientRects` / `getBoundingClientRect`) is computed by the
// `__blitz_range_client_rects` native.
(function () {
    const internals = new WeakMap();
    const data = (obj) => {
        const d = internals.get(obj);
        if (!d) throw new TypeError("Illegal invocation");
        return d;
    };
    const domException = (name, message) => new DOMException(message, name);

    const isCharacterData = (node) =>
        node.nodeType === 3 || node.nodeType === 4 || node.nodeType === 7 || node.nodeType === 8;
    const nodeLength = (node) => {
        if (node.nodeType === 10) return 0;
        if (isCharacterData(node)) return node.data.length;
        return node.childNodes.length;
    };
    const indexOf = (node) => Array.prototype.indexOf.call(node.parentNode.childNodes, node);
    const rootOf = (node) => {
        while (node.parentNode) node = node.parentNode;
        return node;
    };
    const ancestors = (node) => {
        const chain = [];
        for (; node; node = node.parentNode) chain.unshift(node);
        return chain;
    };
    const isInclusiveAncestor = (ancestor, node) => {
        for (; node; node = node.parentNode) if (node === ancestor) return true;
        return false;
    };
    // Tree order of two nodes in the same tree: -1 if `a` precedes `b`, 1 if it
    // follows `b`, 0 if they are the same node
    const treeOrder = (a, b) => {
        if (a === b) return 0;
        const chainA = ancestors(a);
        const chainB = ancestors(b);
        let i = 0;
        while (i < chainA.length && i < chainB.length && chainA[i] === chainB[i]) i++;
        if (i === chainA.length) return -1; // `a` is an ancestor of `b`
        if (i === chainB.length) return 1; // `b` is an ancestor of `a`
        return indexOf(chainA[i]) < indexOf(chainB[i]) ? -1 : 1;
    };
    // https://dom.spec.whatwg.org/#concept-range-bp-position
    const comparePoints = (nodeA, offsetA, nodeB, offsetB) => {
        if (nodeA === nodeB) return offsetA === offsetB ? 0 : offsetA < offsetB ? -1 : 1;
        if (treeOrder(nodeA, nodeB) > 0) return -comparePoints(nodeB, offsetB, nodeA, offsetA);
        if (isInclusiveAncestor(nodeA, nodeB)) {
            let child = nodeB;
            while (child.parentNode !== nodeA) child = child.parentNode;
            if (indexOf(child) < offsetA) return 1;
        }
        return -1;
    };
    const toOffset = (value) => Number(value) >>> 0;
    const checkNode = (node, method) => {
        if (!node || typeof node.nodeType !== "number") {
            throw new TypeError(`Range.${method}: Argument 1 is not an object implementing Node`);
        }
    };

    // https://dom.spec.whatwg.org/#concept-range-bp-set
    const setBoundary = (range, node, offset, isStart, method) => {
        checkNode(node, method);
        if (node.nodeType === 10) {
            throw domException("InvalidNodeTypeError", `Range.${method}: node is a doctype`);
        }
        if (offset > nodeLength(node)) {
            throw domException("IndexSizeError", `Range.${method}: offset is out of range`);
        }
        const r = data(range);
        if (isStart) {
            r.startContainer = node;
            r.startOffset = offset;
            if (
                rootOf(r.endContainer) !== rootOf(node) ||
                comparePoints(node, offset, r.endContainer, r.endOffset) > 0
            ) {
                r.endContainer = node;
                r.endOffset = offset;
            }
        } else {
            r.endContainer = node;
            r.endOffset = offset;
            if (
                rootOf(r.startContainer) !== rootOf(node) ||
                comparePoints(node, offset, r.startContainer, r.startOffset) < 0
            ) {
                r.startContainer = node;
                r.startOffset = offset;
            }
        }
    };
    const parentOrThrow = (node, method) => {
        checkNode(node, method);
        const parent = node.parentNode;
        if (!parent) throw domException("InvalidNodeTypeError", `Range.${method}: node has no parent`);
        return parent;
    };
    const isContained = (r, node) =>
        rootOf(node) === rootOf(r.startContainer) &&
        comparePoints(node, 0, r.startContainer, r.startOffset) > 0 &&
        comparePoints(node, nodeLength(node), r.endContainer, r.endOffset) < 0;
    const nextInTreeOrder = (node) => {
        if (node.firstChild) return node.firstChild;
        for (; node; node = node.parentNode) {
            if (node.nextSibling) return node.nextSibling;
        }
        return null;
    };

    class Range {
        constructor() {
            internals.set(this, {
                startContainer: document,
                startOffset: 0,
                endContainer: document,
                endOffset: 0,
            });
        }

        get startContainer() {
            return data(this).startContainer;
        }
        get startOffset() {
            return data(this).startOffset;
        }
        get endContainer() {
            return data(this).endContainer;
        }
        get endOffset() {
            return data(this).endOffset;
        }
        get collapsed() {
            const r = data(this);
            return r.startContainer === r.endContainer && r.startOffset === r.endOffset;
        }
        get commonAncestorContainer() {
            const r = data(this);
            let container = r.startContainer;
            while (!isInclusiveAncestor(container, r.endContainer)) container = container.parentNode;
            return container;
        }

        setStart(node, offset) {
            setBoundary(this, node, toOffset(offset), true, "setStart");
        }
        setEnd(node, offset) {
            setBoundary(this, node, toOffset(offset), false, "setEnd");
        }
        setStartBefore(node) {
            const parent = parentOrThrow(node, "setStartBefore");
            setBoundary(this, parent, indexOf(node), true, "setStartBefore");
        }
        setStartAfter(node) {
            const parent = parentOrThrow(node, "setStartAfter");
            setBoundary(this, parent, indexOf(node) + 1, true, "setStartAfter");
        }
        setEndBefore(node) {
            const parent = parentOrThrow(node, "setEndBefore");
            setBoundary(this, parent, indexOf(node), false, "setEndBefore");
        }
        setEndAfter(node) {
            const parent = parentOrThrow(node, "setEndAfter");
            setBoundary(this, parent, indexOf(node) + 1, false, "setEndAfter");
        }
        collapse(toStart = false) {
            const r = data(this);
            if (toStart) {
                r.endContainer = r.startContainer;
                r.endOffset = r.startOffset;
            } else {
                r.startContainer = r.endContainer;
                r.startOffset = r.endOffset;
            }
        }
        selectNode(node) {
            const parent = parentOrThrow(node, "selectNode");
            const index = indexOf(node);
            const r = data(this);
            r.startContainer = parent;
            r.startOffset = index;
            r.endContainer = parent;
            r.endOffset = index + 1;
        }
        selectNodeContents(node) {
            checkNode(node, "selectNodeContents");
            if (node.nodeType === 10) {
                throw domException("InvalidNodeTypeError", "Range.selectNodeContents: node is a doctype");
            }
            const r = data(this);
            r.startContainer = node;
            r.startOffset = 0;
            r.endContainer = node;
            r.endOffset = nodeLength(node);
        }

        compareBoundaryPoints(how, sourceRange) {
            how = Number(how) & 0xffff;
            if (how > 3) {
                throw domException("NotSupportedError", "Range.compareBoundaryPoints: invalid comparison type");
            }
            const r = data(this);
            const s = data(sourceRange);
            if (rootOf(r.startContainer) !== rootOf(s.startContainer)) {
                throw domException("WrongDocumentError", "Range.compareBoundaryPoints: ranges are in different trees");
            }
            // START_TO_START, START_TO_END, END_TO_END, END_TO_START
            const [thisStart, sourceStart] = [
                [true, true],
                [false, true],
                [false, false],
                [true, false],
            ][how];
            return comparePoints(
                thisStart ? r.startContainer : r.endContainer,
                thisStart ? r.startOffset : r.endOffset,
                sourceStart ? s.startContainer : s.endContainer,
                sourceStart ? s.startOffset : s.endOffset,
            );
        }
        comparePoint(node, offset) {
            checkNode(node, "comparePoint");
            offset = toOffset(offset);
            const r = data(this);
            if (rootOf(node) !== rootOf(r.startContainer)) {
                throw domException("WrongDocumentError", "Range.comparePoint: node is in a different tree");
            }
            if (node.nodeType === 10) {
                throw domException("InvalidNodeTypeError", "Range.comparePoint: node is a doctype");
            }
            if (offset > nodeLength(node)) {
                throw domException("IndexSizeError", "Range.comparePoint: offset is out of range");
            }
            if (comparePoints(node, offset, r.startContainer, r.startOffset) < 0) return -1;
            if (comparePoints(node, offset, r.endContainer, r.endOffset) > 0) return 1;
            return 0;
        }
        isPointInRange(node, offset) {
            checkNode(node, "isPointInRange");
            const r = data(this);
            if (rootOf(node) !== rootOf(r.startContainer)) return false;
            return this.comparePoint(node, offset) === 0;
        }
        intersectsNode(node) {
            checkNode(node, "intersectsNode");
            const r = data(this);
            if (rootOf(node) !== rootOf(r.startContainer)) return false;
            const parent = node.parentNode;
            if (!parent) return true;
            const offset = indexOf(node);
            return (
                comparePoints(parent, offset, r.endContainer, r.endOffset) < 0 &&
                comparePoints(parent, offset + 1, r.startContainer, r.startOffset) > 0
            );
        }

        cloneRange() {
            const clone = new Range();
            Object.assign(data(clone), data(this));
            return clone;
        }
        detach() {}

        toString() {
            const r = data(this);
            const isText = (node) => node.nodeType === 3;
            if (r.startContainer === r.endContainer && isText(r.startContainer)) {
                return r.startContainer.data.substring(r.startOffset, r.endOffset);
            }
            let s = "";
            if (isText(r.startContainer)) s += r.startContainer.data.substring(r.startOffset);
            const common = this.commonAncestorContainer;
            for (let node = common; node && isInclusiveAncestor(common, node); node = nextInTreeOrder(node)) {
                if (isText(node) && isContained(r, node)) s += node.data;
            }
            if (isText(r.endContainer)) s += r.endContainer.data.substring(0, r.endOffset);
            return s;
        }

        getClientRects() {
            const r = data(this);
            const rects = __blitz_range_client_rects(r.startContainer, r.startOffset, r.endContainer, r.endOffset);
            return __blitz_make_rect_list(rects.map((d) => new DOMRect(d.x, d.y, d.width, d.height)));
        }
        getBoundingClientRect() {
            const r = data(this);
            const rects = __blitz_range_client_rects(r.startContainer, r.startOffset, r.endContainer, r.endOffset);
            if (rects.length === 0) return new DOMRect(0, 0, 0, 0);
            const nonEmpty = rects.filter((d) => d.width !== 0 && d.height !== 0);
            if (nonEmpty.length === 0) {
                return new DOMRect(rects[0].x, rects[0].y, rects[0].width, rects[0].height);
            }
            const left = Math.min(...nonEmpty.map((d) => d.x));
            const top = Math.min(...nonEmpty.map((d) => d.y));
            const right = Math.max(...nonEmpty.map((d) => d.x + d.width));
            const bottom = Math.max(...nonEmpty.map((d) => d.y + d.height));
            return new DOMRect(left, top, right - left, bottom - top);
        }
    }

    const constants = { START_TO_START: 0, START_TO_END: 1, END_TO_END: 2, END_TO_START: 3 };
    for (const [name, value] of Object.entries(constants)) {
        Object.defineProperty(Range, name, { value, enumerable: true });
        Object.defineProperty(Range.prototype, name, { value, enumerable: true });
    }
    Object.defineProperty(Range.prototype, Symbol.toStringTag, { value: "Range", configurable: true });
    Object.defineProperty(globalThis, "Range", {
        value: Range,
        writable: true,
        enumerable: false,
        configurable: true,
    });

    Object.defineProperty(Object.getPrototypeOf(document), "createRange", {
        value: function createRange() {
            return new Range();
        },
        writable: true,
        enumerable: true,
        configurable: true,
    });
})();
