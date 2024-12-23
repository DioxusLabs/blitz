#![cfg_attr(docsrs, feature(doc_cfg))]

//! A native renderer for Dioxus.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`] accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `menu`: Enables the [`muda`] menubar.
//!  - `tracing`: Enables tracing support.

use blitz_html::HtmlDocument;
use blitz_renderer_vello::BlitzVelloRenderer;
use blitz_shell::{
    create_default_event_loop, BlitzApplication, BlitzEvent, BlitzShellNetCallback, Config,
    WindowConfig,
};

#[cfg(feature = "net")]
pub fn launch_url(url: &str) {
    use reqwest::Client;
    use url::Url;

    const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";
    println!("{}", url);

    // Assert that url is valid
    let url = url.to_owned();
    Url::parse(&url).expect("Invalid url");

    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    let client = Client::new();
    let html = rt.block_on(async {
        client
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
    });

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
    #[cfg(feature = "net")]
    {
        // Turn on the runtime and enter it
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = rt.enter();
    }

    launch_internal(html, cfg)
}

fn launch_internal(html: &str, cfg: Config) {
    let event_loop = create_default_event_loop::<BlitzEvent>();

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

    let doc = HtmlDocument::from_html(html, cfg.base_url, cfg.stylesheets, net_provider, None);
    let window: WindowConfig<HtmlDocument, BlitzVelloRenderer> = WindowConfig::new(doc);

    // Create application
    let mut application = BlitzApplication::new(event_loop.create_proxy());
    application.add_window(window);

    // Run event loop
    event_loop.run_app(&mut application).unwrap()
}
