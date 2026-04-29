use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{net::Request, shell::ShellProvider};
use dioxus_core::Task;
use dioxus_native::{SubDocumentAttr, prelude::*};
use linebender_resource_handle::Blob;

use crate::StdNetProvider;
use crate::config::ConfigStore;
use crate::history::{BrowserNavProvider, History, SyncStore};
use crate::special_pages;

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
    pub history: SyncStore<History>,
    pub html_source: Signal<String>,
    pub title: Signal<String>,
}

pub fn make_doc_config(
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
    pub fn new(
        net_provider: Arc<StdNetProvider>,
        config: Arc<ConfigStore>,
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
            config,
            status: Signal::new(DocumentLoaderStatus::Idle),
            request_id_counter: AtomicUsize::new(0),
            doc: Signal::new(None),
            history,
            html_source,
            title,
        }
    }

    pub fn load_document(&self, req: Request) {
        if req.url.scheme() == "about" {
            if let Some(_page) = special_pages::lookup(&req.url) {
                if let DocumentLoaderStatus::Loading { task, .. } = *self.status.peek() {
                    task.cancel();
                }
                let ctx = special_pages::SpecialPageCtx {
                    url: &req.url,
                    history: self.history,
                    config: Arc::clone(&self.config),
                };
                #[allow(clippy::expect_used)] // dispatch must succeed: lookup already matched above
                let html =
                    special_pages::dispatch(&ctx).expect("lookup matched, dispatch must succeed");
                let doc_config = make_doc_config(
                    Some(req.url.to_string()),
                    Arc::clone(&self.net_provider),
                    self.history,
                    self.font_ctx.clone(),
                );
                *self.html_source.write_unchecked() = html.clone();
                let document = HtmlDocument::from_html(&html, doc_config).into_inner();
                let parsed_title = document
                    .find_title_node()
                    .map(|n| n.text_content())
                    .unwrap_or_default();
                *self.title.write_unchecked() = parsed_title;
                *self.doc.write_unchecked() = Some(SubDocumentAttr::new(document));
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
