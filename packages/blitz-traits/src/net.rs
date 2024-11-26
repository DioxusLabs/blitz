pub use bytes::Bytes;
pub use http::Method;
use std::marker::PhantomData;
use std::sync::Arc;
pub use url::Url;

pub type SharedProvider<D> = Arc<dyn NetProvider<Data = D>>;
pub type BoxedHandler<D> = Box<dyn RequestHandler<Data = D>>;
pub type SharedCallback<D> = Arc<dyn Callback<Data = D>>;

pub trait NetProvider: Send + Sync + 'static {
    type Data;
    fn fetch(&self, doc_id: usize, url: Url, handler: BoxedHandler<Self::Data>);
}

pub trait RequestHandler: Send + Sync + 'static {
    type Data;
    fn bytes(self: Box<Self>, doc_id: usize, bytes: Bytes, callback: SharedCallback<Self::Data>);
    fn method(&self) -> Method {
        Method::GET
    }
}

pub trait Callback: Send + Sync + 'static {
    type Data;
    fn call(&self, doc_id: usize, data: Self::Data);
}

pub struct DummyProvider<D>(PhantomData<D>);
impl<D> Default for DummyProvider<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<D: Send + Sync + 'static> NetProvider for DummyProvider<D> {
    type Data = D;
    fn fetch(&self, _doc_id: usize, _url: Url, _handler: BoxedHandler<D>) {}
}

pub struct DummyCallback<D>(PhantomData<D>);
impl<D: Send + Sync + 'static> Default for DummyCallback<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<D: Send + Sync + 'static> Callback for DummyCallback<D> {
    type Data = D;
    fn call(&self, _doc_id: usize, _data: Self::Data) {}
}
