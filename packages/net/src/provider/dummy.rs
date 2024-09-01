use crate::{NetProvider, RequestHandler};
use url::Url;

pub struct DummyProvider;
impl<I, T> NetProvider<I, T> for DummyProvider {
    fn fetch<H>(&self, _url: Url, _i: I, _handler: H)
    where
        H: RequestHandler<T>,
    {
    }
}
