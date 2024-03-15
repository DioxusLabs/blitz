use dioxus::prelude::*;

#[tokio::main]
async fn main() {
    blitz::launch(app).await;
}

fn app(cx: Scope) -> Element {
    let toggle = use_state(cx, || false);

    let font_size = if *toggle.get() { "32px" } else { "1em" };
    cx.render(rsx! {
        div {
            class: "asd",
            height: "30px",
            background_color: "#ffff00",

            onmouseup: |_| {
                toggle.modify(|f| !f);
            },
            "Click me"
        }
        div {
            display: "flex",
            flex_direction: "column",
            width: "100%",
            height: "100%",
            font_size: "{font_size}",

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
