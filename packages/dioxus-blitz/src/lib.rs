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
mod event_handler;

use blitz_dom::DocumentLike;
use dioxus::prelude::{ComponentFunction, Element, VirtualDom};
use url::Url;
use winit::event_loop::{ControlFlow, EventLoop};

use crate::application::Application;
use crate::documents::{DioxusDocument, HtmlDocument};
use crate::waker::{BlitzWindowEvent, BlitzWindowId};
use crate::window::View;

pub use crate::waker::BlitzEvent;
pub use crate::window::WindowConfig;

pub mod exports {
    pub use dioxus;
}

#[derive(Debug, Clone)]
pub enum DioxusBlitzEvent {
    /// A hotreload event, basically telling us to update our templates.
    #[cfg(all(
        feature = "hot-reload",
        debug_assertions,
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    HotReloadEvent(dioxus_hot_reload::HotReloadMsg),
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
    let window = WindowConfig::new(document, 800.0, 600.0);

    launch_with_window(window)
}

fn launch_with_window(window: WindowConfig<DioxusDocument>) {
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let _guard = rt.enter();

    // Build an event loop for the application
    let mut ev_builder = EventLoop::<BlitzEvent<Doc::DocumentEvent>>::with_user_event();
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
                    let _ = proxy.send_event(BlitzEvent::Window {
                        window_id: BlitzWindowId::AllWindows,
                        data: BlitzWindowEvent::DocumentEvent(
                            DioxusBlitzEvent::HotReloadEvent(template),
                        ),
                    });
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
