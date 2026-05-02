use blitz_traits::net::Url;
use dioxus_native::prelude::*;

use crate::tab::{Favicon, Tab, TabId, TabStoreExt, TabStoreImplExt, tab_display_title};

#[cfg(target_os = "macos")]
const TABSTRIP_CLASS: &str = "tabstrip merged-titlebar";
#[cfg(not(target_os = "macos"))]
const TABSTRIP_CLASS: &str = "tabstrip";

#[component]
pub fn TabStrip(
    mut tabs: Store<Vec<Tab>>,
    active_tab_id: Signal<TabId>,
    home_url: Url,
    open_new_tab: Callback<Url>,
) -> Element {
    let switch_tab = use_callback(move |id: TabId| {
        active_tab_id.set(id);
    });

    let close_tab = use_callback(move |id: TabId| {
        let current_active = active_tab_id();
        let idx = tabs.iter().position(|tab| tab.tab_id() == id).unwrap_or(0);
        let len_after = tabs.len() - 1;
        tabs.remove(idx);
        if current_active == id {
            let new_idx = if idx < len_after {
                idx
            } else {
                len_after.saturating_sub(1)
            };
            if let Some(new_id) = tabs.get(new_idx).map(|tab| tab.tab_id()) {
                active_tab_id.set(new_id);
            }
        }
    });

    let tab_count = tabs.len();
    rsx!(
        div { class:TABSTRIP_CLASS,
            for tab_lens in tabs.iter() {
                {
                    let tab_id = tab_lens.tab_id();
                    let is_active = tab_id == active_tab_id();
                    let title = tab_display_title(tab_lens);
                    let favicon_url = tab_lens.favicon_url().cloned();
                    rsx!(
                        div {
                            key: "{tab_id}",
                            class: if is_active { "tab tab--active" } else { "tab" },
                            title: "{title}",
                            onclick: move |_| switch_tab(tab_id),
                            Favicon { url: favicon_url, class: "tab__favicon" }
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
