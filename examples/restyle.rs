use dioxus::prelude::*;
use tokio::time::{Duration, sleep};

fn main() {
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    dioxus_native::launch(app);
}

#[derive(Copy, Clone)]
enum AnimationState {
    Increasing,
    Decreasing,
}

impl std::ops::Not for AnimationState {
    type Output = Self;
    fn not(self) -> Self::Output {
        match self {
            AnimationState::Increasing => AnimationState::Decreasing,
            AnimationState::Decreasing => AnimationState::Increasing,
        }
    }
}

const MIN_SIZE: i32 = 12;
const MAX_SIZE: i32 = 120;

fn app() -> Element {
    let mut size = use_signal(|| 12);
    let mut direction = use_signal(|| AnimationState::Increasing);
    let mut running = use_signal(|| true);

    // `use_future` will spawn an infinitely direction future that can be started and stopped
    use_future(move || async move {
        loop {
            if running() {
                match direction() {
                    AnimationState::Increasing => size += 1,
                    AnimationState::Decreasing => size -= 1,
                }

                let size = *size.read();
                if size <= MIN_SIZE {
                    *direction.write() = AnimationState::Increasing;
                }
                if size >= MAX_SIZE {
                    *direction.write() = AnimationState::Decreasing;
                }
            }

            sleep(Duration::from_millis(16)).await;
        }
    });
    rsx! {
        div {
            style { {STYLES} }
            h1 { "Current size: {size}" }
            div { style: "display: flex",
                div { class: "button", onclick: move |_| running.toggle(), "Start/Stop" }
                div { class: "button", onclick: move |_| size.set(12), "Reset the size" }
            }
            p { style: "font-size: {size}px", "Animate Font Size" }
        }
    }
}

static STYLES: &str = r#"
    .button {
        padding: 6px;
        border: 1px solid #999;
        margin-left: 12px;
        cursor: pointer;
    }

    .button:hover {
        background: #999;
        color: white;
    }
"#;
