use bytes::Bytes;
use core::str::FromStr;
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

    pub document_resource: Option<DocumentResource>,
}

impl NavigationOptions {
    pub fn new(url: Url, source_document: usize) -> Self {
        Self {
            url,
            source_document,
            document_resource: None,
        }
    }
    pub fn set_document_resource(mut self, document_resource: Option<DocumentResource>) -> Self {
        self.document_resource = document_resource;
        self
    }

    pub fn into_request(self) -> Request {
        if let Some(DocumentResource::PostResource {
            body,
            content_type: _,
        }) = self.document_resource
        {
            Request {
                url: self.url,
                method: Method::POST,
                headers: HeaderMap::new(),
                body,
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

#[derive(Debug, Clone)]
pub enum DocumentResource {
    String(String),
    PostResource {
        body: Bytes,
        content_type: RequestContentType,
    },
}

/// Supported content types for HTTP requests
#[derive(Debug, Clone)]
pub enum RequestContentType {
    /// application/x-www-form-urlencoded
    FormUrlEncoded,
    /// multipart/form-data
    MultipartFormData,
    /// text/plain
    TextPlain,
}

impl FromStr for RequestContentType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "application/x-www-form-urlencoded" => RequestContentType::FormUrlEncoded,
            "multipart/form-data" => RequestContentType::MultipartFormData,
            "text/plain" => RequestContentType::TextPlain,
            _ => return Err(()),
        })
    }
}
