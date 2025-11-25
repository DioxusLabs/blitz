use dioxus::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        div {
            style { {CSS} }
            div {
                id: "box-shadow-1",
                class: "box-shadow",
            }
            div {
                id: "box-shadow-2",
                class: "box-shadow",
            }
            div {
                id: "box-shadow-3",
                class: "box-shadow",
            }
        }
    }
}

const CSS: &str = r#"
.box-shadow {
    width: 200px;
    height: 200px;
    background-color: red;
    margin: 60px;
}

#box-shadow-1 {
    width: 100px;
    height: 100px;
    box-shadow: 140px 0 blue;
}

#box-shadow-2 {
    box-shadow: 10px 10px 5px 10px rgb(238 255 7), 10px 10px 5px 30px blue;
}

#box-shadow-3 {
    box-shadow: 0 0 10px 20px rgb(238 255 7);
}
"#;
