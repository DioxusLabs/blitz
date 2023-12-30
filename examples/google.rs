//! Render google.com!

use dioxus::prelude::*;
use tokio::runtime::Handle;

fn main() {
    let cfg = stylo_dioxus::Config {
        stylesheets: vec![],
    };
    stylo_dioxus::launch_cfg(app, cfg);
}

fn app(cx: Scope) -> Element {
    let content = Handle::current().block_on(async move {
        reqwest::get("https://google.com")
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    });

    render! {
        div { dangerous_inner_html: "{content}" }
    }
}
