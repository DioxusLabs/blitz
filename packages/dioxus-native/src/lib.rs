#![cfg_attr(docsrs, feature(doc_cfg))]

//! A native renderer for Dioxus.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`](https://docs.rs/accesskit/latest/accesskit/) accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `menu`: Enables the [`muda`](https://docs.rs/muda/latest/muda/) menubar.
//!  - `tracing`: Enables tracing support.

mod assets;
mod contexts;
mod dioxus_application;
mod dioxus_renderer;
mod link_handler;
mod windowing;

#[cfg(feature = "prelude")]
pub mod prelude;

#[cfg(feature = "net")]
use blitz_traits::net::NetProvider;
#[doc(inline)]
pub use dioxus_native_dom::*;

use assets::DioxusNativeNetProvider;
pub use dioxus_application::{DioxusNativeApplication, DioxusNativeEvent};
pub use dioxus_renderer::DioxusNativeWindowRenderer;
pub use windowing::{DioxusWindowHandle, DioxusWindowInfo, DioxusWindowOptions};

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
/// Set the current [`AndroidApp`](android_activity::AndroidApp).
pub fn set_android_app(app: android_activity::AndroidApp) {
    blitz_shell::set_android_app(app);
}

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
/// Get the current [`AndroidApp`](android_activity::AndroidApp).
/// This will panic if the android activity has not been setup with [`set_android_app`].
pub fn current_android_app() -> android_activity::AndroidApp {
    blitz_shell::current_android_app()
}

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
pub use android_activity::AndroidApp;

#[cfg(any(
    feature = "vello",
    all(
        not(feature = "alt-renderer"),
        not(all(target_os = "ios", target_abi = "sim"))
    )
))]
pub use {
    anyrender_vello::{CustomPaintCtx, CustomPaintSource, DeviceHandle, TextureHandle},
    dioxus_renderer::{use_wgpu, Features, Limits},
};

use blitz_shell::{create_default_event_loop, BlitzShellEvent, Config, WindowConfig};
use dioxus_core::{ComponentFunction, Element, VirtualDom};
use link_handler::DioxusNativeNavigationProvider;
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use windowing::{DioxusWindowQueue, DioxusWindowTemplate};
use winit::window::WindowAttributes;

/// Launch an interactive HTML/CSS renderer driven by the Dioxus virtualdom
pub fn launch(app: fn() -> Element) {
    launch_cfg(app, vec![], vec![])
}

pub fn launch_cfg(
    app: fn() -> Element,
    contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
    cfg: Vec<Box<dyn Any>>,
) {
    launch_cfg_with_props(app, (), contexts, cfg)
}

