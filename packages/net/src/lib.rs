use std::rc::Rc;
use std::sync::Arc;
use url::Url;

#[cfg(feature = "blocking")]
mod blocking;
#[cfg(feature = "non_blocking")]
mod non_blocking;

#[cfg(feature = "blocking")]
pub use blocking::SyncProvider;

#[cfg(feature = "non_blocking")]
pub use non_blocking::AsyncProvider;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub trait NetProvider<I, T> {
    fn fetch<F>(&self, url: Url, i: I, handler: F)
    where
        F: Fn(&[u8]) -> T + Send + Sync + 'static;
}
impl<I, T, P: NetProvider<I, T>> NetProvider<I, T> for Arc<P> {
    fn fetch<F>(&self, url: Url, i: I, handler: F)
    where
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        self.as_ref().fetch(url, i, handler)
    }
}
impl<I, T, P: NetProvider<I, T>> NetProvider<I, T> for Rc<P> {
    fn fetch<F>(&self, url: Url, i: I, handler: F)
    where
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        self.as_ref().fetch(url, i, handler)
    }
}
impl<I, T, P: NetProvider<I, T>> NetProvider<I, T> for &P {
    fn fetch<F>(&self, url: Url, i: I, handler: F)
    where
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
        NetProvider::fetch(*self, url, i, handler);
    }
}

pub struct DummyProvider;
impl<I, T> NetProvider<I, T> for DummyProvider {
    fn fetch<F>(&self, _url: Url, _i: I, _handler: F)
    where
        F: Fn(&[u8]) -> T + Send + Sync + 'static,
    {
    }
}
