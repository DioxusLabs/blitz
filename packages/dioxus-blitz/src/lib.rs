#![cfg_attr(docsrs, feature(doc_cfg))]

mod documents;
mod stylo_to_winit;
mod waker;
mod window;

#[cfg(feature = "accessibility")]
mod accessibility;

use crate::waker::{BlitzEvent, BlitzWindowEvent};
use crate::{documents::HtmlDocument, window::View};

use blitz_dom::DocumentLike;
use dioxus::prelude::*;
use documents::DioxusDocument;
use std::collections::HashMap;
use url::Url;
use winit::event_loop::EventLoop;
use winit::window::WindowId;
use winit::{
    event::{Event, WindowEvent},
    event_loop::ControlFlow,
};

pub use window::WindowConfig;

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
    let window = WindowConfig::new(document, 800.0, 600.0);

    launch_with_window(window)
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
    let document = HtmlDocument::from_html(html, &cfg);
    let window = WindowConfig::new(document, 800.0, 600.0);
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
    let mut builder = EventLoop::<BlitzEvent>::with_user_event();

    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        builder.with_android_app(current_android_app());
    }

    let event_loop = builder.build().unwrap();
    let proxy = event_loop.create_proxy();

    // Multiwindow ftw
    let mut windows: HashMap<WindowId, window::View<'_, Doc>> = HashMap::new();
    let mut pending_windows = Vec::new();

    pending_windows.push(window);

    #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
    let menu_channel = muda::MenuEvent::receiver();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let mut initial = true;

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

    // the move to winit wants us to use a struct with a run method instead of the callback approach
    // we want to just keep the callback approach for now
    #[allow(deprecated)]
    event_loop
        .run(move |event, event_loop| {
            event_loop.set_control_flow(ControlFlow::Wait);

            let on_resume =
                |windows: &mut HashMap<WindowId, window::View<'_, Doc>>,
                 pending_windows: &mut Vec<WindowConfig<Doc>>| {
                    // Resume existing windows
                    for (_, view) in windows.iter_mut() {
                        view.resume(&rt);
                    }

                    // Initialise pending windows
                    for window_config in pending_windows.drain(..) {
                        let mut view = View::init(window_config, event_loop, &proxy);
                        view.resume(&rt);
                        if !view.renderer.is_active() {
                            continue;
                        }
                        windows.insert(view.window_id(), view);
                    }
                };

            let on_suspend = |windows: &mut HashMap<WindowId, window::View<'_, Doc>>| {
                for (_, view) in windows.iter_mut() {
                    view.suspend();
                }
            };

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            if initial {
                on_resume(&mut windows, &mut pending_windows);
                initial = false;
            }

            #[cfg(feature = "tracing")]
            tracing::trace!("Received event: {:?}", event);

            match event {
                Event::Resumed => on_resume(&mut windows, &mut pending_windows),
                Event::Suspended => on_suspend(&mut windows),

                // Exit the app when window close is requested. TODO: Only exit when last window is closed.
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => event_loop.exit(),

                Event::WindowEvent { window_id, event } => {
                    if let Some(window) = windows.get_mut(&window_id) {
                        window.handle_winit_event(event);
                    }
                }

                Event::NewEvents(_) => {
                    for window_id in windows.keys().copied() {
                        _ = proxy.send_event(BlitzEvent::Window {
                            data: BlitzWindowEvent::Poll,
                            window_id,
                        });
                    }
                }

                Event::UserEvent(user_event) => match user_event {
                    BlitzEvent::Window { data, window_id } => {
                        if let Some(view) = windows.get_mut(&window_id) {
                            view.handle_blitz_event(data);
                        };
                    }

                    #[cfg(all(
                        feature = "hot-reload",
                        debug_assertions,
                        not(target_os = "android"),
                        not(target_os = "ios")
                    ))]
                    BlitzEvent::HotReloadEvent(msg) => match msg {
                        dioxus_hot_reload::HotReloadMsg::UpdateTemplate(template) => {
                            for window in windows.values_mut() {
                                if let Some(dx_doc) =
                                    window.dom.as_any_mut().downcast_mut::<DioxusDocument>()
                                {
                                    dx_doc.vdom.replace_template(template);
                                    window.handle_blitz_event(BlitzWindowEvent::Poll);
                                }
                            }
                        }
                        dioxus_hot_reload::HotReloadMsg::Shutdown => event_loop.exit(),
                        dioxus_hot_reload::HotReloadMsg::UpdateAsset(_asset) => {
                            // TODO dioxus-desktop seems to handle this by forcing a reload of all stylesheets.
                        }
                    },
                },

                _ => (),
            }

            #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
            if let Ok(event) = menu_channel.try_recv() {
                if event.id == muda::MenuId::new("dev.show_layout") {
                    for (_, view) in windows.iter_mut() {
                        view.devtools.show_layout = !view.devtools.show_layout;
                        view.request_redraw();
                    }
                }
            }
        })
        .unwrap();
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
