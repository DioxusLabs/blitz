use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;
use std::{fs, sync::Arc, time::Instant};

use blitz_dom::traversal::TreeTraverser;
use blitz_dom::{BaseDocument, Document as _, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_vibey_script::ScriptDocument;
use log::{debug, warn};
use regex::Regex;

use harness_test::WptScriptFetcher;

use crate::{SubtestCounts, TestFlags, TestKind, TestStatus, ThreadCtx};

mod attr_test;
mod crash_test;
mod fuzzy;
mod harness_test;
mod ref_test;

pub use attr_test::process_attr_test;
pub use crash_test::process_crash_test;
pub use harness_test::process_harness_test;
pub use ref_test::process_ref_test;

static TIMEOUT_QUARANTINE: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut tests = HashMap::new();
    for line in include_str!("../../timeout-quarantine.txt").lines() {
        let (path, reason) = line
            .split_once(' ')
            .expect("timeout quarantine entries must contain a path and reason");
        assert!(
            tests.insert(path, reason).is_none(),
            "duplicate timeout quarantine entry: {path}"
        );
    }
    tests
});

/// Is the node a `<script>` element whose `type` would execute as JavaScript?
/// Returns the node if so.
fn as_js_script_element(node: &blitz_dom::Node) -> Option<&blitz_dom::node::ElementData> {
    let element = node.element_data()?;
    if element.name.local != blitz_dom::local_name!("script") {
        return None;
    }

    // Skip non-JavaScript script types (e.g. JSON data blocks)
    let script_type = element
        .attr(blitz_dom::local_name!("type"))
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let is_js = matches!(
        script_type.as_str(),
        "" | "text/javascript" | "application/javascript" | "module"
    );
    is_js.then_some(element)
}

/// Does the parsed document contain any JavaScript which would execute:
/// either a JavaScript-typed `<script>` element with a `src` attribute or
/// inline content, or a `<body onload="...">` handler (which the script
/// runtime installs as the window's `load` handler)?
pub fn document_has_scripts(doc: &BaseDocument) -> bool {
    TreeTraverser::new(doc).any(|node_id| {
        let Some(node) = doc.get_node(node_id) else {
            return false;
        };
        if let Some(element) = node.element_data()
            && element.name.local == blitz_dom::local_name!("body")
            && element.attr(blitz_dom::local_name!("onload")).is_some()
        {
            return true;
        }
        let Some(element) = as_js_script_element(node) else {
            return false;
        };
        element.attr(blitz_dom::local_name!("src")).is_some()
            || !node.text_content().trim().is_empty()
    })
}

/// Matches inline-script statements which don't require script execution for a
/// checkLayout (attr) test: the `checkLayout()` call itself (re-implemented
/// natively by the attr test runner) and testharness `setup()` calls
static TRIVIAL_ATTR_SCRIPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"checkLayout\(\s*['"][^'"]*['"]\s*(,\s*(true|false))?\s*\)\s*;?|setup\(\s*\{[^{}]*\}\s*\)\s*;?"#)
        .unwrap()
});

/// Does a checkLayout (attr) test's inline script do anything beyond calling
/// `checkLayout()` (e.g. generate the test DOM)? If so, scripts must be
/// executed before the native layout checks can see the full test DOM.
pub fn attr_test_needs_scripts(doc: &BaseDocument) -> bool {
    TreeTraverser::new(doc).any(|node_id| {
        let Some(node) = doc.get_node(node_id) else {
            return false;
        };
        let Some(element) = as_js_script_element(node) else {
            return false;
        };
        if element.attr(blitz_dom::local_name!("src")).is_some() {
            return false;
        }
        let body = node.text_content();
        !TRIVIAL_ATTR_SCRIPT_RE
            .replace_all(&body, "")
            .trim()
            .is_empty()
    })
}

/// Wrap a parsed document in a [`ScriptDocument`] with the WPT script fetcher
/// and execute its scripts
pub fn run_document_scripts(ctx: &ThreadCtx, document: BaseDocument) -> ScriptDocument {
    // The runner drives timers manually via `pump_timers`, so the background
    // timer wakeup thread is unnecessary
    let mut script_document = ScriptDocument::from_base_document(document)
        .without_timer_thread()
        .with_fetcher(WptScriptFetcher::new(ctx.wpt_dir.clone()));
    script_document.execute_scripts();
    script_document
}

