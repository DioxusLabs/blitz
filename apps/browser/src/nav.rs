use blitz_traits::navigation::NavigationOptions;
use blitz_traits::net::{Body, Entry, EntryValue, FormData, Method, Request, Url};
use dioxus_native::prelude::Key;

pub fn req_from_string(url_s: &str) -> Option<Request> {
    if let Ok(url) = Url::parse(url_s) {
        return Some(Request::get(url));
    };

    let contains_space = url_s.contains(' ');
    let contains_dot = url_s.contains('.');
    if contains_dot && !contains_space {
        if let Ok(url) = Url::parse(&format!("https://{}", &url_s)) {
            return Some(Request::get(url));
        }
    }

    Some(synthesize_duckduckgo_search_req(url_s))
}

fn synthesize_duckduckgo_search_req(query: &str) -> Request {
    NavigationOptions::new(
        Url::parse("https://html.duckduckgo.com/html/").unwrap(),
        Some(String::from("application/x-www-form-urlencoded")),
        0,
    )
    .set_method(Method::POST)
    .set_document_resource(Body::Form(FormData(vec![Entry {
        name: String::from("q"),
        value: EntryValue::String(query.to_string()),
    }])))
    .into_request()
}

pub fn open_in_external_browser(req: &Request) {
    if req.method == Method::GET && matches!(req.url.scheme(), "http" | "https" | "mailto") {
        if let Err(err) = webbrowser::open(req.url.as_str()) {
            tracing::error!("Failed to open URL: {}", err);
        }
    }
}

pub fn is_enter_key(key: &Key) -> bool {
    matches!(key, Key::Enter) || matches!(key, Key::Character(s) if s == "\n")
}
