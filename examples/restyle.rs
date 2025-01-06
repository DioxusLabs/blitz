use dioxus::prelude::*;
use tokio::time::{sleep, Duration};

fn main() {
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    dioxus_native::launch(app);
}

fn app() -> Element {
    let mut count = use_signal(|| 0);
    let mut running = use_signal(|| true);
    // `use_future` will spawn an infinitely running future that can be started and stopped
    use_future(move || async move {
        loop {
            if running() {
                count += 1;
            }
            sleep(Duration::from_millis(40)).await;
        }
    });
    rsx! {
        div {
            h1 { "Current count: {count}" }
            p {
                style: "font-size: {count}px",
                "Animate Font Size"
            }
            button { onclick: move |_| running.toggle(), "Start/Stop the count"}
            button { onclick: move |_| count.set(0), "Reset the count" }
        }
    }
}
