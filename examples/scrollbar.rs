use dioxus::prelude::*;
use tracing::info;

fn main() {
    tracing_subscriber::fmt::init();
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        div {
            style: "
                width: 100vw;
                height: 100vh;
                overflow: scroll;
            ",

            onscroll: move |_e| {
                info!("onscroll {:?}", _e.data());
            },
            onscrollend: move |_e| {
                info!("onscrollnd {:?}", _e.data());
            },
            onmounted: move |_e| {
                info!("onmounted {:#?}", _e.data());
            },
            onresize: move |_e| {
                info!("onresize {:#?}", _e.data());
            },

            div {
                for i in 0..100 {
                    h1 {
                        "Test {i}"
                    }
                }
            }
        }
    }
}
