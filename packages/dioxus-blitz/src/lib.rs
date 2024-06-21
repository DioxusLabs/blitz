mod documents;
mod waker;
mod window;

use crate::waker::{EventData, UserEvent};
use crate::{documents::HtmlDocument, window::View};

use blitz::RenderState;
use blitz_dom::DocumentLike;
use dioxus::prelude::*;
use documents::DioxusDocument;
use muda::{MenuEvent, MenuId};
use std::collections::HashMap;
use url::Url;
use winit::event_loop::EventLoop;
use winit::window::WindowId;
use winit::{
    event::{Event, WindowEvent},
    event_loop::ControlFlow,
};

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
    let window = View::new(document);

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
    let window = View::new(document);
    launch_with_window(window)
}

fn launch_with_window<Doc: DocumentLike + 'static>(window: View<'static, Doc>) {
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let _guard = rt.enter();

    // Build an event loop for the application
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    let proxy = event_loop.create_proxy();

    // Multiwindow ftw
    let mut windows: HashMap<WindowId, window::View<'_, Doc>> = HashMap::new();
    let mut pending_windows = Vec::new();

    pending_windows.push(window);
    let menu_channel = MenuEvent::receiver();

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
        let Ok(cfg) = dioxus_cli_config::CURRENT_CONFIG.as_ref() else {
            return;
        };

        dioxus_hot_reload::connect_at(cfg.target_dir.join("dioxusin"), {
            let proxy = proxy.clone();
            move |template| {
                let _ = proxy.send_event(UserEvent::HotReloadEvent(template));
            }
        });
    }

    // the move to winit wants us to use a struct with a run method instead of the callback approach
    // we want to just keep the callback approach for now
    #[allow(deprecated)]
    // the move to winit wants us to use a struct with a run method instead of the callback approach
    // we want to just keep the callback approach for now
    #[allow(deprecated)]
    event_loop
        .run(move |event, event_loop| {
            event_loop.set_control_flow(ControlFlow::Wait);

            let mut on_resume = || {
                for (_, view) in windows.iter_mut() {
                    view.resume(event_loop, &proxy, &rt);
                }

                for view in pending_windows.iter_mut() {
                    view.resume(event_loop, &proxy, &rt);
                }

                for window in pending_windows.drain(..) {
                    let RenderState::Active(state) = &window.renderer.render_state else {
                        continue;
                    };
                    windows.insert(state.window.id(), window);
                }
            };

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            if initial {
                on_resume();
                initial = false;
            }

            match event {
                // Exit the app when close is request
                // Not always necessary
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => event_loop.exit(),

                Event::WindowEvent {
                    window_id,
                    event: winit::event::WindowEvent::RedrawRequested,
                } => {
                    if let Some(window) = windows.get_mut(&window_id) {
                        window.renderer.dom.as_mut().resolve();
                        window.renderer.render(&mut window.scene);
                    };
                }

                Event::UserEvent(UserEvent::Window {
                    data: EventData::Poll,
                    window_id: id,
                }) => {
                    if let Some(view) = windows.get_mut(&id) {
                        if view.poll() {
                            view.request_redraw();
                        }
                    };
                }

                #[cfg(all(
                    feature = "hot-reload",
                    debug_assertions,
                    not(target_os = "android"),
                    not(target_os = "ios")
                ))]
                Event::UserEvent(UserEvent::HotReloadEvent(msg)) => match msg {
                    dioxus_hot_reload::HotReloadMsg::UpdateTemplate(template) => {
                        for window in windows.values_mut() {
                            if let Some(dx_doc) = window
                                .renderer
                                .dom
                                .as_any_mut()
                                .downcast_mut::<DioxusDocument>()
                            {
                                dx_doc.vdom.replace_template(template);

                                if window.poll() {
                                    window.request_redraw();
                                }
                            }
                        }
                    }
                    dioxus_hot_reload::HotReloadMsg::Shutdown => event_loop.exit(),
                    dioxus_hot_reload::HotReloadMsg::UpdateAsset(asset) => {
                        // TODO dioxus-desktop seems to handle this by forcing a reload of all stylesheets.
                        dbg!("Update asset {:?}", asset);
                    }
                },

                // Event::UserEvent(_redraw) => {
                //     for (_, view) in windows.iter() {
                //         view.request_redraw();
                //     }
                // }
                Event::NewEvents(_) => {
                    for window_id in windows.keys().copied() {
                        _ = proxy.send_event(UserEvent::Window {
                            data: EventData::Poll,
                            window_id,
                        });
                    }
                }

                Event::Suspended => {
                    for (_, view) in windows.iter_mut() {
                        view.suspend();
                    }
                }

                Event::Resumed => on_resume(),

                Event::WindowEvent {
                    window_id, event, ..
                } => {
                    if let Some(window) = windows.get_mut(&window_id) {
                        window.handle_window_event(event);
                    };
                }

                _ => (),
            }

            if let Ok(event) = menu_channel.try_recv() {
                if event.id == MenuId::new("dev.show_layout") {
                    for (_, view) in windows.iter_mut() {
                        view.renderer.devtools.show_layout = !view.renderer.devtools.show_layout;
                        view.request_redraw();
                    }
                }
            }
        })
        .unwrap();
}
