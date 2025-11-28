clippy:
  cargo +nightly clippy --workspace

fmt:
  cargo fmt --all

wpt target="css/css-flexbox css/css-grid css/css-align":
  cargo run --release --package wpt {{target}}

screenshot target:
  cargo run --release --example screenshot {{target}}

open target:
  cargo run --release --package readme --features log_phase_times {{target}} 

todomvc:
  cargo run --release --example todomvc