/// Pump the document's pending JS timers until `check` produces a result or
/// there are no more timers due within `budget`. `check` runs once before any
/// timer fires and again after each timer poll.
pub fn pump_timers<T>(
    document: &mut ScriptDocument,
    budget: Duration,
    mut check: impl FnMut(&mut ScriptDocument) -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(result) = check(document) {
            return Some(result);
        }
        match document.next_timer_deadline() {
            Some(timer_deadline) if timer_deadline <= deadline => {
                let now = Instant::now();
                if timer_deadline > now {
                    std::thread::sleep(timer_deadline - now);
                }
                document.poll(None);
            }
            // Timer budget expired, or no pending timers
            _ => return None,
        }
    }
}

pub struct SubtestResult {
    pub name: String,
    pub status: TestStatus,
    pub errors: Vec<String>,
}

pub fn process_test_file(
    ctx: &mut ThreadCtx,
    relative_path: &str,
) -> (
    TestKind,
    TestFlags,
    TestStatus,
    SubtestCounts,
    Vec<SubtestResult>,
) {
    debug!("Processing test file: {relative_path}");

    if !ctx.run_quarantined
        && let Some(reason) = TIMEOUT_QUARANTINE.get(relative_path)
    {
        debug!("Skipping quarantined test {relative_path}: {reason}");
        return (
            TestKind::TestHarness,
            TestFlags::empty(),
            TestStatus::Skip,
            SubtestCounts::ZERO_OF_ZERO,
            Vec::new(),
        );
    }

    let file_contents = match fs::read_to_string(ctx.wpt_dir.join(relative_path)) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
            // Tests encoded as UTF-16 or a legacy encoding are not supported: skip them.
            warn!("Skipping {relative_path}: not valid UTF-8");
            return (
                TestKind::Unknown,
                TestFlags::empty(),
                TestStatus::Skip,
                SubtestCounts::ZERO_OF_ZERO,
                Vec::new(),
            );
        }
        Err(err) => panic!("Failed to read {relative_path}: {err}"),
    };

    // Compute flags
    let mut flags = TestFlags::empty();
    if ctx.float_re.is_match(&file_contents) {
        flags |= TestFlags::USES_FLOAT;
    }
    if ctx.intrinsic_re.is_match(&file_contents) {
        flags |= TestFlags::USES_INTRINSIC_SIZE;
    }
    if ctx.calc_re.is_match(&file_contents) {
        flags |= TestFlags::USES_CALC;
    }
    if ctx.direction_re.is_match(&file_contents) {
        flags |= TestFlags::USES_DIRECTION;
    }
    if ctx.writing_mode_re.is_match(&file_contents) {
        flags |= TestFlags::USES_WRITING_MODE;
    }
    if ctx.subgrid_re.is_match(&file_contents) {
        flags |= TestFlags::USES_SUBGRID;
    }
    if ctx.grid_lanes_re.is_match(&file_contents) {
        flags |= TestFlags::USES_GRID_LANES;
    }
    if ctx.script_re.is_match(&file_contents) {
        flags |= TestFlags::USES_SCRIPT;
    }

    // Crash Test
    if is_crash_test(relative_path) {
        let counts = process_crash_test(ctx, relative_path, &file_contents, flags);
        let status = counts.as_status();
        return (TestKind::Crash, flags, status, counts, Vec::new());
    }

    // Ref Test
    let mut match_references: Vec<String> = Vec::new();
    let mut mismatch_references: Vec<String> = Vec::new();
    for link in ctx.link_re.find_iter(&file_contents) {
        let tag = link.as_str();
        let Some(rel) = ctx.rel_re.captures(tag).and_then(|c| c.get(1)) else {
            continue;
        };
        let Some(href) = ctx
            .href_re
            .captures(tag)
            .and_then(|c| c.get(1).or(c.get(2)))
        else {
            continue;
        };
        match rel.as_str() {
            "match" => match_references.push(href.as_str().to_string()),
            "mismatch" => mismatch_references.push(href.as_str().to_string()),
            _ => {}
        }
    }
    if !match_references.is_empty() || !mismatch_references.is_empty() {
        let counts = process_ref_test(
            ctx,
            relative_path,
            file_contents.as_str(),
            &match_references,
            &mismatch_references,
            &mut flags,
        );

        let status = counts.as_status();
        return (TestKind::Ref, flags, status, counts, Vec::new());
    }

    // Attr Test
    let mut matches = ctx.attrtest_re.captures_iter(&file_contents);
    let first = matches.next();
    let second = matches.next();
    if first.is_some() && second.is_none() {
        // TODO: handle tests with multiple calls to checkLayout.
        #[allow(clippy::unnecessary_unwrap)]
        let captures = first.unwrap();
        let selector = captures.get(1).unwrap().as_str().to_string();
        drop(matches);

        debug!("{selector}");

        let (status, counts, results) =
            process_attr_test(ctx, &selector, &file_contents, relative_path);

        return (TestKind::Attr, flags, status, counts, results);
    }
    drop(matches);

    // Testharness (testharness.js) test
    if ctx.testharness_re.is_match(&file_contents) {
        let (status, counts, results) = process_harness_test(ctx, &file_contents, relative_path);
        return (TestKind::TestHarness, flags, status, counts, results);
    }

    // TODO: Handle other test formats.
    (
        TestKind::Unknown,
        flags,
        TestStatus::Skip,
        SubtestCounts::ZERO_OF_ZERO,
        Vec::new(),
    )
}

