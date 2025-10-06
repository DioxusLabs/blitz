check:
  cargo check --workspace

clippy:
  cargo clippy --workspace

fmt:
  cargo fmt --all

wpt *ARGS:
  cargo run --release --package wpt {{ARGS}}

screenshot *ARGS:
  cargo run --release --example screenshot {{ARGS}}

open *ARGS:
  cargo run --release --package readme --features log_frame_times,log_phase_times {{ARGS}}

incr *ARGS:
  cargo run --release --package readme --features incremental,log_frame_times,log_phase_times {{ARGS}}

cpu *ARGS:
  cargo run --release --package readme --no-default-features --features cpu,comrak,incremental,log_frame_times,log_phase_times {{ARGS}}

bump *ARGS:
  cargo run --release --package bump {{ARGS}}

todomvc *ARGS:
  cargo run --release --package todomvc {{ARGS}}

small:
  cargo build --profile small -p counter --no-default-features --features cpu,system_fonts