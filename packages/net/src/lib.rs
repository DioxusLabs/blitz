mod provider;

use std::ops::Deref;
use url::Url;

pub use provider::*;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub trait NetProvider<I, T> {
    fn fetch<H>(&self, url: Url, i: I, handler: H)
    where
        H: RequestHandler<T>;
}

impl<I, T, P, D> NetProvider<I, T> for D
where
    P: NetProvider<I, T>,
    D: Deref<Target = P>,
{
    fn fetch<H>(&self, url: Url, i: I, handler: H)
    where
        H: RequestHandler<T>,
    {
        self.deref().fetch(url, i, handler)
    }
}

pub trait RequestHandler<T>: Send + Sync + 'static {
    fn bytes(self, bytes: &[u8]) -> T;
}
impl<F: Fn(&[u8]) -> T + Sync + Send + 'static, T> RequestHandler<T> for F {
    fn bytes(self, bytes: &[u8]) -> T {
        self(bytes)
    }
}
