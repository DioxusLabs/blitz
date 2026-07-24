//! Drive the renderer from Dioxus, with a transparent, decoration-less,
//! content-sized, draggable window.
use blitz_traits::shell::ShellProvider;
use dioxus_native::prelude::*;
use dioxus_native::{Color, CompositeAlphaMode, Config, LogicalSize, WindowAttributes, use_window};
use std::any::Any;
use std::sync::Arc;

/// Fixed window size, chosen to snugly wrap the card content.
const WINDOW_WIDTH: f64 = 360.0;
const WINDOW_HEIGHT: f64 = 300.0;

/// Launch the app with a transparent window.
pub fn launch() {
    // 1. Tell winit to create a window that is transparent, has no titlebar /
    //    decorations, and is sized to fit the content.
    let window_attributes = WindowAttributes::default()
        .with_title("Dioxus Native - Transparent Window")
        .with_transparent(true)
        .with_decorations(false)
        .with_surface_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));

    // 2. Tell the renderer to composite the surface with an alpha-aware mode
    //    and to clear each frame to a fully transparent color.
    let config = Config::new()
        .with_window_attributes(window_attributes)
        .with_alpha_mode(CompositeAlphaMode::Transparent)
        .with_base_color(Color::TRANSPARENT);

    let configs: Vec<Box<dyn Any>> = vec![Box::new(config)];
    dioxus_native::launch_cfg(app, Vec::new(), configs);
}

pub fn app() -> Element {
    let mut count = use_signal(|| 0);
    let window = use_window();

    // The shell provider lets us drive window chrome (e.g. closing the window)
    // even though the window has no native decorations.
    let shell = use_hook(consume_context::<Arc<dyn ShellProvider>>);

    // Dragging the (empty) background moves the whole window. Because there are
    // no window decorations, this is how the user repositions the window.
    let start_drag = move |evt: Event<MouseData>| {
        evt.prevent_default();
        let _ = window.drag_window();
    };

    let close = move |evt: Event<MouseData>| {
        evt.stop_propagation();
        shell.request_window_close();
    };

    rsx! {
        style { {CSS} }
        // The card fills the whole (content-sized) window. Its rounded corners
        // reveal the transparent window behind them.
        div { class: "card", onmousedown: start_drag,
            button {
                class: "close-button",
                // Stop propagation so pressing close doesn't start a drag.
                onmousedown: |evt| evt.stop_propagation(),
                onclick: close,
                "\u{00d7}"
            }
            h1 { class: "header", "Count: {count}" }
            p { class: "hint", "Drag the background to move the window." }
            div { class: "buttons",
                button {
                    class: "counter-button btn-green",
                    // Stop propagation so pressing a button doesn't start a drag.
                    onmousedown: |evt| evt.stop_propagation(),
                    onclick: move |_| { count += 1 },
                    "Increment"
                }
                button {
                    class: "counter-button btn-red",
                    onmousedown: |evt| evt.stop_propagation(),
                    onclick: move |_| { count -= 1 },
                    "Decrement"
                }
                button {
                    class: "counter-button btn-blue",
                    onmousedown: |evt| evt.stop_propagation(),
                    onclick: move |_| { count.set(0) },
                    "Reset"
                }
            }
        }
    }
}

// 3. The CSS must leave the page background transparent so the transparent
//    window shows through. Only the `.card` element paints a (semi-transparent)
//    background, and it fills the whole window.
const CSS: &str = r#"

* {
    user-select: none;
}

html, body, #main {
    padding: 0;
    margin: 0;
    height: 100%;
    width: 100%;
    /* Transparent so the window's transparency is visible */
    background-color: transparent;
}

.card {
    position: relative;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    height: 100%;
    width: 100%;
    padding: 30px;
    border-radius: 16px;
    /* Semi-transparent card so both the card and the desktop behind are visible */
    background-color: rgba(20, 20, 30, 0.75);
    font-family: sans-serif;
    color: white;
}

.close-button {
    position: absolute;
    top: 10px;
    right: 10px;
    width: 28px;
    height: 28px;
    padding: 0;
    display: flex;
    justify-content: center;
    align-items: center;
    border: none;
    border-radius: 50%;
    background-color: rgba(255, 255, 255, 0.15);
    color: white;
    font-size: 1.2rem;
    line-height: 1;
    cursor: pointer;
}
.close-button:hover {
    background-color: rgba(255, 80, 80, 0.9);
}

.header {
    margin: 0 0 10px 0;
    line-height: 1;
}

.hint {
    margin: 0 0 20px 0;
    max-width: 280px;
    text-align: center;
    opacity: 0.85;
}

.buttons {
    display: flex;
    flex-direction: row;
    justify-content: center;
    align-items: center;
}

.counter-button {
    margin: 0 6px;
    padding: 10px 16px;
    border-radius: 5px;
    font-size: 1.1rem;
    cursor: pointer;
    line-height: 1;
    font-family: sans-serif;
    border-width: 2px;
    border-style: solid;
}
.counter-button:focus {
    outline: 4px solid white;
}

.btn-green {
    background-color: green;
    border-color: green;
    color: white;
}
.btn-green:hover {
    color: green;
    background-color: white;
}

.btn-red {
    background-color: red;
    border-color: red;
    color: white;
}
.btn-red:hover {
    color: red;
    background-color: white;
}

.btn-blue {
    background-color: blue;
    border-color: blue;
    color: white;
}
.btn-blue:hover {
    color: blue;
    background-color: white;
}

"#;
