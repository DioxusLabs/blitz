// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering as Ao};

use dioxus_core::Task;
use dioxus_native::prelude::dioxus_core::{AttributeValue, IntoAttributeValue};
use dioxus_native::{SubDocumentAttr, prelude::*};

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::net::{Request, Url};

type StdNetProvider = blitz_net::Provider<blitz_dom::net::Resource>;

mod icons;
use icons::IconButton;

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    dioxus_native::launch(app)
}

fn app() -> Element {
    let mut url_input_value = use_signal(|| String::from("https://nicoburns.com"));
    let mut url: Signal<Option<String>> = use_signal(|| Some(url_input_value()));

    let net_provider = use_context::<Arc<StdNetProvider>>();
    let loader = use_hook(|| DocumentLoader::new(net_provider));
    let content_doc = loader.doc.clone();

    use_effect(move || {
        if let Some(url) = url() {
            println!("Loading {}...", url);
            if let Ok(url) = Url::parse(&url) {
                loader.load_document(url);
            }
        }
    });

    // use_effect(move || {
    //     let doc = content_doc();
    //     println!("NEW CONTENT DOC {}", doc.is_some());
    // });

    rsx!(
        div {
            id: "frame",
            title { "Blitz Browser" }
            style { {include_str!("./browser.css")} }

            // Toolbar
            div { class: "urlbar",
                IconButton { icon: icons::BACK_ICON }
                IconButton { icon: icons::FORWARDS_ICON }
                IconButton { icon: icons::REFRESH_ICON }
                IconButton { icon: icons::HOME_ICON }
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

enum DocumentLoaderStatus {
    Loading { request_id: usize, task: Task },
    Idle,
}

struct DocumentLoader {
    net_provider: Arc<StdNetProvider>,
    status: Signal<DocumentLoaderStatus>,
    request_id_counter: AtomicUsize,
    doc: Signal<Option<SubDocumentAttr>>,
}

impl Clone for DocumentLoader {
    fn clone(&self) -> Self {
        Self {
            net_provider: self.net_provider.clone(),
            status: self.status.clone(),
            request_id_counter: AtomicUsize::new(self.request_id_counter.load(Ao::SeqCst)),
            doc: self.doc.clone(),
        }
    }
}

impl DocumentLoader {
    fn new(net_provider: Arc<StdNetProvider>) -> Self {
        Self {
            net_provider,
            status: Signal::new(DocumentLoaderStatus::Idle),
            request_id_counter: AtomicUsize::new(0),
            doc: Signal::new(None),
        }
    }

    fn load_document(&self, url: Url) {
        let request_id = self.request_id_counter.fetch_add(1, Ao::Relaxed);
        let net_provider = Arc::clone(&self.net_provider);
        let doc_signal = self.doc.clone();
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
                        navigation_provider: None,
                        shell_provider: None,
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
