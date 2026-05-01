use blitz_traits::net::{Request, Url};
use dioxus_native::prelude::*;

use crate::nav::{is_enter_key, req_from_string};

const NEWTAB_CSS: Asset = asset!("../assets/about-newtab.css");
const STUB_CSS: Asset = asset!("../assets/about-stub.css");
const BLITZ_LOGO: Asset = asset!("../assets/blitz-logo.png");

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
        AboutPage::History => rsx!(StubPage { name: "History" }),
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
                img { src: BLITZ_LOGO, alt: "Blitz" }
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
