//! Demonstrate opening additional windows from a pure Dioxus app.

use dioxus::prelude::*;
use dioxus_native::DioxusWindowHandle;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    let window_handle = use_context::<DioxusWindowHandle>();
    let mut counter = use_signal(|| 1u32);
    let open_simple = window_handle.clone();
    let open_with_props = window_handle.clone();

    rsx! {
        main {
            style { {PRIMARY_STYLES} }
            h1 { "Blitz multi-window" }
            p { "Click the button to open another RSX window." }
            button {
                onclick: move |_| open_simple.open_window(secondary_window),
                "Open secondary window"
            }
            button {
                onclick: move |_| {
                    let idx = counter();
                    open_with_props.open_window_with_props(
                        message_window,
                        MessageWindowProps { message: format!("Window #{idx}") },
                    );
                    counter += 1;
                },
                "Open window with props"
            }
        }
    }
}

fn secondary_window() -> Element {
    rsx! {
        main {
            style { {SECONDARY_STYLES} }
            h1 { "Secondary window" }
            p { "This content comes from another RSX function." }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct MessageWindowProps {
    message: String,
}

fn message_window(props: MessageWindowProps) -> Element {
    rsx! {
        main {
            style { {SECONDARY_STYLES} }
            h1 { "Message window" }
            p { {props.message} }
        }
    }
}

const PRIMARY_STYLES: &str = r#"
    font-family: system-ui, sans-serif;
    min-height: 100vh;
    padding: 40px;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: radial-gradient(circle, #f8fafc, #dbeafe);
"#;

const SECONDARY_STYLES: &str = r#"
    font-family: system-ui, sans-serif;
    min-height: 100vh;
    padding: 56px;
    margin: 0;
    background: #0f172a;
    color: white;
"#;
