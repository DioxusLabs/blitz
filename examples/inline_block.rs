//! https://www.w3schools.com/css/tryit.asp?filename=trycss_inline-block_span1

use dioxus::prelude::*;

fn main() {
    blitz::launch(app);
}

fn app(cx: Scope) -> Element {
    render! {
        head {
            style { CSS }
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
"#;
