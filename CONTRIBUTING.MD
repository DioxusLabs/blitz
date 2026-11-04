# Contributing to Blitz

Welcome to the Dioxus community!
Blitz is a "native" HTML/CSS renderer built to support the "Dioxus Native" project. It is effectively a lightweight webview except that the JavaScript engine is replaced with a native Rust API which allows Rust reactivity / state management libraries like Dioxus to interface with it directly.

Talk to us in: the #native channel in the [Dioxus Discord](https://discord.gg/BWTrn6d3)

## Dependency licenses

CI checks dependency licenses with `cargo deny --workspace --all-features --locked
check licenses`. See [the licensing review](docs/licensing.md) for the allowlist
policy and commercial-distribution obligations, including MPL-2.0 dependencies.

## Prerequisites

All platforms require Rust, Python 3 and a C toolchain.

Rust can be installed via https://rustup.rs

### macOS

Python is pre-installed. We recommend using the C toolchaing from the Xcode Command Line tools.

### Windows

Usually requires installing Python. We recommend installing using https://docs.astral.sh/uv/, but any Python 3 should work.

### Linux

To **build** Blitz you need a C toolchain plus a few dependencies:

 * `pkg-config` and a C compiler (`build-essential`)
 * `python3` — used to generate code while building `stylo`
 * `libssl-dev` (OpenSSL) — required by `reqwest` via `blitz-net`
 * `libfontconfig-dev` (fontconfig) — required by `parley`/`fontique` for system-font enumeration

For example on Ubuntu:

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config python3 libssl-dev libfontconfig-dev
```

To **run** Blitz as windowed application you additionally need X11 and/or Wayland. These are usually already
present on a desktop Linux install, but may need to be installed in server setups or if using a system like
Nix that requires explicit dependencies. needed there.

For example on Ubuntu:

```sh
sudo apt-get update
sudo apt-get install libfontconfig-dev libx11-dev libxcb1-dev libwayland-dev
```

The optional **file dialog feature** uses [`rfd`](https://crates.io/crates/rfd)
with its `xdg-portal` backend. So one of the following must be available for
a dialog to actually open:

 * a running `xdg-desktop-portal` service **plus** a backend for your desktop
   environment (e.g. `xdg-desktop-portal-gtk`, `-kde`, `-gnome`, or `-wlr`)
 * the `zenity` executable, which `rfd` falls back to.


### VSCode
You can add the following JSON to your `.vscode/settings.json` to automically build Blitz on all supported targets.
```json
{
    "rust-analyzer.check.features": "all",
    "rust-analyzer.cargo.features": "all",
    "rust-analyzer.check.allTargets": true,
    "rust-analyzer.cargo.allTargets": true
}
```
