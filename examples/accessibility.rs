use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app() -> Element {
    rsx! {
        body {
            App {}
        }
    }
}

#[component]
fn App() -> Element {
    rsx! {
        div { "Dioxus for all" }
    }
}
