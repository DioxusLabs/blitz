// background: rgb(2,0,36);
// background: linear-gradient(90deg, rgba(2,0,36,1) 0%, rgba(9,9,121,1) 35%, rgba(0,212,255,1) 100%);

use dioxus::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        style { {CSS} }
        div { "padd           " }
        div { "padd           " }
        div { "padd           " }
        div { "padd           " }
        div {
            class: "colorful",
            id: "a",
            div { "Dioxus12312312312321" }
            div { "Dioxus12312312312321" }
            div { "Dioxus12312312312321" }
            div { "Dioxus12312312312321" }
            div { "Dioxus12312312312321" }
            div { "Dioxus12312312312321" }
        }
    }
}

const CSS: &str = r#"
.colorful {
    border-right-color: #000;
    border-left-color: #ff0;
    border-top-color: #F01;
    border-bottom-color: #0f0;
}
#a {
    height:300px;
    background-color: gray;
    border: 1px solid black;
    // border-radius: 50px 20px;
    border-top-color: red;
    // padding:20px;
    // margin:20px;
    // border-radius: 10px;
    border-radius: 10% 30% 50% 70%;
    border-left: 4px solid #000;
    border-top: 10px solid #ff0;
    border-right:  3px solid #F01;
    border-bottom:  9px solid #0f0;
    // box-shadow: 10px 10px gray;

    margin: 100px;
    outline-width: 50px;
    outline-style: solid;
    outline-color: blue;
}
"#;
