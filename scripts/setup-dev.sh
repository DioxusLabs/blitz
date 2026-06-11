#!/usr/bin/env bash
set -euo pipefail

# Blitz development environment setup (Linux)
# Installs system dependencies and ensures the correct Rust toolchain.

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This script targets Linux. On Windows, install Python from the Microsoft Store (see CONTRIBUTING.MD)."
  exit 1
fi

echo "==> Installing Linux build dependencies..."
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
  libasound2-dev \
  libatk1.0-dev \
  libgtk-3-dev \
  libudev-dev \
  libpango1.0-dev \
  libxdo-dev \
  libssl-dev \
  pkg-config

echo "==> Ensuring Rust 1.89+ toolchain..."
if command -v rustup >/dev/null 2>&1; then
  rustup toolchain install 1.89 -c rustfmt -c clippy
  rustup default 1.89
else
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.89
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

echo "==> Verifying toolchain..."
rustc --version
cargo --version

echo "==> Building workspace..."
cargo build --workspace

echo ""
echo "Development environment is ready."
echo ""
echo "Quick verification:"
echo "  cargo test --workspace"
echo "  cargo run --example screenshot -- file:///path/to/local.html"
echo ""
echo "GUI applications:"
echo "  cargo run --release --package browser"
echo "  cargo run --release --package todomvc --bin todomvc_native"
echo "  cargo run --release --package rdme -- ./README.md"
