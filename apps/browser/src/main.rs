// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]
#![allow(clippy::collapsible_if)]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::cell::RefCell;

use std::rc::Rc;
use std::sync::{atomic::AtomicUsize, atomic::Ordering as Ao, Arc};

use blitz_traits::shell::ShellProvider;
use dioxus_core::Task;
use dioxus_native::{prelude::*, NodeHandle, SubDocumentAttr};

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

fn use_sync_store<T: Send + Sync + 'static>(value: impl FnOnce() -> T) -> SyncStore<T> {
    use_hook(|| Store::new_maybe_sync(value()))
}

fn app() -> Element {
    let home_url = use_hook(|| Url::parse("https://html.duckduckgo.com").unwrap());

    let mut url_input_handle = use_signal(|| None);
    let mut webview_node_handle: Signal<Option<NodeHandle>> = use_signal(|| None);
    let mut url_input_value = use_signal(|| home_url.to_string());
    let mut is_focussed = use_signal(|| false);
    let block_mouse_up = use_hook(|| Rc::new(RefCell::new(false)));
    let mut history: SyncStore<History> = use_sync_store(|| History::new(home_url.clone()));

    let html_source: Signal<String> = use_signal(String::new);

    let net_provider = use_context::<Arc<StdNetProvider>>();
    #[cfg(feature = "vello")]
    let mut show_fps = use_signal(|| false);
    let loader = use_hook(|| Rc::new(DocumentLoader::new(net_provider, history, html_source)));
    let content_doc = loader.doc;

    let loader_for_load = loader.clone();
    let load_current_url = use_callback(move |_| {
        let request = (*history.current_url().read()).clone();
        *url_input_value.write_unchecked() = request.url.to_string();

        if let Some(handle) = &*webview_node_handle.peek() {
            let node_id = handle.node_id();
            let mut doc = handle.doc_mut();
            if let Some(sub_doc) = doc
                .get_node_mut(node_id)
                .and_then(|node| node.element_data_mut())
                .and_then(|el| el.sub_doc_data_mut())
            {
                let mut sub_doc = sub_doc.inner_mut();
                sub_doc.clear_focus();
            }
        }

        println!("Loading {}...", &request.url.as_str());
        loader_for_load.load_document(request);
    });

    use_effect(move || load_current_url(()));

    let back_action = use_callback(move |_| history.go_back());
    let forward_action = use_callback(move |_| history.go_forward());
    let home_action = use_callback(move |_| history.navigate(Request::get(home_url.clone())));
    let refresh_action = load_current_url;
    let open_action =
        use_callback(move |_| open_in_external_browser(&history.current_url().read()));
    let mut menu_open = use_signal(|| false);

    let view_source_action = use_callback(move |_| {
        menu_open.set(false);
        let source = html_source.read().clone();
        if source.is_empty() {
            return;
        }

        // Update URL bar to show view-source:// URL
        let current_url = history.current_url().read().url.to_string();
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
            font_ctx: Some(loader.font_ctx.clone()),
            media_type: None,
        };
        let mut document = HtmlDocument::from_html(view_source_html, config).into_inner();
        if let Some(parent_id) = document.get_element_by_id("source") {
            let mut mutator = document.mutate();
            let text_node = mutator.create_text_node(&source);
            mutator.append_children(parent_id, &[text_node]);
        }
        *loader.doc.write_unchecked() = Some(SubDocumentAttr::new(document));
    });

    #[cfg(feature = "screenshot")]
    let screenshot_action = use_callback(move |_| {
        menu_open.set(false);
        async move {
            let Some(path) = capture::try_get_save_path("PNG Image", "png").await else {
                return;
            };

            if let Some(handle) = webview_node_handle() {
                let node_id = handle.node_id();
                let mut doc = handle.doc_mut();
                if let Some(sub_doc) = doc
                    .get_node_mut(node_id)
                    .and_then(|node| node.element_data_mut())
                    .and_then(|el| el.sub_doc_data_mut())
                {
                    let sub_doc = sub_doc.inner();
                    capture::capture_screenshot(&sub_doc, &path);
                }
            }
        }
    });

    #[cfg(feature = "capture")]
    let capture_action = use_callback(move |_| {
        menu_open.set(false);
        async move {
            let Some(path) = capture::try_get_save_path("AnyRender Scene", "scene").await else {
                return;
            };

            if let Some(handle) = webview_node_handle() {
                let node_id = handle.node_id();
                let mut doc = handle.doc_mut();
                if let Some(sub_doc) = doc
                    .get_node_mut(node_id)
                    .and_then(|node| node.element_data_mut())
                    .and_then(|el| el.sub_doc_data_mut())
                {
                    let sub_doc = sub_doc.inner();
                    capture::capture_anyrender_scene(&sub_doc, &path);
                }
            }
        }
    });

    let devtools_action = use_callback(move |_| {
        menu_open.set(false);
        if let Some(handle) = webview_node_handle() {
            let node_id = handle.node_id();
            let mut doc = handle.doc_mut();
            if let Some(sub_doc) = doc
                .get_node_mut(node_id)
                .and_then(|node| node.element_data_mut())
                .and_then(|el| el.sub_doc_data_mut())
            {
                let mut sub_doc = sub_doc.inner_mut();
                sub_doc.devtools_mut().toggle_highlight_hover();
            }
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

    #[cfg(feature = "vello")]
    let fps_overlay_el = rsx!(if show_fps() {
        fps_overlay::FpsOverlay {}
    });
    #[cfg(not(feature = "vello"))]
    let fps_overlay_el = rsx!();

    rsx!(
        div { id: "frame",
              padding_top: TOP_PAD,
              padding_bottom: BOTTOM_PAD,
              class: if IS_MOBILE {
                "mobile"
              } else {
                ""
              },
            title { "Blitz Browser" }
            document::Link { rel: "stylesheet", href: BROWSER_UI_STYLES }

            // Toolbar
            div { class: "urlbar",
                IconButton { icon: icons::BACK_ICON, action: back_action }
                if !IS_MOBILE {
                    IconButton { icon: icons::FORWARDS_ICON, action: forward_action }
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
                    onblur: move |_evt| {
                        *is_focussed.write() = false;
                    },
                    onfocus: move |_evt| {
                        *is_focussed.write() = true;
                        if let Some(handle) = url_input_handle() {
                            let node_id = handle.node_id();
                            let mut doc = handle.doc_mut();
                            doc.with_text_input(node_id, |mut driver| driver.select_all());
                        }
                    },
                    onpointerdown: {
                        let block_mouse_up = block_mouse_up.clone();
                        move |_evt| {
                            *block_mouse_up.borrow_mut() = !is_focussed();
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
                                core::mem::drop(handle.set_focus(false));
                            }
                            let req = req_from_string(&url_input_value.read());
                            if let Some(req) = req {
                                history.navigate(req);
                            } else {
                                println!("Error parsing URL {}", &*url_input_value.read());
                            }
                        }
                    },
                    oninput: move |evt| { *url_input_value.write() = evt.value() },
                }

                div { class: "menu-wrapper",
                    IconButton { icon: icons::MENU_ICON, action: move |_| menu_open.toggle(), active: menu_open() },
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
                        }
                    }
                }
            }

            // Web content
            web-view {
                class: "webview",
                "__webview_document": content_doc(),
                onmounted: move |evt: Event<MountedData>| {
                    let node_handle = evt.downcast::<NodeHandle>().unwrap();
                    *webview_node_handle.write() = Some(node_handle.clone());
                },
            }
            {fps_overlay_el}
        }
    )
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
            println!("Failed to open URL: {}", err);
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

    fn refresh(&mut self) {
        // Trigger change detection without actually changing the URL
        let _ = self.current().write();
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
}

