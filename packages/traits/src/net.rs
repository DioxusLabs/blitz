pub use bytes::Bytes;
pub use http::Method;
use std::marker::PhantomData;
use std::sync::Arc;
pub use url::Url;

pub type BoxedHandler<D> = Box<dyn RequestHandler<Data = D>>;
pub type SharedCallback<D> = Arc<dyn Callback<Data = D>>;
pub type SharedProvider<D> = Arc<dyn NetProvider<Data = D>>;

pub trait NetProvider: Send + Sync + 'static {
    type Data;
    fn fetch(&self, url: Url, handler: BoxedHandler<Self::Data>);
}

pub trait RequestHandler: Send + Sync + 'static {
    type Data;
    fn bytes(self: Box<Self>, bytes: Bytes, callback: SharedCallback<Self::Data>);
    fn method(&self) -> Method {
        Method::GET
    }
}

pub trait Callback: Send + Sync + 'static {
    type Data;
    fn call(self: Arc<Self>, data: Self::Data);
}

pub struct DummyProvider<D>(PhantomData<D>);
impl<D> Default for DummyProvider<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<D: Sync + Send + 'static> NetProvider for DummyProvider<D> {
    type Data = D;
    fn fetch(&self, _url: Url, _handler: BoxedHandler<Self::Data>) {}
}pub use bytes::Bytes;
pub use http::Method;
use std::marker::PhantomData;
use std::sync::Arc;
pub use url::Url;

pub type BoxedHandler<D> = Box<dyn RequestHandler<Data = D>>;
pub type SharedCallback<D> = Arc<dyn Callback<Data = D>>;
pub type SharedProvider<D> = Arc<dyn NetProvider<Data = D>>;

pub trait NetProvider: Send + Sync + 'static {
    type Data;
    fn fetch(&self, url: Url, handler: BoxedHandler<Self::Data>);
}

pub trait RequestHandler: Send + Sync + 'static {
    type Data;
    fn bytes(self: Box<Self>, bytes: Bytes, callback: SharedCallback<Self::Data>);
    fn method(&self) -> Method {
        Method::GET
    }
}

pub trait Callback: Send + Sync + 'static {
    type Data;
    fn call(self: Arc<Self>, data: Self::Data);
}

pub struct DummyProvider<D>(PhantomData<D>);
impl<D> Default for DummyProvider<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<D: Sync + Send + 'static> NetProvider for DummyProvider<D> {
    type Data = D;
    fn fetch(&self, _url: Url, _handler: BoxedHandler<Self::Data>) {}
}