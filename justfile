check:
  cargo check --workspace

clippy:
  cargo +nightly clippy --workspace

fmt:
  cargo fmt --all

wpt *ARGS:
  cargo run --release --package wpt {{ARGS}}

screenshot *ARGS:
  cargo run --release --example screenshot {{ARGS}}

open *ARGS:
  cargo run --release --package readme {{ARGS}}

todomvc:
  cargo run --release --example todomvc

small:
  cargo build --profile small -p counter --no-default-features --features cpu_backend,system_fonts