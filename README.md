<p>
<picture >
  <source media="(prefers-color-scheme: dark)" srcset="https://blitz-website.fly.dev/static/blitz-logo-with-text3-white.svg">
  <img height="70" alt="Blitz" src="https://blitz-website.fly.dev/static/blitz-logo-with-text3.svg">
</picture>
</p>

**A modular HTML/CSS rendering engine**

[![Build Status](https://github.com/dioxuslabs/blitz/actions/workflows/ci.yml/badge.svg)](https://github.com/dioxuslabs/blitz/actions)
[![Crates.io](https://img.shields.io/crates/v/blitz.svg)](https://crates.io/crates/blitz)
[![Docs](https://docs.rs/blitz/badge.svg)](https://docs.rs/blitz)
[![Crates.io License](https://img.shields.io/crates/l/blitz)](#license)
[![dependency status](https://deps.rs/repo/github/dioxuslabs/blitz/status.svg)](https://deps.rs/repo/github/dioxuslabs/blitz)

Talk to us in: the [#native](https://discord.gg/AnNPqT95pu) channel in the [Dioxus Discord](https://discord.gg/AnNPqT95pu)

## Status

Blitz is currently in a **beta** state. It can already render many popular no-JS websites (Wikipedia, (Old) Reddit, etc), and is usable for making apps if you are an early adopter and willing to live on the bleeding edge. But there are also still many bugs and missing features. We are actively working on bringing it up to production quality.

Check out:
- The [downloads page](https://blitz.is/downloads) for runnable builds of the Browser UI (Windows / macOS / Linux / Android)
- The [status page](https://blitz.is/status/css) for current status
- The [roadmap issue](https://github.com/DioxusLabs/blitz/issues/119) for planned work


![screenshot](https://raw.githubusercontent.com/DioxusLabs/screenshots/main/blitz/counter-example.png)


## Trying it out

1. Clone this repo
2. Run one of our examples:
    - The Browser UI: `cargo run -rp browser`
    - The Markdown viewer: `cargo run -rp rdme ./README.md`
    - TodoMVC app: `cargo run -rp todomvc`
    - WGPU texture integration example: `cargo run -rp wgpu_texture`

Other examples are available in the [examples/](./examples/) folder.

## Goals

Blitz is designed to render HTML and CSS - we *don't* want to support the entirety of browser features (or at least we want to make all such "extra" features opt-in). In our opinion, the browser is bloated for the basic use case of rendering HTML/CSS.

We do intend to support:

- Modern HTML layout (flexbox, grid, table, block, inline, absolute/fixed, etc).
- Advanced CSS (complex selectors, media queries, css variables)
- HTML Form controls
- Accessibility using AccessKit
- Extensibility via custom widgets

Notably we *don't* provide features like webrtc, websockets, bluetooth, localstorage, etc. In a native app, much of this functionality can be fulfilled using regular Rust crates and doesn't need to be coupled with the renderer.

We don't yet have Blitz bindings for other languages (JavaScript, Python, etc) but would accept contributions along those lines.

## Architecture

Blitz consists of a core DOM abstraction; several modular pieces which provide additional functionality like networking, rendering, windows, and state management; and two high-level wrappers that support rendering either a Dioxus application or HTML with a simplified API.

These pieces can be combined together to make a cohesive web engine.

### High-level "wrapper" crates

- **`blitz`** - An HTML/markdown frontend that can render an HTML string. This is useful for previewing HTML and/or markdown files but currently lacks interactivity.
<br /><small><b>Uses: `blitz-dom`, `blitz-html`, `blitz-shell`, `blitz-renderer-vello`</b></small>
- **`dioxus-native`** - A Dioxus frontend that can render a Dioxus VirtualDom. This has full interactivity support via Dioxus's event handling.
<br /><small><b>Uses: `blitz-dom`, `dioxus-core`, `blitz-shell`, `blitz-renderer-vello`</b></small>

Both wrappers can optionally use <b>`blitz-net`</b> to fetch sub-resources.


### Using the git verison of Dioxus Native

The latest development version of the Dioxus Native lives in this repository. As Dioxus Native is under rapid development it can be useful to use this version to get access to the latest features and bug fixes sooner than they are available in an official release.

To use the git version of `dioxus-native`:

- Remove your dependency on the `dioxus` crate entirely.
- Add `dioxus-native = { git = "https://github.com/DioxusLabs/blitz", rev = "e64a3d8", features = ["prelude"] }`
- (replace `e64a3d8` with the git commit id of the version you want to use)
- In your rust code change all instances of `use dioxus::prelude::*` to `use dioxus_native::prelude::*`.
- If you need to access additonal functionality from the `dioxus` crate that is not exported from the Dioxus Native prelude then you can import it from the individual sub-crates (`dioxus-html`, `dioxus-signals`, `dioxus-router`, etc) instead.

The git versions of Dioxus Native still depend on the stable v0.7.x version of Dioxus from crates.io, so any additional libraries that you are using (`dioxus-sdk`, `dioxus-components`, `dioxus-free-icons`, etc) should still work.

### Modular Components

#### Core crates

- **`blitz-dom`** - The core DOM abstraction that includes style resolution, layout and event handling (but not parsing, rendering or system integration).
<br /><small><b>Uses: [Stylo](https://github.com/servo/stylo) (CSS parsing/resolution), [Taffy](https://github.com/DioxusLabs/taffy) (box-level layout), [Parley](https://github.com/linebender/parley) (text layout)</b></small>
- **`blitz-traits`** - Minimal base crate containing types and traits to allow the other crates to interoperate without depending on each other

#### Additional crates

- **`blitz-net`** -  Networking that can fetch resources over http, from the file-system or from encoded data URIs.
<br /><small><b>Uses: [reqwest](https://github.com/seanmonstar/reqwest) (HTTP client)</b></small>
- **`blitz-paint`** - Translates a `blitz-dom` tree into `anyrender` draw commands.
<br /><small><b>Uses: [anyrender](https://github.com/dioxuslabs/anyrender) (2D drawing abstraction)</b></small>
- **`blitz-html`** -  Adds HTML parsing to `blitz-dom`
<br /><small><b>Uses: [html5ever](https://github.com/servo/html5ever) (HTML parsing) and [xml5ever](https://github.com/servo/html5ever/tree/main/xml5ever) (XHTML parsing)</b></small>
- **`blitz-shell`** - A shell that allows Blitz to render to a window (integrates a Winit event loop, AccessKit, Muda etc).
<br /><small><b>Uses: [winit](https://github.com/rust-windowing/winit) (windowing/input), [accesskit](https://github.com/AccessKit/accesskit) (accessibility), [muda](https://github.com/tauri-apps/muda) (system menus)</b></small>

The AnyRender rendering abstraction now lives in it's repository over at https://github.com/dioxuslabs/anyrender

## License

This project is dual licensed under the Apache 2.0 and MIT licenses.

The `stylo_taffy` crate is ADDITIONALLY licensed under MPL 2.0 (so it is triple licensed under Apache 2.0, MIT, and MPL 2.0 licenses) for easier interop with the Servo project.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Blitz by you, shall be dual licensed as Apache 2.0 and MIT (and MPL 2.0 if submitted to the `stylo_taffy` crate), without any additional terms or conditions.
