// background: rgb(2,0,36);
// background: linear-gradient(90deg, rgba(2,0,36,1) 0%, rgba(9,9,121,1) 35%, rgba(0,212,255,1) 100%);

use dioxus::prelude::*;

fn main() {
    mini_dxn::launch(app);
}

fn app() -> Element {
    rsx! {
        style { {CSS} }
        img { 
            class: "image",
            src: "https://images.dog.ceo/breeds/pitbull/dog-3981540_1280.jpg" 
        }
    }
}

const CSS: &str = r#"
.image {
    clip-path: circle(40%);
}
"#;
