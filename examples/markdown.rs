//! Render the readme.md using the gpu renderer

use std::{ffi::OsStr, path::Path};

use comrak::{markdown_to_html, ExtensionOptionsBuilder, Options};
use dioxus_blitz::Config;

fn main() {
    let stylesheet = include_str!("./google_bits/github-markdown-light.css");
    let contents = include_str!("../README.md");
    // let contents = include_str!("../../taffy/README.md");
    let body_html = markdown_to_html(
        contents,
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

    let f = file!();

    let mut base_path = Path::new(OsStr::new(&f)).to_owned();
    base_path.pop();

    dioxus_blitz::launch_static_html_cfg(
        &html,
        Config {
            stylesheets: vec![String::from(stylesheet)],
            base_url: Some(format!(
                "file://{}",
                base_path.as_os_str().to_string_lossy()
            )),
        },
    );
}