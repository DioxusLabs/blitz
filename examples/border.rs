// background: rgb(2,0,36);
// background: linear-gradient(90deg, rgba(2,0,36,1) 0%, rgba(9,9,121,1) 35%, rgba(0,212,255,1) 100%);

use dioxus::prelude::*;
use stylo_dioxus::Config;

fn main() {
    let cfg = Config {
        stylesheets: vec![CSS.to_string()],
    };
    stylo_dioxus::launch_cfg(app, cfg);
}

fn app(cx: Scope) -> Element {
    render! {
        div { id: "a", " Dioxus12312312312321\n\n\n        hi " }
    }
}

const CSS: &str = r#"
#a {
    background-color: red;
    font-color: white;
    border: 10px solid black;
    border-radius: 50px 20px;
    border-top-color: green;
    border-bottom-color: green;
}
"#;
