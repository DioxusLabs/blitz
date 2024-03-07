//! Render google.com!

use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app() -> Element {
    let content = tokio::runtime::Handle::current().block_on(async move {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0")
            .build()
            .unwrap();

        client
            .get("https://google.com")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    });
    let content = include_str!("./google_bits/google.html");

    rsx! {
        div { dangerous_inner_html: "{content}" }
    }
}
