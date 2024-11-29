# Blitz

**A [radically modular](https://github.com/DioxusLabs/blitz?tab=readme-ov-file#architecture) HTML/CSS rendering engine**


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


> Note: This repo contains a new version of Blitz (v0.2+) which uses Stylo. The source code for the old version (v0.1) is still available on the [legacy](https://github.com/DioxusLabs/blitz/tree/legacy) branch but is not under active development.


## Trying it out

### Prerequisites
#### Linux

You need xdo-tools installed, e.g. on Arch: https://man.archlinux.org/man/xdotool.1.en

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

Blitz consists of a core DOM abstraction; several modular pieces which provide additional functionality like networking, rendering, windows, and state management; and two high-level wrappers that support rendering either a Dioxus application or HTML.

These pieces can be combined together to make a cohesive web engine.

### Notable 3rd-party dependencies

Blitz builds upon the following libraries:

- [Stylo](https://github.com/servo/stylo) (Firefox's parallel browser-grade CSS engine) for CSS resolution
- [Vello](https://github.com/linebender/vello) + [WGPU](https://github.com/gfx-rs/wgpu) for rendering
- [Taffy](https://github.com/DioxusLabs/taffy) for box-level layout
- [Parley](https://github.com/linebender/parley) for text/inline-level layout
- [AccessKit](https://github.com/AccessKit/accesskit) for accessibility
- [Winit](https://github.com/rust-windowing/winit) for windowing and input handling

### 1st-party modules

#### Core crates

- **`blitz-traits`** - Minimal crate containing types and traits to allow the other crates to interoperate without depending on each other
- **`blitz-dom`** - The core DOM abstraction that includes style resolution and layout but not drawing/painting. Combines the best of Stylo and Taffy that allows you to build extendable dom-like structures. This crate currently also includes the `HtmlDocument` (an HTML parsing layer).

#### Optional Modules

- **`blitz-renderer-vello`** - Adds a Vello/WGPU based renderer to `blitz-dom`
- **`blitz-net`** -  Networking that can fetch resources over http, from the file-system or from encoded data URIs.
- **`blitz-html`** -  Adds HTML (and XHTML) parsing to `blitz-dom`
- **`blitz-shell`** - A shell that allows Blitz to render to a window (integrates a Winit event loop, AccessKit, Muda etc). This crate currently hardcodes `blitz-renderer-vello`, but is expected to be generic over renderers in future.

#### High-level Entry points
- **`blitz-shell`** - An HTML/markdown frontend that can render an HTML string. This is useful for previewing HTML and/or markdown files but currently lacks interactivity.
- **`dioxus-native`** - A Dioxus frontend that can render a Dioxus VirtualDom. This has full interactivity support via Dioxus's event handling.

## License

This project is dual licensed under the Apache 2.0 and MIT licenses.

The `stylo_taffy` crate is ADDITIONALLY licensed under MPL 2.0 (so it is triple licensed under Apache 2.0, MIT, and MPL 2.0 licenses) for easier interop with the Servo project.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Blitz by you, shall be dual licensed as Apache 2.0 and MIT (and MPL 2.0 if submitted to the `stylo_taffy` crate), without any additional terms or conditions.
