pub use bytes::Bytes;
pub use http::Method;
use std::marker::PhantomData;
use std::sync::Arc;
pub use url::Url;

pub type SharedProvider<D> = Arc<dyn NetProvider<Data = D>>;
pub type BoxedHandler<D> = Box<dyn NetHandler<Data = D>>;
pub type SharedCallback<D> = Arc<dyn NetCallback<Data = D>>;

/// A type that fetches resources for a Document.
///
/// This may be over the network via http(s), via the filesystem, or some other method.
pub trait NetProvider: Send + Sync + 'static {
    type Data;
    fn fetch(&self, doc_id: usize, url: Url, handler: BoxedHandler<Self::Data>);
}

/// A type that parses raw bytes from a network request into a Self::Data and then calls
/// the NetCallack with the result.
pub trait NetHandler: Send + Sync + 'static {
    type Data;
    fn bytes(self: Box<Self>, doc_id: usize, bytes: Bytes, callback: SharedCallback<Self::Data>);
    fn method(&self) -> Method {
        Method::GET
    }
}

/// A type which accepts the parsed result of a network request and sends it back to the Document
/// (or does arbitrary things with it)
pub trait NetCallback: Send + Sync + 'static {
    type Data;
    fn call(&self, doc_id: usize, data: Self::Data);
}

/// A default noop NetProvider
pub struct DummyNetProvider<D>(PhantomData<D>);
impl<D> Default for DummyNetProvider<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<D: Send + Sync + 'static> NetProvider for DummyNetProvider<D> {
    type Data = D;
    fn fetch(&self, _doc_id: usize, _url: Url, _handler: BoxedHandler<D>) {}
}

/// A default noop NetCallback
pub struct DummyNetCallback<D>(PhantomData<D>);
impl<D: Send + Sync + 'static> Default for DummyNetCallback<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<D: Send + Sync + 'static> NetCallback for DummyNetCallback<D> {
    type Data = D;
    fn call(&self, _doc_id: usize, _data: Self::Data) {}
}
