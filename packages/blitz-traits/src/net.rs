pub use bytes::Bytes;
pub use http::{self, HeaderMap, Method};
use std::sync::Arc;
pub use url::Url;

pub type SharedProvider<D> = Arc<dyn NetProvider<D>>;
pub type BoxedHandler<D> = Box<dyn NetHandler<D>>;
pub type SharedCallback<D> = Arc<dyn NetCallback<D>>;

/// A type that fetches resources for a Document.
///
/// This may be over the network via http(s), via the filesystem, or some other method.
pub trait NetProvider<Data>: Send + Sync + 'static {
    fn fetch(&self, doc_id: usize, request: Request, handler: BoxedHandler<Data>);
}

/// A type that parses raw bytes from a network request into a Data and then calls
/// the NetCallack with the result.
pub trait NetHandler<Data>: Send + Sync + 'static {
    fn bytes(self: Box<Self>, doc_id: usize, bytes: Bytes, callback: SharedCallback<Data>);
}

/// A type which accepts the parsed result of a network request and sends it back to the Document
/// (or does arbitrary things with it)
pub trait NetCallback<Data>: Send + Sync + 'static {
    fn call(&self, doc_id: usize, result: Result<Data, Option<String>>);
}

impl<D, F: Fn(usize, Result<D, Option<String>>) + Send + Sync + 'static> NetCallback<D> for F {
    fn call(&self, doc_id: usize, result: Result<D, Option<String>>) {
        self(doc_id, result)
    }
}

#[non_exhaustive]
#[derive(Debug)]
/// A request type loosely representing <https://fetch.spec.whatwg.org/#requests>
pub struct Request {
    pub url: Url,
    pub method: Method,
    pub headers: HeaderMap,
    pub body: Bytes,
}
impl Request {
    /// A get request to the specified Url and an empty body
    pub fn get(url: Url) -> Self {
        Self {
            url,
            method: Method::GET,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        }
    }
}

/// A default noop NetProvider
#[derive(Default)]
pub struct DummyNetProvider;
impl<D: Send + Sync + 'static> NetProvider<D> for DummyNetProvider {
    fn fetch(&self, _doc_id: usize, _request: Request, _handler: BoxedHandler<D>) {}
}

/// A default noop NetCallback
#[derive(Default)]
pub struct DummyNetCallback;
impl<D: Send + Sync + 'static> NetCallback<D> for DummyNetCallback {
    fn call(&self, _doc_id: usize, _result: Result<D, Option<String>>) {}
}
