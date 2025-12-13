//! Demonstrate opening additional windows from a pure Dioxus app.

use blitz_traits::shell::ShellProvider;
use dioxus::prelude::*;
use dioxus_native::DioxusNativeProvider;
use std::sync::Arc;
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
    let mut counter = use_signal(|| 0);
    rsx! {
        main {
            h1 { "Blitz multi-window" }
            p { "Click the button to open another RSX window." }
            div {
                button {
                    onclick: move |_| {
                        let vdom = VirtualDom::new(secondary_window);
                        let attributes = WindowAttributes::default()
                            .with_title(format!("window#{}", counter))
                            .with_inner_size(winit::dpi::LogicalSize::new(400.0, 300.0));
                        provider.create_document_window(vdom, attributes);
                        counter += 1;
                    },
                    "Open secondary window"
                }
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
