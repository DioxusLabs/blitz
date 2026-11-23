//! resvg's test suite (`crates/resvg/tests`): one SVG per feature with a
//! reference PNG rendered by resvg.
//!
//! `wpt resvg [<paths>] [<runner flags>]` fetches resvg at `wpt/RESVG_COMMIT`
//! into `wpt/resvg/checkout` (with the git CLI), runs the selected tests (all
//! of them by default) and prints per-mode, per-directory pass counts. Results
//! go to `wpt/resvg/output`. Paths select tests by name (`img/structure`,
//! `structure/style`) or glob. Each SVG runs in three modes:
//!
//! - `img`: in an `<img>`, so through usvg, against resvg's PNG
//! - `doc`: loaded as the top-level document (where browsers load external
//!   resources) against resvg's PNG, with whichever backend the runner uses
//!   for inline SVG (`BLITZ_NATIVE_SVG=1` for native)
//! - `parity`: `doc` against `img`, so native against usvg on the same
//!   rasterizer with `BLITZ_NATIVE_SVG=1`
//!
//! As resvg's harness scales each SVG to 300px wide, every mode renders in a
//! viewport the size of the PNG, `doc` at a device scale that fits the SVG to
//! it. Text renders with only resvg's fonts (see `font_context`). A single
//! test runs in `test_runners::resvg_test`.

use parley::FontContext;
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, LazyLock};

use crate::manifest::{Test, TestType};

const RESVG_REPO: &str = "https://github.com/linebender/resvg";
const RESVG_COMMIT: &str = include_str!("../../RESVG_COMMIT");

/// Matches `GLOBAL_FONTDB` in resvg's `crates/resvg/tests/integration/main.rs`.
const FONT_FAMILIES: &[(GenericFamily, &str)] = &[
    (GenericFamily::Serif, "Noto Serif"),
    (GenericFamily::SansSerif, "Noto Sans"),
    (GenericFamily::Cursive, "Yellowtail"),
    (GenericFamily::Fantasy, "Sedgwick Ave Display"),
    (GenericFamily::Monospace, "Noto Mono"),
];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Img,
    Doc,
    Parity,
}

impl Mode {
    const ALL: [Mode; 3] = [Mode::Img, Mode::Doc, Mode::Parity];
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Img => "img",
            Mode::Doc => "doc",
            Mode::Parity => "parity",
        })
    }
}

/// Where results go by default: `wpt/resvg/output`
pub fn output_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).with_file_name("resvg/output")
}

/// resvg's test directory at the pinned commit, and the fonts to render it with
pub struct Suite {
    /// `crates/resvg/tests`, which serves as the `WPT_DIR`
    pub dir: PathBuf,
    pub font_ctx: FontContext,
    /// The listed tests, relative to `dir/tests` and without `.svg`
    tests: Vec<String>,
}

impl Suite {
    pub fn load() -> Self {
        let dir = checkout(RESVG_COMMIT.trim()).join("crates/resvg/tests");
        let render_rs = fs::read_to_string(dir.join("integration/render.rs")).unwrap();
        Self {
            font_ctx: font_context(&dir.join("fonts")),
            tests: listed_tests(&render_rs),
            dir,
        }
    }

    /// The tests in every mode, named `resvg/<mode>/<test>`, that `suites`
    /// select (all of them if it is empty).
    pub fn select_tests(&self, suites: &[String]) -> Vec<Test> {
        let mut tests = Vec::new();
        for mode in Mode::ALL {
            for test in &self.tests {
                let name = format!("{mode}/{test}");
                if suites.is_empty() || suites.iter().any(|suite| selects(suite, &name, test)) {
                    tests.push(Test {
                        path: self.dir.join(format!("tests/{test}.svg")),
                        url: format!("resvg/{name}"),
                        test_type: TestType::Resvg(mode),
                    });
                }
            }
        }
        tests
    }
}

