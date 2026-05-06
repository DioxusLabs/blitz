use std::time::{Duration, SystemTime};

use blitz_traits::net::{Request, Url};
use dioxus_native::prelude::*;

use crate::browser_history::{
    BrowsingHistoryStoreExt, HistoryEntry, HistoryService, format_elapsed,
};
use crate::nav::{is_enter_key, req_from_string};
use crate::tab::Favicon;

// How often the history page refreshes its "Just now / N min ago" labels.
const HISTORY_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

const NEWTAB_CSS: Asset = asset!("../assets/about-newtab.css");
const STUB_CSS: Asset = asset!("../assets/about-stub.css");
const HISTORY_CSS: Asset = asset!("../assets/about-history.css");
const BLITZ_LOGO: Asset = asset!("../assets/blitz-logo.png");
const BLITZ_LOGO_WTIH_TEXT: Asset = asset!("../assets/blitz-logo-with-text3.svg");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AboutPage {
    NewTab,
    Settings,
    History,
    Bookmarks,
}

impl AboutPage {
    pub fn from_url(url: &Url) -> Option<Self> {
        if url.scheme() != "about" {
            return None;
        }
        match url.path() {
            "newtab" | "" => Some(Self::NewTab),
            "settings" => Some(Self::Settings),
            "history" => Some(Self::History),
            "bookmarks" => Some(Self::Bookmarks),
            _ => None,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::NewTab => "New Tab",
            Self::Settings => "Settings",
            Self::History => "History",
            Self::Bookmarks => "Bookmarks",
        }
    }

    pub const fn url_str(self) -> &'static str {
        match self {
            Self::NewTab => "about:newtab",
            Self::Settings => "about:settings",
            Self::History => "about:history",
            Self::Bookmarks => "about:bookmarks",
        }
    }

    #[allow(
        clippy::unwrap_used,
        reason = "URL strings are static literals — Url::parse cannot fail"
    )]
    pub fn parsed_url(self) -> Url {
        Url::parse(self.url_str()).unwrap()
    }
}

#[component]
pub fn AboutPageView(page: AboutPage, on_navigate: Callback<Request>) -> Element {
    match page {
        AboutPage::NewTab => rsx!(NewTabPage { on_navigate }),
        AboutPage::Settings => rsx!(StubPage { name: "Settings" }),
        AboutPage::History => rsx!(HistoryPage { on_navigate }),
        AboutPage::Bookmarks => rsx!(StubPage { name: "Bookmarks" }),
    }
}

#[component]
fn NewTabPage(on_navigate: Callback<Request>) -> Element {
    let mut query = use_signal(String::new);
    rsx! {
        document::Link { rel: "stylesheet", href: NEWTAB_CSS }
        div { class: "about-newtab",
            div { class: "container",
                img { src: BLITZ_LOGO_WTIH_TEXT, alt: "Blitz" }
                input {
                    class: "search-input",
                    r#type: "text",
                    name: "q",
                    autofocus: true,
                    value: "{query}",
                    oninput: move |evt| query.set(evt.value()),
                    onkeydown: move |evt| {
                        if is_enter_key(&evt.key()) {
                            evt.prevent_default();
                            let q = query.read().clone();
                            if !q.is_empty() {
                                if let Some(req) = req_from_string(&q) {
                                    on_navigate(req);
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn HistoryPage(on_navigate: Callback<Request>) -> Element {
    // `now` is bumped on a fixed cadence so each row's elapsed-time label
    // refreshes while the page is open. Computing the label here (rather than
    // per-row) keeps the wall-clock read in one place.
    let mut now = use_signal(SystemTime::now);
    use_future(move || async move {
        loop {
            tokio::time::sleep(HISTORY_REFRESH_INTERVAL).await;
            now.set(SystemTime::now());
        }
    });
    let now = now();

    let history = use_context::<HistoryService>();
    let browsing_history = history.browsing();

    let entries_lens = browsing_history.entries();
    let entries = entries_lens.read();
    rsx! {
        document::Link { rel: "stylesheet", href: HISTORY_CSS }
        div { class: "about-history",
            div { class: "toolbar",
                h1 { "History" }
                if !entries.is_empty() {
                    button {
                        class: "clear-btn",
                        onclick: move |_| history.clear(),
                        "Clear history"
                    }
                }
            }
            if entries.is_empty() {
                p { class: "empty", "No history yet." }
            } else {
                ul {
                    for entry in entries.iter() {
                        HistoryEntryRow {
                            key: "{entry.id}",
                            entry: entry.clone(),
                            elapsed: format_elapsed(entry.visited_at, now),
                            on_navigate,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn HistoryEntryRow(
    entry: HistoryEntry,
    elapsed: String,
    on_navigate: Callback<Request>,
) -> Element {
    let url = entry.url.clone();
    rsx! {
        li {
            Favicon { url: entry.favicon_url.clone(), class: "favicon" }
            div { class: "entry-info",
                div { class: "entry-title",
                    a {
                        href: "#",
                        onclick: move |evt| {
                            evt.prevent_default();
                            on_navigate(Request::get(url.clone()));
                        },
                        "{entry.title}"
                    }
                }
                div { class: "entry-url", "{entry.url}" }
                div { class: "entry-time", "{elapsed}" }
            }
        }
    }
}

#[component]
fn StubPage(name: &'static str) -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: STUB_CSS }
        div { class: "about-stub",
            h1 { "{name}" }
            p { "Coming soon." }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_paths() {
        for (path, expected) in [
            ("about:newtab", AboutPage::NewTab),
            ("about:settings", AboutPage::Settings),
            ("about:history", AboutPage::History),
            ("about:bookmarks", AboutPage::Bookmarks),
        ] {
            let url = Url::parse(path).unwrap();
            assert_eq!(AboutPage::from_url(&url), Some(expected), "{path}");
        }
    }

    #[test]
    fn rejects_unknown() {
        assert_eq!(
            AboutPage::from_url(&Url::parse("about:nope").unwrap()),
            None
        );
        assert_eq!(
            AboutPage::from_url(&Url::parse("https://example.com").unwrap()),
            None
        );
    }

    #[test]
    fn url_roundtrip() {
        for p in [
            AboutPage::NewTab,
            AboutPage::Settings,
            AboutPage::History,
            AboutPage::Bookmarks,
        ] {
            assert_eq!(AboutPage::from_url(&p.parsed_url()), Some(p));
        }
    }
}
