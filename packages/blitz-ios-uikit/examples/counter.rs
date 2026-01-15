//! iOS Counter Example - demonstrates blitz-ios-uikit with Dioxus
//!
//! This example creates a simple counter app using the UIKit renderer with
//! Dioxus for reactive UI. It also includes various input types for testing
//! text input and IME interaction.
//!
//! # Running
//!
//! Via Dioxus CLI:
//! ```sh
//! dx serve --example counter --platform ios
//! ```
//!
//! Direct build for iOS Simulator:
//! ```sh
//! cargo build --example counter --target aarch64-apple-ios-sim
//! ```

#![cfg(target_os = "ios")]

use blitz_ios_uikit::launch;
use dioxus::prelude::*;

fn main() {
    launch(app);
}

fn app() -> Element {
    let mut count = use_signal(|| 0);
    let mut text_value = use_signal(|| String::new());
    let mut number_value = use_signal(|| String::new());
    let mut email_value = use_signal(|| String::new());
    let mut password_value = use_signal(|| String::new());
    let mut search_value = use_signal(|| String::new());

    rsx! {
        style { {CSS} }
        div { class: "container",
            img {
                class: "logo",
                src: "https://avatars.githubusercontent.com/u/79236386?s=200&v=4",
                alt: "Dioxus Logo",
            }
            h1 { "Counter" }
            div { class: "count", "{count}" }
            div { class: "buttons",
                button { class: "btn-decrement", onclick: move |_| count -= 1, "-" }
                button { class: "btn-increment", onclick: move |_| count += 1, "+" }
            }
            button { class: "btn-reset", onclick: move |_| count.set(0), "Reset" }

            // Input testing section
            h2 { "Input Tests" }

            div { class: "input-group",
                label { "Text Input:" }
                input {
                    r#type: "text",
                    placeholder: "Enter text...",
                    value: "{text_value}",
                    oninput: move |e| text_value.set(e.value()),
                }
                span { class: "input-value", "Value: {text_value}" }
            }
            div { class: "input-group",
                label { "Text Input:" }
                input {
                    r#type: "text",
                    placeholder: "Enter text...",
                    value: "{text_value}",
                    oninput: move |e| text_value.set(e.value()),
                }
                span { class: "input-value", "Value: {text_value}" }
            }

            // Checkbox test
            div { class: "input-group",
                label { "Checkbox:" }
                input {
                    r#type: "checkbox",
                    onchange: move |e| {
                        println!("Checkbox changed: {:?}", e.checked());
                    },
                }
            }
        }
    }
}

const CSS: &str = r#"
    html, body {
        margin: 0;
        padding: 0;
        height: 100%;
        font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    }

    body {
        display: flex;
        flex-direction: column;
        justify-content: center;
        align-items: center;
        background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
        padding: 20px;
    }

    .container {
        background: white;
        border-radius: 20px;
        padding: 30px;
        text-align: center;
        min-width: 300px;
        max-width: 400px;
    }

    h1 {
        color: #333;
        margin: 0 0 16px 0;
        font-size: 32px;
    }

    h2 {
        color: #333;
        margin: 24px 0 16px 0;
        font-size: 20px;
        border-top: 1px solid #eee;
        padding-top: 16px;
    }

    .count {
        font-size: 48px;
        font-weight: bold;
        color: #667eea;
        margin: 16px 0;
    }

    .buttons {
        display: flex;
        gap: 10px;
        justify-content: center;
    }

    button {
        padding: 15px 30px;
        font-size: 24px;
        border: none;
        border-radius: 10px;
    }

    .btn-increment {
        background: #4CAF50;
        color: white;
    }

    .btn-decrement {
        background: #f44336;
        color: white;
    }

    .btn-reset {
        background: #2196F3;
        color: white;
        margin-top: 10px;
    }

    .logo {
        width: 80px;
        height: 80px;
        margin-bottom: 16px;
        border-radius: 16px;
    }

    .input-group {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        margin: 12px 0;
        width: 100%;
    }

    .input-group label {
        font-size: 14px;
        color: #666;
        margin-bottom: 4px;
    }

    .input-group input[type="text"],
    .input-group input[type="number"],
    .input-group input[type="email"],
    .input-group input[type="password"],
    .input-group input[type="search"] {
        width: 100%;
        padding: 12px;
        font-size: 16px;
        border: 1px solid #ddd;
        border-radius: 8px;
        box-sizing: border-box;
    }

    .input-group input:focus {
        border-color: #667eea;
        outline: none;
    }

    .input-value {
        font-size: 12px;
        color: #999;
        margin-top: 4px;
    }

    .input-group input[type="checkbox"] {
        width: 24px;
        height: 24px;
    }
"#;
