//! Drive the renderer from Dioxus
//!
//!
//!
//!

use blitz::Config;
use dioxus::prelude::*;

fn main() {
    let cfg = Config {
        stylesheets: vec![CSS.to_string()],
    };
    blitz::launch_cfg(app, cfg);
}

fn app(cx: Scope) -> Element {
    render! {
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
