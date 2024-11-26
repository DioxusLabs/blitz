//! Render google.com!

use blitz_shell::Config;

fn main() {
    blitz_shell::launch_static_html_cfg(
        &get_html(),
        Config {
            stylesheets: Vec::new(),
            base_url: Some(String::from("https://www.google.com/")),
        },
    );
}

fn get_html() -> std::borrow::Cow<'static, str> {
    // Fetch HTML from google.com
    // let content = tokio::runtime::Handle::current().block_on(async move {
    //     let client = reqwest::Client::builder()
    //         .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0")
    //         .build()
    //         .unwrap();

    //     client
    //         .get("https://google.com")
    //         .send()
    //         .await
    //         .unwrap()
    //         .text()
    //         .await
    //         .unwrap()
    // });

    // Load static HTML
    let content = include_str!("./assets/google.html");

    content.into()
}
