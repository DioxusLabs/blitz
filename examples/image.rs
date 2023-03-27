use dioxus::prelude::*;

#[tokio::main]
async fn main() {
    blitz::launch(app).await;
}

fn app(cx: Scope) -> Element {
    cx.render(rsx! {
        for _ in 0..10 {
            img {
                width: "100px",
                height: "100px",
                right: "10px",
                top: "10px",
                src: "assets/logo.png",
            }
        }
    })
}
