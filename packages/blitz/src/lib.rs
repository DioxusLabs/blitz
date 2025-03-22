#![cfg_attr(docsrs, feature(doc_cfg))]

//! A native renderer for Dioxus.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`] accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `menu`: Enables the [`muda`] menubar.
//!  - `tracing`: Enables tracing support.

use std::sync::Arc;

use blitz_html::HtmlDocument;
use blitz_renderer_vello::BlitzVelloRenderer;
use blitz_shell::{
    create_default_event_loop, BlitzApplication, BlitzShellEvent, BlitzShellNetCallback, Config,
    WindowConfig,
};
use blitz_traits::navigation::DummyNavigationProvider;

#[cfg(feature = "net")]
pub fn launch_url(url: &str) {
    // Assert that url is valid
    println!("{}", url);
    let url = url.to_owned();
    url::Url::parse(&url).expect("Invalid url");

    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    let html = rt.block_on(blitz_net::get_text(&url));

    launch_internal(
        &html,
        Config {
            stylesheets: Vec::new(),
            base_url: Some(url),
        },
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

    launch_internal(html, cfg)
}

fn launch_internal(html: &str, cfg: Config) {
    let event_loop = create_default_event_loop::<BlitzShellEvent>();

    #[cfg(feature = "net")]
    let net_provider = {
        let proxy = event_loop.create_proxy();
        let callback = BlitzShellNetCallback::shared(proxy);
        blitz_net::Provider::shared(callback)
    };
    #[cfg(not(feature = "net"))]
    let net_provider = {
        use blitz_traits::net::DummyNetProvider;
        Arc::new(DummyNetProvider::default())
    };

    let navigation_provider = Arc::new(DummyNavigationProvider);

    let doc = HtmlDocument::from_html(
        html,
        cfg.base_url,
        cfg.stylesheets,
        net_provider,
        None,
        navigation_provider,
    );
    let window: WindowConfig<HtmlDocument, BlitzVelloRenderer> = WindowConfig::new(doc);

    // Create application
    let mut application = BlitzApplication::new(event_loop.create_proxy());
    application.add_window(window);

    // Run event loop
    event_loop.run_app(&mut application).unwrap()
}
