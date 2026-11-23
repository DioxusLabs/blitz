# AGENTS.md — parley_code_editor

Standalone code editing control (`floem-editor-core` + Parley + AnyRender); doesn't depend on `blitz-dom`. It may move out of this repo.

- Test: `cargo test -p parley_code_editor`
- Render to a PNG: `cargo run -p parley_code_editor --example screenshot [--wrap] [file] [out.png]`
- Open a window: `cargo run -p parley_code_editor --example editor [--cpu] [--wrap] [file]`
