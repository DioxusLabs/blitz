use dioxus::prelude::*;

fn main() {
    tracing_subscriber::fmt::init();

    dioxus_blitz::launch(app);
}

fn app() -> Element {
    rsx! {
        body {
            style { {CSS} }
            div { id: "a",
                "Some text"
                em { "Another block of text" }
                "Should connect no space between"
            }
            ol {
                li { "Item 1" }
                li { "Item 2" }
                li {
                    ul {
                        li { "Nested Item 1" }
                        li { "Nested Item 2" }
                    }
                }
            }
            ul {
                class: "square",
                li { "Square item" }
            }
            ul {
                class: "circle",
                li { "Circle item" }
            }
        }
    }
}

const CSS: &str = r#"
#a {
}
ul.square {
    list-style-type: square;
}
ul.circle {
    list-style-type: circle;
}
"#;
