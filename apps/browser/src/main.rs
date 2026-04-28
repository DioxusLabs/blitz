// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]
#![allow(clippy::collapsible_if)]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use blitz_traits::shell::ShellProvider;
use dioxus_core::Task;
use dioxus_native::{NodeHandle, SubDocumentAttr, prelude::*};

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{Body, Entry, EntryValue, FormData, Method, Request, Url};
use linebender_resource_handle::Blob;

type StdNetProvider = blitz_net::Provider;

#[cfg(any(feature = "screenshot", feature = "capture"))]
mod capture;

#[cfg(feature = "vello")]
mod fps_overlay;
mod icons;
use icons::IconButton;

static BROWSER_UI_STYLES: Asset = asset!("../assets/browser.css");
const IS_MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));
const HOME_URL_STR: &str = "about:newtab";

#[unsafe(no_mangle)]
#[cfg(target_os = "android")]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    dioxus_native::set_android_app(android_app);
    main()
}

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();
    dioxus_native::launch_cfg(app, vec![], Vec::new())
}

type SyncStore<T> = Store<T, CopyValue<T, SyncStorage>>;
type TabId = u64;

static TAB_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_tab_id() -> TabId {
    TAB_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
struct Tab {
    id: TabId,
    history: SyncStore<History>,
    loader: Rc<DocumentLoader>,
    document: Signal<Option<SubDocumentAttr>>,
    node_handle: Signal<Option<NodeHandle>>,
    html_source: Signal<String>,
    title: Signal<String>,
}

impl Tab {
    fn new(url: Url, net_provider: Arc<StdNetProvider>) -> Self {
        let id = next_tab_id();
        let history: SyncStore<History> = Store::new_maybe_sync(History::new(url));
        let html_source: Signal<String> = Signal::new(String::new());
        let title: Signal<String> = Signal::new(String::new());
        let loader = Rc::new(DocumentLoader::new(
            net_provider,
            history,
            html_source,
            title,
        ));
        let document = loader.doc;
        Tab {
            id,
            history,
            loader,
            document,
            node_handle: Signal::new(None),
            html_source,
            title,
        }
    }
}

fn active_tab(tabs: &Signal<Vec<Tab>>, active_id: TabId) -> Tab {
    let tabs_ref = tabs.read();
    tabs_ref
        .iter()
        .find(|t| t.id == active_id)
        .or_else(|| tabs_ref.first())
        .expect("tabs vec is never empty")
        .clone()
}

fn tab_title_or_url(tab: &Tab) -> String {
    let title = tab.title.read();
    if !title.trim().is_empty() {
        return title.clone();
    }
    tab.history.current_url().read().url.to_string()
}

fn app() -> Element {
    let home_url = use_hook(|| Url::parse(HOME_URL_STR).unwrap());
    let net_provider = use_context::<Arc<StdNetProvider>>();

    let url_input_handle: Signal<Option<NodeHandle>> = use_signal(|| None);
    let url_input_value = use_signal(|| home_url.to_string());

    let mut tabs: Signal<Vec<Tab>> =
        use_hook(|| Signal::new(vec![Tab::new(home_url.clone(), net_provider.clone())]));
    let mut active_tab_id: Signal<TabId> =
        use_hook(|| Signal::new(tabs.read().first().map(|t| t.id).unwrap_or(0)));

    let open_new_tab = use_callback(move |url: Url| {
        let new_tab = Tab::new(url, net_provider.clone());
        let new_id = new_tab.id;
        tabs.write().push(new_tab);
        active_tab_id.set(new_id);
        if let Some(handle) = url_input_handle() {
            drop(handle.set_focus(true));
        }
    });

    // HACK: Winit doesn't support "safe area" on Android yet.
    // So we just hardcode a fallback safe area.
    const TOP_PAD: &str = if cfg!(target_os = "android") {
        "30px"
    } else {
        ""
    };
    const BOTTOM_PAD: &str = if cfg!(target_os = "android") {
        "44px"
    } else {
        ""
    };

    let show_fps: Signal<bool> = use_signal(|| false);

    #[cfg(feature = "vello")]
    let fps_overlay_el = rsx!(if show_fps() {
        fps_overlay::FpsOverlay {}
    });
    #[cfg(not(feature = "vello"))]
    let fps_overlay_el = rsx!();

    let window_title = tab_title_or_url(&active_tab(&tabs, active_tab_id()));

    rsx!(
        div {
            id: "frame",
            padding_top: TOP_PAD,
            padding_bottom: BOTTOM_PAD,
            class: if IS_MOBILE { "mobile" } else { "" },
            title { "{window_title}" }
            document::Link { rel: "stylesheet", href: BROWSER_UI_STYLES }
            TabStrip {
                tabs,
                active_tab_id,
                home_url: home_url.clone(),
                open_new_tab,
            }
            Toolbar {
                url_input_handle,
                url_input_value,
                tabs,
                active_tab_id,
                show_fps,
            }
            for tab in tabs() {
                {
                    let mut tab_node_handle = tab.node_handle;
                    rsx!(
                        web-view {
                            key: "{tab.id}",
                            class: "webview",
                            style: if tab.id == active_tab_id() { "display: block" } else { "display: none" },
                            "__webview_document": tab.document.cloned(),
                            onmounted: move |evt: Event<MountedData>| {
                                let node_handle = evt.downcast::<NodeHandle>().unwrap();
                                *tab_node_handle.write() = Some(node_handle.clone());
                            },
                        }
                    )
                }
            }
            {fps_overlay_el}
            StatusBar { tabs, active_tab_id }
        }
    )
}

#[component]
fn TabStrip(
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
                            onclick: move |_| switch_tab(tab_id),
                            span { class: "tab__title", "{title}" }
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

#[component]
fn Toolbar(
    url_input_handle: Signal<Option<NodeHandle>>,
    mut url_input_value: Signal<String>,
    tabs: Signal<Vec<Tab>>,
    active_tab_id: Signal<TabId>,
    mut show_fps: Signal<bool>,
) -> Element {
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
        tab.loader.load_document(request);
    });

    use_effect(move || load_current_url(()));

    let back_action = use_callback(move |_| {
        active_tab(&tabs, *active_tab_id.peek()).history.go_back();
    });
    let forward_action = use_callback(move |_| {
        active_tab(&tabs, *active_tab_id.peek())
            .history
            .go_forward();
    });
    let home_action = use_callback(move |_| {
        active_tab(&tabs, *active_tab_id.peek())
            .history
            .navigate(Request::get(home_url.clone()));
    });
    let refresh_action = load_current_url;
    let open_action = use_callback(move |_| {
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
            let Some(path) = capture::try_get_save_path("PNG Image", "png").await else {
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
                    capture::capture_screenshot(&sub_doc.inner(), &path);
                }
            }
        }
    });

    #[cfg(feature = "capture")]
    let capture_action = use_callback(move |_| {
        menu_open.set(false);
        let node_handle = active_tab(&tabs, *active_tab_id.peek()).node_handle;
        async move {
            let Some(path) = capture::try_get_save_path("AnyRender Scene", "scene").await else {
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
                    capture::capture_anyrender_scene(&sub_doc.inner(), &path);
                }
            }
        }
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
                            active_tab(&tabs, *active_tab_id.peek()).history.navigate(req);
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

fn hovered_href(tab: &Tab) -> Option<String> {
    let nh = tab.node_handle.peek();
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
fn StatusBar(tabs: Signal<Vec<Tab>>, active_tab_id: Signal<TabId>) -> Element {
    let mut hover_url: Signal<String> = use_signal(String::new);

    // Hover state lives inside blitz-dom's BaseDocument, not a Dioxus signal,
    // so we poll it at ~10 fps (same pattern as FpsOverlay).
    use_hook(move || {
        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                let tab = active_tab(&tabs, *active_tab_id.peek());
                let raw_href = hovered_href(&tab);
                // All doc borrows dropped here; safe to read history.
                let found = match raw_href {
                    None => String::new(),
                    Some(raw) => {
                        let base = tab.history.current_url().read().url.clone();
                        base.join(&raw).map(|u| u.to_string()).unwrap_or(raw)
                    }
                };
                if found != *hover_url.read() {
                    hover_url.set(found);
                }
            }
        });
    });

    let tab = active_tab(&tabs, active_tab_id());
    let is_loading = matches!(
        *tab.loader.status.read(),
        DocumentLoaderStatus::Loading { .. }
    );

    let status_text = {
        let hov = hover_url.read();
        if !hov.is_empty() {
            hov.clone()
        } else if is_loading {
            format!("Loading {}…", tab.history.current_url().read().url)
        } else {
            String::new()
        }
    };

    if status_text.is_empty() {
        return rsx!();
    }

    rsx!(div { class: "statusbar", "{status_text}" })
}

