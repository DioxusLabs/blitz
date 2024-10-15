use dioxus::prelude::*;

fn main() {
    dioxus_native::launch(app);
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
