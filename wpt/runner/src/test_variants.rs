//! WPT URL variants declared by HTML metadata or JS `// META:` directives.

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use html5ever::tokenizer::{
    BufferQueue, StartTag, Token, TokenSink, TokenSinkResult, Tokenizer, states::RawKind,
};

use crate::test_runners::js_test_variants;

pub struct Test {
    pub path: PathBuf,
    pub url: String,
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

pub fn test_url(path: &str, variant: &str) -> String {
    let path = if let Some(stem) = path.strip_suffix(".any.js") {
        format!("{stem}.any.html")
    } else if let Some(stem) = path.strip_suffix(".window.js") {
        format!("{stem}.window.html")
    } else {
        path.to_string()
    };
    format!("{path}{variant}")
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

pub fn test_variants(path: &str, source: &str) -> Vec<String> {
    let mut variants = if path.ends_with(".js") {
        js_test_variants(source)
    } else if !source.contains("variant") {
        Vec::new()
    } else if is_xml(path) {
        roxmltree::Document::parse_with_options(
            source,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            },
        )
        .map(|doc| {
            doc.descendants()
                .filter(|node| {
                    node.has_tag_name(("http://www.w3.org/1999/xhtml", "meta"))
                        && node.attribute("name") == Some("variant")
                })
                .filter_map(|node| node.attribute("content").map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
    } else {
        let input = BufferQueue::default();
        input.push_back(source.into());
        let tokenizer = Tokenizer::new(VariantSink::default(), Default::default());
        let _ = tokenizer.feed(&input);
        tokenizer.end();
        tokenizer.sink.0.into_inner()
    };

    for variant in &variants {
        assert!(
            variant.is_empty()
                || (variant.starts_with(['?', '#'])
                    && variant.len() > 1
                    && !variant.starts_with("?#")),
            "Invalid WPT variant in {path}: {variant:?}"
        );
    }
    if variants.is_empty() {
        variants.push(String::new());
    }
    variants
}

pub fn expand_test(wpt_dir: &Path, path: PathBuf, suffix: &str) -> Vec<Test> {
    let relative_path = path
        .strip_prefix(wpt_dir)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let variants = if suffix.is_empty() {
        std::fs::read_to_string(&path)
            .map(|source| test_variants(&relative_path, &source))
            .unwrap_or_else(|_| vec![String::new()])
    } else {
        vec![suffix.to_string()]
    };
    variants
        .into_iter()
        .map(|variant| Test {
            path: path.clone(),
            url: test_url(&relative_path, &variant),
        })
        .collect()
}

#[derive(Default)]
struct VariantSink(RefCell<Vec<String>>);

impl TokenSink for VariantSink {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let Token::TagToken(tag) = token else {
            return TokenSinkResult::Continue;
        };
        if tag.kind != StartTag {
            return TokenSinkResult::Continue;
        }
        match tag.name.as_ref() {
            "script" => return TokenSinkResult::RawData(RawKind::ScriptData),
            "style" | "xmp" | "iframe" | "noembed" | "noframes" => {
                return TokenSinkResult::RawData(RawKind::Rawtext);
            }
            "title" | "textarea" => return TokenSinkResult::RawData(RawKind::Rcdata),
            "plaintext" => return TokenSinkResult::Plaintext,
            "meta" => {
                let attr = |name: &str| {
                    tag.attrs
                        .iter()
                        .find(|attr| &*attr.name.local == name)
                        .map(|attr| attr.value.as_ref())
                };
                if attr("name") == Some("variant")
                    && let Some(content) = attr("content")
                {
                    self.0.borrow_mut().push(content.to_string());
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_variants_replace_default_and_decode_entities() {
        assert_eq!(
            test_variants(
                "test.html",
                r##"
            <meta content='?a=1&amp;b=2' name=variant>
            <META NAME="variant" CONTENT="#fragment">
            <meta name=variant content="">
            <meta name=variant>
        "##
            ),
            ["?a=1&b=2", "#fragment", ""]
        );
    }

    #[test]
    fn ignores_metadata_in_comments_and_scripts() {
        assert_eq!(
            test_variants(
                "test.html",
                r#"
            <!-- <meta name=variant content='?comment'> -->
            <script>const tag = '<meta name=variant content="?script">';</script>
            <textarea><meta name=variant content='?text'></textarea>
            <meta name=variant content='?real'>
        "#
            ),
            ["?real"]
        );
    }

    #[test]
    fn xml_variants_require_html_namespace() {
        assert_eq!(
            test_variants(
                "test.svg",
                r#"
            <svg xmlns="http://www.w3.org/2000/svg" xmlns:h="http://www.w3.org/1999/xhtml">
                <meta name="variant" content="?ignored"/>
                <h:meta name="variant" content="?a=1&amp;b=2"/>
            </svg>
        "#
            ),
            ["?a=1&b=2"]
        );
    }

    #[test]
    fn xhtml_variants_with_doctype_and_prefixed_elements() {
        assert_eq!(
            test_variants(
                "test.xhtml",
                r#"<!DOCTYPE html>
            <h:html xmlns:h="http://www.w3.org/1999/xhtml">
            <h:head><h:meta name="variant" content="?xml#part"/></h:head>
            <h:body><h:script>test(() => {});</h:script></h:body></h:html>"#
            ),
            ["?xml#part"]
        );
    }

    #[test]
    fn js_variants_only_from_initial_metadata_block() {
        assert_eq!(
            test_variants(
                "test.any.js",
                "// META: variant=?first\n// META: variant=?second#hash\ntest(() => {});\n// META: variant=?ignored"
            ),
            ["?first", "?second#hash"]
        );
    }

    #[test]
    fn undeclared_variants_run_once() {
        assert_eq!(test_variants("test.html", "<p>No variants</p>"), [""]);
        assert_eq!(test_variants("test.window.js", "test(() => {});"), [""]);
    }

    #[test]
    fn wrapper_urls_preserve_suffix() {
        assert_eq!(
            test_url("dom/test.any.js", "?a=1#b"),
            "dom/test.any.html?a=1#b"
        );
        assert_eq!(
            test_url("dom/test.window.js", "?a=1"),
            "dom/test.window.html?a=1"
        );
        assert_eq!(split_test_url("test.html?a=1#b"), ("test.html", "?a=1#b"));
        assert!(is_xml("test.xht?a=1"));
    }

    #[test]
    fn script_location_uses_wrapper_url_and_variant() {
        let mut doc = blitz_vibey_script::ScriptDocument::from_html(
            "",
            blitz_dom::DocumentConfig {
                base_url: Some(format!(
                    "http://dummy.local/{}",
                    test_url("dom/test.any.js", "?a=1&b=2#part")
                )),
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

    #[test]
    #[should_panic(expected = "Invalid WPT variant")]
    fn rejects_empty_query() {
        test_variants("test.html", "<meta name=variant content='?#hash'>");
    }
}
