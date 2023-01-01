# Blitz: A wgpu renderer for Dioxus

Blitz is a native renderer for Dioxus that uses WGPU (via [Vello](https://github.com/linebender/vello)) to draw the Dioxus virtualdom to the screen.

Blitz can be used without Dioxus as a regular CSS + HTML renderer. We try to maintain an API similar to the browser's DOM API for general compatibility.

Because the default Dioxus element set relies on HTML, so does Blitz, meaning Blitz can be used as a partial replacement for the web rendering engine in modern browsers.

CSS is handled via [lightningcss](https://github.com/parcel-bundler/parcel-css) and layout is handled with [Taffy](https://github.com/DioxusLabs/taffy).

## Status

Blitz is in a very much WIP state right now. Lots of stuff works but even more doesn't. 

- CSS doesn't cascade
- Many types of events aren't handled
- No support for images/videos or multimedia

That being said....

Please contribute! There's a lot of solid foundations here:

- Taffy is underpinning layout
- Vello is underpinning drawing
- Dioxus is underpinning state management

Blitz is for *everyone*, so you don't need Dioxus to drive updates to the final render tree.

### MacOs support

There is a known issue with the wgpu on MacOS. If you're on MacOS, you'll need to add the following patch to your `Cargo.toml` to make Blitz run:

```toml
[patch.crates-io]
naga = { git = "https://github.com/gfx-rs/naga", rev = "ddcd5d3121150b2b1beee6e54e9125ff31aaa9a2" }
```