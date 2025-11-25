use blitz_shell::BlitzShellNetCallback;
use std::sync::Arc;

use blitz_dom::net::Resource;
use blitz_shell::BlitzShellEvent;
use blitz_traits::net::{NetHandler, NetProvider, Request};
use winit::event_loop::EventLoopProxy;

pub struct DioxusNativeNetProvider {
    inner_net_provider: Option<Arc<dyn NetProvider<Resource> + 'static>>,
}

#[allow(unused)]
impl DioxusNativeNetProvider {
    pub fn shared(proxy: EventLoopProxy<BlitzShellEvent>) -> Arc<dyn NetProvider<Resource>> {
        Arc::new(Self::new(proxy)) as Arc<dyn NetProvider<Resource>>
    }

    pub fn new(proxy: EventLoopProxy<BlitzShellEvent>) -> Self {
        let net_callback = BlitzShellNetCallback::shared(proxy);

        #[cfg(feature = "net")]
        let inner_net_provider = Some(blitz_net::Provider::shared(net_callback.clone()));
        #[cfg(all(feature = "data-uri", not(feature = "net")))]
        let inner_net_provider = Some(blitz_shell::DataUriNetProvider::shared(
            net_callback.clone(),
        ));
        #[cfg(all(not(feature = "data-uri"), not(feature = "net")))]
        let inner_net_provider = None;

        Self { inner_net_provider }
    }

    pub fn with_inner(
        proxy: EventLoopProxy<BlitzShellEvent>,
        inner: Arc<dyn NetProvider<Resource>>,
    ) -> Self {
        Self {
            inner_net_provider: Some(inner),
        }
    }

    pub fn inner(&self) -> Option<&Arc<dyn NetProvider<Resource>>> {
        self.inner_net_provider.as_ref()
    }
}

impl NetProvider<Resource> for DioxusNativeNetProvider {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        if request.url.scheme() == "dioxus" {
            #[allow(clippy::single_match)] // cfg'd code
            match dioxus_asset_resolver::native::serve_asset(request.url.path()) {
                Ok(res) => {
                    #[cfg(feature = "tracing")]
                    tracing::trace!("fetching asset from file system success {request:#?}");
                    handler.bytes(request.url.to_string(), res.into_body().into())
                }
                Err(_) => {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("fetching asset from file system error {request:#?}");
                }
            }
        } else if let Some(inner) = &self.inner_net_provider {
            inner.fetch(doc_id, request, handler);
        } else {
            #[cfg(feature = "tracing")]
            tracing::warn!("net feature not enabled, cannot fetch {request:#?}");
        }
    }
}
