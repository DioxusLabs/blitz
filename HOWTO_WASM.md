# Building Blitz Examples for WASM

Blitz runs in the browser on `wasm32-unknown-unknown` via [trunk]. The renderer is `anyrender_vello_hybrid` with the `webgl` feature; system fonts aren't available in the browser, so wasm examples bundle a font (DejaVu Sans) at compile time.

## Examples that target wasm

| Example       | Description                                                                |
| ------------- | -------------------------------------------------------------------------- |
| `wasm_hello`  | Minimal blitz-shell proof. Drives `BlitzApplication` against static HTML.  |
| `seven_guis`  | The [7GUIs](https://7guis.github.io/7guis/) benchmark, built with Dioxus. Also runs natively. |
| `todomvc`     | Classic TodoMVC, built with Dioxus. Also runs natively.                    |

## Prerequisites

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk
```

## Build / serve

From the repo root:

```sh
just wasm-build EXAMPLE     # build to examples/EXAMPLE/dist
just wasm-serve EXAMPLE     # build + serve with live reload
```

Or directly from the example directory:

```sh
trunk build --release
trunk serve  --release
```

The bundle uses relative paths (`--public-url ./`) so `dist/` can be served from any URL or opened over `file://`.

## Size optimisation

Each example's `index.html` sets `data-wasm-opt="z"` on the trunk rust link, which runs Binaryen's `-Oz` size pass over the produced `.wasm`.

## Wire size

Trunk doesn't compress its output. For real deploys, pre-compress the bundle so static hosts (and most CDNs) can serve `.br` directly:

```sh
brotli -q 11 -f dist/*.wasm dist/*.js
```

This typically cuts the over-the-wire size by 3–4×.

## Renderer selection

Examples that support multiple renderers (`seven_guis`, `todomvc`) pin the wasm build to `vello-hybrid` via trunk's `data-cargo-features="hybrid"` plus `data-cargo-no-default-features`. The native default (`vello`) stays untouched.