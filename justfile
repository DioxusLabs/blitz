
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
  cargo run --release --package wpt -- {{ARGS}}

# resvg's test suite at wpt/RESVG_COMMIT, wrapped as reftests (see wpt/runner/src/resvg.rs).
# Prefix with BLITZ_NATIVE_SVG=1 to render inline SVG with the native backend, and with
# RESVG_PNG_FUZZY=<threshold>;<pixels> to compare against resvg's PNGs at another tolerance.
resvg-suite *ARGS="resvg":
  [ "$(git -C wpt/resvg/checkout rev-parse HEAD 2>/dev/null)" = "$(cat wpt/RESVG_COMMIT)" ] || ( \
    git init -q wpt/resvg/checkout && \
    git -C wpt/resvg/checkout fetch -q --depth 1 https://github.com/linebender/resvg "$(cat wpt/RESVG_COMMIT)" && \
    git -C wpt/resvg/checkout checkout -q FETCH_HEAD )
  cargo run --release --package wpt -- resvg-generate wpt/resvg/checkout wpt/resvg/out
  WPT_DIR=wpt/resvg/out WPT_OUTPUT_DIR="$PWD/wpt/resvg/output" FONTCONFIG_FILE="$PWD/wpt/resvg/out/resvg/fonts.conf" cargo run --release --package wpt -- {{ARGS}}
  WPT_OUTPUT_DIR="$PWD/wpt/resvg/output" cargo run --release --package wpt -- resvg-summary

browser *ARGS:
  cargo run --release --package browser --features log-frame-times,log-phase-times {{ARGS}}

browser-with-perf:
  cargo run --release --package browser --features log-frame-times,log-phase-times

browskia:
  cargo run -rp browser --no-default-features --features skia,cookies,cache,log-frame-times,log-phase-times

## Browser

screenshot *ARGS:
  cargo run --release --example screenshot {{ARGS}}

open *ARGS:
  cargo run --release --package rdme --features log-frame-times,log-phase-times {{ARGS}}

openskia *ARGS:
  cargo run --release --package rdme --no-default-features --features skia,comrak,floats,log-frame-times,log-phase-times {{ARGS}}

opencpu *ARGS:
  cargo run --release --package rdme --no-default-features --features cpu,comrak,floats,log-frame-times,log-phase-times {{ARGS}}

dev *ARGS:
  cargo run --package rdme --features log-frame-times,log-phase-times {{ARGS}}

cpu *ARGS:
  cargo run --release --package rdme --no-default-features --features cpu,comrak,floats,log-frame-times,log-phase-times {{ARGS}}

hybrid *ARGS:
  cargo run --release --package rdme --no-default-features --features hybrid,comrak,floats,log-frame-times,log-phase-times {{ARGS}}

skia *ARGS:
  cargo run --release --package rdme --no-default-features --features skia,comrak,floats,log-frame-times,log-phase-times {{ARGS}}

skia-pixels *ARGS:
  cargo run --release --package rdme --no-default-features --features skia-pixels,comrak,floats,log-frame-times,log-phase-times {{ARGS}}

skia-softbuffer *ARGS:
  cargo run --release --package rdme --no-default-features --features skia-softbuffer,comrak,floats,log-frame-times,log-phase-times {{ARGS}}

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