# Real-page layout benchmark: Barack Obama Wikipedia article, 1280 px

Headless parse + style + layout of `https://en.wikipedia.org/wiki/Barack_Obama` in Blitz,
measured with `examples/obama_bench.rs`, across several Parley revisions. This documents
the measurements behind Parley PRs
[#19](https://github.com/DioxusLabs/parley/pull/19) (cross-layout font cache) and
[#21](https://github.com/DioxusLabs/parley/pull/21) (`Line::items()` glyph-run iteration),
and the numbers for the Parley revision pinned by this branch.

## Reproducing

```sh
mkdir -p ~/bench/obama && cd ~/bench/obama
curl -L -o obama.html https://en.wikipedia.org/wiki/Barack_Obama
# Save the two `load.php` stylesheets referenced by <link rel=stylesheet> next to the HTML
# (e.g. style0.css / style1.css) and rewrite the <link href> attributes to those local names,
# so every run uses identical inputs and no network is involved.

cargo build --release --example obama_bench
/usr/bin/time -v target/release/examples/obama_bench ~/bench/obama/obama.html 20 3
```

The example:

- builds `HtmlDocument::from_html(html, DocumentConfig { base_url: file://…, net_provider: LocalFiles, viewport: 1280×800 @ scale 1, .. })`
  and calls `resolve(0.0)` twice (the `NetProvider` serves `file://` synchronously, so the
  stylesheets are applied in the first resolve; non-`file://` URLs — images, fonts — are dropped);
- **first layout** = wall time of `from_html` + the two `resolve()` calls, i.e. parse + style +
  layout from scratch on a fresh document, 3 warm-ups + 20 measured builds;
- **relayout** = wall time of `resolve(0.0)` after `set_viewport()` toggling the scale between
  `1.0` and `1.00001`, which invalidates every inline context (all text re-shaped and re-laid-out)
  and re-runs style resolution; 3 warm-ups + 20 measured;
- reports median / p90 over the 20 iterations and heap usage from a counting
  `#[global_allocator]`. Peak RSS comes from `/usr/bin/time -v`.

Both numbers are end-to-end Blitz timings (style + Taffy/inline layout + Blitz's `line.items()`
walk), not isolated Parley timings; `perf` profiles were used to attribute the differences.

All variants below: `--release`, default features of the root `blitz` crate, `rustc 1.94.0`,
Intel Xeon Platinum 8559C (8 vCPU), nothing else running, variants run in 3 interleaved rounds
with medians of the per-round medians reported. Run-to-run noise is about ±3%.

## 1. Parley `vertical-align` (#18) and font cache (#19)

| Variant | Blitz | Parley |
|---|---|---|
| A | `main` @ `18848608` | crates.io `parley 0.11.1` |
| B | PR #832 @ `a875dd47` | `ff838004` (Parley #18 head) |
| C | PR #832 @ `a875dd47`, repinned | `5c31f79b` (Parley #19 head) |
| D | `main` @ `18848608` + API shims | `83acf8f9` (Parley `main`, merge-base of #18) |

| Variant | first layout ms (median / p90) | relayout ms (median / p90) | peak RSS MB | live heap after layout MB | cache entries (primary fonts, metrics) |
|---|---:|---:|---:|---:|---|
| A  Blitz main, parley 0.11.1 | **269 / 282** | **147 / 148** | 172.8 | 77.5 | n/a |
| B  #832, parley #18 | 847 / 898 | 735 / 774 | 199.4 | 91.3 | n/a |
| C  #832, parley #19 | 1070 / 1149 | 959 / 1045 | 199.5 | 91.3 | 6 / 84 |
| D  main, parley main (no #18) | 1050 / 1129 | 931 / 990 | 186.9 | 84.7 | n/a |

Where the time goes (`perf`, `cpu-clock`, 4 builds + 4 relayouts per profile, ms):

| | A | B | C | D |
|---|---:|---:|---:|---:|
| total | 1 670 | 5 906 | 7 206 | 7 188 |
| `parley::layout::line::GlyphRunIter::next` (from `compute_inline_layout_inner`'s `for item in line.items()`) | 216 | 3 826 | 4 929 | 4 980 |
| of which `libc memcpy` (self) | — | 2 093 | 3 286 | 3 427 |
| `build_inline_layout_into` / `parley::builder::build_into_layout` | 448 | 524 | 484 | 517 |
| `fontique::Query::matches_with` (all callers) | 104 | 102 | 21 | 97 |
| `parley::layout::style_metrics::resolve_style_metrics` | — | 16.5 | 6.5 | — |
| `harfrust … shape` (sanity) | 112 | 111 | 103 | 110 |

Interpretation:

1. The 3–4× regression from A to B/C/D is neither #18 nor the cache; it is Parley `main` vs
   the `0.11.1` release and lives entirely in `GlyphRunIter::next`. `Line::items()` rebuilt
   `run.visual_clusters().flat_map(|c| c.glyphs()).skip(glyph_start)` on every `next()`
   (O(glyphs × glyph runs) per run, already so in 0.11.1), and after the `parley_engine`
   refactor each skipped element is a by-value `Cluster` plus a large composed glyph-iterator
   state — the `memcpy` is 35–46% of the whole run. Fixed by Parley #21 (section 2).
2. #18 itself is barely visible on this page: construction ~+1% (524 vs 517 ms over 8 layouts),
   `resolve_style_metrics` ≈ 2 ms per full page, +6.5 MB live heap.
3. #19 recovers what it targets: `fontique::Query::matches_with` −80%, construction −8%
   (~5 ms per full-page layout), with 6 primary-font + 84 metrics entries and a footprint below
   10 kB (heap identical to B within measurement).
4. B is ~20% faster end-to-end than C and D on the untouched `GlyphRunIter` path (codegen
   differences in the `flat_map().skip()` chain, most likely); treat that gap as noise made
   irrelevant by #21.

## 2. Parley #21: linear, cluster-free `Line::items()`

Blitz `main` (`18848608`, with the API-compat edits on this branch) with `[patch]` pointing at
each Parley worktree:

| Variant | Parley | first layout ms (median / p90) | relayout ms (median / p90) | peak RSS MB |
|---|---|---:|---:|---:|
| base | `main` (`83acf8f9`) | 1051 / 1109 | 929 / 986 | 186.8 |
| cursor only | `28ea7e7` (resumable cursor instead of `skip`) | 473 / 510 | 352 / 371 | 186.7 |
| slice only | scratch: `5a2c020` with the cursor disabled | 303 / 328 | 183 / 194 | 186.7 |
| both | `5a2c020` (#21 head, pinned by this branch) | **263 / 282** | **151 / 172** | 186.8 |

Per-round relayout medians: base 929/926/946, cursor 352/348/353, slice 185/181/183,
both 144/151/156. The cursor removes the quadratic term; walking the run's `ShapedSlice`
atoms directly removes the per-element `Cluster`/iterator materialisation. Together they
restore Parley 0.11.1's cost for the renderer's item walk (147 ms relayout on the same machine),
so this branch is on par with Blitz `main` + `parley 0.11.1` while running Parley `main`.

`parley_bench` gained a `Styled Items` benchmark for this path (none of the existing benchmarks
executed `GlyphRunIter::next`); Tango there reports −92…−94% for both fixes combined.

A follow-up variant with the LTR/RTL direction monomorphised out of the atom cursor's inner
loop (instead of a branch on a loop-invariant `is_rtl`) measured within noise: relayout
136.2/136.6/138.2 ms (branch) vs 135.0/134.9/135.4 ms (monomorphised), and 0–4% *slower* on
Tango `Styled Items`, so #21 keeps the simpler branch.

## Caveats

- Images and web fonts are not loaded (only the HTML and its two stylesheets), so text uses the
  system fallback fonts of the benchmark VM.
- Blitz's relayout re-shapes all text (no incremental relayout keeps shaped text), so
  "relayout" ≈ "layout without parsing".
- The `NEG_INFINITY` ascent/descent passed to `append_inline_box_to_line` for floats keeps
  out-of-flow boxes from contributing to line height, matching the previous `0.0` box height.
