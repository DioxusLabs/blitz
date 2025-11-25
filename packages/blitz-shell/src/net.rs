use std::sync::Arc;

use blitz_traits::net::NetWaker;
use winit::event_loop::EventLoopProxy;

use crate::BlitzShellEvent;

/// A NetWaker that wakes up our winit event loop
pub struct BlitzShellNetWaker(EventLoopProxy<BlitzShellEvent>);

impl BlitzShellNetWaker {
    pub fn new(proxy: EventLoopProxy<BlitzShellEvent>) -> Self {
        Self(proxy)
    }

    pub fn shared(proxy: EventLoopProxy<BlitzShellEvent>) -> Arc<dyn NetWaker> {
        Arc::new(Self(proxy))
    }
}
impl NetWaker for BlitzShellNetWaker {
    fn wake(&self, doc_id: usize) {
        self.0
            .send_event(BlitzShellEvent::RequestRedraw { doc_id })
            .unwrap()
    }
}

#[cfg(feature = "data-uri")]
mod data_uri_net_provider {
    //! Data-URI only networking for Blitz
    //!
    //! Provides an implementation of the [`blitz_traits::net::NetProvider`] trait.

    use blitz_traits::net::{Bytes, NetHandler, NetProvider, NetWaker, Request};
    use data_url::DataUrl;
    use std::sync::Arc;

    pub struct DataUriNetProvider {
        #[allow(unused)]
        waker: Option<Arc<dyn NetWaker>>,
    }
    impl DataUriNetProvider {
        pub fn new(waker: Option<Arc<dyn NetWaker>>) -> Self {
            Self { waker }
        }
        pub fn shared(waker: Option<Arc<dyn NetWaker>>) -> Arc<dyn NetProvider> {
            Arc::new(Self::new(waker))
        }
    }

    impl NetProvider for DataUriNetProvider {
        fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
            // let callback = &self.resource_callback;
            match request.url.scheme() {
                "data" => {
                    let Ok(data_url) = DataUrl::process(request.url.as_str()) else {
                        // callback.call(doc_id, Err(Some(String::from("Failed to parse data uri"))));
                        return;
                    };
                    let Ok(decoded) = data_url.decode_to_vec() else {
                        // callback.call(doc_id, Err(Some(String::from("Failed to decode data uri"))));
                        return;
                    };
                    let bytes = Bytes::from(decoded.0);
                    handler.bytes(request.url.to_string(), bytes);
                }
                _ => {
                    // callback.call(doc_id, Err(Some(String::from("UnsupportedScheme"))));
                }
            };
        }
    }
}
#[cfg(feature = "data-uri")]
pub use data_uri_net_provider::DataUriNetProvider;
