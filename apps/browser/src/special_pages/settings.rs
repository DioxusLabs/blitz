use std::sync::Arc;

use dioxus_native::prelude::*;

use super::NavigateFn;
use crate::config::ConfigStore;
use crate::history::{History, SyncStore};

pub fn render(
    _history: SyncStore<History>,
    config: Arc<ConfigStore>,
    _navigate: NavigateFn,
) -> Element {
    let theme = use_signal_sync(|| config.get("theme").unwrap_or_else(|| "light".into()));

    use_hook(|| {
        config.subscribe(move |key, value| {
            if key == "theme" {
                let mut t = theme;
                t.set(value.to_string());
            }
        });
    });

    let current = theme();
    let other: &'static str = if current == "dark" { "light" } else { "dark" };
    let switch = move |_| config.set("theme", other);

    rsx!(
        h1 { "Settings" }
        div { class: "sp-section",
            h2 { "Appearance" }
            p { "Theme: " strong { "{current}" } }
            button { class: "sp-btn", onclick: switch, "Switch to {other}" }
        }
        div { class: "sp-section",
            h2 { "About" }
            p { class: "sp-muted",
                "Settings persist for the current session only. On-disk persistence is coming."
            }
        }
    )
}
