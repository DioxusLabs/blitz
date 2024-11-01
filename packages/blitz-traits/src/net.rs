pub use bytes::Bytes;
pub use http::Method;
use std::marker::PhantomData;
use std::sync::Arc;
pub use url::Url;

pub type SharedProvider = Arc<dyn NetProvider>;
pub type BoxedHandler = Box<dyn RequestHandler>;
pub type SharedCallback<D> = Arc<dyn Callback<Data = D>>;

pub trait NetProvider: Send + Sync + 'static {
    fn fetch(&self, url: Url, handler: BoxedHandler);
}

pub trait RequestHandler: Send + Sync + 'static {
    fn bytes(self: Box<Self>, bytes: Bytes);
    fn method(&self) -> Method {
        Method::GET
    }
}

pub trait Callback: Send + Sync + 'static {
    type Data;
    fn call(&self, data: Self::Data);
}

pub struct DummyProvider;
impl NetProvider for DummyProvider {
    fn fetch(&self, _url: Url, _handler: BoxedHandler) {}
}

pub struct DummyCallback<D>(PhantomData<D>);
impl<D> Default for DummyCallback<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<D: Send + Sync + 'static> Callback for DummyCallback<D> {
    type Data = D;
    fn call(&self, _data: Self::Data) {}
}

impl<F: FnOnce(Bytes) + Send + Sync + 'static> RequestHandler for F {
    fn bytes(self: Box<Self>, bytes: Bytes) {
        self(bytes)
    }
}
