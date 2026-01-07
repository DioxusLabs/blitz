//! Drive the renderer from Dioxus
use dioxus_native::prelude::*;

const SVG: Asset = asset!("./assets/hello_world.svg");

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        div {
            h1 { "Test SVG" }
            img { src: SVG }
        }
    }
}
