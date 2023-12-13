// Example: dioxus + css + stylo
//
// Create a style context for a dioxus document.

use dioxus::prelude::*;

fn main() {
    let css = r#"
        body {
            background-color: red;
        }

        div {
            background-color: blue;
        }

        div:hover {
            background-color: green;
        }
        "#;

    let nodes = rsx! {
        body {
            div { background_color: "red", padding: "10px",
                div { "hello world" }
            }
        }
    };

    stylo_dioxus::render(css, nodes);
}
