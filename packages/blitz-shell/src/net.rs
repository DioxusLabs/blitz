use std::sync::Arc;

use blitz_dom::net::Resource;
use blitz_traits::net::NetCallback;
use winit::event_loop::EventLoopProxy;

use crate::BlitzShellEvent;

/// A NetCallback that injects the fetched Resource into our winit event loop
pub struct BlitzShellNetCallback(EventLoopProxy<BlitzShellEvent>);

impl BlitzShellNetCallback {
    pub fn new(proxy: EventLoopProxy<BlitzShellEvent>) -> Self {
        Self(proxy)
    }

    pub fn shared(proxy: EventLoopProxy<BlitzShellEvent>) -> Arc<dyn NetCallback<Resource>> {
        Arc::new(Self(proxy))
    }
}
impl NetCallback<Resource> for BlitzShellNetCallback {
    fn call(&self, doc_id: usize, result: Result<Resource, Option<String>>) {
        // TODO: handle error case
        if let Ok(data) = result {
            self.0
                .send_event(BlitzShellEvent::ResourceLoad { doc_id, data })
                .unwrap()
        }
    }
}

#[cfg(feature = "data-uri")]
mod data_uri_net_provider {
    //! Data-URI only networking for Blitz
    //!
    //! Provides an implementation of the [`blitz_traits::net::NetProvider`] trait.

    use blitz_traits::net::{Bytes, NetCallback, NetHandler, NetProvider, Request};
    use data_url::DataUrl;
    use std::sync::Arc;

    pub struct DataUriNetProvider<D> {
        resource_callback: Arc<dyn NetCallback<D>>,
    }
    impl<D: 'static> DataUriNetProvider<D> {
        pub fn new(resource_callback: Arc<dyn NetCallback<D>>) -> Self {
            Self { resource_callback }
        }
        pub fn shared(res_callback: Arc<dyn NetCallback<D>>) -> Arc<dyn NetProvider<D>> {
            Arc::new(Self::new(res_callback))
        }
    }

    impl<D: 'static> NetProvider<D> for DataUriNetProvider<D> {
        fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler<D>>) {
            let callback = &self.resource_callback;
            match request.url.scheme() {
                "data" => {
                    let Ok(data_url) = DataUrl::process(request.url.as_str()) else {
                        callback.call(doc_id, Err(Some(String::from("Failed to parse data uri"))));
                        return;
                    };
                    let Ok(decoded) = data_url.decode_to_vec() else {
                        callback.call(doc_id, Err(Some(String::from("Failed to decode data uri"))));
                        return;
                    };
                    let bytes = Bytes::from(decoded.0);
                    handler.bytes(doc_id, bytes, Arc::clone(callback));
                }
                _ => {
                    callback.call(doc_id, Err(Some(String::from("UnsupportedScheme"))));
                }
            };
        }
    }
}
#[cfg(feature = "data-uri")]
pub use data_uri_net_provider::DataUriNetProvider;
