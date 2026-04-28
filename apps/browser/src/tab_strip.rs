use blitz_traits::net::Url;
use dioxus_native::prelude::*;

use crate::tab::{Tab, TabId, tab_title_or_url};

#[component]
pub fn TabStrip(
    tabs: Signal<Vec<Tab>>,
    active_tab_id: Signal<TabId>,
    home_url: Url,
    open_new_tab: Callback<Url>,
) -> Element {
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
        div { class: "tabstrip",
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
                "+"
            }
        }
    )
}
