//! Drive the renderer from Dioxus
//!
//!
//!
//!

use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app(cx: Scope) -> Element {
    render! {
        style { CSS }
        h1 { "h1" }
        h2 { "h2" }
        h3 { "h3" }
        h4 { "h4" }

        h1 { "h1" }
        h2 { "h2" }
        h3 { "h3" }
        div { class: "header", "h4" }
    }
}

const CSS: &str = r#"
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

.header {
    background-color: pink;
}
"#;
