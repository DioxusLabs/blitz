# Blitz Core: A wgpu renderer for Everyone

Blitz Core is a native renderer for Dioxus that uses WGPU (via [Vello](https://github.com/linebender/vello)) to draw a DOM-like structure to the screen.

Blitz Core can be used without Dioxus as a regular CSS + HTML renderer. We try to maintain an API similar to the browser's DOM API for general compatibility.

Blitz Core elements relies on HTML, and Blitz Core events mimic HTML events. Blitz can be used as a partial replacement for the web rendering engine in modern browsers.

CSS is handled via [lightningcss](https://github.com/parcel-bundler/parcel-css) and layout is handled with [Taffy](https://github.com/DioxusLabs/taffy).

## Status

Blitz Core is in a very much WIP state right now. Lots of stuff works but even more doesn't.

- Many CSS properties aren't supported
- Many types of events aren't handled
- No support for images/videos or multimedia

That being said....

Please contribute! There's a lot of solid foundations here:

- Taffy is underpinning layout
- Vello is underpinning drawing

Blitz Core is for _everyone_, so you don't need Dioxus to drive updates to the final render tree.

## Architecture

Blitz Core is built on top of [native-core](https://github.com/DioxusLabs/dioxus/tree/master/packages/native-core) which handles the incremental updates to the tree state. It allows Blitz to incrementally propagate and compute styles when an attribute or element changes.

Blitz exposes a DOM-like tree to allow frameworks to change the UI. It uses plain elements and attributes to render the tree. Consumers just need to provide a way to update the tree, respond to events, and schedule updates.
