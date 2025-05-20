use bytes::Bytes;
use http::{HeaderMap, Method};
use url::Url;

use crate::net::Request;

/// A provider to enable a document to bubble up navigation events (e.g. clicking a link)
pub trait NavigationProvider: Send + Sync + 'static {
    fn navigate_to(&self, options: NavigationOptions);
}

pub struct DummyNavigationProvider;

impl NavigationProvider for DummyNavigationProvider {
    fn navigate_to(&self, _options: NavigationOptions) {
        // Default impl: do nothing
    }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct NavigationOptions {
    /// The URL to navigate to
    pub url: Url,

    /// Source document for the navigation
    pub source_document: usize,

    pub document_resource: Option<Bytes>,
}

impl NavigationOptions {
    pub fn new(url: Url, source_document: usize) -> Self {
        Self {
            url,
            source_document,
            document_resource: None,
        }
    }
    pub fn set_document_resource(mut self, document_resource: Option<Bytes>) -> Self {
        self.document_resource = document_resource;
        self
    }

    pub fn into_request(self) -> Request {
        if let Some(document_resource) = self.document_resource {
            Request {
                url: self.url,
                method: Method::POST,
                headers: HeaderMap::new(),
                body: document_resource,
            }
        } else {
            Request {
                url: self.url,
                method: Method::GET,
                headers: HeaderMap::new(),
                body: Bytes::new(),
            }
        }
    }
}
