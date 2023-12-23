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
        div { id: "container",
            div { id: "a", " Dioxus " }
            div { id: "b", " plus " }
            div { id: "c", " Stylo " }
        }
    }
}

const CSS: &str = r#"
#a {
    background-color: red;
    font-size: 40px;
    font-color: white;
}
#b {
    background-color: green;
    font-size: 60px;
    font-color: white;
}
#c {
    background-color: blue;
    font-size: 80px;
    font-color: white;
}
"#;
