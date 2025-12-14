//! Drive the renderer from Dioxus

use dioxus::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    let style = 
            r#"
                #main, html, body {
                    max_height: 300px;
                    height: 300px;
                }
            "#;

    rsx! {
        style {
            "{style}"
        }

        div {
            padding: "10px",
            max_height: "300px",
            background: "yellow",
            overflow: "scroll",
            onscroll: |e| println!("onscrollouter"),

            input {
                
            }
        }
    }
}
