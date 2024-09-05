use crate::{NetProvider, RequestHandler};
use url::Url;

pub struct DummyProvider;
impl<T> NetProvider<T> for DummyProvider {
    fn fetch<H>(&self, _url: Url, _handler: H)
    where
        H: RequestHandler<T>,
    {
    }
}
