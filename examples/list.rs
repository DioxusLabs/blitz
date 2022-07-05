use dioxus::prelude::*;

fn main() {
    blitz::launch(app);
}

fn app(cx: Scope) -> Element {
    cx.render(rsx! {
        div { class: "asd", "Jon" }
        div { display: "flex", flex_direction: "column", width: "100%", height: "100%",
            ul {
                (1..8).map(|y|
                    rsx! {
                        li {
                            key: "{y}",
                            "hello"
                        }
                    }
                )
            }
        }
    })
}
