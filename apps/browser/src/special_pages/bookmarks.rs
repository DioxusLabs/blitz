use std::sync::Arc;

use dioxus_native::prelude::*;

use super::NavigateFn;
use crate::config::ConfigStore;
use crate::history::{History, SyncStore};

pub fn render(
    _history: SyncStore<History>,
    _config: Arc<ConfigStore>,
    _navigate: NavigateFn,
) -> Element {
    rsx!(
        h1 { "Bookmarks" }
        div { class: "sp-section",
            p { class: "sp-muted", "Coming soon. Bookmark management will live here." }
        }
    )
}
