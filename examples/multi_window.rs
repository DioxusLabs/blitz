//! Demonstrate opening additional windows from a pure Dioxus app.

use blitz_traits::shell::ShellProvider;
use dioxus::prelude::*;
use dioxus_native::DioxusNativeProvider;
use std::sync::Arc;
use std::sync::Weak;
use winit::window::Window;
use winit::window::WindowId;
use winit::window::{WindowAttributes, WindowButtons};

fn main() {
    // Demonstrate how to pass custom WindowAttributes (title, size, decorations).
    let attrs = WindowAttributes::default()
        .with_title("Multi-window Demo")
        .with_inner_size(winit::dpi::LogicalSize::new(600.0, 800.0))
        .with_enabled_buttons(WindowButtons::all());
    dioxus_native::launch_cfg(app, vec![], vec![Box::new(attrs)]);
}

fn app() -> Element {
    let provider = use_context::<DioxusNativeProvider>();
    let mut counter = use_signal(|| 0u32);
    let spawned_windows = use_signal(Vec::<(WindowId, Weak<Window>)>::new);

    rsx! {
        main {
            h1 { "Blitz multi-window" }
            p { "Click the button to open another RSX window." }
            div {
                button {
                    onclick: move |_| {
                        let vdom = VirtualDom::new(secondary_window);
                        let title = format!("window#{}", counter());
                        let attributes = WindowAttributes::default()
                            .with_title(title)
                            .with_inner_size(winit::dpi::LogicalSize::new(400.0, 300.0));
                        let config = dioxus_native::Config::new().with_window_attributes(attributes);
                        let receiver = provider.new_window(vdom, config);
                        let mut spawned_windows = spawned_windows;
                        spawn(async move {
                            if let Ok((window_id, window)) = receiver.await {
                                let mut next = spawned_windows();
                                next.push((window_id, Arc::downgrade(&window)));
                                spawned_windows.set(next);
                            }
                        });
                        counter += 1;
                    },
                    "Open secondary window"
                }
            }

            h2 { "Spawned windows" }
            ul {
                {spawned_windows().into_iter().map(|(id, window)| match window.upgrade() {
                    Some(window) => {
                        let title = window.title();
                        let title = if title.is_empty() { String::from("<unsupported>") } else { title };
                        rsx! { li { "{title} (ID: {id:?})" } }
                    }
                    None => rsx! { li { "<closed> (ID: {id:?})" } },
                })}
            }
        }
    }
}

fn secondary_window() -> Element {
    let shell_provider = use_context::<Arc<dyn ShellProvider>>();

    rsx! {
        main {
            h1 { "Secondary window" }
            p { "This content comes from another RSX function." }
            button {
                onclick: move |_| {
                    shell_provider.set_window_title(format!("Time: {:?}", std::time::SystemTime::now()))
                },
                "click to update title",
            }
        }
    }
}
