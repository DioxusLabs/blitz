# Blitz

**A modular HTML/CSS renderer with a native Rust API**


[![Discord](https://img.shields.io/discord/899851952891002890.svg?logo=discord&style=flat-square&label=discord)](https://discord.gg/XgGxMSkvUM)
[![dependency status](https://deps.rs/repo/github/dioxuslabs/blitz/status.svg)](https://deps.rs/repo/github/dioxuslabs/blitz)
![Crates.io License](https://img.shields.io/crates/l/blitz)
[![Build Status](https://github.com/dioxuslabs/blitz/actions/workflows/ci.yml/badge.svg)](https://github.com/dioxuslabs/blitz/actions)
[![Crates.io](https://img.shields.io/crates/v/blitz.svg)](https://crates.io/crates/blitz)
[![Docs](https://docs.rs/blitz/badge.svg)](https://docs.rs/blitz)

Talk to us in: the [#native](https://discord.gg/AnNPqT95pu) channel in the [Dioxus Discord](https://discord.gg/AnNPqT95pu)

## Status

Blitz is currently in a **pre-alpha** state. It already has a very capable renderer, but there are also still many bugs and missing features. We are actively working on bringing into a usable state but we would not yet recommend building apps with it.

Check out the [roadmap issue](https://github.com/DioxusLabs/blitz/issues/119) for more details. 

## Screenshot

![screenshot](https://raw.githubusercontent.com/DioxusLabs/screenshots/main/blitz/counter-example.png)


## Blitz builds upon:

- [Stylo](https://github.com/servo/stylo) (Firefox's parallel browser-grade CSS engine) for CSS resolution
- [Vello](https://github.com/linebender/vello) + [WGPU](https://github.com/gfx-rs/wgpu) for rendering
- [Taffy](https://github.com/DioxusLabs/taffy) for box-level layout
- [Parley](https://github.com/linebender/parley) for text/inline-level layout
- [AccessKit](https://github.com/AccessKit/accesskit) for accessibility
- [Winit](https://github.com/rust-windowing/winit) for windowing and input handling

> Note: This repo contains a new version of Blitz (v0.2+) which uses Stylo. The source code for the old version (v0.1) is still available on the [legacy](https://github.com/DioxusLabs/blitz/tree/legacy) branch but is not under active development.


## Trying it out

1. Clone this repo
2. Run an example:
    - `cargo run --release --example todomvc`
    - `cargo run --release --example google`
3. Or our "browser" package:
    - `cargo run --release --package readme ./README.md`
    - `cargo run --release --package readme https://news.ycombinator.com`

Other examples available.

## Goals

Blitz is designed to render HTML and CSS - we *don't* want to support the entirety of browser features (or at least we want to make all such "extra" features opt-in). In our opinion, the browser is bloated for the basic usecase of rendering HTML/CSS.

We do intend to support:

- Modern HTML layout (flexbox, grid, table, block, inline, absolute/fixed, etc).
- Advanced CSS (complex selectors, media queries, css variables)
- HTML Form controls
- Accessibility using AccessKit
- Extensibility via custom widgets

Notably we *don't* provide features like webrtc, websockets, bluetooth, localstorage, etc. In a native app, much of this functionality can be fulfilled using regular Rust crates and doesn't need to be coupled with the renderer.

We don't yet have Blitz bindings for other languages (JavaScript, Python, etc) but would accept contributions along those lines.

## Architecture

Blitz consists of a core DOM abstraction (`blitz-dom`), and several modular pieces which provide functionality like networking, rendering, windows, and state management. These pieces can be combined together to make a cohesive web engine.

### Entry points
- An HTML/markdown frontend that can render an HTML string. This is useful for previewing HTML and/or markdown files but currently lacks interactivity.
- A Dioxus frontend that can render a Dioxus VirtualDom. This has full interactivity support via Dioxus's event handling.

### Crates

**Core:**

- `blitz-traits`: Minimal crate containing types and traits to allow the other crates to interoperate without depending on each other
- `blitz-dom`: The core DOM abstraction that includes style resolution and layout but not drawing/painting. Combines the best of Stylo and Taffy that allows you to build extendable dom-like structures.

**Modules**:
- `blitz-renderer-vello`: Adds a Vello/WGPU based renderer to `blitz-dom`
- `blitz-net`: Networking that can fetch resources over http, from the file-system or from encoded data URIs.
- `dioxus-native`: This crate should contain just a dioxus integration layer for Blitz. However, it currently contains all of the following functionality:
  - `DioxusDocument` - A dioxus integration layer for Blitz
  - `HtmlDocument` - An HTML rendering layer for Blitz
  - `Window` - A winit-based "shell" for running Blitz applications in a window.

  These different parts will be split into separate crates in future.


## License

This project is dual licensed under the Apache 2.0 and MIT licenses.

The `stylo_taffy` crate is ADDITIONALLY licensed under MPL 2.0 (so it is triple licensed under Apache 2.0, MIT, and MPL 2.0 licenses) for easier interop with the Servo project.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Blitz by you, shall be dual licensed as Apache 2.0 and MIT (and MPL 2.0 if submitted to the `stylo_taffy` crate), without any additional terms or conditions.
