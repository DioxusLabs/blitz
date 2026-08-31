//! Synthesizes the HTML wrapper documents which wptserve auto-generates for
//! JS-file tests (`.any.js` and `.window.js`), mirroring `AnyHtmlHandler` and
//! `WindowHandler` in wpt's `tools/serve/serve.py`. This lets the runner
//! execute these tests through the regular testharness path without a server.

/// The maximum set of `// META:` directives at the top of a JS test file,
/// in file order. Parsing stops at the first non-META line (matching
/// upstream's `read_script_metadata`).
fn parse_metas(js_source: &str) -> Vec<(&str, &str)> {
    let mut metas = Vec::new();
    for line in js_source.lines() {
        let Some(rest) = line.strip_prefix("//") else {
            break;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix("META:") else {
            break;
        };
        let Some((key, value)) = rest.trim_start().split_once('=') else {
            break;
        };
        metas.push((key, value));
    }
    metas
}

fn escape_html_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;")
}

fn escape_html_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
}

/// Does the test's `// META: global=` line (or the `.any.js` default of
/// "window,dedicatedworker") include the `window` global?
fn globals_include_window(metas: &[(&str, &str)]) -> bool {
    let Some((_, value)) = metas.iter().find(|(key, _)| *key == "global") else {
        return true;
    };
    value.split(',').any(|item| item.trim() == "window")
}

/// Build the wrapper HTML for a `.any.js` or `.window.js` test. Returns `None`
/// for `.any.js` tests whose `// META: global=` list excludes the `window`
/// global (worker-only tests), which have no window variant to run.
pub fn wrapper_html_for_js_test(relative_path: &str, js_source: &str) -> Option<String> {
    let is_any = relative_path.ends_with(".any.js");
    let metas = parse_metas(js_source);

    if is_any && !globals_include_window(&metas) {
        return None;
    }

    let mut meta_tags = String::new();
    let mut script_tags = String::new();
    for (key, value) in &metas {
        match *key {
            "timeout" if *value == "long" => {
                meta_tags.push_str("<meta name=\"timeout\" content=\"long\">\n");
            }
            "title" => {
                meta_tags.push_str(&format!("<title>{}</title>\n", escape_html_text(value)));
            }
            "script" => {
                script_tags.push_str(&format!(
                    "<script src=\"{}\"></script>\n",
                    escape_html_attr(value)
                ));
            }
            _ => {}
        }
    }

    let global_block = if is_any {
        concat!(
            "<script>\n",
            "self.GLOBAL = {\n",
            "  isWindow: function() { return true; },\n",
            "  isWorker: function() { return false; },\n",
            "  isShadowRealm: function() { return false; },\n",
            "};\n",
            "</script>\n",
        )
    } else {
        ""
    };

    Some(format!(
        "<!doctype html>\n\
         <meta charset=utf-8>\n\
         {meta_tags}\
         {global_block}\
         <script src=\"/resources/testharness.js\"></script>\n\
         <script src=\"/resources/testharnessreport.js\"></script>\n\
         {script_tags}\
         <div id=log></div>\n\
         <script src=\"/{relative_path}\"></script>\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meta_block() {
        let js = "// META: title=A <title>\n\
                  // META: script=/common/utils.js\n\
                  //META: timeout=long\n\
                  'use strict';\n\
                  // META: script=ignored-after-code.js\n";
        let html = wrapper_html_for_js_test("dom/foo.any.js", js).unwrap();
        assert!(html.contains("<title>A &lt;title></title>"));
        assert!(html.contains("<script src=\"/common/utils.js\"></script>"));
        assert!(html.contains("<meta name=\"timeout\" content=\"long\">"));
        assert!(!html.contains("ignored-after-code.js"));
        assert!(html.contains("self.GLOBAL"));
        assert!(html.contains("<script src=\"/dom/foo.any.js\"></script>"));
    }

    #[test]
    fn window_js_has_no_global_block() {
        let html = wrapper_html_for_js_test("dom/foo.window.js", "test(() => {});").unwrap();
        assert!(!html.contains("self.GLOBAL"));
        assert!(html.contains("<script src=\"/dom/foo.window.js\"></script>"));
    }

    #[test]
    fn worker_only_any_js_is_excluded() {
        let js = "// META: global=worker\ntest(() => {});";
        assert!(wrapper_html_for_js_test("dom/foo.any.js", js).is_none());
        let js = "// META: global=window,worker\ntest(() => {});";
        assert!(wrapper_html_for_js_test("dom/foo.any.js", js).is_some());
    }
}
