//! Test discovery from the WPT manifest, generated in memory by the `wpt_manifest` crate
//! (a port of `./wpt manifest`), so that the runner selects the same tests, variants and
//! references as upstream WPT tooling.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use serde_json::Value;
use wpt_manifest::{FuzzySpec, ItemType, Manifest};

#[derive(Clone)]
pub enum TestType {
    TestHarness,
    Crash,
    Ref {
        match_references: Vec<String>,
        mismatch_references: Vec<String>,
        fuzzy: Vec<FuzzySpec>,
    },
}

pub struct Test {
    pub path: PathBuf,
    pub url: String,
    pub test_type: TestType,
}

impl Test {
    /// This test at another variant, with its references moved to the same variant.
    fn with_variant(&self, variant: &str) -> Test {
        let (url_path, old_variant) = split_test_url(&self.url);
        let move_variant = |reference: &String| {
            if reference.starts_with("about:") {
                return reference.clone();
            }
            let base = match reference.strip_suffix(old_variant) {
                Some(base) if !old_variant.is_empty() => base,
                _ => reference,
            };
            format!("{base}{variant}")
        };
        let test_type = match &self.test_type {
            TestType::Ref {
                match_references,
                mismatch_references,
                fuzzy,
            } => TestType::Ref {
                match_references: match_references.iter().map(move_variant).collect(),
                mismatch_references: mismatch_references.iter().map(move_variant).collect(),
                fuzzy: fuzzy.clone(),
            },
            test_type => test_type.clone(),
        };
        Test {
            path: self.path.clone(),
            url: format!("{url_path}{variant}"),
            test_type,
        }
    }
}

pub fn split_test_url(url: &str) -> (&str, &str) {
    let index = url.find(['?', '#']).unwrap_or(url.len());
    url.split_at(index)
}

pub fn is_xml(path: &str) -> bool {
    [".xht", ".xhtm", ".xhtml", ".xml", ".svg"]
        .iter()
        .any(|ext| split_test_url(path).0.ends_with(ext))
}

/// Keep variant screenshots distinct without unsafe or overlong filenames.
pub fn artifact_name(url: &str) -> String {
    let (path, suffix) = split_test_url(url);
    if suffix.is_empty() {
        return path.to_string();
    }
    let mut hasher = DefaultHasher::new();
    suffix.hash(&mut hasher);
    format!("{path}-variant-{:016x}", hasher.finish())
}

/// Generates the manifest for `wpt_dir` and returns its tests which the runner can run.
///
/// Worker, module and jsshell variants of `.js` tests are excluded: the runner only runs
/// classic scripts in a window global.
pub fn load_tests(wpt_dir: &Path) -> Vec<Test> {
    let manifest = Manifest::build(wpt_dir, "/")
        .unwrap_or_else(|err| panic!("Failed to generate the WPT manifest: {err}"));
    let json = manifest.to_json();

    let mut tests = Vec::new();
    for item_type in [
        ItemType::Testharness,
        ItemType::Reftest,
        ItemType::Crashtest,
    ] {
        let Some(trie) = json["items"].get(item_type.as_str()) else {
            continue;
        };
        let mut stack = vec![(String::new(), trie)];
        while let Some((prefix, node)) = stack.pop() {
            for (name, value) in node.as_object().into_iter().flatten() {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                match value {
                    Value::Array(leaf) => {
                        for item in &leaf[1..] {
                            if let Some(test) = manifest_test(wpt_dir, item_type, &path, item) {
                                tests.push(test);
                            }
                        }
                    }
                    _ => stack.push((path, value)),
                }
            }
        }
    }
    tests.sort_unstable_by(|a, b| a.url.cmp(&b.url));
    tests
}

