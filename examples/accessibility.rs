use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app() -> Element {
    rsx! {
        body {
            "Dioxus 4 all"
        }
    }
}
