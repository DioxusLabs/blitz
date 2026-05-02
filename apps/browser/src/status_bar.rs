use std::time::Duration;

use dioxus_native::prelude::*;

use crate::document_loader::DocumentLoaderStatus;
use crate::history::HistoryNav;
use crate::tab::{Tab, TabId, TabStoreExt, TabStoreImplExt, active_tab};

fn hovered_href<L>(tab: Store<Tab, L>) -> Option<String>
where
    L: Copy + Readable<Target = Tab> + 'static,
{
    let nh = tab.node_handle().peek_unchecked();
    let handle = (*nh).as_ref()?;
    let node_id = handle.node_id();
    // Skip this cycle if the event loop currently holds a mutable borrow.
    let doc = handle.try_doc()?;
    let sub_doc = doc
        .get_node(node_id)
        .and_then(|n| n.element_data())
        .and_then(|el| el.sub_doc_data())?;
    let inner = sub_doc.inner();
    let mut cur_id = inner.get_hover_node_id()?;
    loop {
        let node = inner.get_node(cur_id)?;
        if let Some(el) = node.element_data() {
            if el.name.local.as_ref() == "a" {
                return el
                    .attrs()
                    .iter()
                    .find(|a| a.name.local.as_ref() == "href")
                    .map(|a| a.value.clone());
            }
        }
        cur_id = node.layout_parent.get()?;
    }
}

#[component]
pub fn StatusBar(tabs: Store<Vec<Tab>>, active_tab_id: Signal<TabId>) -> Element {
    let mut hover_url: Signal<String> = use_signal(String::new);

    // Hover state lives inside blitz-dom's BaseDocument, not a Dioxus signal,
    // so we poll it at ~10 fps (same pattern as FpsOverlay).
    use_hook(move || {
        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                let tab = active_tab(tabs, active_tab_id());
                let raw_href = hovered_href(tab);
                // All doc borrows dropped here; safe to read history.
                let found = match raw_href {
                    None => String::new(),
                    Some(raw) => {
                        let base = tab.nav_history().current_url().read().url.clone();
                        base.join(&raw).map(|u| u.to_string()).unwrap_or(raw)
                    }
                };
                if found != *hover_url.read() {
                    hover_url.set(found);
                }
            }
        });
    });

    let tab = active_tab(tabs, active_tab_id());
    let is_loading = matches!(
        *tab.loader_rc().status.read(),
        DocumentLoaderStatus::Loading
    );

    let status_text = {
        let hov = hover_url.read();
        if !hov.is_empty() {
            hov.clone()
        } else if is_loading {
            format!("Loading {}…", tab.nav_history().current_url().read().url)
        } else {
            String::new()
        }
    };

    if status_text.is_empty() {
        return rsx!();
    }

    rsx!(div { class: "statusbar", "{status_text}" })
}
