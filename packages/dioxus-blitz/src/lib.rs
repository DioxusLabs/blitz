#![cfg_attr(docsrs, feature(doc_cfg))]

//! A native renderer for Dioxus.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`] accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `menu`: Enables the [`muda`] menubar.
//!  - `tracing`: Enables tracing support.

mod application;
mod documents;
mod stylo_to_winit;
mod waker;
mod window;

#[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
mod menu;

#[cfg(feature = "accessibility")]
mod accessibility;

use blitz_dom::{DocumentLike, HtmlDocument};
use dioxus::prelude::{ComponentFunction, Element, VirtualDom};
use url::Url;
use winit::{
    dpi::LogicalSize,
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

use crate::application::Application;
use crate::window::View;

pub use crate::documents::DioxusDocument;
pub use crate::waker::BlitzEvent;
pub use crate::window::WindowConfig;

pub mod exports {
    pub use dioxus;
}

#[derive(Default)]
pub struct Config {
    pub stylesheets: Vec<String>,
    pub base_url: Option<String>,
}

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
    // Spin up the virtualdom
    // We're going to need to hit it with a special waker
    let vdom = VirtualDom::new_with_props(root, props);
    let document = DioxusDocument::new(vdom);

    launch_with_document(document)
}

pub fn launch_url(url: &str) {
    const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";
    println!("{}", url);

    // Assert that url is valid
    let url = url.to_owned();
    Url::parse(&url).expect("Invalid url");

    let html = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .call()
        .unwrap()
        .into_string()
        .unwrap();

    launch_static_html_cfg(
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
    let document = HtmlDocument::from_html(html, cfg.base_url, cfg.stylesheets);
    launch_with_document(document)
}

fn launch_with_document(doc: impl DocumentLike) {
    let mut window_attrs = Window::default_attributes();
    if !cfg!(all(target_os = "android", target_os = "ios")) {
        window_attrs.inner_size = Some(
            LogicalSize {
                width: 800.,
                height: 800.,
            }
            .into(),
        );
    }
    let window = WindowConfig::new(doc);

    launch_with_window(window)
}

fn launch_with_window<Doc: DocumentLike + 'static>(window: WindowConfig<Doc>) {
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let _guard = rt.enter();

    // Build an event loop for the application
    let mut ev_builder = EventLoop::<BlitzEvent>::with_user_event();
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        ev_builder.with_android_app(current_android_app());
    }
    let event_loop = ev_builder.build().unwrap();
    let proxy = event_loop.create_proxy();
    event_loop.set_control_flow(ControlFlow::Wait);

    // Setup hot-reloading if enabled.
    #[cfg(all(
        feature = "hot-reload",
        debug_assertions,
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    {
        if let Ok(cfg) = dioxus_cli_config::CURRENT_CONFIG.as_ref() {
            dioxus_hot_reload::connect_at(cfg.target_dir.join("dioxusin"), {
                let proxy = proxy.clone();
                move |template| {
                    let _ = proxy.send_event(BlitzEvent::HotReloadEvent(template));
                }
            })
        }
    }

    // Create application
    let mut application = Application::new(rt, proxy);
    application.add_window(window);

    // Run event loop
    event_loop.run_app(&mut application).unwrap()
}

#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::OnceLock<android_activity::AndroidApp> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
/// Set the current [`AndroidApp`](android_activity::AndroidApp).
pub fn set_android_app(app: android_activity::AndroidApp) {
    ANDROID_APP.set(app).unwrap()
}

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
/// Get the current [`AndroidApp`](android_activity::AndroidApp).
/// This will panic if the android activity has not been setup with [`set_android_app`].
pub fn current_android_app(app: android_activity::AndroidApp) -> AndroidApp {
    ANDROID_APP.get().unwrap().clone()
}
