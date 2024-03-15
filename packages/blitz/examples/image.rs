use dioxus::prelude::*;

#[tokio::main]
async fn main() {
    blitz::launch(app).await;
}

fn app(cx: Scope) -> Element {
    cx.render(rsx! {
        div {
            img {
                src: "assets/logo.png",
            }
        }
    })
}
