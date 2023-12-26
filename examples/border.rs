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
        div { id: "b", "    Dioxus12312312312321\n\n\n\n\n\n\n\n        hi " }
        div { id: "c", "    Dioxus12312312312321\n\n\n\n\n\n\n\n        hi " }
        div { id: "d", "    Dioxus12312312312321\n\n\n\n\n\n\n\n        hi " }
        div { id: "e", "    Dioxus12312312312321\n\n\n\n\n\n\n\n        hi " }
        div { id: "a", "    Dioxus12312312312321\n\n\n        hi " }
    }
}

const CSS: &str = r#"
#a {
    height:300px;
    background-color: gray;
    border: 1px solid black;
    border-radius: 50px 20px;
    border-top-color: red;
    padding:20px;
    margin:20px;
    border-radius: 10% 30% 50% 70%;
    border-left: 4px solid #000;
    border-top: 10px solid #ff0;
    border-right:  3px solid #F01;
    border-bottom:  9px solid #0f0;
    box-shadow: 10px 10px gray;
}

#b {
    border: 20px solid black;
    background-color: red;
    border-radius: 10px;
    border-top-width: 32px;
    border-left-width: 4px;
    border-right-width: 4px;
}
#c {
    border: 20px solid black;
    background-color: red;
    border-top-left-radius: 0px;
    border-top-right-radius: 40px;
    border-top-width: 32px;
    border-left-width: 8px;
    border-right-width: 16px;
}
#d {
    border: 20px solid black;
    background-color: red;
    border-top-width: 32px;
    border-left-width: 8px;
    border-right-width: 16px;
    border-bottom-width: 20px;
}
#e {
    background-color: pink;
    border: 20px solid black;
    border-radius: 30px;
}
"#;

// border-radius: 1px;

// outline-style: solid;
// outline-color: blue;
