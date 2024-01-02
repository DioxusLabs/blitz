// background: rgb(2,0,36);
// background: linear-gradient(90deg, rgba(2,0,36,1) 0%, rgba(9,9,121,1) 35%, rgba(0,212,255,1) 100%);

use blitz::Config;
use dioxus::prelude::*;

fn main() {
    let cfg = Config {
        stylesheets: vec![],
    };
    blitz::launch_cfg(app, cfg);
}

fn app(cx: Scope) -> Element {
    render! {
        div {
            style { CSS }
            div { id: "container",
                div { "Hello " }
                div { "world! " }
            }
            div { id: "container",
                div { "Dioxus " }
                div { "plus " }
                div { "stylo " }
            }
        }
    }
}

const CSS: &str = r#"
#container {
    flex: 1 1 auto;
    display: flex;
    flex-direction: row;
    justify-content: center;
    align-items: center;
    align-content: center;
    background-color: gray;
    border: 1px solid black;
    border-top-color: red;
    border-left: 4px solid #000;
    border-top: 10px solid #ff0;
    border-right:  3px solid #F01;
    border-bottom:  9px solid #0f0;
    box-shadow: 10px 10px gray;

    // outline-width: 50px;
    outline-style: solid;
    outline-color: blue;
    border-radius: 50px 20px;

    // border-radius: 10% 30% 50% 70%;
    padding:20px;
    margin:20px;
    // border-radius: 10px;
}
"#;