fn is_crash_test(relative_path: &str) -> bool {
    relative_path.split('/').any(|seg| seg == "crashtests")
        || ["", ".https", ".h2", ".www"].iter().any(|flag| {
            [".html", ".htm", ".xht", ".xhtm", ".xhtml", ".xml", ".svg"]
                .iter()
                .any(|ext| relative_path.ends_with(&format!("-crash{flag}{ext}")))
        })
}

fn parse_and_resolve_document(
    ctx: &mut ThreadCtx,
    html: &str,
    relative_path: &str,
) -> BaseDocument {
    ctx.net_provider.reset();
    let config = DocumentConfig {
        base_url: Some(ctx.dummy_base_url.join(relative_path).unwrap().to_string()),
        font_ctx: Some(ctx.font_ctx.clone()),
        net_provider: Some(Arc::clone(&ctx.net_provider) as _),
        navigation_provider: Some(Arc::clone(&ctx.navigation_provider)),
        // Required for `innerHTML` support when the document is upgraded to
        // a `ScriptDocument` (harness tests and ref tests with scripts)
        html_parser_provider: Some(Arc::new(HtmlProvider)),
        ..Default::default()
    };

    // Extensions which wptserve serves with an XML content type must be parsed
    // as XML: content sniffing cannot detect all XHTML documents (e.g. ones
    // with a plain `<!DOCTYPE html>`)
    let is_xml = [".xht", ".xhtm", ".xhtml", ".xml", ".svg"]
        .iter()
        .any(|ext| relative_path.ends_with(ext));
    let mut document = if is_xml {
        HtmlDocument::from_xml(html, config)
    } else {
        HtmlDocument::from_html(html, config)
    };

    document.as_mut().set_viewport(ctx.viewport.clone());
    document.as_mut().resolve(0.0);
    pump_net_provider(ctx, document.as_mut());

    document.into()
}

/// Load pending resources (stylesheets, images, fonts), re-resolving the document
/// as they arrive. Loops because loading a resource may result in further
/// resources being requested.
pub fn pump_net_provider(ctx: &ThreadCtx, document: &mut BaseDocument) {
    let start = Instant::now();
    while ctx.net_provider.pending_item_count() > 0 {
        ctx.net_provider.for_each(|_| {});
        document.resolve(0.0);
        if Instant::now().duration_since(start).as_millis() > 500 {
            ctx.net_provider.log_pending_items();
            panic!(
                "Timeout. {} pending items.",
                ctx.net_provider.pending_item_count()
            );
        }
    }

    document.resolve(0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_quarantine_is_valid() {
        assert_eq!(TIMEOUT_QUARANTINE.len(), 259);
        assert_eq!(
            TIMEOUT_QUARANTINE.get("css/selectors/focus-visible-001.html"),
            Some(&"testdriver")
        );
    }
}
