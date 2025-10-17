#![cfg_attr(docsrs, feature(doc_cfg))]

//! A native renderer for Dioxus.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`] accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `tracing`: Enables tracing support.

mod dioxus_document;
mod events;
mod mutation_writer;

mod dioxus_renderer;
use std::any::Any;

#[cfg(feature = "gpu")]
pub use anyrender_vello::wgpu::{Features, Limits};
#[cfg(feature = "gpu")]
pub use dioxus_renderer::use_wgpu;

pub use dioxus_document::DioxusDocument;
pub use dioxus_renderer::DxnWindowRenderer;
pub use mutation_writer::MutationWriter;

use blitz_dom::{LocalName, Namespace, QualName, ns};
use blitz_shell::{BlitzApplication, BlitzShellEvent, WindowConfig, create_default_event_loop};
use dioxus_core::{Element, VirtualDom};

type NodeId = usize;

/// Launch an interactive HTML/CSS renderer driven by the Dioxus virtualdom
pub fn launch(root: fn() -> Element) {
    launch_cfg(root, Vec::new(), Vec::new())
}

/// Launches the WebView and runs the event loop, with configuration and root props.
pub fn launch_cfg(
    root: fn() -> Element,
    contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
    platform_config: Vec<Box<dyn Any>>,
) {
    // Read config values
    #[cfg(feature = "gpu")]
    let mut features = None;
    #[cfg(feature = "gpu")]
    let mut limits = None;
    for mut cfg in platform_config {
        #[cfg(feature = "gpu")]
        {
            cfg = match cfg.downcast::<Features>() {
                Ok(value) => {
                    features = Some(*value);
                    continue;
                }
                Err(cfg) => cfg,
            };
            cfg = match cfg.downcast::<Limits>() {
                Ok(value) => {
                    limits = Some(*value);
                    continue;
                }
                Err(cfg) => cfg,
            };
        }
        let _ = cfg;
    }

    let event_loop = create_default_event_loop::<BlitzShellEvent>();

    #[cfg(feature = "net")]
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(feature = "net")]
    let _guard = rt.enter();

    #[cfg(any(feature = "data-uri", feature = "net"))]
    let net_provider = {
        #[cfg(feature = "net")]
        use blitz_net::Provider as NetProvider;
        #[cfg(all(feature = "data-uri", not(feature = "net")))]
        use blitz_shell::DataUriNetProvider as NetProvider;

        use blitz_shell::BlitzShellNetCallback;

        let proxy = event_loop.create_proxy();
        let net_callback = BlitzShellNetCallback::shared(proxy);
        let net_provider = NetProvider::shared(net_callback);

        Some(net_provider)
    };

    #[cfg(all(not(feature = "net"), not(feature = "data-uri")))]
    let net_provider = None;

    // Create the renderer
    #[cfg(feature = "gpu")]
    let renderer = DxnWindowRenderer::with_features_and_limits(features, limits);
    #[cfg(any(feature = "cpu-base", feature = "hybrid"))]
    let renderer = DxnWindowRenderer::new();

    // Spin up the virtualdom
    let mut vdom = VirtualDom::new(root);
    vdom.insert_any_root_context(Box::new(renderer.clone()));
    for context in contexts {
        vdom.insert_any_root_context(context());
    }

    // Create the document and renderer
    let doc = DioxusDocument::new(vdom, net_provider);
    let window = WindowConfig::new(Box::new(doc) as _, renderer);

    // Create application
    let mut application = BlitzApplication::<DxnWindowRenderer>::new(event_loop.create_proxy());
    application.add_window(window);

    // Run event loop
    event_loop.run_app(&mut application).unwrap();
}

pub(crate) fn qual_name(local_name: &str, namespace: Option<&str>) -> QualName {
    QualName {
        prefix: None,
        ns: namespace.map(Namespace::from).unwrap_or(ns!(html)),
        local: LocalName::from(local_name),
    }
}

// Syntax sugar to make tracing calls less noisy in function below
macro_rules! trace {
    ($pattern:literal) => {{
        #[cfg(feature = "tracing")]
        tracing::info!($pattern);
    }};
    ($pattern:literal, $item1:expr) => {{
        #[cfg(feature = "tracing")]
        tracing::info!($pattern, $item1);
    }};
    ($pattern:literal, $item1:expr, $item2:expr) => {{
        #[cfg(feature = "tracing")]
        tracing::info!($pattern, $item1, $item2);
    }};
    ($pattern:literal, $item1:expr, $item2:expr, $item3:expr) => {{
        #[cfg(feature = "tracing")]
        tracing::info!($pattern, $item1, $item2);
    }};
    ($pattern:literal, $item1:expr, $item2:expr, $item3:expr, $item4:expr) => {{
        #[cfg(feature = "tracing")]
        tracing::info!($pattern, $item1, $item2, $item3, $item4);
    }};
}
pub(crate) use trace;
