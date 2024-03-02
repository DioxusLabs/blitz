//! Render the readme.md using the gpu renderer

use comrak::{markdown_to_html, Options};
use dioxus::prelude::*;

fn root() -> Element {
    let contents = include_str!("../README.md");
    let html = markdown_to_html(contents, &Options::default());
    rsx! {
        div { dangerous_inner_html: "{html}" }
    }
}

fn main() {
    dioxus_blitz::launch(root);
}
