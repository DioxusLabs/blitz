mod provider;

use std::ops::Deref;
use url::Url;

pub use http::Method;

pub use provider::*;

#[cfg(any(feature = "blocking", feature = "non_blocking"))]
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";

pub trait NetProvider<T> {
    fn fetch<H>(&self, url: Url, handler: H)
    where
        H: RequestHandler<T>;
}

impl<T, P, D> NetProvider<T> for D
where
    P: NetProvider<T>,
    D: Deref<Target = P>,
{
    fn fetch<H>(&self, url: Url, handler: H)
    where
        H: RequestHandler<T>,
    {
        self.deref().fetch(url, handler)
    }
}

pub trait RequestHandler<T>: Send + Sync + 'static {
    fn bytes(self, bytes: &[u8]) -> T;
    fn method(&self) -> Method {
        Method::GET
    }
}
impl<F: Fn(&[u8]) -> T + Sync + Send + 'static, T> RequestHandler<T> for F {
    fn bytes(self, bytes: &[u8]) -> T {
        self(bytes)
    }
}
