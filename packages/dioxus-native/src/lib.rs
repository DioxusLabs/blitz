#![cfg_attr(docsrs, feature(doc_cfg))]

//! A native renderer for Dioxus.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`] accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `menu`: Enables the [`muda`] menubar.
//!  - `tracing`: Enables tracing support.

mod dioxus_document;
mod events;
mod mutation_writer;

#[cfg(feature = "gpu_backend")]
use anyrender_vello::VelloWindowRenderer;
#[cfg(feature = "cpu_backend")]
use anyrender_vello_cpu::VelloCpuWindowRenderer as VelloWindowRenderer;

pub use dioxus_document::DioxusDocument;
pub use mutation_writer::MutationWriter;

use blitz_dom::{Atom, QualName, ns};
use blitz_shell::{
    BlitzApplication, BlitzShellEvent, Config, WindowConfig, create_default_event_loop,
};
use dioxus_core::{ComponentFunction, Element, VirtualDom};

type NodeId = usize;

/// Launch an interactive HTML/CSS renderer driven by the Dioxus virtualdom
pub fn launch(root: fn() -> Element) {
    launch_cfg(root, Config::default())
}

pub fn launch_cfg(root: fn() -> Element, cfg: Config) {
    launch_cfg_with_props(root, (), cfg)
}

// todo: props shouldn't have the clone bound - should try and match dioxus-desktop behavior
pub fn launch_cfg_with_props<P: Clone + 'static, M: 'static>(
    root: impl ComponentFunction<P, M>,
    props: P,
    _cfg: Config,
) {
    let event_loop = create_default_event_loop::<BlitzShellEvent>();

    #[cfg(feature = "net")]
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(feature = "net")]
    let _guard = rt.enter();

    #[cfg(feature = "net")]
    let net_provider = {
        use blitz_net::Provider;
        use blitz_shell::BlitzShellNetCallback;

        let proxy = event_loop.create_proxy();
        let net_callback = BlitzShellNetCallback::shared(proxy);
        let net_provider = Provider::shared(net_callback);

        Some(net_provider)
    };

    #[cfg(not(feature = "net"))]
    let net_provider = None;

    // Spin up the virtualdom
    // We're going to need to hit it with a special waker
    let vdom = VirtualDom::new_with_props(root, props);
    let doc = DioxusDocument::new(vdom, net_provider);
    let window = WindowConfig::new(Box::new(doc) as _);

    // Create application
    let mut application = BlitzApplication::<VelloWindowRenderer>::new(event_loop.create_proxy());
    application.add_window(window);

    // Run event loop
    event_loop.run_app(&mut application).unwrap();
}

pub(crate) fn qual_name(local_name: &str, namespace: Option<&str>) -> QualName {
    QualName {
        prefix: None,
        ns: namespace.map(Atom::from).unwrap_or(ns!(html)),
        local: Atom::from(local_name),
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
