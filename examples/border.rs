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
        div { id: "a", "    Dioxus12312312312321\n\n\n        hi " }
        div { id: "b", "    Dioxus12312312312321\n\n\n\n\n\n\n\n        hi " }
    }
}

const CSS: &str = r#"
#a {
    background-color: gray;
    font-color: white;
    border: 5px solid black;
    border-top-color: green;
    border-bottom-color: green;
    border-radius: 10px;

    // border-radius: 1px;
    // border-radius: 10% 30% 50% 70%;
    // border-left: 4px solid #000;
    // border-top: 10px solid #ff0;
    // border-right:  3px solid red;
    // border-bottom:  9px solid #0f0;
    // box-shadow: 10px 10px gray;

}

#b {
    border: 20px solid black;
    background-color: red;
    border-top-left-radius: 20px;
    border-top-right-radius: 40px;
    // border-radius: 10% 30% 50% 70%;

    // border-radius: 5px;
    // border-top-width: 8px;
    // border-left-width: 8px;
    // border-radius: 10px;
    // border-radius: 10px;
    // border-radius: 50%;
}
"#;

// border-radius: 1px;

// outline-style: solid;
// outline-color: blue;
