# DOM APIs used by React (react-dom / react-dom-bindings)

AI-generated analysis of the React source (`facebook/react`). The browser DOM APIs
live almost entirely in the `react-dom-bindings` package (the client host config,
property/attribute handling, and the event system), with a small amount in
`react-dom` and `scheduler`.

## Global objects & entry points
- `window` — `window.event`, `window.scrollX`/`scrollY`, `window.scrollTo`, `window.innerWidth`/`innerHeight`, `window.addEventListener`/`removeEventListener`, `window.clipboardData`, `window.Element.prototype.moveBefore` (feature detection)
- `document` — `documentElement`, `head`, `body`, `activeElement`, `readyState`, `defaultView`, `documentMode` (legacy IE), `fonts.ready`, `__reactViewTransition` (custom marker)
- `navigator`, `performance` (`performance.now()`), `matchMedia`, `getComputedStyle`, `CSS` (`CSS.escape`), `URL` (`createObjectURL`/`revokeObjectURL`)

## Document / node creation
- `document.createElement`, `document.createElementNS`
- `document.createTextNode`
- `document.createComment` — comment/marker nodes
- `document.createDocumentFragment`
- `document.createRange` (+ `Range.selectNodeContents`, `setStart`, `setEnd`)
- `document.createEvent` + `Event.initEvent`/`initMouseEvent` — legacy synthetic dispatch
- `node.cloneNode`

Namespaces used with `createElementNS` (`DOMNamespaces.js`):
- `http://www.w3.org/1999/xhtml` (HTML)
- `http://www.w3.org/2000/svg` (SVG)
- `http://www.w3.org/1998/Math/MathML` (MathML)

## Tree traversal & mutation (`Node`)
- `parent.appendChild`, `insertBefore`, `removeChild`, `replaceChild`
- `element.prepend`, `element.remove`
- `element.moveBefore` — new state-preserving move API (feature detected)
- `node.contains`, `node.compareDocumentPosition` (+ `Node.DOCUMENT_POSITION_CONTAINED_BY`)
- `node.getRootNode`
- `node.hasChildNodes`
- Reads: `parentNode`, `firstChild`, `lastChild`, `nextSibling`, `previousSibling`, `childNodes`, `children`, `firstElementChild`, `nextElementSibling`, `ownerDocument`, `nodeType`, `nodeName`, `nodeValue`, `tagName`, `namespaceURI`, `data` (comment/text)

## Selecting / querying
- `document.getElementById`
- `document.getElementsByTagName`
- `element.querySelector`, `element.querySelectorAll`

## Attributes & properties (`Element`)
- `element.setAttribute`, `setAttributeNS`
- `element.getAttribute`, `getAttributeNS`
- `element.removeAttribute`, `removeAttributeNS`, `removeAttributeNode`
- `element.hasAttribute`
- `element.dataset`
- Direct DOM/reflected properties: `id`, `className`, `value`, `defaultValue`, `checked`, `defaultChecked`, `selected`, `selectedIndex`, `multiple`, `disabled`, `href`, `src`, `srcSet`, `type`, `name`, `title`, `is`, `nonce`, `crossOrigin`, `integrity`, `referrerPolicy`, `media`, `loading`, `fetchPriority`, `contentEditable`/`isContentEditable`, `autofocus`, `innerHTML`, `outerHTML`, `textContent`
- `element.srcObject` (media elements), via `URL.createObjectURL`

## Text / HTML content
- `setTextContent` — via `node.textContent` / `firstChild.nodeValue`
- `setInnerHTML` — via `innerHTML` (Trusted Types–aware; see below)

## Styles (`CSSStyleDeclaration`)
- `element.style` (read)
- `style.setProperty` — custom properties (`--*`) and general props
- `style.cssFloat`
- `style[prop] = value` — individual style properties
- `getComputedStyle`

## Focus / selection (inputs & contentEditable)
- `element.focus` (incl. `{preventScroll}`), `element.blur`
- `document.activeElement` (`getActiveElement` helper)
- Input selection: `input.selectionStart`, `input.selectionEnd`, `input.setSelectionRange`, `input.select`
- Selection API: `window.getSelection`, `selection.rangeCount`, `anchorNode`/`anchorOffset`, `focusNode`/`focusOffset`, `removeAllRanges`, `addRange`, `extend` (+ `Range` `setStart`/`setEnd`/`selectNodeContents`)
- Forms: `form.requestSubmit`, `form.submit`, `form.reset`

