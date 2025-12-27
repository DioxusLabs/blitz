#[cfg(feature = "data-uri")]
mod data_uri_net_provider {
    //! Data-URI only networking for Blitz
    //!
    //! Provides an implementation of the [`blitz_traits::net::NetProvider`] trait.

    use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
    use blitz_traits::shell::EventLoopWaker;
    use data_url::DataUrl;
    use std::sync::Arc;

    pub struct DataUriNetProvider {
        #[allow(unused)]
        waker: Option<Arc<dyn EventLoopWaker>>,
    }
    impl DataUriNetProvider {
        pub fn new(waker: Option<Arc<dyn EventLoopWaker>>) -> Self {
            Self { waker }
        }
        pub fn shared(waker: Option<Arc<dyn EventLoopWaker>>) -> Arc<dyn NetProvider> {
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
