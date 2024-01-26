/*
Servo doesn't have:
- space-evenly?
- gap
*/

use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app(cx: Scope) -> Element {
    render! {
        style { CSS }
        main {
            display: "flex",
            article { "Article" }
            nav { "Nav" }
            aside { "Aside" }
        }
    }
}

const CSS: &str = r#"
main {
  display: flex;
  text-align: center;
}
main > article {
  flex: 1;
  order: 2;
}
main > nav {
  width: 200px;
  order: 1;
}
main > aside {
  width: 200px;
  order: 3;
}
"#;
