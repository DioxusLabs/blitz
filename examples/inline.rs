//! https://www.w3schools.com/css/tryit.asp?filename=trycss_inline-block_span1

use dioxus::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        head {
            style { {CSS} }
        }
        body {
            h1 { "The display Property" }
            h2 { "display: inline" }
            div {
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum consequat scelerisque elit sit amet consequat. Aliquam erat volutpat. "
                span { class: "a", "Aliquam" }
                span { class: "a", "venenatis" }
                " gravida nisl sit amet facilisis. Nullam cursus fermentum velit sed laoreet. "
            }
            h2 { "display: inline-block" }
            div {
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum consequat scelerisque elit sit amet consequat. Aliquam erat volutpat. "
                span { class: "b", "Aliquam" }
                span { class: "b", "venenatis" }
                " gravida nisl sit amet facilisis. Nullam cursus fermentum velit sed laoreet. "
            }
            h2 { "display: block" }
            div {
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum consequat scelerisque elit sit amet consequat. Aliquam erat volutpat. "
                span { class: "c", "Aliquam" }
                span { class: "c", "venenatis" }
                " gravida nisl sit amet facilisis. Nullam cursus fermentum velit sed laoreet. "
            }
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
span.a {
  display: inline; /* the default for span */
  width: 100px;
  height: 100px;
  padding: 5px;
  border: 1px solid blue;
  background-color: yellow;
}

span.b {
  display: inline-block;
  width: 100px;
  height: 100px;
  padding: 5px;
  border: 1px solid blue;
  background-color: yellow;
}

span.c {
  display: block;
  width: 100px;
  height: 100px;
  padding: 5px;
  border: 1px solid blue;
  background-color: yellow;
}

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
