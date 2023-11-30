use dioxus::prelude::*;

#[tokio::main]
async fn main() {
    stylo_dioxus::render(app).await;
}

fn app(cx: Scope) -> Element {
    render! {
        div {
            h1 { "Hello World!" }
            p { "This is a paragraph." }
        }
    }
}
