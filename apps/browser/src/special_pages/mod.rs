use std::sync::Arc;

use blitz_traits::net::Url;
use dioxus_native::prelude::Element;

use crate::config::ConfigStore;
use crate::history::{History, SyncStore};

mod bookmarks;
mod history_page;
mod settings;
mod start;

/// Callback passed to special-page render functions so they can trigger tab navigation.
pub type NavigateFn = Arc<dyn Fn(Url) + Send + Sync>;

type RenderFn = fn(SyncStore<History>, Arc<ConfigStore>, NavigateFn) -> Element;

/// A type-erased Dioxus component for a special page.
///
/// `name` is a stable `&'static str` used as the Dioxus component key; changing it forces
/// a full remount (and hook-state reset) when the user navigates between page types.
#[derive(Clone)]
pub struct SpecialPageComponent {
    pub name: &'static str,
    pub render: Arc<dyn Fn() -> Element + Send + Sync>,
}

impl PartialEq for SpecialPageComponent {
    fn eq(&self, other: &Self) -> bool {
        // Two components are "equal" for Dioxus memoisation iff they are the same page type.
        self.name == other.name
    }
}

/// What a tab is currently displaying.
#[derive(Clone)]
pub enum TabContent {
    /// A blitz-dom sub-document rendered inside a `<web-view>`.
    Web,
    /// A native Dioxus component rendered directly into the browser UI's virtual DOM.
    Special(SpecialPageComponent),
}

/// Returns `(display_title, render_fn)` for a recognised `about:` URL, or `None`.
pub fn lookup(url: &Url) -> Option<(&'static str, RenderFn)> {
    if url.scheme() != "about" {
        return None;
    }
    let host = url.path().split('/').next().unwrap_or("");
    match host {
        "newtab" => Some(("New Tab", start::render)),
        "settings" => Some(("Settings", settings::render)),
        "history" => Some(("History", history_page::render)),
        "bookmarks" => Some(("Bookmarks", bookmarks::render)),
        _ => None,
    }
}
