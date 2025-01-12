use dioxus::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    let mut hover_count = use_signal(|| 0);

    rsx! {
        div {
            style: "padding: 20px; background: #eee; cursor: pointer;",
            onmouseenter: move |_| hover_count += 1,
            "Hover count: {hover_count}"
        }
    }
}
