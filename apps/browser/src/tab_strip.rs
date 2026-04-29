use blitz_traits::net::Url;
use dioxus_native::prelude::*;

use crate::icons::{PLUS_ICON, icon_data_url};
use crate::tab::{Tab, TabId, tab_title_or_url};

#[cfg(target_os = "macos")]
const TABSTRIP_CLASS: &str = "tabstrip merged-titlebar";
#[cfg(not(target_os = "macos"))]
const TABSTRIP_CLASS: &str = "tabstrip";

#[component]
pub fn TabStrip(
    tabs: Signal<Vec<Tab>>,
    active_tab_id: Signal<TabId>,
    home_url: Url,
    open_new_tab: Callback<Url>,
) -> Element {
    let plus_light = use_hook(|| icon_data_url(PLUS_ICON, "#1a1a1a"));
    let plus_dark = use_hook(|| icon_data_url(PLUS_ICON, "#e6e6e6"));

    let switch_tab = use_callback(move |id: TabId| {
        active_tab_id.set(id);
    });

    let close_tab = use_callback(move |id: TabId| {
        let mut tabs_w = tabs.write();
        let current_active = *active_tab_id.peek();
        let idx = tabs_w.iter().position(|t| t.id == id).unwrap_or(0);
        tabs_w.remove(idx);
        if current_active == id {
            let new_idx = if idx < tabs_w.len() {
                idx
            } else {
                tabs_w.len().saturating_sub(1)
            };
            if let Some(t) = tabs_w.get(new_idx) {
                let new_id = t.id;
                drop(tabs_w);
                active_tab_id.set(new_id);
            }
        }
    });

    rsx!(
        div { class: TABSTRIP_CLASS,
            for tab in tabs() {
                {
                    let is_active = tab.id == active_tab_id();
                    let tab_id = tab.id;
                    let title = tab_title_or_url(&tab);
                    let tab_count = tabs.read().len();
                    rsx!(
                        div {
                            key: "{tab_id}",
                            class: if is_active { "tab tab--active" } else { "tab" },
                            title: "{title}",
                            onclick: move |_| switch_tab(tab_id),
                            span { class: "tab__title", "{title}" }
                            span { class: "tab__tooltip", "{title}" }
                            if tab_count > 1 {
                                div {
                                    class: "tab__close",
                                    onclick: move |evt| { evt.stop_propagation(); close_tab(tab_id); },
                                    "×"
                                }
                            }
                        }
                    )
                }
            }
            div {
                class: "tab-new",
                onclick: move |_| open_new_tab(home_url.clone()),
                img { class: "tab-new-icon urlbar-icon-light", src: plus_light.clone() }
                img { class: "tab-new-icon urlbar-icon-dark", src: plus_dark.clone() }
            }
        }
    )
}
