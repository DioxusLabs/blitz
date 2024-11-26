use dioxus::prelude::*;

fn main() {
    blitz_shell::launch(app);
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
