# Blitz: A wgpu renderer for Dioxus

Blitz is a native renderer for Dioxus that uses WGPU to draw the Dioxus virtualdom to the screen.

Because the default Dioxus element set relies on HTML, so does Blitz, meaning Blitz can be used as a partial replacement for the web rendering engine in modern browsers.

CSS is handled via [ParcelCSS](https://github.com/parcel-bundler/parcel-css) and layout is handled with [Taffy](https://github.com/DioxusLabs/taffy).

## Extending Blitz

One project that extends Blitz is SciViz: a high-performance plotting toolkit for Rust and Python. It uses Dioxus to draw the UI and extends the regular Dioxus syntax with a custom set of components and elements.

