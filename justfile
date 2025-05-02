clippy:
  cargo +nightly clippy --workspace

fmt:
  cargo fmt --all

wpt target="css/css-flexbox css/css-grid css/css-align":
  cargo run --release --package wpt {{target}}

screenshot *ARGS:
  cargo run --release --example screenshot {{ARGS}}

open *ARGS:
  cargo run --release --package readme {{ARGS}}

todomvc:
  cargo run --release --example todomvc