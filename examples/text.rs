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
            ul {
                li { "Item 1" }
                li { "Item 2" }
                li {
                    ul {
                        li { "Nested Item 1" }
                        li { "Nested Item 2" }
                    }
                }
                li { "Item 3" }
                li { "Item 4" }
                ul {
                    li { "Sub 1" }
                    li { "Sub 2" }
                }
                li {
                    class: "square",
                    "Square item"
                }
                li {
                    class: "circle",
                    "Circle item"
                }
                li {
                    class: "disclosure-open",
                    "Disclosure open item"
                }
                li {
                    class: "disclosure-closed",
                    "Disclosure closed item"
                }
            }
        }
    }
}

const CSS: &str = r#"
#a {
}
ol {
    list-style-type: upper-alpha;
}
li.square {
    list-style-type: square;
}
li.circle {
    list-style-type: circle;
}
li.disclosure-open {
    list-style-type: disclosure-open;
}
li.disclosure-closed {
    list-style-type: disclosure-closed;
}
"#;
