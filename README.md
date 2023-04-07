# Blitz: A wgpu renderer for Dioxus

Blitz is a native renderer for Dioxus that uses WGPU (via [Vello](https://github.com/linebender/vello)) to draw the Dioxus VirtualDom to the screen.

[Blitz Core](https://github.com/DioxusLabs/blitz/tree/master/blitz-core) can be used without Dioxus as a regular CSS + HTML renderer. We try to maintain an API similar to the browser's DOM API for general compatibility.

Because the default Dioxus elements rely on HTML, so does Blitz, meaning Blitz can be used as a partial replacement for the web rendering engine in modern browsers.

CSS is handled via [lightningcss](https://github.com/parcel-bundler/parcel-css) and layout is handled with [Taffy](https://github.com/DioxusLabs/taffy).

## Status

Blitz is in a very much WIP state right now. Lots of stuff works but even more doesn't.

- Many CSS properties aren't supported
- Many types of events aren't handled
- No support for images/videos or multimedia

That being said....

Please contribute! There's a lot of solid foundations here:

- Taffy is underpinning layout
- Vello is underpinning drawing
- Dioxus is underpinning state management
