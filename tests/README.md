# Integration tests

This directory hosts workspace-member test crates that exercise individual Blitz packages from the outside.

## Layout

- `blitz-net/` — public-API regression tests for `packages/blitz-net`. Uses `wiremock` to stand up in-process HTTP servers; asserts on `Provider` behavior, scheme dispatch, body encoding, abort signals, per-host concurrency limits, and feature-gated paths (`cookies`, `cache`, `multipart`).
- `stylo_usage.rs` — historical example file; not wired into a crate.

`cargo test --workspace` runs the default-feature tests. Feature-gated tests need explicit flags (see the crate's `Cargo.toml`).

## Why these tests exist alongside WPT

Blitz already has a large test surface in `wpt/runner/` — the [web-platform-tests](https://github.com/web-platform-tests/wpt) conformance suite. The crates here cover what WPT can't, for two reasons:

**WPT doesn't use `blitz-net::Provider`.** The WPT runner ships its own `NetProvider` impl (`wpt/runner/src/net_provider.rs`) that resolves URLs against a local checkout of the WPT git repo. No HTTP, no `reqwest`, no semaphore, no cookies. Every behavior worth regressing on in `blitz-net` — per-host limiting, User-Agent injection, status-code mapping, cache middleware, multipart bodies, abort signals — is bypassed by the runner. Wiring `blitz-net::Provider` into WPT would require running a `wptserve` instance and adapting the runner's loader, which is a much larger project than testing the crate directly.

**WPT's assertion model is shaped for renderer conformance, not API regression.** WPT compares rendered bitmaps against reference images via `dify` pixel-diffs. The blitz-net tests need structural assertions on Rust values — `matches!(err, ProviderError::HttpStatus { status, .. })`, `provider.count() == 1`, `received_requests().len() == 6` — that have no natural expression in a visual-diff framework.

The two suites are complementary: WPT answers "does Blitz render this HTML/CSS correctly per spec?"; the crates here answer "does this package's public API still behave as documented?"

## Running the tests

```sh
# Default-feature tests (also covered by `cargo test --workspace`)
cargo test -p blitz-net-tests

# Feature-gated tests — `cookies,cache` and `cookies,multipart` are run separately
# because the `cache` and `multipart` features are currently incompatible in blitz-net.
cargo test -p blitz-net-tests --features cookies,cache
cargo test -p blitz-net-tests --features cookies,multipart

# Single test
cargo test -p blitz-net-tests injects_user_agent_header

# Single test file
cargo test -p blitz-net-tests --test concurrency
```

