// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]
#![allow(clippy::collapsible_if)]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering as Ao};

use blitz_traits::shell::ShellProvider;
use dioxus_core::Task;
use dioxus_native::{NodeHandle, SubDocumentAttr, prelude::*};

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{Body, Entry, EntryValue, FormData, Method, Request, Url};
use linebender_resource_handle::Blob;

type StdNetProvider = blitz_net::Provider;

mod icons;
use icons::IconButton;

static BROWSER_UI_STYLES: Asset = asset!("../assets/browser.css");

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    dioxus_native::launch(app)
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

    let net_provider = use_context::<Arc<StdNetProvider>>();
    let loader = use_hook(|| Rc::new(DocumentLoader::new(net_provider, history)));
    let content_doc = loader.doc;

    let load_current_url = use_callback(move |_| {
        let request = (*history.current_url().read()).clone();
        *url_input_value.write_unchecked() = request.url.to_string();
        println!("Loading {}...", &request.url.as_str());
        loader.load_document(request);
    });

    use_effect(move || load_current_url(()));

    let back_action = use_callback(move |_| history.go_back());
    let forward_action = use_callback(move |_| history.go_forward());
    let home_action = use_callback(move |_| history.navigate(Request::get(home_url.clone())));
    let refresh_action = load_current_url;
    let open_action =
        use_callback(move |_| open_in_external_browser(&history.current_url().read()));

    let devtools_action = use_callback(move |_| {
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

    rsx!(
        div { id: "frame",
            title { "Blitz Browser" }
            document::Link { rel: "stylesheet", href: BROWSER_UI_STYLES }

            // Toolbar
            div { class: "urlbar",
                IconButton { icon: icons::BACK_ICON, action: back_action }
                IconButton { icon: icons::FORWARDS_ICON, action: forward_action }
                IconButton { icon: icons::REFRESH_ICON, action: refresh_action }
                IconButton { icon: icons::HOME_ICON, action: home_action }
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
                    onmousedown: {
                        let block_mouse_up = block_mouse_up.clone();
                        move |_evt| {
                            *block_mouse_up.borrow_mut() = !is_focussed();
                        }
                    },
                    onmousemove: {
                        let block_mouse_up = block_mouse_up.clone();
                        move |evt| {
                            if *block_mouse_up.borrow() {
                                evt.prevent_default();
                            }
                        }
                    },
                    onmouseup: move |evt| {
                        if *block_mouse_up.borrow() {
                            evt.prevent_default();
                        }
                    },
                    onkeydown: move |evt| {
                        if evt.key() == Key::Enter {
                            evt.prevent_default();
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
                IconButton { icon: icons::EXTERNAL_LINK_ICON, action: open_action }
                IconButton { icon: icons::MENU_ICON, action: devtools_action }
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
        String::from("application/x-www-form-urlencoded"),
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
    fn new(net_provider: Arc<StdNetProvider>, history: SyncStore<History>) -> Self {
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
        }
    }

    fn load_document(&self, req: Request) {
        let request_id = self.request_id_counter.fetch_add(1, Ao::Relaxed);
        let net_provider = Arc::clone(&self.net_provider);
        let font_ctx = self.font_ctx.clone();
        let status = self.status;
        let doc_signal = self.doc;
        let history = self.history;

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
                    };

                    let html = if bytes.is_empty() {
                        include_str!("../assets/404.html")
                    } else {
                        str::from_utf8(&bytes).unwrap()
                    };

                    // println!("{}", html);

                    let document = HtmlDocument::from_html(html, config).into_inner();
                    *doc_signal.write_unchecked() = Some(SubDocumentAttr::new(document));
                }
                Err(err) => {
                    println!("Error loading document {:?}", err);
                }
            }
            // do something with result
        });

        *self.status.write_unchecked() = DocumentLoaderStatus::Loading { request_id, task };
    }
}
