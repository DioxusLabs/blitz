use blitz_shell::BlitzShellProxy;
use std::sync::Arc;

use blitz_traits::net::{NetHandler, NetProvider, Request};

pub struct DioxusNativeNetProvider {
    inner_net_provider: Option<Arc<dyn NetProvider + 'static>>,
}

#[allow(unused)]
impl DioxusNativeNetProvider {
    pub fn shared(proxy: BlitzShellProxy) -> Arc<dyn NetProvider> {
        Arc::new(Self::new(proxy)) as Arc<dyn NetProvider>
    }

    pub fn new(proxy: BlitzShellProxy) -> Self {
        #[cfg(any(feature = "data-uri", feature = "net"))]
        let net_waker = Some(Arc::new(proxy) as _);

        #[cfg(feature = "net")]
        let inner_net_provider = Some(blitz_net::Provider::shared(net_waker.clone()));
        #[cfg(all(feature = "data-uri", not(feature = "net")))]
        let inner_net_provider = Some(blitz_shell::DataUriNetProvider::shared(net_waker.clone()));
        #[cfg(all(not(feature = "data-uri"), not(feature = "net")))]
        let inner_net_provider = None;

        Self { inner_net_provider }
    }

    pub fn with_inner(proxy: BlitzShellProxy, inner: Arc<dyn NetProvider>) -> Self {
        Self {
            inner_net_provider: Some(inner),
        }
    }

    pub fn inner(&self) -> Option<&Arc<dyn NetProvider>> {
        self.inner_net_provider.as_ref()
    }
}

/// Reports a web asset's response under its `dioxus://` URL, which is what
/// the document keys the pending image or stylesheet by.
#[cfg(target_arch = "wasm32")]
struct WebAssetHandler {
    original_url: String,
    inner: Box<dyn NetHandler>,
}

#[cfg(target_arch = "wasm32")]
impl NetHandler for WebAssetHandler {
    fn bytes(self: Box<Self>, _resolved_url: String, bytes: blitz_traits::net::Bytes) {
        self.inner.bytes(self.original_url, bytes)
    }

    fn error(self: Box<Self>, _url: String, error: String) {
        self.inner.error(self.original_url, error)
    }
}

#[cfg(target_arch = "wasm32")]
fn web_asset_url(path: &str) -> Option<blitz_traits::net::Url> {
    let base = web_sys::window()?.document()?.base_uri().ok()??;
    let base = blitz_traits::net::Url::parse(&base).ok()?;
    base.join(path.trim_start_matches('/')).ok()
}

impl NetProvider for DioxusNativeNetProvider {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        if request.url.scheme() == "dioxus" {
            // Bundled assets are served over HTTP next to the app's index.html
            #[cfg(target_arch = "wasm32")]
            if let (Some(inner), Some(url)) =
                (&self.inner_net_provider, web_asset_url(request.url.path()))
            {
                let original_url = request.url.to_string();
                let mut request = request;
                request.url = url;
                let handler = Box::new(WebAssetHandler {
                    original_url,
                    inner: handler,
                });
                inner.fetch(doc_id, request, handler);
            }
            #[cfg(not(target_arch = "wasm32"))]
            #[allow(clippy::single_match)] // cfg'd code
            match dioxus_asset_resolver::native::serve_asset(request.url.path()) {
                Ok(res) => {
                    #[cfg(feature = "tracing")]
                    tracing::trace!("fetching asset from file system success {request:#?}");
                    handler.bytes(request.url.to_string(), res.into_body().into())
                }
                Err(_) => {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("fetching asset from file system error {request:#?}");
                }
            }
        } else if let Some(inner) = &self.inner_net_provider {
            inner.fetch(doc_id, request, handler);
        } else {
            #[cfg(feature = "tracing")]
            tracing::warn!("net feature not enabled, cannot fetch {request:#?}");
        }
    }
}