fn req_from_string(url_s: &str) -> Option<Request> {
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

fn open_in_external_browser(req: &Request) {
    if req.method == Method::GET && matches!(req.url.scheme(), "http" | "https" | "mailto") {
        if let Err(err) = webbrowser::open(req.url.as_str()) {
            tracing::error!("Failed to open URL: {}", err);
        }
    }
}

#[derive(Store)]
struct History {
    urls: Vec<Request>,
    current: usize,
}

impl History {
    fn new(initial_url: Url) -> Self {
        Self {
            urls: vec![Request::get(initial_url)],
            current: 0,
        }
    }
}

#[store]
impl<Lens> Store<History, Lens> {
    fn current_idx(&self) -> usize {
        *self.current().read()
    }

    fn current_url(&self) -> impl Readable<Target = Request> {
        self.urls().get(self.current_idx()).unwrap()
    }

    fn has_back(&self) -> bool {
        self.current_idx() > 0
    }

    fn has_forward(&self) -> bool {
        self.current_idx() < self.urls().len() - 1
    }

    fn go_back(&mut self) {
        if self.has_back() {
            *self.current().write() -= 1;
        }
    }

    fn go_forward(&mut self) {
        if self.has_forward() {
            *self.current().write() += 1;
        }
    }

    fn navigate(&self, req: Request)
    where
        Lens: Writable,
    {
        let idx = self.current_idx();
        self.urls().write().truncate(idx + 1);
        self.urls().push(req);
        *self.current().write() += 1;
    }
}

struct BrowserNavProvider {
    history: SyncStore<History>,
}

impl NavigationProvider for BrowserNavProvider {
    fn navigate_to(&self, options: NavigationOptions) {
        self.history.navigate(options.into_request());
    }
}

enum DocumentLoaderStatus {
    Loading { request_id: usize, task: Task },
    Idle,
}

struct DocumentLoader {
    font_ctx: FontContext,
    net_provider: Arc<StdNetProvider>,
    status: Signal<DocumentLoaderStatus>,
    request_id_counter: AtomicUsize,
    doc: Signal<Option<SubDocumentAttr>>,
    history: SyncStore<History>,
    html_source: Signal<String>,
    title: Signal<String>,
}

fn make_doc_config(
    base_url: Option<String>,
    net_provider: Arc<StdNetProvider>,
    history: SyncStore<History>,
    font_ctx: FontContext,
) -> DocumentConfig {
    DocumentConfig {
        viewport: None,
        base_url,
        ua_stylesheets: None,
        net_provider: Some(net_provider as _),
        navigation_provider: Some(Arc::new(BrowserNavProvider { history })),
        shell_provider: Some(consume_context::<Arc<dyn ShellProvider>>()),
        html_parser_provider: Some(Arc::new(HtmlProvider)),
        font_ctx: Some(font_ctx),
        media_type: None,
    }
}

impl DocumentLoader {
    fn new(
        net_provider: Arc<StdNetProvider>,
        history: SyncStore<History>,
        html_source: Signal<String>,
        title: Signal<String>,
    ) -> Self {
        let mut font_ctx = FontContext::default();
        font_ctx
            .collection
            .register_fonts(Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None);

        Self {
            font_ctx,
            net_provider,
            status: Signal::new(DocumentLoaderStatus::Idle),
            request_id_counter: AtomicUsize::new(0),
            doc: Signal::new(None),
            history,
            html_source,
            title,
        }
    }

    fn load_document(&self, req: Request) {
        if req.url.scheme() == "about" && req.url.path() == "newtab" {
            if let DocumentLoaderStatus::Loading { task, .. } = *self.status.peek() {
                task.cancel();
            }
            let config = make_doc_config(
                None,
                Arc::clone(&self.net_provider),
                self.history,
                self.font_ctx.clone(),
            );
            let html = include_str!("../assets/start.html");
            *self.html_source.write_unchecked() = html.to_string();
            let document = HtmlDocument::from_html(html, config).into_inner();
            *self.title.write_unchecked() = String::new();
            *self.doc.write_unchecked() = Some(SubDocumentAttr::new(document));
            *self.status.write_unchecked() = DocumentLoaderStatus::Idle;
            return;
        }

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);
        let net_provider = Arc::clone(&self.net_provider);
        let font_ctx = self.font_ctx.clone();
        let status = self.status;
        let doc_signal = self.doc;
        let history = self.history;
        let html_source = self.html_source;
        let title = self.title;

        if let DocumentLoaderStatus::Loading { task, .. } = *self.status.peek() {
            task.cancel();
        };

        let task = spawn(async move {
            let response = net_provider.fetch_async(req).await;

            // Discard response if a newer navigation has started
            if let DocumentLoaderStatus::Loading {
                request_id: stored_id,
                ..
            } = *status.peek()
            {
                if request_id != stored_id {
                    tracing::debug!("Ignoring stale navigation response (id {request_id})");
                    return;
                }
            }

            match response {
                Ok((resolved_url, bytes)) => {
                    tracing::info!("Loaded {}", resolved_url);
                    let config =
                        make_doc_config(Some(resolved_url), net_provider, history, font_ctx);

                    let bytes_str;
                    let html: &str = if bytes.is_empty() {
                        include_str!("../assets/404.html")
                    } else {
                        bytes_str = String::from_utf8_lossy(&bytes);
                        &bytes_str
                    };

                    *html_source.write_unchecked() = html.to_string();

                    let document = HtmlDocument::from_html(html, config).into_inner();
                    let parsed_title = document
                        .find_title_node()
                        .map(|n| n.text_content())
                        .unwrap_or_default();
                    *title.write_unchecked() = parsed_title;
                    *doc_signal.write_unchecked() = Some(SubDocumentAttr::new(document));
                }
                Err(err) => {
                    tracing::error!("Error loading document: {:?}", err);

                    let error_msg = format!("{err:?}");
                    let config = make_doc_config(None, net_provider, history, font_ctx);

                    let error_html = include_str!("../assets/error.html");
                    let mut document = HtmlDocument::from_html(error_html, config).into_inner();
                    if let Some(text_node) = document
                        .get_element_by_id("error")
                        .and_then(|el| document.get_node(el))
                        .and_then(|node| node.children.first().copied())
                    {
                        document.mutate().set_node_text(text_node, &error_msg);
                    }
                    *title.write_unchecked() = String::new();
                    *doc_signal.write_unchecked() = Some(SubDocumentAttr::new(document));
                }
            }
            *status.write_unchecked() = DocumentLoaderStatus::Idle;
        });

        *status.write_unchecked() = DocumentLoaderStatus::Loading { request_id, task };
    }
}
