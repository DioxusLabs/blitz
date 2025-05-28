use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method};
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

    pub content_type: String,

    /// Source document for the navigation
    pub source_document: usize,

    pub document_resource: Option<Bytes>,
}

impl NavigationOptions {
    pub fn new(url: Url, content_type: String, source_document: usize) -> Self {
        Self {
            url,
            content_type,
            source_document,
            document_resource: None,
        }
    }
    pub fn set_document_resource(mut self, document_resource: Option<Bytes>) -> Self {
        self.document_resource = document_resource;
        self
    }

    pub fn into_request(self) -> Request {
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            HeaderValue::from_str(&self.content_type).unwrap(),
        );

        if let Some(document_resource) = self.document_resource {
            Request {
                url: self.url,
                method: Method::POST,
                headers,
                body: document_resource,
                signal: None,
            }
        } else {
            Request {
                url: self.url,
                method: Method::GET,
                headers,
                body: Bytes::new(),
                signal: None,
            }
        }
    }
}
