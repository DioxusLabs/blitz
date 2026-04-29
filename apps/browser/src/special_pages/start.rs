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

    let nav = navigate.clone();
    let go_settings = move |_| {
        #[allow(clippy::unwrap_used)]
        nav(Url::parse("about:settings").unwrap());
    };
    let nav = navigate.clone();
    let go_history = move |_| {
        #[allow(clippy::unwrap_used)]
        nav(Url::parse("about:history").unwrap());
    };
    let go_bookmarks = move |_| {
        #[allow(clippy::unwrap_used)]
        navigate(Url::parse("about:bookmarks").unwrap());
    };

    rsx!(
        div { class: "sp-hero",
            img { class: "sp-logo", src: logo_url, alt: "Blitz" }
            h1 { "Blitz" }
            div { class: "sp-actions",
                button { class: "sp-btn", onclick: go_settings, "Settings" }
                button { class: "sp-btn", onclick: go_history, "History" }
                button { class: "sp-btn", onclick: go_bookmarks, "Bookmarks" }
            }
        }
    )
}
