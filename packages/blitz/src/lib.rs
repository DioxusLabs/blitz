#![cfg_attr(docsrs, feature(doc_cfg))]

//! Blitz is a modular, embeddable web engine with a native Rust API.
//!
//! It powers the [`dioxus-native`] UI framework.
//!
//! This crate exists to collect the most important functionality for users together in one place.
//! It does not bring any unique functionality, but rather, it re-exports the relevant crates as modules.
//! The exported crate corresponding to each module is also available in a stand-alone manner, i.e. [`blitz-dom`] as [`blitz::dom`](crate::dom).
//!
//! [`dioxus-native`]: https://docs.rs/dioxus-native
//! [`blitz-dom`]: https://docs.rs/blitz-dom

use std::sync::Arc;

use anyrender_vello::VelloWindowRenderer as WindowRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_shell::{
    BlitzApplication, BlitzShellProxy, Config, EventLoop, WindowConfig, create_default_event_loop,
};
use blitz_traits::net::NetProvider;

#[doc(inline)]
/// Re-export of [`blitz_dom`].
pub use blitz_dom as dom;
#[doc(inline)]
/// Re-export of [`blitz_html`]. HTML parsing on top of blitz-dom
pub use blitz_html as html;
#[cfg(feature = "net")]
#[doc(inline)]
/// Re-export of [`blitz_net`].
pub use blitz_net as net;
#[doc(inline)]
/// Re-export of [`blitz_paint`].
pub use blitz_paint as paint;
#[doc(inline)]
/// Re-export of [`blitz_shell`].
pub use blitz_shell as shell;
#[doc(inline)]
/// Re-export of [`blitz_traits`](https://docs.rs/blitz-traits). Base types and traits for interoperability between modules
pub use blitz_traits as traits;

#[cfg(feature = "net")]
pub fn launch_url(url: &str) {
    // Assert that url is valid
    println!("{url}");
    let url = url.to_owned();
    let url = url::Url::parse(&url).expect("Invalid url");

    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    let event_loop = create_default_event_loop();
    let (proxy, reciever) = BlitzShellProxy::new(event_loop.create_proxy());
    let net_provider = create_net_provider(proxy.clone());
    let application = BlitzApplication::new(proxy, reciever);

    let (url, bytes) = rt
        .block_on(net_provider.fetch_async(blitz_traits::net::Request::get(url)))
        .unwrap();
    let html = std::str::from_utf8(bytes.as_ref()).unwrap();

    launch_internal(
        html,
        Config {
            stylesheets: Vec::new(),
            base_url: Some(url),
        },
        event_loop,
        application,
        net_provider,
    )
}

pub fn launch_static_html(html: &str) {
    launch_static_html_cfg(html, Config::default())
}

pub fn launch_static_html_cfg(html: &str, cfg: Config) {
    // Turn on the runtime and enter it
    #[cfg(feature = "net")]
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(feature = "net")]
    let _guard = rt.enter();

    let event_loop = create_default_event_loop();
    let (proxy, reciever) = BlitzShellProxy::new(event_loop.create_proxy());
    let net_provider = create_net_provider(proxy.clone());
    let application = BlitzApplication::new(proxy, reciever);

    launch_internal(html, cfg, event_loop, application, net_provider)
}

fn launch_internal(
    html: &str,
    cfg: Config,
    event_loop: EventLoop,
    mut application: BlitzApplication<WindowRenderer>,
    net_provider: Arc<dyn NetProvider>,
) {
    let doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            base_url: cfg.base_url,
            ua_stylesheets: Some(cfg.stylesheets),
            net_provider: Some(net_provider),
            ..Default::default()
        },
    );
    let renderer = WindowRenderer::new();
    let window = WindowConfig::new(Box::new(doc) as _, renderer);

    // Create application

    application.add_window(window);

    // Run event loop
    event_loop.run_app(application).unwrap()
}

#[cfg(feature = "net")]
type EnabledNetProvider = blitz_net::Provider;
#[cfg(not(feature = "net"))]
type EnabledNetProvider = blitz_traits::net::DummyNetProvider;

fn create_net_provider(proxy: BlitzShellProxy) -> Arc<EnabledNetProvider> {
    #[cfg(feature = "net")]
    let net_provider = Arc::new(blitz_net::Provider::new(Some(Arc::new(proxy))));
    #[cfg(not(feature = "net"))]
    let net_provider = {
        use blitz_traits::net::DummyNetProvider;

        // This isn't used without the net feature, so ignore it here to not
        // get unnused warnings.
        let _ = event_loop;
        Arc::new(DummyNetProvider::default())
    };

    net_provider
}
