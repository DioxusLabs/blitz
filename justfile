clippy:
  cargo +nightly clippy --workspace

fmt:
  cargo fmt --all

wpt target="css/css-flexbox css/css-grid css/css-align":
  cargo run --release --package wpt {{target}}

screenshot target:
  cargo run --release --example screenshot {{target}}

open target:
  cargo run --release --package readme --features log_phase_times,log_frame_times {{target}} 

build:
  cargo build --release --package readme --features log_phase_times,log_frame_times
  @mv ./target/release/readme ./target/release/blitz-baseline
  @echo "Binary built at ./target/release/blitz-baseline"
  @echo "Run './target/release/blitz-baseline <urL>' to run or copy it to convenient location."

todomvc:
  cargo run --release --example todomvc