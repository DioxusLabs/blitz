// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering as Ao};

use blitz_traits::shell::ShellProvider;
use dioxus_core::Task;
use dioxus_native::{SubDocumentAttr, prelude::*};

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{Body, Entry, EntryValue, FormData, Method, Request, Url};

type StdNetProvider = blitz_net::Provider<blitz_dom::net::Resource>;

mod icons;
use icons::IconButton;

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
    let home_url = use_hook(|| Url::parse("https://wikipedia.org").unwrap());

    let mut url_input_value = use_signal(|| home_url.to_string());
    let mut history: SyncStore<History> = use_sync_store(|| History::new(home_url.clone()));

    let net_provider = use_context::<Arc<StdNetProvider>>();
    let loader = use_hook(|| DocumentLoader::new(net_provider, history.clone()));
    let content_doc = loader.doc.clone();

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

    rsx!(
        div { id: "frame",
            title { "Blitz Browser" }
            style { {include_str!("../assets/browser.css")} }

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
                    onkeydown: move |evt| {
                        if evt.key() == Key::Enter {
                            evt.prevent_default();
                            let req = req_from_string(&*url_input_value.read());
                            if let Some(req) = req {
                                history.navigate(req);
                            } else {
                                println!("Error parsing URL {}", &*url_input_value.read());
                            }
                        }
                    },
                    oninput: move |evt| { *url_input_value.write() = evt.value() },
                }
                IconButton { icon: icons::MENU_ICON }
            }

            // Web content
            web-view { class: "webview", "__webview_document": content_doc() }
        }
    )
}

fn req_from_string(url_s: &str) -> Option<Request> {
    if let Ok(url) = Url::parse(&url_s) {
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
    net_provider: Arc<StdNetProvider>,
    status: Signal<DocumentLoaderStatus>,
    request_id_counter: AtomicUsize,
    doc: Signal<Option<SubDocumentAttr>>,
    history: SyncStore<History>,
}

impl Clone for DocumentLoader {
    fn clone(&self) -> Self {
        Self {
            net_provider: self.net_provider.clone(),
            status: self.status.clone(),
            request_id_counter: AtomicUsize::new(self.request_id_counter.load(Ao::SeqCst)),
            doc: self.doc.clone(),
            history: self.history.clone(),
        }
    }
}

impl DocumentLoader {
    fn new(net_provider: Arc<StdNetProvider>, history: SyncStore<History>) -> Self {
        Self {
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
        let status = self.status.clone();
        let doc_signal = self.doc.clone();
        let history = self.history.clone();

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
                        font_ctx: None,
                    };

                    let html = if bytes.len() == 0 {
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