## Scrolling / geometry
- `element.getBoundingClientRect`, `element.getClientRects`
- `element.scrollIntoView`
- `element.scrollTop`/`scrollLeft`, `window.scrollTo`, `window.scrollX`/`scrollY`, `DOMRect`

## Events (`EventTarget` / `Event`)
- `target.addEventListener`/`removeEventListener` with `{capture, passive, once}` (option support detected in `checkPassiveEvents`)
- `element.dispatchEvent`
- Legacy IE: `attachEvent`/`detachEvent`
- `unstable_createEventHandle` — imperative event handle API
- On the native event: `preventDefault`, `stopPropagation`, `getModifierState`, `composedPath` (via `getEventTarget`); reads of `type`, `target`, `relatedTarget`, `currentTarget`, `keyCode`, `charCode`, `which`, `pointerId`, `clipboardData`, `dataTransfer`, `defaultPrevented`, `timeStamp`
- Native event names (`DOMEventNames.js` / `DOMEventProperties.js`): the full standard set — `click`, `dblclick`, `input`, `change`, `submit`, `reset`, `focusin`/`focusout`, `keydown`/`keyup`/`keypress`, `mouse*`, `pointer*`, `touch*`, `drag*`/`drop`, `wheel`/`scroll`, `copy`/`cut`/`paste`, `compositionstart`/`update`/`end`, `animation*`, `transition*`, media events (`play`, `pause`, `ended`, `seeking`, `loadeddata`, …), `load`/`error`/`abort`, `toggle`, `beforetoggle`, `resize`, `select`, `selectionchange`, etc.
- Event constructors: `Event`, `CustomEvent`, `InputEvent`, `MouseEvent`, `KeyboardEvent`, `UIEvent` (constructed and used for interface detection)

## Observers, animations & scheduling
- `MutationObserver` (`observe`, `disconnect`, `takeRecords`) — Fizz external runtime & instruction set
- `IntersectionObserver`, `ResizeObserver` (`observe`, `unobserve`, `disconnect`)
- `element.animate`, `element.getAnimations`, `Animation` control (`cancel`, `pause`, `finished`, `getTiming`, `getKeyframes`) — View Transitions / gestures
- `document.startViewTransition` — View Transitions API
- `requestAnimationFrame`, `setTimeout`/`clearTimeout`

## Type checks / interfaces referenced
`Node`, `Element`, `HTMLElement`, `SVGElement`, `HTMLIFrameElement` (`contentWindow`/`contentDocument`), `Document`, `DocumentFragment`, `Text`, `Comment`, `ShadowRoot`, `Selection`, `Range`, `Blob`, `AnimationTimeline`, `FormData` — via `instanceof`, `.prototype`, or `typeof` feature detection.

## Security integration
- **Trusted Types** — when `enableTrustedTypesIntegration` is on, React passes `TrustedHTML`/`TrustedScriptURL` values straight through to `innerHTML` and URL-bearing attributes instead of stringifying them.

## `scheduler` package (bundled with react-dom)
The cooperative scheduler uses host APIs for task yielding:
- `MessageChannel` + `port.postMessage`/`onmessage` (primary task loop)
- `setTimeout`/`setImmediate` (fallbacks)
- `requestAnimationFrame`
- `performance.now` (and `navigator.scheduling.isInputPending` in experimental builds)

## Notes
- **Source of truth:** core host ops in `react-dom-bindings/src/client/ReactFiberConfigDOM.js`; attributes in `client/DOMPropertyOperations.js` & `client/ReactDOMComponent.js`; styles in `client/CSSPropertyOperations.js`; text/HTML in `client/setTextContent.js` & `client/setInnerHTML.js`; selection/focus in `client/ReactInputSelection.js`, `client/ReactDOMSelection.js`, `client/getActiveElement.js`; events in `src/events/*` (esp. `DOMEventNames.js`, `DOMPluginEventSystem.js`, `EventListener.js`); server/hydration runtime in `src/server/ReactDOMServerExternalRuntime.js` and `src/server/fizz-instruction-set/*`.
- Compared to core preact, React's surface is far larger: it uses both `createElement` and `createElementNS`, a full synthetic event system with the entire native event-name table, the Selection/Range APIs, `MutationObserver`/`IntersectionObserver`/`ResizeObserver`, the Web Animations & View Transitions APIs, and Trusted Types integration.
