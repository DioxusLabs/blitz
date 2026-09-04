# Running WPT tests against Parley

Blitz uses [Parley](https://github.com/linebender/parley) for all of its text
layout. Its [Web Platform Tests](https://github.com/web-platform-tests/wpt)
(WPT) runner makes it possible to test Parley against thousands of real-world
text layout tests covering line breaking, shaping, bidirectional text, font
fallback, and font selection.

This document explains how to run the Blitz WPT runner against a local Parley
checkout.

## Prerequisites

1. **A clone of Blitz** (this repository):
   ```sh
   git clone https://github.com/DioxusLabs/blitz.git
   ```
2. **A clone of Parley**:
   ```sh
   git clone https://github.com/linebender/parley.git
   ```
3. **A clone of the WPT test suite** (large, a shallow clone is fine):
   ```sh
   git clone --depth 1 https://github.com/web-platform-tests/wpt.git
   ```

The layout assumed by the rest of this document is sibling directories:

```text
~/code/blitz
~/code/parley
~/code/wpt
```

You may also find the unofficial
[WPT CLI](https://crates.io/crates/wpt), maintained by nicoburns, useful:

```sh
cargo install wpt --locked
```

## Pointing Blitz at your local Parley

Blitz normally depends on a released version of Parley from crates.io. To test
local changes, change the dependency in Blitz's root workspace `Cargo.toml` to
a local path dependency:

```toml
parley = { path = "../parley/parley" }
```

Use `parley/parley` rather than plain `parley`, because the `parley` crate is
inside the `parley` directory of its repository.

### Version compatibility gotchas

- It is generally best to remove the `version` specifier from the dependency
  specification, as above. If `version` is present, Cargo will require that
  your local checkout of Parley's version matches it.
- Blitz usually tracks Parley releases, not Parley `main`. If your branch is
  based on `main` and Parley's API has moved on since the last release, Blitz
  may fail to compile against it. You can:
  - fix the usually small API mismatches in Blitz locally;
  - check whether Blitz has a branch that already tracks a newer Parley; or
  - base your Parley branch on the branch or tag matching the release Blitz
    uses, such as `v0.11.x`.

Verify that the path dependency took effect with:

```sh
cargo tree -p parley
```

The output should include a path, for example:
`parley vX.Y.Z (/path/to/your/parley/parley)`.

## Running the tests

The runner needs the `WPT_DIR` environment variable pointing at your WPT clone.
From the Blitz repository root:

```sh
WPT_DIR=../wpt cargo run -rp wpt css/css-text
```

You may wish to export `WPT_DIR` from your shell configuration to avoid setting
it every time. You may also install the
[just](https://github.com/casey/just) task runner. With both adjustments, the
command becomes:

```sh
just wpt css/css-text
```

The positional arguments are path filters relative to the WPT root. You can
pass:

- a directory: `css/css-text/word-break`
- multiple suites: `css/css-text css/css-fonts`
- a single test file:
  `css/css-text/word-break/word-break-normal-ja-000.html`

If no filter is given, the runner defaults to `css/css-flexbox` and
`css/css-grid`, so for Parley work you will generally want to pass a
text-related filter.

### Suites most relevant to Parley

Core suites for layout:

| Suite | Exercises |
| --- | --- |
| `css/CSS2/text` | Basic and older tests for line breaking, `word-break`, `overflow-wrap`, `white-space`, `text-align`, and letter and word spacing |
| `css/CSS2/bidi-text` | Older, more basic tests for bidirectional text |
| `css/css-text` | Advanced and newer tests for line breaking, `word-break`, `overflow-wrap`, `white-space`, `text-align`, and letter and word spacing |
| `css/css-inline` | Inline layout, baselines, `line-height`, and `vertical-align` |
| `css/css-writing-modes` | Vertical text, bidirectional text, and `direction` |

Other suites that may be useful:

| Suite | Exercises |
| --- | --- |
| `css/css-fonts` | Font selection, fallback, `font-variant`, weights, and styles (Fontique) |
| `css/css-ruby` | Ruby annotation layout (not yet implemented in Parley) |
| `css/css-text-decor` | Underlines, `text-decoration`, and `text-emphasis` |

Failures in these suites are not necessarily Parley bugs. A test may exercise a
CSS feature that Blitz does not yet implement, or the bug may be in Blitz's
inline layout integration rather than in Parley itself.

### Useful flags and environment variables

- `-v` / `--verbose`: print each test result as it completes instead of using a
  progress display.
- `RUST_LOG=info`: enable the runner's logging.
- `RAYON_NUM_THREADS=1`: use a single thread when debugging.

## Interpreting the output

You should get a line per test:

```text
[0011/1902] FAIL (0/1) css/css-text/bidi/bidi-lines-001.html (4ms) REF
[0012/1902] FAIL (0/1) css/css-text/bidi/bidi-lines-002.html (4ms) REF (D)
[0013/1902] PASS (1/1) css/css-text/bidi/bidi-tab-001.html (2ms) REF
[0014/1902] FAIL (0/1) css/css-text/bidi/empty-span-001.html (19ms) REF
[0015/1902] PASS (1/1) css/css-text/boundary-shaping/boundary-shaping-001.html (6ms) REF
[0016/1902] FAIL (0/1) css/css-text/boundary-shaping/boundary-shaping-002.html (9ms) REF
```

At the end of a run, you get a summary:

```text
 105 tests FOUND
   1 tests SKIPPED (0.95%)
 104 tests RUN (99.05%)
  39 tests PASSED (37.50% of run; 37.14% of found)
  65 tests FAILED (62.50% of run; 61.90% of found)

Of those tests which failed:
  22 do not use unsupported features
   4 use floats (F)
   9 use intrinsic size keywords (I)
  30 use script (X)
```

The runner supports four kinds of test:

- reftests (`REF`), which compare an image against a reference page;
- attribute tests (`ATT`), whose `checkLayout()`-style expectations are encoded
  in `data-expected-*` attributes;
- crashtests (`CRA`), which pass if they render without panicking; and
- `testharness.js` tests (`HAR`), which require a JavaScript engine.

The single-letter flags after each result (`F`, `I`, `C`, `D`, `W`, `X`, and
others) mark tests that use features Blitz does not fully support, such as
floats, intrinsic sizing keywords, `calc()`, direction, writing modes, or
scripts.

## Artifacts in `wpt/output/`

Each run wipes and repopulates `wpt/output/` in the Blitz repository:

- `<test>.html-test.png`: Blitz's rendering of the test page
- `<test>.html-ref.png` (or `-ref-N.png`): rendering of the reference pages
- `<test>.html-diff.png`: pixel diff for failing comparisons
- `wptreport.json`: standard WPT report format, consumable by WPT tooling and
  dashboards

## A typical Parley-change workflow

1. Set up the path dependency as described above.
2. Export the `WPT_DIR` environment variable.
3. Run the relevant suite before your change and save the report:
   ```sh
   cargo run -rp wpt css/css-text
   cp wpt/output/wptreport.json /tmp/before.json
   ```
4. Make your Parley change.
5. Re-run the suite and compare the reports:
   ```sh
   cargo run -rp wpt css/css-text
   wpt diff /tmp/before.json wpt/output/wptreport.json
   ```
6. For any regression, inspect the `-test.png`, `-ref.png`, and `-diff.png`
   images in `wpt/output/`. The test itself lives in your WPT clone and can
   also be viewed at `https://wpt.live/<test path>` for comparison against real
   browsers.
