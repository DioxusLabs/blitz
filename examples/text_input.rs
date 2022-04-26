use blitz::*;
use dioxus::prelude::*;

fn main() {
    launch(app);
}

fn app(cx: Scope) -> Element {
    cx.render(rsx! {
        div{
            width: "100%",
            height: "100%",
            Input{
                initial_text: "hello world".to_string(),
            }
        }
    })
}
