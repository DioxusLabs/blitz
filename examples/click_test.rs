//! Drive the renderer from Dioxus

use dioxus::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        div {
            width: "30px",
            height: "30px",
            background: "red",
            onclick: |_| println!("onclick"),
            ondoubleclick: |_| println!("ondblclick"),
            oncontextmenu: |_| println!("oncontextmenu"),
        }
    }
}
