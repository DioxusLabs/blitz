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
use blitz_traits::net::{Method, Request, Url};

type StdNetProvider = blitz_net::Provider<blitz_dom::net::Resource>;

mod icons;
use icons::IconButton;

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    dioxus_native::launch(app)
}

fn app() -> Element {
    let home_url = use_hook(|| String::from("https://wikipedia.org"));

    let mut url_input_value = use_signal(|| home_url.clone());
    let history: SyncSignal<Vec<String>> = use_signal_sync(|| Vec::new());
    let mut url: SyncSignal<Option<String>> = use_signal_sync(|| Some(url_input_value()));

    let net_provider = use_context::<Arc<StdNetProvider>>();
    let loader = use_hook(|| DocumentLoader::new(net_provider, url.clone(), history.clone()));
    let content_doc = loader.doc.clone();

    let refresh = use_callback(move |_| {
        if let Some(url_s) = url() {
            println!("Loading {}...", url_s);
            if let Ok(url) = Url::parse(&url_s) {
                *url_input_value.write_unchecked() = url_s;
                loader.load_document(url);
            }
        }
    });

    use_effect(move || refresh(()));

    let back_action = use_callback(move |_| {
        if let Some(prev) = history.write_unchecked().pop() {
            *url.write_unchecked() = Some(prev);
        }
    });

    let refresh_action = refresh;

    let home_action = use_callback(move |_| {
        if let Some(prev) = url.read().as_ref() {
            history.write_unchecked().push(prev.clone());
        }
        *url.write_unchecked() = Some(home_url.clone())
    });

    rsx!(
        div {
            id: "frame",
            title { "Blitz Browser" }
            style { {include_str!("./browser.css")} }

            // Toolbar
            div { class: "urlbar",
                IconButton { icon: icons::BACK_ICON, action: back_action }
                IconButton { icon: icons::FORWARDS_ICON }
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
                            *url.write() = Some(url_input_value());
                        }
                    },
                    oninput: move |evt| {
                        *url_input_value.write() = evt.value()
                    },
                }
                IconButton { icon: icons::MENU_ICON }
            }

            // Web content
            web-view {
                class: "webview",
                "__webview_document": content_doc()
            }
        }
    )
}

struct BrowserNavProvider {
    url_signal: SyncSignal<Option<String>>,
    history: SyncSignal<Vec<String>>,
}

impl NavigationProvider for BrowserNavProvider {
    fn navigate_to(&self, options: NavigationOptions) {
        if options.method == Method::GET {
            if let Some(prev) = self.url_signal.read().as_ref() {
                self.history.write_unchecked().push(prev.clone());
            }
            let url = options.url.to_string();
            *self.url_signal.write_unchecked() = Some(url.clone());
        }
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
    url_signal: SyncSignal<Option<String>>,
    history: SyncSignal<Vec<String>>,
}

impl Clone for DocumentLoader {
    fn clone(&self) -> Self {
        Self {
            net_provider: self.net_provider.clone(),
            status: self.status.clone(),
            request_id_counter: AtomicUsize::new(self.request_id_counter.load(Ao::SeqCst)),
            doc: self.doc.clone(),
            url_signal: self.url_signal.clone(),
            history: self.history.clone(),
        }
    }
}

impl DocumentLoader {
    fn new(
        net_provider: Arc<StdNetProvider>,
        url_signal: SyncSignal<Option<String>>,
        history: SyncSignal<Vec<String>>,
    ) -> Self {
        Self {
            net_provider,
            status: Signal::new(DocumentLoaderStatus::Idle),
            request_id_counter: AtomicUsize::new(0),
            doc: Signal::new(None),
            url_signal,
            history,
        }
    }

    fn load_document(&self, url: Url) {
        let request_id = self.request_id_counter.fetch_add(1, Ao::Relaxed);
        let net_provider = Arc::clone(&self.net_provider);
        let doc_signal = self.doc.clone();
        let url_signal = self.url_signal.clone();
        let history = self.history.clone();
        let task = spawn(async move {
            let request = net_provider.fetch_async(Request::get(url));
            match request.await {
                Ok((resolved_url, bytes)) => {
                    println!("Loaded {}", resolved_url);
                    let config = DocumentConfig {
                        viewport: None,
                        base_url: Some(resolved_url),
                        ua_stylesheets: None,
                        net_provider: Some(net_provider as _), // FIXME
                        navigation_provider: Some(Arc::new(BrowserNavProvider {
                            url_signal,
                            history,
                        })),
                        shell_provider: Some(consume_context::<Arc<dyn ShellProvider>>()),
                        html_parser_provider: Some(Arc::new(HtmlProvider)),
                        font_ctx: None,
                    };

                    let html = str::from_utf8(&bytes).unwrap();
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
