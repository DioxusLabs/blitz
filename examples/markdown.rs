//! Render the readme.md using the gpu renderer

use comrak::{markdown_to_html, Options};
use dioxus_blitz::Config;

fn main() {
    let stylesheet = include_str!("./google_bits/github-markdown-light.css");
    let contents = include_str!("../README.md");
    let body_html = markdown_to_html(contents, &Options::default());

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
        },
    );
}
