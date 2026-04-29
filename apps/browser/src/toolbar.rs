use std::{cell::RefCell, rc::Rc, sync::Arc};

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::navigation::NavigationOptions;
use blitz_traits::net::{Body, Entry, EntryValue, FormData, Method, Request, Url};
use dioxus_native::{NodeHandle, SubDocumentAttr, prelude::*};

use crate::document_loader::LoadTrigger;
use crate::history::HistoryNav;
use crate::icons::{self, IconButton};
use crate::tab::{Tab, TabId, active_tab};
use crate::{HOME_URL_STR, IS_MOBILE, StdNetProvider};

#[component]
pub fn Toolbar(
    url_input_handle: Signal<Option<NodeHandle>>,
    mut url_input_value: Signal<String>,
    tabs: Signal<Vec<Tab>>,
    active_tab_id: Signal<TabId>,
    mut show_fps: Signal<bool>,
) -> Element {
    #[allow(clippy::unwrap_used)] // HOME_URL_STR is a hard-coded valid URL
    let home_url = use_hook(|| Url::parse(HOME_URL_STR).unwrap());
    let mut is_focused = use_signal(|| false);
    let block_mouse_up = use_hook(|| Rc::new(RefCell::new(false)));
    let mut menu_open = use_signal(|| false);
    #[cfg(feature = "cache")]
    let net_provider = use_context::<Arc<StdNetProvider>>();

    // Sync URL bar when active tab changes
    use_effect(move || {
        let aid = active_tab_id();
        let tab = active_tab(&tabs, aid);
        *url_input_value.write_unchecked() = tab.history.current_url().read().url.to_string();
    });

    let load_current_url = use_callback(move |_| {
        let tab = active_tab(&tabs, *active_tab_id.peek());
        let request = (*tab.history.current_url().read()).clone();
        *url_input_value.write_unchecked() = request.url.to_string();

        if let Some(handle) = &*tab.node_handle.peek() {
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

        tracing::info!("Loading {}", request.url.as_str());
        let mut lt = tab.load_trigger;
        lt.set(LoadTrigger::BackForward(request));
    });

    let back_action = use_callback(move |_| {
        let mut tab = active_tab(&tabs, *active_tab_id.peek());
        tab.history.go_back();
        let req = (*tab.history.current_url().read()).clone();
        let mut lt = tab.load_trigger;
        lt.set(LoadTrigger::BackForward(req));
    });
    let forward_action = use_callback(move |_| {
        let mut tab = active_tab(&tabs, *active_tab_id.peek());
        tab.history.go_forward();
        let req = (*tab.history.current_url().read()).clone();
        let mut lt = tab.load_trigger;
        lt.set(LoadTrigger::BackForward(req));
    });
    let home_action = use_callback(move |_| {
        let tab = active_tab(&tabs, *active_tab_id.peek());
        let mut lt = tab.load_trigger;
        lt.set(LoadTrigger::NewNav(Request::get(home_url.clone())));
    });
    let refresh_action = load_current_url;
    let open_action = use_callback(move |_| {
        menu_open.set(false);
        open_in_external_browser(
            &active_tab(&tabs, *active_tab_id.peek())
                .history
                .current_url()
                .read(),
        );
    });

    let view_source_action = use_callback(move |_| {
        menu_open.set(false);
        let tab = active_tab(&tabs, *active_tab_id.peek());
        let source = tab.html_source.read().clone();
        if source.is_empty() {
            return;
        }
        let current_url = tab.history.current_url().read().url.to_string();
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
            font_ctx: Some(tab.loader.font_ctx.clone()),
            media_type: None,
        };
        let mut document = HtmlDocument::from_html(view_source_html, config).into_inner();
        if let Some(parent_id) = document.get_element_by_id("source") {
            let mut mutator = document.mutate();
            let text_node = mutator.create_text_node(&source);
            mutator.append_children(parent_id, &[text_node]);
        }
        *tab.document.write_unchecked() = Some(SubDocumentAttr::new(document));
    });

    #[cfg(feature = "screenshot")]
    let screenshot_action = use_callback(move |_| {
        menu_open.set(false);
        let node_handle = active_tab(&tabs, *active_tab_id.peek()).node_handle;
        async move {
            let Some(path) = crate::capture::try_get_save_path("PNG Image", "png").await else {
                return;
            };
            if let Some(handle) = node_handle() {
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
        let node_handle = active_tab(&tabs, *active_tab_id.peek()).node_handle;
        async move {
            let Some(path) = crate::capture::try_get_save_path("AnyRender Scene", "scene").await
            else {
                return;
            };
            if let Some(handle) = node_handle() {
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

    let go_special = use_callback(move |path: &'static str| {
        menu_open.set(false);
        let url_str = format!("about:{path}");
        #[allow(clippy::unwrap_used)]
        // path is a hard-coded &'static str, always a valid about: URL
        let url = Url::parse(&url_str).unwrap();
        // Update the address bar manually since about: pages don't commit to history.
        *url_input_value.write() = url_str;
        let tab = active_tab(&tabs, *active_tab_id.peek());
        let mut lt = tab.load_trigger;
        lt.set(LoadTrigger::NewNav(Request::get(url)));
    });

    let devtools_action = use_callback(move |_| {
        menu_open.set(false);
        let node_handle = active_tab(&tabs, *active_tab_id.peek()).node_handle;
        if let Some(handle) = node_handle() {
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
            icons::MenuItemIcon { icon: icons::CAMERA_ICON }
            "Capture Screenshot"
        }
    );
    #[cfg(not(feature = "screenshot"))]
    let screenshot_item = rsx!();

    #[cfg(feature = "capture")]
    let capture_item = rsx!(
        div { class: "menu-item", onclick: move |_| capture_action(()),
            icons::MenuItemIcon { icon: icons::CAMERA_ICON }
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

    let current_tab = active_tab(&tabs, active_tab_id());
    let has_back = current_tab.history.has_back();
    let has_forward = current_tab.history.has_forward();

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
            input {
                class: "urlbar-input",
                "type": "text",
                name: "url",
                value: url_input_value(),
                onmounted: move |evt: Event<MountedData>| {
                    #[allow(clippy::unwrap_used)] // NodeHandle is always present on onmounted events
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
                    let is_enter = match evt.key() {
                        Key::Enter => true,
                        Key::Character(s) if s == "\n" => true,
                        _ => false,
                    };
                    if is_enter {
                        evt.prevent_default();
                        if let Some(handle) = url_input_handle() {
                            drop(handle.set_focus(false));
                        }
                        let req = req_from_string(&url_input_value.read());
                        if let Some(req) = req {
                            let tab = active_tab(&tabs, *active_tab_id.peek());
                            let mut lt = tab.load_trigger;
                            lt.set(LoadTrigger::NewNav(req));
                        } else {
                            tracing::warn!("Error parsing URL {}", &*url_input_value.read());
                        }
                    }
                },
                oninput: move |evt| { *url_input_value.write() = evt.value() },
            }
            div { class: "menu-wrapper",
                IconButton {
                    icon: icons::MENU_ICON,
                    action: move |_| menu_open.toggle(),
                    active: menu_open(),
                }
                if menu_open() {
                    div { class: "menu-dropdown",
                        div { class: "menu-item", onclick: move |_| go_special("settings"), "Settings" }
                        div { class: "menu-item", onclick: move |_| go_special("history"), "History" }
                        div { class: "menu-item", onclick: move |_| go_special("bookmarks"), "Bookmarks" }
                        div { class: "menu-item", onclick: open_action,
                            icons::MenuItemIcon { icon: icons::EXTERNAL_LINK_ICON }
                            "Open in External Browser"
                        }
                        div { class: "menu-item", onclick: move |_| view_source_action(()),
                            icons::MenuItemIcon { icon: icons::CODE_ICON }
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

pub fn req_from_string(url_s: &str) -> Option<Request> {
    if let Ok(url) = Url::parse(url_s) {
        return Some(Request::get(url));
    };

    let contains_space = url_s.contains(' ');
    let contains_dot = url_s.contains('.');
    if contains_dot && !contains_space {
        if let Ok(url) = Url::parse(&format!("https://{}", &url_s)) {
            return Some(Request::get(url));
        }
    }

    Some(synthesize_duckduckgo_search_req(url_s))
}

fn synthesize_duckduckgo_search_req(query: &str) -> Request {
    NavigationOptions::new(
        #[allow(clippy::unwrap_used)] // hard-coded valid URL
        Url::parse("https://html.duckduckgo.com/html/").unwrap(),
        Some(String::from("application/x-www-form-urlencoded")),
        0,
    )
    .set_method(Method::POST)
    .set_document_resource(Body::Form(FormData(vec![Entry {
        name: String::from("q"),
        value: EntryValue::String(query.to_string()),
    }])))
    .into_request()
}

pub fn open_in_external_browser(req: &Request) {
    if req.method == Method::GET && matches!(req.url.scheme(), "http" | "https" | "mailto") {
        if let Err(err) = webbrowser::open(req.url.as_str()) {
            tracing::error!("Failed to open URL: {}", err);
        }
    }
}
