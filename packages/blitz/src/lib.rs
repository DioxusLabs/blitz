#![cfg_attr(docsrs, feature(doc_cfg))]

//! A native renderer for Dioxus.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`] accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `tracing`: Enables tracing support.

use std::sync::Arc;

use anyrender_vello::VelloWindowRenderer as WindowRenderer;
use blitz_dom::net::Resource;
use blitz_html::HtmlDocument;
use blitz_shell::{
    BlitzApplication, BlitzShellEvent, BlitzShellNetCallback, Config, EventLoop, WindowConfig,
    create_default_event_loop,
};
use blitz_traits::navigation::DummyNavigationProvider;

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

    let event_loop = create_default_event_loop::<BlitzShellEvent>();
    let net_provider = create_net_provider(&event_loop);

    let (url, bytes) = rt
        .block_on(net_provider.fetch_async(Request::get(url)))
        .unwrap();
    let html = str::from_utf8(bytes.as_ref()).unwrap();

    launch_internal(
        html,
        Config {
            stylesheets: Vec::new(),
            base_url: Some(url),
        },
        event_loop,
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

    let event_loop = create_default_event_loop::<BlitzShellEvent>();
    let net_provider = create_net_provider(&event_loop);

    launch_internal(html, cfg, event_loop, net_provider)
}

fn launch_internal(
    html: &str,
    cfg: Config,
    event_loop: EventLoop<BlitzShellEvent>,
    net_provider: Arc<dyn NetProvider<Resource>>,
) {
    let navigation_provider = Arc::new(DummyNavigationProvider);
    let doc = HtmlDocument::from_html(
        html,
        cfg.base_url,
        cfg.stylesheets,
        net_provider,
        None,
        navigation_provider,
    );
    let renderer = WindowRenderer::new();
    let window = WindowConfig::new(Box::new(doc) as _, renderer);

    // Create application
    let mut application = BlitzApplication::new(event_loop.create_proxy());
    application.add_window(window);

    // Run event loop
    event_loop.run_app(&mut application).unwrap()
}

#[cfg(feature = "net")]
type EnabledNetProvider = blitz_net::Provider<Resource>;
#[cfg(not(feature = "net"))]
type EnabledNetProvider = blitz_traits::net::DummyNetProvider;

fn create_net_provider(
    event_loop: &blitz_shell::EventLoop<BlitzShellEvent>,
) -> Arc<EnabledNetProvider> {
    #[cfg(feature = "net")]
    let net_provider = {
        let proxy = event_loop.create_proxy();
        let callback = BlitzShellNetCallback::shared(proxy);
        Arc::new(blitz_net::Provider::new(callback))
    };
    #[cfg(not(feature = "net"))]
    let net_provider = {
        use blitz_traits::net::DummyNetProvider;
        Arc::new(DummyNetProvider::default())
    };

    net_provider
}
