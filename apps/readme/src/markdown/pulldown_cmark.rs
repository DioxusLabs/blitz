//! Render the readme.md using the gpu renderer

use pulldown_cmark::{Options, Parser};

pub(crate) fn markdown_to_html(contents: String) -> String {
    // Set up options and parser.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_GFM);
    let parser = Parser::new_ext(&contents, options);

    // Write to String buffer.
    let mut body_html = String::new();
    pulldown_cmark::html::push_html(&mut body_html, parser);

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
