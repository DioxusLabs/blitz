# DOM APIs used by the core preact library

AI-generated analysis of the vendored core preact library (`vendor/preact.min.js`):

Since minifiers don't rename properties accessed on native DOM objects, the API names
are preserved in the minified source and can be enumerated reliably.

## Document / node creation
- `document` — global, checked as the default render root
- `document.documentElement` — substituted when the render container is `document`
- `document.createElementNS(namespace, type, options)` — the **only** element-creation call (preact never uses `createElement`). The `options` arg carries `{ is }` for customized built-ins.
- `document.createTextNode(text)` — text node creation

Namespaces passed to `createElementNS`:
- `http://www.w3.org/1999/xhtml` (default HTML)
- `http://www.w3.org/2000/svg` (for `<svg>`)
- `http://www.w3.org/1998/Math/MathML` (for `<math>`)
- inherited via `node.namespaceURI` for descendants

## Tree traversal & mutation (`Node`)
- `node.parentNode` (read)
- `node.parentNode.removeChild(child)` — node removal (preact does **not** use `node.remove()`)
- `parent.insertBefore(node, referenceNode)` — insertion/moving
- `node.firstChild` (read)
- `node.childNodes` (read + `.length`, iterated) — child diffing / hydration
- `node.nextSibling` (read) — sibling walking, skipping comment nodes
- `node.nodeType` (read) — compared against `3` (Text) and `8` (Comment)

## Attributes & properties (`Element`)
- `element.setAttribute(name, value)`
- `element.removeAttribute(name)`
- `element[prop] = value` — direct property assignment when `prop in element` (e.g. `className`, `id`, `value`, `checked`, most DOM IDL props)
- `element.attributes` (collection) — iterated during hydration; reads `.length`, `attr.name`, `attr.value`
- `"setAttribute" in node` — feature test to distinguish element vs. text nodes during hydration
- `element.localName` (read) — element-type matching during hydration

## Text content (`CharacterData`)
- `textNode.data` (read/write) — updating text nodes

## Styles (`CSSStyleDeclaration`)
- `element.style` (read)
- `element.style.cssText` (write) — string styles / clearing styles
- `element.style.setProperty(name, value)` — for custom properties (`--*`)
- `element.style[prop] = value` — individual style properties

## HTML content
- `element.innerHTML` (read/write) — `dangerouslySetInnerHTML`
- `templateElement.content` (`HTMLTemplateElement.content`) — rendering into `<template>`

## Events (`EventTarget` / `Event`)
- `element.addEventListener(type, listener, useCapture)`
- `element.removeEventListener(type, listener, useCapture)`
- `event.type` (read) — used inside preact's shared event-proxy dispatcher

## Form-element properties (set as IDL properties, feature-detected)
- `value` / `defaultValue` (`input`, `textarea`, `progress`, `option`)
- `checked` / `defaultChecked` (`input`)

## Notes
- **Not used by core preact:** `createElement`, `createComment`, `createDocumentFragment`, `textContent`, `getAttribute`, `setAttributeNS`/`removeAttributeNS`, `node.remove()`, `classList`, `cloneNode`. (Legacy `xlink:*`/SVG attribute quirks are handled by rewriting the attribute *name* and calling the regular `setAttribute`, not via NS variants.)
- **preact/hooks** (`vendor/hooks.umd.js`) touches no DOM element APIs — it only uses the timing globals `requestAnimationFrame`, `setTimeout`, and `clearTimeout`.
- For driving DOM support in an engine, the two easiest things to overlook are the `createElementNS`-only element creation and the direct `element[prop] = value` fallback path (with `prop in element` detection).
