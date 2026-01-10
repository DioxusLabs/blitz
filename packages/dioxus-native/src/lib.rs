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
// windowing module removed

#[cfg(feature = "prelude")]
pub mod prelude;

#[cfg(feature = "net")]
use blitz_traits::net::NetProvider;
#[doc(inline)]
pub use dioxus_native_dom::*;

use assets::DioxusNativeNetProvider;
pub use dioxus_application::DioxusNativeProvider;
#[doc(hidden)]
pub use dioxus_application::OpaquePtr;
#[doc(hidden)]
pub use dioxus_application::UnsafeBox;
pub use dioxus_application::{DioxusNativeApplication, DioxusNativeEvent};
pub use dioxus_renderer::DioxusNativeWindowRenderer;

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

use blitz_shell::{create_default_event_loop, BlitzShellEvent, Config as BlitzConfig};
use dioxus_core::{ComponentFunction, Element, VirtualDom};
use link_handler::DioxusNativeNavigationProvider;
use std::any::Any;
use std::sync::Arc;
use winit::window::WindowAttributes;

/// Window configuration for Dioxus Native.
#[derive(Clone)]
pub struct Config {
    window_attributes: WindowAttributes,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_attributes: WindowAttributes::default(),
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_window(mut self, window_attributes: WindowAttributes) -> Self {
        self.window_attributes = window_attributes;
        self
    }

    pub fn with_window_attributes(self, window_attributes: WindowAttributes) -> Self {
        self.with_window(window_attributes)
    }

    pub fn window_attributes(&self) -> &WindowAttributes {
        &self.window_attributes
    }

    pub fn into_window_attributes(self) -> WindowAttributes {
        self.window_attributes
    }
}

impl From<WindowAttributes> for Config {
    fn from(window_attributes: WindowAttributes) -> Self {
        Self { window_attributes }
    }
}

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
        cfg = try_read_config!(cfg, _config, BlitzConfig);
        let _ = cfg;
    }

    let event_loop = create_default_event_loop::<BlitzShellEvent>();
    let proxy = event_loop.create_proxy();

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
    let app = app.clone();

    #[cfg(feature = "net")]
    let (net_provider, inner_net_provider) = {
        use blitz_shell::BlitzShellNetWaker;

        let proxy = event_loop.create_proxy();
        let net_waker = Some(BlitzShellNetWaker::shared(proxy.clone()));

        let inner_net_provider = Arc::new(blitz_net::Provider::new(net_waker.clone()));

        let net_provider = Arc::new(DioxusNativeNetProvider::with_inner(
            proxy,
            Arc::clone(&inner_net_provider) as _,
        )) as Arc<dyn NetProvider>;

        (net_provider, Some(inner_net_provider))
    };

    #[cfg(not(feature = "net"))]
    let net_provider = DioxusNativeNetProvider::shared(event_loop.create_proxy());

    // contexts/providers are injected via window runtime

    #[cfg(feature = "html")]
    let html_parser_provider = {
        let html_parser = Arc::new(blitz_html::HtmlProvider) as _;
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
        let features = features;
        let limits = limits.clone();
        Arc::new(move || {
            DioxusNativeWindowRenderer::with_features_and_limits(features, limits.clone())
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
        Arc::new(DioxusNativeWindowRenderer::new);

    let window_attributes = window_attributes.unwrap_or_default();
    let window_config = Config::from(window_attributes);

    let vdom = VirtualDom::new_with_props(app, props);
    let vdom = UnsafeBox::new(Box::new(vdom));

    let mut application = DioxusNativeApplication::new(
        proxy.clone(),
        renderer_factory,
        Arc::clone(&contexts),
        Arc::clone(&net_provider),
        #[cfg(feature = "net")]
        inner_net_provider,
        html_parser_provider.clone(),
        navigation_provider.clone(),
    );

    // Queue the initial window creation via an embedder event.
    let _ = proxy.send_event(BlitzShellEvent::embedder_event(
        DioxusNativeEvent::CreateDocumentWindow {
            vdom,
            config: window_config,
            reply: None,
        },
    ));

    // Run event loop
    event_loop.run_app(&mut application).unwrap();
}
