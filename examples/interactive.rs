//! Drive the renderer from Dioxus
//!
//!
//!
//!

use dioxus::prelude::*;

fn main() {
    stylo_dioxus::launch(app);
}

fn app(cx: Scope) -> Element {
    render! {
        style { CSS }
        div {
            h1 { class: "heading", "h1" }
            h2 { class: "heading", "h2" }
            h3 { class: "heading", "h3" }
            h4 { class: "heading", "h4" }
        }
    }
}

static CSS: &str = r#"
    h1 {
        background-color: red;
    }

    h2 {
        background-color: green;
    }

    h3 {
        background-color: blue;
    }

    h4 {
        background-color: yellow;
    }

    .heading {
        padding: 5px;
        border-radius: 5px;
        border: 2px solid #73AD21;
    }

    div {
        margin: 35px;
    }
"#;
