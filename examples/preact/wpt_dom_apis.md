# DOM APIs used in `grid-flex-track-intrinsic-sizes-003.html`

The test links four scripts: the inline `<script>`, `/resources/testharness.js`,
`/resources/testharnessreport.js`, and
`../grid-definition/support/testing-utils.js`. Pure ECMAScript built-ins (e.g.
`Array`, `JSON`, `Object`, `Promise`) and testharness's own functions (e.g.
`test()`, `assert_in_array()`) are excluded — only browser-provided APIs are
listed.

## Inline script (`grid-flex-track-intrinsic-sizes-003.html`)

- `document.getElementById()`
- `element.style` (CSSOM `CSSStyleDeclaration`) — set via `style.gridColumn`,
  `style.gridRow`, `style.minWidth`, `style.minHeight`

## `testing-utils.js` (grid support script)

- `document.getElementById()`
- `window.getComputedStyle()` — and reading `.gridTemplateColumns`,
  `.gridTemplateRows`, `.gridTemplateAreas`
- `element.style` (CSSOM) — set via `style.gridTemplateColumns`,
  `style.gridTemplateRows`, `style.gridTemplateAreas`

## `testharnessreport.js`

- `window.opener` (and the `in` property check on it)

## `testharness.js`

This is the large test framework; it pulls in a broad set of APIs.

### window / global

- `self`, `window`, `window.parent`, `window.opener`
- `window.postMessage()`
- `window.addEventListener()` / `removeEventListener()`
- `window.dispatchEvent()`
- `setTimeout()` / `clearTimeout()`

### document

- `document.getElementById()`, `document.getElementsByTagName()`
- `document.querySelector()`, `document.querySelectorAll()`
- `document.createElementNS()`, `document.createTextNode()`
- `document.body`, `document.documentElement`, `document.defaultView`,
  `document.readyState`
- `document.appendChild()`, `document.removeChild()`

### Element / Node

- `element.appendChild()`, `removeChild()`, `insertAdjacentText()`
- `element.setAttribute()`, `getAttribute()`
- `element.textContent`, `innerHTML`, `lastChild`, `childNodes`
- `element.querySelector()`
- `node.nodeType`, `localName`, `attributes`, `data`, `target`
- `Node.ELEMENT_NODE`, `TEXT_NODE`, `PROCESSING_INSTRUCTION_NODE`,
  `COMMENT_NODE`, `DOCUMENT_NODE`, `DOCUMENT_TYPE_NODE`,
  `DOCUMENT_FRAGMENT_NODE`
- `script.src`, `script.href` (+ `href.baseVal`)
- `SVGSVGElement`, `HTMLAllCollection`

### Events

- `addEventListener()` / `removeEventListener()`
- `Event` constructor, `EventTarget`
- Event properties: `data`, `source`, `type`, `message`, `filename`, `stack`,
  `error`, `lineno`, `colno`, `reason`
- `event.preventDefault()`
- `ErrorEvent`, `PromiseRejectionEvent` (via `error` / `unhandledrejection`)

### Workers / messaging

- `Worker`, `SharedWorker`, `ServiceWorker`
- `DedicatedWorkerGlobalScope`, `SharedWorkerGlobalScope`,
  `ServiceWorkerGlobalScope`, `WorkerGlobalScope`
- `MessageChannel`, `MessagePort`, `port.start()`, `postMessage()`
- `navigator.serviceWorker`, `worker.port`
- `ShadowRealm`

### Location / navigation

- `location`, `location.href`, `location.pathname`, `location.reload()`

### Console

- `console.log()`, `console.debug()`

### Networking / other Web APIs

- `fetch()`, `response.json()`
- `DOMException`, `QuotaExceededError`
- `AbortController` (`.abort()`, `.signal`), `AbortSignal`

## Authored-code-only subset

If you only care about the APIs the *authored* test code exercises (excluding
the testharness framework), the relevant set is just:

- `document.getElementById`
- `element.style` (CSSOM)
- `window.getComputedStyle`
- `window.opener`
