
## Build/lint commands

check:
  cargo check --workspace

clippy:
  cargo clippy --workspace

fmt:
  cargo fmt --all

small:
  cargo build --profile small -p counter --no-default-features --features cpu,system_fonts

## WPT test runner

wpt *ARGS:
  cargo run --release --package wpt {{ARGS}}

## Browser

screenshot *ARGS:
  cargo run --release --example screenshot {{ARGS}}

open *ARGS:
  cargo run --release --package readme --features log_frame_times,log_phase_times {{ARGS}}

dev *ARGS:
  cargo run --package readme --features log_frame_times,log_phase_times {{ARGS}}

incr *ARGS:
  cargo run --release --package readme --features incremental,log_frame_times,log_phase_times {{ARGS}}

cpu *ARGS:
  cargo run --release --package readme --no-default-features --features cpu,comrak,incremental,log_frame_times,log_phase_times {{ARGS}}

hybrid *ARGS:
  cargo run --release --package readme --no-default-features --features hybrid,comrak,incremental,log_frame_times,log_phase_times {{ARGS}}

skia *ARGS:
  cargo run --release --package readme --no-default-features --features skia,comrak,incremental,log_frame_times,log_phase_times {{ARGS}}

skia-pixels *ARGS:
  cargo run --release --package readme --no-default-features --features skia-pixels,comrak,incremental,log_frame_times,log_phase_times {{ARGS}}

skia-softbuffer *ARGS:
  cargo run --release --package readme --no-default-features --features skia-softbuffer,comrak,incremental,log_frame_times,log_phase_times {{ARGS}}

## TodoMVC commands

todomvc *ARGS:
  cargo run --release --package todomvc {{ARGS}}

todoskia *ARGS:
  cargo run --release --package todomvc {{ARGS}} --no-default-features --features skia

## Ops

bump *ARGS:
  cargo run --release --package bump {{ARGS}}