fn manifest_test(wpt_dir: &Path, item_type: ItemType, path: &str, item: &Value) -> Option<Test> {
    let item = item.as_array()?;
    let url = item
        .first()?
        .as_str()
        .unwrap_or(path)
        .trim_start_matches('/');
    let extras = item.last()?;
    let url_path = split_test_url(url).0;
    // `.js` tests run in the wrapper documents which wptserve generates per global: only
    // the classic-script window ones can run here
    if path.ends_with(".js")
        && ![".any.html", ".window.html"]
            .iter()
            .any(|s| url_path.ends_with(s))
    {
        return None;
    }

    let test_type = match item_type {
        ItemType::Testharness => TestType::TestHarness,
        ItemType::Crashtest => TestType::Crash,
        ItemType::Reftest => {
            let mut match_references = Vec::new();
            let mut mismatch_references = Vec::new();
            for reference in item.get(1)?.as_array()? {
                let href = reference.get(0)?.as_str()?.to_string();
                match reference.get(1)?.as_str()? {
                    "==" => match_references.push(href),
                    _ => mismatch_references.push(href),
                }
            }
            let fuzzy = FuzzySpec::from_extras(extras);
            TestType::Ref {
                match_references,
                mismatch_references,
                fuzzy,
            }
        }
        _ => return None,
    };

    Some(Test {
        path: wpt_dir.join(path),
        url: url.to_string(),
        test_type,
    })
}

fn is_glob(pattern: &str) -> bool {
    pattern.contains(['*', '?', '['])
}

/// Does `suite` (a path, directory or glob relative to the WPT root, without a variant)
/// select the test with this source path or URL?
fn suite_matches(suite: &str, test: &Test, relative_path: &str) -> bool {
    let suite = suite.trim_end_matches('/');
    let url_path = split_test_url(&test.url).0;
    if suite.is_empty() || url_path == suite {
        return true;
    }
    let ancestors = std::iter::once(relative_path).chain(
        relative_path
            .match_indices('/')
            .map(|(index, _)| &relative_path[..index]),
    );
    if is_glob(suite) {
        let Ok(pattern) = glob::Pattern::new(suite) else {
            panic!("Invalid glob pattern: {suite}");
        };
        let options = glob::MatchOptions {
            require_literal_separator: true,
            ..Default::default()
        };
        pattern.matches_with(url_path, options)
            || ancestors
                .into_iter()
                .any(|path| pattern.matches_with(path, options))
    } else {
        ancestors.into_iter().any(|path| path == suite)
    }
}

/// Selects the tests for `suites`: paths, directories or globs relative to the WPT root.
/// A suite with a `?query` or `#fragment` runs the matching test at exactly that URL, even
/// if the manifest does not list it as a variant.
pub fn select_tests(wpt_dir: &Path, all_tests: Vec<Test>, suites: &[String]) -> Vec<Test> {
    let mut tests = Vec::new();
    let mut overridden = HashSet::new();
    for test in all_tests {
        let relative_path = test
            .path
            .strip_prefix(wpt_dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mut selected = false;
        for suite in suites {
            let (suite_path, suffix) = split_test_url(suite);
            if !suite_matches(suite_path, &test, &relative_path) {
                continue;
            }
            if suffix.is_empty() {
                selected = true;
            } else if overridden.insert((test.path.clone(), suite)) {
                tests.push(test.with_variant(suffix));
            }
        }
        if selected {
            tests.push(test);
        }
    }
    tests
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_variants() {
        assert_eq!(split_test_url("test.html?a=1#b"), ("test.html", "?a=1#b"));
        assert!(is_xml("test.xht?a=1"));
    }

    #[test]
    fn script_location_uses_wrapper_url_and_variant() {
        let mut doc = blitz_vibey_script::ScriptDocument::from_html(
            "",
            blitz_dom::DocumentConfig {
                base_url: Some("http://dummy.local/dom/test.any.html?a=1&b=2#part".to_string()),
                ..Default::default()
            },
        )
        .without_timer_thread();
        doc.eval(
            "__blitz_send_message([location.pathname, location.search, location.hash].join('|'));",
        );
        assert!(doc.take_js_errors().is_empty());
        assert_eq!(doc.take_messages(), ["/dom/test.any.html|?a=1&b=2|#part"]);
    }

    #[test]
    fn artifact_suffix_cannot_change_directories() {
        let name = artifact_name("css/test.html?a=/../b&c=%2F#d");
        assert_eq!(Path::new(&name).parent(), Some(Path::new("css")));
        assert!(name.starts_with("css/test.html-variant-"));
        assert_ne!(name, artifact_name("css/test.html?a=other"));
        assert!(artifact_name(&format!("css/test.html?a={}", "x".repeat(500))).len() < 100);
        assert_eq!(artifact_name("css/test.html"), "css/test.html");
    }
}
