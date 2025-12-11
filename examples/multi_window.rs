//! Demonstrate opening additional windows from a pure Dioxus app.

use dioxus::prelude::*;
use dioxus_native::{DioxusWindowHandle, DioxusWindowInfo, DioxusWindowOptions};

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    let window_handle = use_context::<DioxusWindowHandle>();
    let mut counter = use_signal(|| 1u32);
    let open_simple = window_handle.clone();
    let open_with_props = window_handle.clone();
    let refresh_handle = window_handle.clone();
    let base_focus_handle = window_handle.clone();
    let base_rename_handle = window_handle.clone();
    let mut known_windows: Signal<Vec<DioxusWindowInfo>> =
        use_signal(|| window_handle.list_windows());
    let known_windows_signal = known_windows.clone();
    let refresh_handle_for_list = refresh_handle.clone();
    let rename_counter = use_signal(|| 1u32);
    let window_rows = {
        let windows_snapshot = known_windows();
        windows_snapshot
            .into_iter()
            .map(|info| {
                let focus_handle = base_focus_handle.clone();
                let rename_handle = base_rename_handle.clone();
                let mut rename_counter_signal = rename_counter.clone();
                let mut update_list_signal = known_windows_signal.clone();
                let update_source = refresh_handle_for_list.clone();
                let id = info.id;
                let title = info.title;
                rsx! {
                    li {
                        "{title} (ID: {id:?})"
                        button {
                            onclick: move |_| focus_handle.focus_window(id),
                            "Focus"
                        }
                        button {
                            onclick: move |_| {
                                let idx = rename_counter_signal();
                                rename_handle.set_window_title(id, format!("Renamed {idx}"));
                                rename_counter_signal += 1;
                                update_list_signal.set(update_source.list_windows());
                            },
                            "Rename"
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
    };

    rsx! {
        main {
            style { {PRIMARY_STYLES} }
            h1 { "Blitz multi-window" }
            p { "Click the button to open another RSX window." }
            div {
                button {
                    onclick: move |_| {
                        open_simple.open_window_with_options(
                            secondary_window,
                            DioxusWindowOptions {
                                title: Some("Secondary window".into()),
                                ..Default::default()
                            },
                        );
                        known_windows.set(open_simple.list_windows());
                    },
                    "Open secondary window"
                }
                button {
                    onclick: move |_| {
                        let idx = counter();
                        open_with_props.open_window_with_props_and_options(
                            message_window,
                            MessageWindowProps {
                                message: format!("Window #{idx}"),
                            },
                            DioxusWindowOptions {
                                title: Some(format!("Window #{idx}")),
                                ..Default::default()
                            },
                        );
                        counter += 1;
                        known_windows.set(open_with_props.list_windows());
                    },
                    "Open window with props"
                }
                button {
                    onclick: move |_| known_windows.set(refresh_handle.list_windows()),
                    "Refresh list"
                }
            }
            ul {{ window_rows.into_iter() }}
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
