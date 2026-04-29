use std::sync::Arc;

use blitz_traits::net::Url;
use dioxus_native::prelude::*;

use super::NavigateFn;
use crate::config::ConfigStore;
use crate::history::{History, SyncStore};

static BLITZ_LOGO: Asset = asset!("../../assets/blitz-logo.png");

pub fn render(
    _history: SyncStore<History>,
    _config: Arc<ConfigStore>,
    navigate: NavigateFn,
) -> Element {
    let logo_url = format!("file://{}", BLITZ_LOGO.resolve().display());
    let mut search_query = use_signal(String::new);

    let nav = navigate.clone();
    let do_search = move || {
        let q = search_query.read().clone();
        if q.is_empty() {
            return;
        }
        #[allow(clippy::unwrap_used)]
        let mut url = Url::parse("https://html.duckduckgo.com/html/").unwrap();
        url.query_pairs_mut().append_pair("q", &q);
        nav(url);
    };
    let do_search_click = {
        let do_search = do_search.clone();
        move |_| do_search()
    };
    let do_search_key = move |evt: Event<KeyboardData>| {
        if matches!(evt.key(), Key::Enter) {
            do_search();
        }
    };

    rsx!(
        div { class: "sp-hero",
            img { class: "sp-logo", src: logo_url, alt: "Blitz" }
            h1 { "Blitz" }
            div { class: "sp-search",
                input {
                    r#type: "text",
                    class: "sp-search-input",
                    placeholder: "Search with DuckDuckGo",
                    value: "{search_query}",
                    oninput: move |evt| search_query.set(evt.value()),
                    onkeydown: do_search_key,
                }
                button { class: "sp-search-btn", onclick: do_search_click, "Search" }
            }
        }
    )
}