// impl Clone for DocumentLoader {
//     fn clone(&self) -> Self {
//         Self {
//             font_ctx: self.font_ctx.clone(),
//             net_provider: self.net_provider.clone(),
//             status: self.status,
//             request_id_counter: AtomicUsize::new(self.request_id_counter.load(Ao::SeqCst)),
//             doc: self.doc,
//             history: self.history,
//         }
//     }
// }

impl DocumentLoader {
    fn new(
        net_provider: Arc<StdNetProvider>,
        history: SyncStore<History>,
        html_source: Signal<String>,
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
        }
    }

    fn load_document(&self, req: Request) {
        let request_id = self.request_id_counter.fetch_add(1, Ao::Relaxed);
        let net_provider = Arc::clone(&self.net_provider);
        let font_ctx = self.font_ctx.clone();
        let status = self.status;
        let doc_signal = self.doc;
        let history = self.history;
        let html_source = self.html_source;

        if let DocumentLoaderStatus::Loading { task, .. } = *self.status.peek() {
            task.cancel();
        };

        let task = spawn(async move {
            let request = net_provider.fetch_async(req);

            let response = request.await;

            match *status.peek() {
                DocumentLoaderStatus::Loading {
                    request_id: stored_req_id,
                    ..
                } if request_id == stored_req_id => {
                    // Do nothing
                }
                _ => {
                    println!("Ignoring load as it is not the most recent navigation request");
                }
            };

            match response {
                Ok((resolved_url, bytes)) => {
                    println!("Loaded {}", resolved_url);
                    let config = DocumentConfig {
                        viewport: None,
                        base_url: Some(resolved_url),
                        ua_stylesheets: None,
                        net_provider: Some(net_provider as _), // FIXME
                        navigation_provider: Some(Arc::new(BrowserNavProvider { history })),
                        shell_provider: Some(consume_context::<Arc<dyn ShellProvider>>()),
                        html_parser_provider: Some(Arc::new(HtmlProvider)),
                        font_ctx: Some(font_ctx),
                        media_type: None,
                    };

                    let html = if bytes.is_empty() {
                        include_str!("../assets/404.html")
                    } else {
                        str::from_utf8(&bytes).unwrap()
                    };

                    *html_source.write_unchecked() = html.to_string();

                    let document = HtmlDocument::from_html(html, config).into_inner();
                    *doc_signal.write_unchecked() = Some(SubDocumentAttr::new(document));
                }
                Err(err) => {
                    println!("Error loading document {:?}", err);

                    let error_msg = format!("{err:?}");

                    let config = DocumentConfig {
                        viewport: None,
                        base_url: None,
                        ua_stylesheets: None,
                        net_provider: Some(net_provider as _),
                        navigation_provider: Some(Arc::new(BrowserNavProvider { history })),
                        shell_provider: Some(consume_context::<Arc<dyn ShellProvider>>()),
                        html_parser_provider: Some(Arc::new(HtmlProvider)),
                        font_ctx: Some(font_ctx),
                        media_type: None,
                    };

                    let error_html = include_str!("../assets/error.html");
                    let mut document = HtmlDocument::from_html(error_html, config).into_inner();
                    if let Some(text_node) = document
                        .get_element_by_id("error")
                        .and_then(|el| document.get_node(el))
                        .and_then(|node| node.children.first().copied())
                    {
                        document.mutate().set_node_text(text_node, &error_msg);
                    }
                    *doc_signal.write_unchecked() = Some(SubDocumentAttr::new(document));
                }
            }
            // do something with result
        });

        *self.status.write_unchecked() = DocumentLoaderStatus::Loading { request_id, task };
    }
}
