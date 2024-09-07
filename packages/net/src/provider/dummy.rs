use crate::{NetProvider, RequestHandler};
use std::any::Any;
use url::Url;

pub struct DummyProvider;
impl<T> NetProvider<T> for DummyProvider {
    fn fetch<H>(&self, _url: Url, _handler: H)
    where
        H: RequestHandler<T>,
    {
    }
    fn resolve_all<M: Any>(&self, _marker: M) -> Option<Vec<T>> {
        None
    }
}
