# AGENTS.md — Blitz

Blitz is a modular HTML/CSS rendering engine (Stylo for style, Taffy for layout, Parley for text, anyrender/Vello/Skia for paint) used by Dioxus Native. Rust workspace.

## Build, lint, test
Linux needs `build-essential pkg-config python3 libssl-dev libfontconfig-dev` (see CONTRIBUTING.MD).

Before pushing, run what CI runs:
- `cargo fmt --all --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo clippy -p blitz-dom --no-default-features --features svg,woff,accessibility,system-fonts,file-input,custom-widget --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo doc` and, if you touched `Cargo.toml`, `cargo deny --workspace --all-features --locked check licenses`
- If you touched `.github/scripts`: `python3 -m unittest discover -s .github/scripts`

Don't use `--all-features` for clippy/test: `rdme/avif` needs system dav1d >= 1.3 and `accesskit_xplat/tokio` conflicts with accesskit_unix. Pass an explicit feature list instead.

Headless machines: the default GPU renderer has no adapter. Prefer rendering to an image (`cargo run --release --example screenshot <file-or-url>`); for a window use the CPU renderer (`--no-default-features --features cpu-softbuffer` or the `cpu` just recipes).

## Web Platform Tests
WPT is the main correctness signal; results are deterministic.
- Don't add tests for features WPT already covers (most style, layout and rendering changes). The WPT results are the test.
- Clone https://github.com/web-platform-tests/wpt and check out the commit in `wpt/WPT_COMMIT`, then `WPT_DIR=<path> cargo run -rp wpt <test paths or globs>` (or `just wpt ...`). Arguments are paths/globs, not filename prefixes. Report and rendered PNGs land in `wpt/output/`.
- `RUST_LOG=warn` shows uncaught JS errors in testharness tests. A very low subtest count usually means the test failed before reporting.
- Tests that a change newly makes reach an unsupported feature and time out go in `wpt/runner/timeout-quarantine.txt` (`<path> <reason>`, reusing an existing reason) in the same PR. They're skipped unless `--run-quarantined`.
- CI posts a WPT diff into the PR description between `<!-- wpt-results-start -->` / `<!-- wpt-results-end -->`. Don't remove those markers when you edit a PR body.
- To check what browsers do: `https://wpt.fyi/api/search?q=<path>&label=master&label=stable&product=chrome&product=firefox&product=safari`.

## Engine gotchas
- Attribute changes only get layout damage from the computed-style diff. If box construction or layout reads an attribute directly (inline SVG source, `<col span>`, form attributes, ...), add it to the `read_by_layout` list in `restyle_for_attribute_change` (`mutator.rs`), or invalidation silently breaks. The `*-change`/`*-invalidation` reftests catch this.
- Facts about a node derived from its children belong in box-tree construction flags (`NodeFlags`, reset in `reset_construction_flags`), not recomputed at paint time.
- There's no CSS 2.1 block-in-inline split: an inline box containing blocks is laid out as a block. Inline `img`/`svg`/`input` still paint through `render_element`.
- `apps/browser` paints pages as sub-documents (`draw_sub_document` runs its own `paint_scene`). Any extra top-level paint pass also has to run per sub-document, or it works in examples but not in the browser.
- Generated form-control content must be layout-only anonymous content, not real DOM children (those are visible to selectors/scripts and break WPT).

## Pull requests
- Human-written PR descriptions are preferred. Write a short plain-prose explanation of what changed and why, the way a person would. Skip generated boilerplate, heading templates and file-by-file change lists.
- One feature per PR; follow-ups go in a new PR. Don't implement unrelated features to fix WPT losses. Note the losses in the PR description instead.
- Search existing PRs before starting on a missing feature (`gh pr list --state all --search <feature>`); there are often stacks already open.
- Fixes in Taffy/Parley/anyrender/Stylo go upstream in those repos, not patched in Blitz.
