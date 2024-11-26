use dioxus::prelude::*;

fn main() {
    tracing_subscriber::fmt::init();

    dioxus_native::launch(app);
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
            h1 { "ul" }
            ul {
                li { "Item 1" }
                li { "Item 2" }
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
            h1 { "ol - decimal" }
            ol {
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
                ol {
                    li { "Sub 1" }
                    li { "Sub 2" }
                }
            }
            h1 { "ol - alpha" }
            ol { class: "alpha",
                li { "Item 1" }
                li { "Item 2" }
                li { "Item 3" }
            }
        }
    }
}

const CSS: &str = r#"
#a {
}
h1 {
    font-size: 20px;
}
ol.alpha {
    list-style-type: lower-alpha;
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
