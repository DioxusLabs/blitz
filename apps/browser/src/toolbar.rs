use std::{cell::RefCell, rc::Rc, sync::Arc};

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::net::{Request, Url};
use dioxus_native::{NodeHandle, SubDocumentAttr, prelude::*};

use crate::about_pages::AboutPage;
use crate::history::HistoryNav;
use crate::icons::{self, IconButton};
use crate::nav::{is_enter_key, open_in_external_browser, req_from_string};
use crate::tab::{Tab, TabId, TabStoreExt, TabStoreImplExt, active_tab};
use crate::url_suggestions::{Suggestion, SuggestionKind, UrlSuggester, UrlSuggestions};
use crate::{IS_MOBILE, StdNetProvider};

#[component]
pub fn Toolbar(
    url_input_handle: Signal<Option<NodeHandle>>,
    mut url_input_value: Signal<String>,
    tabs: Store<Vec<Tab>>,
    active_tab_id: Signal<TabId>,
    open_new_tab: Callback<Url>,
    mut show_fps: Signal<bool>,
) -> Element {
    let home_url = use_hook(|| AboutPage::NewTab.parsed_url());
    let mut is_focused = use_signal(|| false);
    let block_mouse_up = use_hook(|| Rc::new(RefCell::new(false)));
    let mut menu_open = use_signal(|| false);
    #[cfg(feature = "cache")]
    let net_provider = use_context::<Arc<StdNetProvider>>();

    let mut selected_suggestion = use_signal::<Option<usize>>(|| None);

    let suggester = use_context::<UrlSuggester>();
    let suggestions = suggester.suggestions();
    use_effect(move || suggester.set_query(url_input_value.read().clone()));

    use_effect(move || {
        let active_id = active_tab_id();
        let tab = active_tab(tabs, active_id);
        *url_input_value.write_unchecked() = tab.nav_history().current_url().read().url.to_string();
    });

    let clear_document_focus = use_callback(move |_| {
        let tab = active_tab(tabs, active_tab_id());
        let node_handle_lens = tab.node_handle();
        let node_handle_guard = node_handle_lens.peek();
        if let Some(handle) = (*node_handle_guard).as_ref() {
            let node_id = handle.node_id();
            let mut doc = handle.doc_mut();
            if let Some(sub_doc) = doc
                .get_node_mut(node_id)
                .and_then(|node| node.element_data_mut())
                .and_then(|el| el.sub_doc_data_mut())
            {
                sub_doc.inner_mut().clear_focus();
            }
        }
    });

    // Closing the dropdown is implicit: its visibility is gated on `is_focused`.
    let blur_urlbar = use_callback(move |_| {
        if let Some(handle) = url_input_handle() {
            drop(handle.set_focus(false));
        }
        *is_focused.write() = false;
        selected_suggestion.set(None);
    });

    // Single path through which the urlbar commits a navigation (Enter or pick).
    let navigate_from_urlbar = use_callback(move |req: Request| {
        blur_urlbar(());
        clear_document_focus(());
        active_tab(tabs, active_tab_id()).navigate(req);
    });

    let on_pick = use_callback(move |s: Suggestion| {
        // Literal and Search rows fall back to the bare-input parse path,
        // which already handles "URL or DuckDuckGo search" itself. The Literal
        // row is here so users can return to "what Enter would do" after
        // moving through the list with arrow keys.
        let req = match s.kind {
            SuggestionKind::History(entry) => Some(Request::get(entry.url)),
            SuggestionKind::Literal | SuggestionKind::Search => {
                req_from_string(&url_input_value.read())
            }
        };
        match req {
            Some(req) => navigate_from_urlbar(req),
            None => blur_urlbar(()),
        }
    });

    let move_selection_down = use_callback(move |_| {
        let len = suggestions.read().len();
        if len == 0 {
            return;
        }
        let next = match selected_suggestion() {
            Some(i) => Some((i + 1) % len),
            None => Some(0),
        };
        selected_suggestion.set(next);
    });

    let move_selection_up = use_callback(move |_| {
        let len = suggestions.read().len();
        if len == 0 {
            return;
        }
        let next = match selected_suggestion() {
            Some(0) | None => Some(len - 1),
            Some(i) => Some(i - 1),
        };
        selected_suggestion.set(next);
    });

    let submit_urlbar = use_callback(move |_| {
        if let Some(i) = selected_suggestion() {
            if let Some(s) = suggestions.read().get(i).cloned() {
                on_pick.call(s);
                return;
            }
        }
        match req_from_string(&url_input_value.read()) {
            Some(req) => navigate_from_urlbar(req),
            None => {
                tracing::warn!("Error parsing URL {}", &*url_input_value.read());
                blur_urlbar(());
            }
        }
    });

    let back_action = use_callback(move |_| {
        clear_document_focus(());
        active_tab(tabs, active_tab_id()).go_back();
    });
    let forward_action = use_callback(move |_| {
        clear_document_focus(());
        active_tab(tabs, active_tab_id()).go_forward();
    });
    let home_action = use_callback(move |_| {
        clear_document_focus(());
        active_tab(tabs, active_tab_id()).navigate(Request::get(home_url.clone()));
    });
    let refresh_action = use_callback(move |_| {
        clear_document_focus(());
        active_tab(tabs, active_tab_id()).reload();
    });
    let nav_about = use_callback(move |page: AboutPage| {
        menu_open.set(false);
        clear_document_focus(());
        // NewTab always gets a fresh tab; for other about pages, focus an
        // existing tab already showing the page if there is one, otherwise
        // open it in a new tab.
        if page == AboutPage::NewTab {
            open_new_tab(page.parsed_url());
            return;
        }
        let existing = tabs.iter().find_map(|tab| {
            let history = tab.nav_history();
            let url_signal = history.current_url();
            let cur = url_signal.read();
            (AboutPage::from_url(&cur.url) == Some(page)).then(|| tab.tab_id())
        });
        match existing {
            Some(id) => active_tab_id.set(id),
            None => open_new_tab(page.parsed_url()),
        }
    });
    let open_action = use_callback(move |_| {
        menu_open.set(false);
        open_in_external_browser(
            &active_tab(tabs, active_tab_id())
                .nav_history()
                .current_url()
                .read(),
        );
    });

    let view_source_action = use_callback(move |_| {
        menu_open.set(false);
        let tab = active_tab(tabs, active_tab_id());
        let source = tab.html_source().cloned();
        if source.is_empty() {
            return;
        }
        let current_url = tab.nav_history().current_url().read().url.to_string();
        *url_input_value.write() = format!("view-source://{current_url}");

        let view_source_html = include_str!("../assets/view-source.html");
        let config = DocumentConfig {
            viewport: None,
            base_url: None,
            ua_stylesheets: None,
            net_provider: None,
            navigation_provider: None,
            shell_provider: None,
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            font_ctx: Some(tab.loader_rc().font_ctx.clone()),
            media_type: None,
        };
        let mut document = HtmlDocument::from_html(view_source_html, config).into_inner();
        if let Some(parent_id) = document.get_element_by_id("source") {
            let mut mutator = document.mutate();
            let text_node = mutator.create_text_node(&source);
            mutator.append_children(parent_id, &[text_node]);
        }
        *tab.document().write_unchecked() = Some(SubDocumentAttr::new(document));
    });

    #[cfg(feature = "screenshot")]
    let screenshot_action = use_callback(move |_| {
        menu_open.set(false);
        let node_handle = active_tab(tabs, active_tab_id()).node_handle();
        async move {
            let Some(path) = crate::capture::try_get_save_path("PNG Image", "png").await else {
                return;
            };
            if let Some(handle) = node_handle.cloned() {
                let node_id = handle.node_id();
                let mut doc = handle.doc_mut();
                if let Some(sub_doc) = doc
                    .get_node_mut(node_id)
                    .and_then(|node| node.element_data_mut())
                    .and_then(|el| el.sub_doc_data_mut())
                {
                    crate::capture::capture_screenshot(&sub_doc.inner(), &path);
                }
            }
        }
    });

    #[cfg(feature = "capture")]
    let capture_action = use_callback(move |_| {
        menu_open.set(false);
        let node_handle = active_tab(tabs, active_tab_id()).node_handle();
        async move {
            let Some(path) = crate::capture::try_get_save_path("AnyRender Scene", "scene").await
            else {
                return;
            };
            if let Some(handle) = node_handle.cloned() {
                let node_id = handle.node_id();
                let mut doc = handle.doc_mut();
                if let Some(sub_doc) = doc
                    .get_node_mut(node_id)
                    .and_then(|node| node.element_data_mut())
                    .and_then(|el| el.sub_doc_data_mut())
                {
                    crate::capture::capture_anyrender_scene(&sub_doc.inner(), &path);
                }
            }
        }
    });

    let devtools_action = use_callback(move |_| {
        menu_open.set(false);
        let node_handle = active_tab(tabs, active_tab_id()).node_handle();
        if let Some(handle) = node_handle.cloned() {
            let node_id = handle.node_id();
            let mut doc = handle.doc_mut();
            if let Some(sub_doc) = doc
                .get_node_mut(node_id)
                .and_then(|node| node.element_data_mut())
                .and_then(|el| el.sub_doc_data_mut())
            {
                sub_doc.inner_mut().devtools_mut().toggle_highlight_hover();
            }
        }
    });

    #[cfg(feature = "screenshot")]
    let screenshot_item = rsx!(
        div { class: "menu-item", onclick: move |_| screenshot_action(()),
            img { class: "menu-item-icon", src: icons::CAMERA_ICON }
            "Capture Screenshot"
        }
    );
    #[cfg(not(feature = "screenshot"))]
    let screenshot_item = rsx!();

    #[cfg(feature = "capture")]
    let capture_item = rsx!(
        div { class: "menu-item", onclick: move |_| capture_action(()),
            img { class: "menu-item-icon", src: icons::CAMERA_ICON }
            "Capture AnyRender Archive"
        }
    );
    #[cfg(not(feature = "capture"))]
    let capture_item = rsx!();

    #[cfg(feature = "vello")]
    let fps_toggle_item = rsx!(
        div { class: "menu-item", onclick: move |_| {
            menu_open.set(false);
            show_fps.toggle();
        }, "Toggle FPS" }
    );
    #[cfg(not(feature = "vello"))]
    let fps_toggle_item = rsx!();

    #[cfg(feature = "cache")]
    let clear_cache_item = {
        let clear_cache_action = use_callback(move |_| {
            menu_open.set(false);
            let net = net_provider.clone();
            async move { net.clear_cache().await }
        });
        rsx!(
            div { class: "menu-item", onclick: move |_| clear_cache_action(()),
                "Clear Cache"
            }
        )
    };
    #[cfg(not(feature = "cache"))]
    let clear_cache_item = rsx!();

    let current_tab = active_tab(tabs, active_tab_id());
    let has_back = current_tab.nav_history().has_back();
    let has_forward = current_tab.nav_history().has_forward();

    rsx!(
        div { class: "urlbar",
            IconButton {
                icon: icons::BACK_ICON,
                action: back_action,
                disabled: !has_back,
            }
            if !IS_MOBILE {
                IconButton {
                    icon: icons::FORWARDS_ICON,
                    action: forward_action,
                    disabled: !has_forward,
                }
            }
            IconButton { icon: icons::REFRESH_ICON, action: refresh_action }
            if !IS_MOBILE {
                IconButton { icon: icons::HOME_ICON, action: home_action }
            }
            div { class: "urlbar-input-wrapper",
                input {
                    class: "urlbar-input",
                    "type": "text",
                    name: "url",
                    value: url_input_value(),
                    onmounted: move |evt: Event<MountedData>| {
                        let node_handle = evt.downcast::<NodeHandle>().unwrap();
                        *url_input_handle.write() = Some(node_handle.clone());
                    },
                    onblur: move |_| {
                        *is_focused.write() = false;
                    },
                    onfocus: move |_| {
                        *is_focused.write() = true;
                        if let Some(handle) = url_input_handle() {
                            let node_id = handle.node_id();
                            let mut doc = handle.doc_mut();
                            doc.with_text_input(node_id, |mut driver| driver.select_all());
                        }
                    },
                    onpointerdown: {
                        let block_mouse_up = block_mouse_up.clone();
                        move |_| {
                            *block_mouse_up.borrow_mut() = !is_focused();
                        }
                    },
                    onpointermove: {
                        let block_mouse_up = block_mouse_up.clone();
                        move |evt| {
                            if *block_mouse_up.borrow() {
                                evt.prevent_default();
                            }
                        }
                    },
                    onpointerup: move |evt| {
                        if *block_mouse_up.borrow() {
                            evt.prevent_default();
                        }
                    },
                    onkeydown: move |evt| {
                        let key = evt.key();
                        match &key {
                            Key::ArrowDown => {
                                evt.prevent_default();
                                move_selection_down(());
                            }
                            Key::ArrowUp => {
                                evt.prevent_default();
                                move_selection_up(());
                            }
                            Key::Escape => {
                                evt.prevent_default();
                                blur_urlbar(());
                            }
                            k if is_enter_key(k) => {
                                evt.prevent_default();
                                submit_urlbar(());
                            }
                            _ => {}
                        }
                    },
                    oninput: move |evt| {
                        *url_input_value.write() = evt.value();
                        selected_suggestion.set(Some(0));
                    },
                }
                if is_focused() && !suggestions.read().is_empty() {
                    UrlSuggestions {
                        suggestions,
                        selected_idx: selected_suggestion,
                        on_pick,
                    }
                }
            }
            div { class: "menu-wrapper",
                IconButton {
                    icon: icons::MENU_ICON,
                    action: move |_| menu_open.toggle(),
                    active: menu_open(),
                }
                if menu_open() {
                    div { class: "menu-dropdown",
                        div { class: "menu-item", onclick: move |_| nav_about(AboutPage::Settings), "Settings" }
                        div { class: "menu-item", onclick: move |_| nav_about(AboutPage::History), "History" }
                        div { class: "menu-item", onclick: move |_| nav_about(AboutPage::Bookmarks), "Bookmarks" }
                        div { class: "menu-item-separator" }
                        div { class: "menu-item", onclick: open_action,
                            img { class: "menu-item-icon", src: icons::EXTERNAL_LINK_ICON }
                            "Open in External Browser"
                        }
                        div { class: "menu-item", onclick: move |_| view_source_action(()),
                            img { class: "menu-item-icon", src: icons::CODE_ICON }
                            "View Source"
                        }
                        {screenshot_item}
                        {capture_item}
                        div { class: "menu-item", onclick: move |_| devtools_action(()), "Toggle DevTools" }
                        {fps_toggle_item}
                        {clear_cache_item}
                    }
                }
            }
        }
    )
}
