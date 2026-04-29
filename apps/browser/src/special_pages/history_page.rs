use std::sync::Arc;

use dioxus_native::prelude::*;

use super::NavigateFn;
use crate::config::ConfigStore;
use crate::history::{History, HistoryStoreExt, SyncStore};

pub fn render(
    history: SyncStore<History>,
    _config: Arc<ConfigStore>,
    navigate: NavigateFn,
) -> Element {
    let urls: Vec<_> = history.urls().read().clone();
    let titles: Vec<_> = history.titles().read().clone();

    let entries: Vec<_> = urls
        .iter()
        .zip(titles.iter())
        .filter(|(req, _)| req.url.scheme() != "about")
        .map(|(req, title)| {
            let url = req.url.clone();
            let display = title
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or(url.as_str())
                .to_string();
            (url, display)
        })
        .collect();

    rsx!(
        h1 { "History" }
        div { class: "sp-section",
            h2 { "Current tab" }
            if entries.is_empty() {
                p { class: "sp-muted", "No history yet." }
            } else {
                ul {
                    for (url, display) in entries {
                        {
                            let nav = navigate.clone();
                            let url_clone = url.clone();
                            rsx!(
                                li { key: "{url}",
                                    span {
                                        class: "sp-link",
                                        onclick: move |_| nav(url_clone.clone()),
                                        "{display}"
                                    }
                                }
                            )
                        }
                    }
                }
            }
        }
        div { class: "sp-section",
            h2 { "Coming soon" }
            p { class: "sp-muted",
                "Cross-tab history is not yet implemented. This page currently lists only the active tab's history."
            }
        }
    )
}
