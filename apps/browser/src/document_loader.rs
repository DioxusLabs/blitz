use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    navigation::{NavigationOptions, NavigationProvider},
    net::{Request, Url},
    shell::ShellProvider,
};
use dioxus_core::Task;
use dioxus_native::{SubDocumentAttr, prelude::*};

/// A `LoadTriggerSignal` that is `Send + Sync`, required by `NavigationProvider`.
pub type LoadTriggerSignal = Signal<LoadTrigger, SyncStorage>;
use linebender_resource_handle::Blob;

use crate::StdNetProvider;
use crate::config::ConfigStore;
use crate::history::{History, HistoryNav, SyncStore};
use crate::special_pages::{self, NavigateFn, SpecialPageComponent, TabContent};

/// Drives the `use_effect` in `TabView`.
///
/// `NewNav` — user-initiated navigation; commit the resolved URL to history on success.
/// `BackForward` — navigating within already-committed history; just reload and update title.
#[derive(Clone)]
pub enum LoadTrigger {
    NewNav(Request),
    BackForward(Request),
}

impl LoadTrigger {
    pub fn request(&self) -> &Request {
        match self {
            LoadTrigger::NewNav(req) | LoadTrigger::BackForward(req) => req,
        }
    }

    fn is_new_nav(&self) -> bool {
        matches!(self, LoadTrigger::NewNav(_))
    }
}

struct BrowserNavProvider {
    load_trigger: LoadTriggerSignal,
}

impl NavigationProvider for BrowserNavProvider {
    fn navigate_to(&self, options: NavigationOptions) {
        let mut lt = self.load_trigger;
        lt.set(LoadTrigger::NewNav(options.into_request()));
    }
}

pub enum DocumentLoaderStatus {
    Loading { request_id: usize, task: Task },
    Idle,
}

pub struct DocumentLoader {
    pub font_ctx: FontContext,
    pub net_provider: Arc<StdNetProvider>,
    pub config: Arc<ConfigStore>,
    pub status: Signal<DocumentLoaderStatus>,
    pub request_id_counter: AtomicUsize,
    pub doc: Signal<Option<SubDocumentAttr>>,
    pub content: Signal<TabContent>,
    pub history: SyncStore<History>,
    pub load_trigger: LoadTriggerSignal,
    pub html_source: Signal<String>,
    pub title: Signal<String>,
}

pub fn make_doc_config(
    base_url: Option<String>,
    net_provider: Arc<StdNetProvider>,
    load_trigger: LoadTriggerSignal,
    font_ctx: FontContext,
) -> DocumentConfig {
    DocumentConfig {
        viewport: None,
        base_url,
        ua_stylesheets: None,
        net_provider: Some(net_provider as _),
        navigation_provider: Some(Arc::new(BrowserNavProvider { load_trigger })),
        shell_provider: Some(consume_context::<Arc<dyn ShellProvider>>()),
        html_parser_provider: Some(Arc::new(HtmlProvider)),
        font_ctx: Some(font_ctx),
        media_type: None,
    }
}

impl DocumentLoader {
    pub fn new(
        net_provider: Arc<StdNetProvider>,
        config: Arc<ConfigStore>,
        history: SyncStore<History>,
        load_trigger: LoadTriggerSignal,
        html_source: Signal<String>,
        title: Signal<String>,
        content: Signal<TabContent>,
    ) -> Self {
        let mut font_ctx = FontContext::default();
        font_ctx
            .collection
            .register_fonts(Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None);

        Self {
            font_ctx,
            net_provider,
            config,
            status: Signal::new(DocumentLoaderStatus::Idle),
            request_id_counter: AtomicUsize::new(0),
            doc: Signal::new(None),
            content,
            history,
            load_trigger,
            html_source,
            title,
        }
    }

    pub fn load_document(&self, trigger: LoadTrigger) {
        let req = trigger.request().clone();
        let commit_to_history = trigger.is_new_nav();

        if req.url.scheme() == "about" {
            if let Some((title, render_fn)) = special_pages::lookup(&req.url) {
                if let DocumentLoaderStatus::Loading { task, .. } = *self.status.peek() {
                    task.cancel();
                }
                let history = self.history;
                let config = Arc::clone(&self.config);
                let lt = self.load_trigger;
                let navigate: NavigateFn = Arc::new(move |url: Url| {
                    let mut load_trigger = lt;
                    load_trigger.set(LoadTrigger::NewNav(Request::get(url)));
                });
                let component =
                    Arc::new(move || render_fn(history, Arc::clone(&config), navigate.clone()))
                        as Arc<dyn Fn() -> dioxus_native::prelude::Element + Send + Sync>;
                // about: pages are never committed to history and must not overwrite
                // the title of the current real page in history.
                *self.title.write_unchecked() = title.to_string();
                *self.html_source.write_unchecked() = String::new();
                *self.content.write_unchecked() = TabContent::Special(SpecialPageComponent {
                    name: title,
                    render: component,
                });
                *self.status.write_unchecked() = DocumentLoaderStatus::Idle;
                return;
            }
        }

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);
        let net_provider = Arc::clone(&self.net_provider);
        let font_ctx = self.font_ctx.clone();
        let status = self.status;
        let doc_signal = self.doc;
        let history = self.history;
        let load_trigger = self.load_trigger;
        let html_source = self.html_source;
        let title = self.title;
        let content = self.content;

        if let DocumentLoaderStatus::Loading { task, .. } = *self.status.peek() {
            task.cancel();
        };

        let task = spawn(async move {
            let response = net_provider.fetch_async(req.clone()).await;

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

                    // Use the resolved (post-redirect) URL for history; fall back to original.
                    let commit_req = Url::parse(&resolved_url).map(Request::get).unwrap_or(req);

                    let config =
                        make_doc_config(Some(resolved_url), net_provider, load_trigger, font_ctx);

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
                    if commit_to_history {
                        history.navigate(commit_req);
                    }
                    history.set_current_title(parsed_title.clone());
                    *title.write_unchecked() = parsed_title;
                    *content.write_unchecked() = TabContent::Web;
                    *doc_signal.write_unchecked() = Some(SubDocumentAttr::new(document));
                }
                Err(err) => {
                    // On load failure, do NOT commit to history.
                    tracing::error!("Error loading document: {:?}", err);

                    let error_msg = format!("{err:?}");
                    let config = make_doc_config(None, net_provider, load_trigger, font_ctx);

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
                    *content.write_unchecked() = TabContent::Web;
                    *doc_signal.write_unchecked() = Some(SubDocumentAttr::new(document));
                }
            }
            *status.write_unchecked() = DocumentLoaderStatus::Idle;
        });

        *status.write_unchecked() = DocumentLoaderStatus::Loading { request_id, task };
    }
}
