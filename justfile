
## Build/lint commands

check:
  cargo check --workspace

clippy:
  cargo clippy --workspace

fmt:
  cargo fmt --all

small:
  cargo build --profile small -p counter --no-default-features --features cpu,system-fonts

## WPT test runner

wpt *ARGS:
  cargo run --release --package wpt {{ARGS}}

browser *ARGS:
  cargo run --release --package browser --features log-frame-times,log-phase-times {{ARGS}}

browser-with-perf:
  cargo run --release --package browser --features log-frame-times,log-phase-times

browskia:
  cargo run -rp browser --no-default-features --features skia,floats,incremental,cookies,cache,log-frame-times,log-phase-times

## Browser

screenshot *ARGS:
  cargo run --release --example screenshot {{ARGS}}

open *ARGS:
  cargo run --release --package rdme --features log-frame-times,log-phase-times {{ARGS}}

openskia *ARGS:
  cargo run --release --package rdme --no-default-features --features skia,comrak,floats,incremental,log-frame-times,log-phase-times {{ARGS}}

opencpu *ARGS:
  cargo run --release --package rdme --no-default-features --features cpu,comrak,floats,incremental,log-frame-times,log-phase-times {{ARGS}}

dev *ARGS:
  cargo run --package rdme --features log-frame-times,log-phase-times {{ARGS}}

incr *ARGS:
  cargo run --release --package rdme --features incremental,comrak,floats,log-frame-times,log-phase-times {{ARGS}}

cpu *ARGS:
  cargo run --release --package rdme --no-default-features --features cpu,comrak,incremental,floats,log-frame-times,log-phase-times {{ARGS}}

hybrid *ARGS:
  cargo run --release --package rdme --no-default-features --features hybrid,comrak,incremental,floats,log-frame-times,log-phase-times {{ARGS}}

skia *ARGS:
  cargo run --release --package rdme --no-default-features --features skia,comrak,incremental,floats,log-frame-times,log-phase-times {{ARGS}}

skia-pixels *ARGS:
  cargo run --release --package rdme --no-default-features --features skia-pixels,comrak,floats,incremental,log-frame-times,log-phase-times {{ARGS}}

skia-softbuffer *ARGS:
  cargo run --release --package rdme --no-default-features --features skia-softbuffer,comrak,floats,incremental,log-frame-times,log-phase-times {{ARGS}}

## 7GUIs

seven_guis *ARGS:
  cargo run --release --package seven_guis --bin seven_guis_native {{ARGS}}

## TodoMVC commands

todomvc *ARGS:
  cargo run --release --package todomvc --bin todomvc_native {{ARGS}}

todoskia *ARGS:
  cargo run --release --package todomvc --bin todomvc_native {{ARGS}} --no-default-features --features skia

todoandroid *ARGS:
  export CARGO_APK_RELEASE_KEYSTORE="$HOME/.android/debug.keystore"
  export CARGO_APK_RELEASE_KEYSTORE_PASSWORD="android"
  cargo apk run --lib --no-default-features --features skia -p todomvc

counterandroid *ARGS:
  export CARGO_APK_RELEASE_KEYSTORE="$HOME/.android/debug.keystore"
  export CARGO_APK_RELEASE_KEYSTORE_PASSWORD="android"
  cargo apk run --lib --no-default-features --features skia -p counter

## WASM

wasm-build APP *ARGS:
  cd examples/{{APP}} && trunk build --release --public-url ./ {{ARGS}}

wasm-serve APP *ARGS:
  cd examples/{{APP}} && trunk serve --release --public-url ./ {{ARGS}}

## Ops

bump *ARGS:
  cargo run --release --package bump {{ARGS}}