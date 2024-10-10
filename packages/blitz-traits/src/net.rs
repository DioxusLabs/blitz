pub use bytes::Bytes;
pub use http::Method;
use std::marker::PhantomData;
use std::sync::{Arc, OnceLock};
pub use url::Url;

static GLOBAL_PROVIDER: OnceLock<Box<dyn NetProvider>> = OnceLock::new();
pub fn fetch<H: RequestHandler>(url: Url, handler: H) {
    GLOBAL_PROVIDER
        .get_or_init(|| Box::new(DummyProvider))
        .fetch(url, Box::new(handler))
}
pub fn set_provider<P: NetProvider>(provider: P) {
    let _ = GLOBAL_PROVIDER.set(Box::new(provider));
}

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
    fn call(self: Arc<Self>, data: Self::Data);
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
    fn call(self: Arc<Self>, _data: Self::Data) {}
}

impl<F: FnOnce(Bytes) + Send + Sync + 'static> RequestHandler for F {
    fn bytes(self: Box<Self>, bytes: Bytes) {
        self(bytes)
    }
}
