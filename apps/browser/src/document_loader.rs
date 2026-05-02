use std::sync::Arc;

use blitz_dom::{DocumentConfig, FontContext};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{net::Request, shell::ShellProvider};
use dioxus_native::{SubDocumentAttr, prelude::*};
use linebender_resource_handle::Blob;

use crate::StdNetProvider;
use crate::history::{BrowserNavProvider, History, SyncStore};

pub enum DocumentLoaderStatus {
    Loading,
    Idle,
}

#[derive(Clone)]
pub struct LoadedDocument {
    pub document: SubDocumentAttr,
    pub html_source: String,
    pub title: String,
}

pub struct DocumentLoader {
    pub font_ctx: FontContext,
    pub net_provider: Arc<StdNetProvider>,
    pub status: Signal<DocumentLoaderStatus>,
    pub history: SyncStore<History>,
    pub reload_generation: Signal<u64>,
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
    pub fn new(net_provider: Arc<StdNetProvider>, history: SyncStore<History>) -> Self {
        let mut font_ctx = FontContext::default();
        font_ctx
            .collection
            .register_fonts(Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None);

        Self {
            font_ctx,
            net_provider,
            status: Signal::new(DocumentLoaderStatus::Idle),
            history,
            reload_generation: Signal::new(0),
        }
    }

    pub fn reload(&self) {
        let mut reload_generation = self.reload_generation;
        *reload_generation.write() += 1;
    }

    pub fn reload_generation(&self) -> u64 {
        *self.reload_generation.read()
    }

    pub async fn load_document(&self, req: Request) -> LoadedDocument {
        let net_provider = Arc::clone(&self.net_provider);
        let font_ctx = self.font_ctx.clone();
        let history = self.history;

        let response = net_provider.fetch_async(req).await;

        match response {
            Ok((resolved_url, bytes)) => {
                tracing::info!("Loaded {}", resolved_url);
                let config = make_doc_config(Some(resolved_url), net_provider, history, font_ctx);

                let bytes_str;
                let html: &str = if bytes.is_empty() {
                    include_str!("../assets/404.html")
                } else {
                    bytes_str = String::from_utf8_lossy(&bytes);
                    &bytes_str
                };

                let document = HtmlDocument::from_html(html, config).into_inner();
                let parsed_title = document
                    .find_title_node()
                    .map(|n| n.text_content())
                    .unwrap_or_default();
                LoadedDocument {
                    document: SubDocumentAttr::new(document),
                    html_source: html.to_string(),
                    title: parsed_title,
                }
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
                LoadedDocument {
                    document: SubDocumentAttr::new(document),
                    html_source: error_html.to_string(),
                    title: String::new(),
                }
            }
        }
    }
}
