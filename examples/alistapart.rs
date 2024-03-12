//! Render alistapart.com

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

fn main() {
    let html = ureq::get("https://alistapart.com")
        .set("User-Agent", USER_AGENT)
        .call()
        .unwrap()
        .into_string()
        .unwrap();

    dioxus_blitz::launch_static_html(&html);
}
