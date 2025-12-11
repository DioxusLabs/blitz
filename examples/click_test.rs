//! Drive the renderer from Dioxus

use dioxus::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        div {
            padding: "10px",
            background: "yellow",
            onmouseenter: |_| println!("onmouseenter outer"),
            onmouseleave: |_| println!("onmouseleave outer"),
            onmouseover: |_| println!("onmouseover outer"),
            onmouseout: |_| println!("onmouseout outer"),

            div {
                width: "30px",
                height: "30px",
                background: "red",
                onmouseenter: |_| println!("onmouseenter"),
                onmouseleave: |_| println!("onmouseleave"),
                onmouseover: |_| println!("onmouseover"),
                onmouseout: |_| println!("onmouseout"),
            }
        }
    }
}