/// Does `suite` (a path or glob, optionally starting with `resvg/` and ending
/// in `.svg`) select the test named `name` (`<mode>/<test>`)?
fn selects(suite: &str, name: &str, test: &str) -> bool {
    let suite = suite.trim_matches('/');
    let suite = suite.strip_prefix("resvg/").unwrap_or(suite);
    let suite = suite.strip_suffix(".svg").unwrap_or(suite);
    if suite == "resvg" || suite.is_empty() {
        return true;
    }
    if suite.contains(['*', '?', '[']) {
        let Ok(pattern) = glob::Pattern::new(suite) else {
            panic!("Invalid glob pattern: {suite}");
        };
        return [name, test].iter().any(|path| pattern.matches(path));
    }
    [name, test].iter().any(|path| {
        path.strip_prefix(suite)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// Prints per-mode, per-directory pass counts from a `wpt_expectations.txt`.
/// "parity where img passes" counts native/usvg parity only for tests whose
/// usvg rendering matches resvg's PNG, so both backends failing alike is not a
/// pass.
pub fn summarize(expectations_path: &Path) {
    let expectations = fs::read_to_string(expectations_path).unwrap_or_else(|err| {
        eprintln!("Can't read {}: {err}", expectations_path.display());
        std::process::exit(1);
    });
    let mut results = HashMap::new();
    for line in expectations.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(status)) = (fields.next(), fields.next()) else {
            continue;
        };
        let mut parts = name.splitn(3, '/');
        if let (Some("resvg"), Some(mode), Some(test)) = (parts.next(), parts.next(), parts.next())
        {
            results.insert((mode, test), status == "PASS");
        }
    }

    let mut counts: BTreeMap<(&str, &str), (u32, u32)> = BTreeMap::new();
    for (&(mode, test), &passed) in &results {
        let mut modes = vec![mode];
        if mode == "parity" && results.get(&("img", test)) == Some(&true) {
            modes.push("parity where img passes");
        }
        let dir = test.split('/').next().unwrap();
        for mode in modes {
            for dir in [dir, "Total"] {
                let count = counts.entry((mode, dir)).or_default();
                count.0 += passed as u32;
                count.1 += 1;
            }
        }
    }

    let modes: BTreeSet<_> = counts.keys().map(|(mode, _)| *mode).collect();
    let mut dirs: Vec<_> = counts
        .keys()
        .map(|(_, dir)| *dir)
        .filter(|dir| *dir != "Total")
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    dirs.push("Total");

    let header: Vec<_> = modes.iter().copied().collect();
    println!("| Directory | {} |", header.join(" | "));
    println!("|---|{}", "---:|".repeat(modes.len()));
    for dir in dirs {
        let cells: Vec<_> = modes
            .iter()
            .map(|mode| match counts.get(&(mode, dir)) {
                Some((pass, total)) => format!("{pass} / {total}"),
                None => String::new(),
            })
            .collect();
        println!("| {dir} | {} |", cells.join(" | "));
    }
}

/// Shallow-fetches `commit` into `wpt/resvg/checkout` unless it is already checked out.
fn checkout(commit: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).with_file_name("resvg/checkout");
    let head = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    if head.as_deref() != Some(commit) {
        println!("Fetching resvg {commit}");
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["fetch", "-q", "--depth", "1", RESVG_REPO, commit]);
        git(&dir, &["checkout", "-q", "FETCH_HEAD"]);
    }
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap_or_else(|err| panic!("failed to run git: {err}"));
    if !status.success() {
        eprintln!("git {} failed in {}", args.join(" "), dir.display());
        std::process::exit(1);
    }
}

/// The fonts to render the suite with: only resvg's `tests/fonts`, with the
/// generic families resvg's harness uses, for both Blitz's text (returned)
/// and SVG images (usvg). System fonts are left out so results don't depend on
/// the machine.
fn font_context(fonts_dir: &Path) -> FontContext {
    let mut font_paths: Vec<PathBuf> = fs::read_dir(fonts_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "ttf" | "ttc" | "otf" | "otc"
                    )
                })
        })
        .collect();
    font_paths.sort();

    let mut font_ctx = FontContext {
        collection: Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        }),
        source_cache: SourceCache::default(),
    };
    let mut font_db = usvg::fontdb::Database::new();
    for path in &font_paths {
        let data = fs::read(path).unwrap();
        font_ctx
            .collection
            .register_fonts(Blob::new(Arc::new(data.clone())), None);
        font_db.load_font_data(data);
    }
    for &(generic, family) in FONT_FAMILIES {
        let id = font_ctx.collection.family_id(family).unwrap();
        font_ctx
            .collection
            .set_generic_families(generic, std::iter::once(id));
        match generic {
            GenericFamily::Serif => font_db.set_serif_family(family),
            GenericFamily::SansSerif => font_db.set_sans_serif_family(family),
            GenericFamily::Cursive => font_db.set_cursive_family(family),
            GenericFamily::Fantasy => font_db.set_fantasy_family(family),
            _ => font_db.set_monospace_family(family),
        }
    }
    blitz_dom::util::set_svg_font_db(Arc::new(font_db))
        .expect("SVG fonts are set before any SVG is parsed");
    font_ctx
}

/// The tests resvg runs, from its generated `integration/render.rs`. Some
/// SVG/PNG pairs in `tests/` aren't listed there and their PNGs are stale.
/// Names are relative to `tests/`.
fn listed_tests(render_rs: &str) -> Vec<String> {
    static RENDER_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\brender\("tests/([^"]+)"\)"#).unwrap());
    let mut tests: Vec<String> = RENDER_RE
        .captures_iter(render_rs)
        .map(|caps| caps[1].to_string())
        .collect();
    tests.sort();
    tests
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_render_tests() {
        let render_rs = r#"
#[test] fn a() { assert_eq!(render("tests/painting/fill/#RGB-color"), 0); }
#[test] fn b() { assert_eq!(render("tests/filters/feBlend/empty"), 0); }
#[test] fn c() { assert_eq!(render_extra("extra/hidden-element"), 0); }
"#;
        assert_eq!(
            listed_tests(render_rs),
            ["filters/feBlend/empty", "painting/fill/#RGB-color"]
        );
    }

    #[test]
    fn selects_by_name_or_test_path() {
        let name = "img/structure/style/attribute";
        let test = "structure/style/attribute";
        for suite in [
            "resvg",
            "img",
            "resvg/img/structure/",
            "structure",
            test,
            "structure/style/attribute.svg",
            "*/structure/*/attr*",
        ] {
            assert!(selects(suite, name, test), "{suite}");
        }
        for suite in ["doc", "struct", "structure/style/attr", "text"] {
            assert!(!selects(suite, name, test), "{suite}");
        }
    }
}
