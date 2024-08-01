//! Render the readme.md using the gpu renderer

use std::path::Path;

use comrak::{markdown_to_html, ExtensionOptionsBuilder, Options, RenderOptionsBuilder};
use dioxus_blitz::Config;

fn main() {
    let (base_url, contents) = std::env::args()
        .skip(1)
        .next()
        .map(|path| {
            let base_path = std::path::absolute(Path::new(&path)).unwrap();
            let base_path = base_path.parent().unwrap().to_string_lossy();
            let base_url = format!("file://{}/", base_path);
            let contents = std::fs::read_to_string(path).unwrap();
            (base_url, contents)
        })
        .unwrap_or({
            let base_url = "https://raw.githubusercontent.com/DioxusLabs/blitz/main/".to_string();
            let contents = include_str!("../README.md").to_string();
            (base_url, contents)
        });

    let stylesheet = include_str!("./assets/github-markdown-light.css");
    let body_html = markdown_to_html(
        &contents,
        &Options {
            extension: ExtensionOptionsBuilder::default()
                .strikethrough(true)
                .tagfilter(false)
                .table(true)
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
            base_url: Some(base_url),
        },
    );
}
