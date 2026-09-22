//! Detection of tests that depend on JavaScript to render correctly.
//!
//! Blitz does not execute JavaScript, so ref tests whose rendering depends on script
//! (e.g. tests that mutate the DOM from a `<script>` block) would produce bogus results
//! if run. However, many ref tests use script in a *trivial* way that does not affect
//! the final rendering in this runner: waiting for fonts/load events and then taking a
//! screenshot (or removing the `reftest-wait` class). The runner already waits for
//! fonts and resource loading, so such tests are safe to run.
//!
//! A test is considered to depend on script unless every script it contains is trivial:
//!
//! - External scripts (`<script src>`) are trivial only if they are one of the known
//!   wait-helper scripts (`/common/reftest-wait.js`, `/common/rendering-utils.js`).
//! - Inline scripts (and inline `on*` event-handler attributes) are trivial only if,
//!   after stripping comments, every string literal and identifier they contain is on
//!   an allowlist of pure "wait then screenshot" vocabulary. Any other identifier
//!   (DOM mutation, CSSOM access, `setTimeout`, etc.) marks the test as script-dependent.

use regex::Regex;
use std::sync::LazyLock;

static SCRIPT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<script\b([^>]*)>(.*?)</script>"#).unwrap());
static SRC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)src\s*=\s*["']?([^"'\s>]+)"#).unwrap());
static EVENT_HANDLER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\son[a-z]+\s*=\s*("[^"]*"|'[^']*')"#).unwrap());
static COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)//[^\n]*|/\*.*?\*/"#).unwrap());
static STRING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""([^"\\]*)"|'([^'\\]*)'"#).unwrap());
static IDENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[A-Za-z_$][A-Za-z0-9_$]*"#).unwrap());

/// External scripts that only provide wait/screenshot helpers and have no
/// rendering side-effects of their own.
const TRIVIAL_SCRIPT_SRCS: &[&str] = &["/reftest-wait.js", "/rendering-utils.js"];

/// Identifiers allowed in a trivial "wait then screenshot" inline script.
const TRIVIAL_IDENTS: &[&str] = &[
    // Keywords / declarations
    "async",
    "await",
    "const",
    "function",
    "let",
    "new",
    "return",
    "var",
    // Waiting vocabulary
    "Promise",
    "addEventListener",
    "document",
    "fonts",
    "load",
    "onload",
    "ready",
    "requestAnimationFrame",
    "resolve",
    "then",
    "finally",
    "window",
    // reftest-wait helpers (defined by /common/reftest-wait.js and
    // /common/rendering-utils.js)
    "takeScreenshot",
    "waitForAtLeastOneFrame",
    // Removing the `reftest-wait` class
    "body",
    "classList",
    "documentElement",
    "remove",
];

/// String literals allowed in a trivial inline script.
const TRIVIAL_STRINGS: &[&str] = &["", "reftest-wait", "load", "DOMContentLoaded"];

fn src_is_trivial(src: &str) -> bool {
    TRIVIAL_SCRIPT_SRCS
        .iter()
        .any(|suffix| src.ends_with(suffix))
}

fn inline_script_is_trivial(body: &str) -> bool {
    let body = COMMENT_RE.replace_all(body, "");
    for caps in STRING_RE.captures_iter(&body) {
        let string = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map_or("", |m| m.as_str());
        if !TRIVIAL_STRINGS.contains(&string) {
            return false;
        }
    }
    let without_strings = STRING_RE.replace_all(&body, "\"\"");
    IDENT_RE
        .find_iter(&without_strings)
        .all(|ident| TRIVIAL_IDENTS.contains(&ident.as_str()))
}

/// Returns true if the document depends on JavaScript to render correctly
/// (i.e. contains any script that is not a trivial "wait then screenshot" script).
pub fn uses_nontrivial_script(html: &str) -> bool {
    for caps in SCRIPT_RE.captures_iter(html) {
        let attrs = caps.get(1).unwrap().as_str();
        let body = caps.get(2).unwrap().as_str();
        if let Some(src) = SRC_RE.captures(attrs).and_then(|c| c.get(1)) {
            if !src_is_trivial(src.as_str()) {
                return true;
            }
        } else if !body.trim().is_empty() && !inline_script_is_trivial(body) {
            return true;
        }
    }

    // Content outside of well-formed <script>...</script> elements: an unclosed
    // <script> tag (conservatively treated as script-dependent) or inline event
    // handler attributes.
    let remaining = SCRIPT_RE.replace_all(html, " ");
    if remaining.to_ascii_lowercase().contains("<script") {
        return true;
    }
    for caps in EVENT_HANDLER_RE.captures_iter(&remaining) {
        let quoted = caps.get(1).unwrap().as_str();
        let body = &quoted[1..quoted.len() - 1];
        if !inline_script_is_trivial(body) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::uses_nontrivial_script;

    #[test]
    fn no_script() {
        assert!(!uses_nontrivial_script("<!DOCTYPE html><div></div>"));
    }

    #[test]
    fn dom_mutating_script() {
        let html = r#"
            <div id="target" style="width: 0px;"></div>
            <script>
            document.body.offsetTop;
            document.getElementById('target').style.width = '100px';
            </script>
        "#;
        assert!(uses_nontrivial_script(html));
    }

    #[test]
    fn font_wait_screenshot() {
        let html = r#"
            <html class="reftest-wait">
            <script src="/common/reftest-wait.js"></script>
            <script>document.fonts.ready.then(takeScreenshot);</script>
        "#;
        assert!(!uses_nontrivial_script(html));
    }

    #[test]
    fn remove_reftest_wait_on_font_ready() {
        let html = r#"
            <script>
            document.fonts.ready.then(
                () => { document.documentElement.classList.remove("reftest-wait"); });
            </script>
        "#;
        assert!(!uses_nontrivial_script(html));
    }

    #[test]
    fn onload_raf_screenshot() {
        let html = r#"
            <script src="/common/rendering-utils.js"></script>
            <script>
            window.addEventListener("load", () => {
                requestAnimationFrame(() => requestAnimationFrame(takeScreenshot));
            });
            </script>
        "#;
        assert!(!uses_nontrivial_script(html));
    }

    #[test]
    fn external_helper_script() {
        let html = r#"<script src="support/utils.js"></script>"#;
        assert!(uses_nontrivial_script(html));
    }

    #[test]
    fn nontrivial_event_handler_attribute() {
        let html = r#"<body onload="doSomething()"></body>"#;
        assert!(uses_nontrivial_script(html));
    }

    #[test]
    fn nontrivial_string_literal() {
        let html = r#"<script>document.documentElement.classList.remove("other-class");</script>"#;
        assert!(uses_nontrivial_script(html));
    }

    #[test]
    fn unclosed_script_tag() {
        assert!(uses_nontrivial_script("<script>document.write('x');"));
    }
}
