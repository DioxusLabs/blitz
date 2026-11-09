//! Optional fetch proxy for the web build. Browsers only let a page fetch
//! cross-origin resources that send CORS headers, so the web build can route
//! every http(s) request through a proxy given as a URL prefix: a request for
//! `https://example.com/` becomes `<prefix>https%3A%2F%2Fexample.com%2F`.
//! Responses are still reported under the original URL so relative URLs and
//! resource lookups resolve against the real page.

use std::sync::{Arc, OnceLock};

use blitz_net::ProviderError;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request, Url};

use crate::StdNetProvider;

static PROXY_PREFIX: OnceLock<String> = OnceLock::new();

/// Set the proxy prefix. It must be an absolute URL.
#[cfg(target_arch = "wasm32")]
pub fn set_prefix(prefix: String) {
    let _ = PROXY_PREFIX.set(prefix);
}

/// Rewrite `request` to go through the proxy, returning the original URL if it did.
fn proxy_request(request: &mut Request) -> Option<String> {
    let prefix = PROXY_PREFIX.get()?;
    if !matches!(request.url.scheme(), "http" | "https") {
        return None;
    }
    let original = request.url.to_string();
    let encoded: String = url::form_urlencoded::byte_serialize(original.as_bytes()).collect();
    request.url = Url::parse(&format!("{prefix}{encoded}")).ok()?;
    Some(original)
}

pub async fn fetch_async(
    net_provider: &StdNetProvider,
    mut request: Request,
) -> Result<(String, Bytes), ProviderError> {
    let original = proxy_request(&mut request);
    let (url, bytes) = net_provider.fetch_async(request).await?;
    Ok((original.unwrap_or(url), bytes))
}

/// The net provider for page documents, so their subresources are proxied too.
pub fn doc_net_provider(net_provider: Arc<StdNetProvider>) -> Arc<dyn NetProvider> {
    if PROXY_PREFIX.get().is_some() {
        Arc::new(ProxyNetProvider(net_provider))
    } else {
        net_provider
    }
}

struct ProxyNetProvider(Arc<StdNetProvider>);

impl NetProvider for ProxyNetProvider {
    fn fetch(&self, doc_id: usize, mut request: Request, handler: Box<dyn NetHandler>) {
        match proxy_request(&mut request) {
            Some(original_url) => {
                let handler = Box::new(OriginalUrlHandler {
                    original_url,
                    inner: handler,
                });
                self.0.fetch(doc_id, request, handler)
            }
            None => self.0.fetch(doc_id, request, handler),
        }
    }
}

struct OriginalUrlHandler {
    original_url: String,
    inner: Box<dyn NetHandler>,
}

impl NetHandler for OriginalUrlHandler {
    fn bytes(self: Box<Self>, _resolved_url: String, bytes: Bytes) {
        self.inner.bytes(self.original_url, bytes)
    }

    fn error(self: Box<Self>, _url: String, error: String) {
        self.inner.error(self.original_url, error)
    }
}
