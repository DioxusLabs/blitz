//! Render markdown to HTML via comrak, with syntect-based syntax highlighting.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use comrak::adapters::SyntaxHighlighterAdapter;
use comrak::plugins::syntect::SyntectAdapter;
use comrak::{Options, markdown_to_html_with_plugins, options};

/// Wraps `SyntectAdapter` to inject a `<span class="codeblock-lang">` inside
/// each `<code>` element so CSS can render a corner label.
struct LangLabelAdapter(SyntectAdapter);

impl SyntaxHighlighterAdapter for LangLabelAdapter {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        self.0.write_highlighted(output, lang, code)
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        self.0.write_pre_tag(output, attributes)
    }

    fn write_code_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        // Emit a real <span> label rather than a data-attr — Blitz's pseudo
        // element resolver doesn't handle `content: attr(...)` yet, so the
        // visible label has to live in the DOM.
        if let Some(class) = attributes.get("class")
            && let Some(rest) = class.strip_prefix("language-")
        {
            let lang = rest.split(',').next().unwrap_or(rest).trim();
            if !lang.is_empty() {
                write!(output, "<span class=\"codeblock-lang\">{lang}</span>")?;
            }
        }
        self.0.write_code_tag(output, attributes)
    }
}

pub(crate) fn markdown_to_html(contents: String) -> String {
    let syntax_highlighter = LangLabelAdapter(SyntectAdapter::new(None));
    let mut plugins = options::Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&syntax_highlighter);

    let body_html = markdown_to_html_with_plugins(
        &contents,
        &Options {
            extension: options::Extension {
                strikethrough: true,
                tagfilter: false,
                table: true,
                autolink: true,
                tasklist: true,
                superscript: false,
                header_id_prefix: None,
                footnotes: false,
                description_lists: false,
                front_matter_delimiter: None,
                multiline_block_quotes: false,
                alerts: true,
                ..options::Extension::default()
            },
            render: options::Render {
                r#unsafe: true,
                tasklist_classes: true,
                ..options::Render::default()
            },
            ..Options::default()
        },
        &plugins,
    );

    // Strip trailing newlines in code blocks
    let body_html = body_html.replace("\n</code", "</code");

    format!(
        r#"
        <!DOCTYPE html>
        <html>
        <body>
        <div class="markdown-body">{body_html}</div>
        </body>
        </html>
        "#
    )
}
