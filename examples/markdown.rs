//! Render the readme.md using the gpu renderer

use comrak::{markdown_to_html, ExtensionOptionsBuilder, Options, RenderOptionsBuilder};
use dioxus_blitz::Config;

fn main() {
    let contents = std::env::args()
        .skip(1)
        .next()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .unwrap_or(include_str!("../README.md").to_string());

    let stylesheet = include_str!("./assets/github-markdown-light.css");
    let body_html = markdown_to_html(
        &contents,
        &Options {
            extension: ExtensionOptionsBuilder::default()
                .strikethrough(true)
                .tagfilter(false)
                .table(false)
                .autolink(true)
                .tasklist(false)
                .superscript(false)
                .header_ids(None)
                .footnotes(false)
                .description_lists(false)
                .front_matter_delimiter(None)
                .multiline_block_quotes(false)
                .build()
                .unwrap(),
            render: RenderOptionsBuilder::default()
                .unsafe_(true)
                .build()
                .unwrap(),
            ..Default::default()
        },
    );

    let html = format!(
        r#"
        <!DOCTYPE html>
        <html>
        <body>
        <div class="markdown-body">{}</div>
        </body>
        </html>
    "#,
        body_html
    );

    dioxus_blitz::launch_static_html_cfg(
        &html,
        Config {
            stylesheets: vec![String::from(stylesheet)],
            base_url: Some("https://raw.githubusercontent.com/DioxusLabs/blitz/main/".to_string()),
        },
    );
}
