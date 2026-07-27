use dioxus_native::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        "Outer"
        div {
            "Inner"
        }
    }
}