// todo: props shouldn't have the clone bound - should try and match dioxus-desktop behavior
pub fn launch_cfg_with_props<P: Clone + 'static, M: 'static>(
    app: impl ComponentFunction<P, M>,
    props: P,
    contexts: Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>,
    configs: Vec<Box<dyn Any>>,
) {
    // Macro to attempt to downcast a type out of a Box<dyn Any>
    macro_rules! try_read_config {
        ($input:ident, $store:ident, $kind:ty) => {
            // Try to downcast the Box<dyn Any> to type $kind
            match $input.downcast::<$kind>() {
                // If the type matches then write downcast value to variable $store
                Ok(value) => {
                    $store = Some(*value);
                    continue;
                }
                // Else extract the original Box<dyn Any> value out of the error type
                // and return it so that we can try again with a different type.
                Err(cfg) => cfg,
            }
        };
    }

    // Read config values
    #[cfg(any(
        feature = "vello",
        all(
            not(feature = "alt-renderer"),
            not(all(target_os = "ios", target_abi = "sim"))
        )
    ))]
    let (mut features, mut limits) = (None, None);
    let mut window_attributes = None;
    let mut _config = None;
    for mut cfg in configs {
        #[cfg(any(
            feature = "vello",
            all(
                not(feature = "alt-renderer"),
                not(all(target_os = "ios", target_abi = "sim"))
            )
        ))]
        {
            cfg = try_read_config!(cfg, features, Features);
            cfg = try_read_config!(cfg, limits, Limits);
        }
        cfg = try_read_config!(cfg, window_attributes, WindowAttributes);
        cfg = try_read_config!(cfg, _config, Config);
        let _ = cfg;
    }

    let event_loop = create_default_event_loop::<BlitzShellEvent>();

    // Turn on the runtime and enter it
    #[cfg(feature = "net")]
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    #[cfg(feature = "net")]
    let _guard = rt.enter();

    // Setup hot-reloading if enabled.
    #[cfg(all(feature = "hot-reload", debug_assertions))]
    {
        let proxy = event_loop.create_proxy();
        dioxus_devtools::connect(move |event| {
            let dxn_event = DioxusNativeEvent::DevserverEvent(event);
            let _ = proxy.send_event(BlitzShellEvent::embedder_event(dxn_event));
        })
    }

    // Spin up the virtualdom
    // We're going to need to hit it with a special waker
    // Note that we are delaying the initialization of window-specific contexts (net provider, document, etc)
    let contexts = Arc::new(contexts);
    let mut vdom = VirtualDom::new_with_props(app, props);

    // Add contexts
    for context in contexts.iter() {
        vdom.insert_any_root_context(context());
    }

    #[cfg(feature = "net")]
    let net_provider = {
        use blitz_shell::BlitzShellNetWaker;

        let proxy = event_loop.create_proxy();
        let net_waker = Some(BlitzShellNetWaker::shared(proxy.clone()));

        let inner_net_provider = Arc::new(blitz_net::Provider::new(net_waker.clone()));
        vdom.provide_root_context(Arc::clone(&inner_net_provider));

        Arc::new(DioxusNativeNetProvider::with_inner(
            proxy,
            inner_net_provider as _,
        )) as Arc<dyn NetProvider>
    };

    #[cfg(not(feature = "net"))]
    let net_provider = DioxusNativeNetProvider::shared(event_loop.create_proxy());

    vdom.provide_root_context(Arc::clone(&net_provider));

    #[cfg(feature = "html")]
    let html_parser_provider = {
        let html_parser = Arc::new(blitz_html::HtmlProvider) as _;
        vdom.provide_root_context(Arc::clone(&html_parser));
        Some(html_parser)
    };
    #[cfg(not(feature = "html"))]
    let html_parser_provider = None;

    let navigation_provider = Some(Arc::new(DioxusNativeNavigationProvider) as _);

    #[cfg(any(
        feature = "vello",
        all(
            not(feature = "alt-renderer"),
            not(all(target_os = "ios", target_abi = "sim"))
        )
    ))]
    let renderer_factory: Arc<dyn Fn() -> DioxusNativeWindowRenderer + Send + Sync> = {
        let features = features.clone();
        let limits = limits.clone();
        Arc::new(move || {
            DioxusNativeWindowRenderer::with_features_and_limits(features.clone(), limits.clone())
        })
    };
    #[cfg(not(any(
        feature = "vello",
        all(
            not(feature = "alt-renderer"),
            not(all(target_os = "ios", target_abi = "sim"))
        )
    )))]
    let renderer_factory: Arc<dyn Fn() -> DioxusNativeWindowRenderer + Send + Sync> =
        Arc::new(|| DioxusNativeWindowRenderer::new());

    // Create document + window from the baked virtualdom
    let doc = DioxusDocument::new(
        vdom,
        DocumentConfig {
            net_provider: Some(Arc::clone(&net_provider)),
            html_parser_provider: html_parser_provider.clone(),
            navigation_provider: navigation_provider.clone(),
            ..Default::default()
        },
    );
    let window_attributes = window_attributes.unwrap_or_default();
    let renderer = renderer_factory();
    let config = WindowConfig::with_attributes(
        Box::new(doc) as _,
        renderer.clone(),
        window_attributes.clone(),
    );
    let initial_title = window_attributes.title.clone();
    // Create application
    let template = Arc::new(DioxusWindowTemplate::new(
        contexts,
        renderer_factory,
        window_attributes.clone(),
        Arc::clone(&net_provider),
        html_parser_provider.clone(),
        navigation_provider.clone(),
    ));
    let window_queue = Rc::new(DioxusWindowQueue::new());
    let window_registry = Rc::new(RefCell::new(Vec::new()));
    let mut application = DioxusNativeApplication::new(
        event_loop.create_proxy(),
        (config, initial_title),
        template,
        window_queue,
        window_registry,
    );

    // Run event loop
    event_loop.run_app(&mut application).unwrap();
}
