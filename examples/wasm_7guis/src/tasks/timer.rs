use dioxus_native::prelude::*;

#[component]
pub fn Timer() -> Element {
    let mut elapsed = use_signal(|| 0.0f64);
    let mut duration = use_signal(|| 15.0f64);

    use_coroutine(move |_: UnboundedReceiver<()>| async move {
        loop {
            futures_timer::Delay::new(std::time::Duration::from_millis(100)).await;
            let dur = *duration.peek();
            let current = *elapsed.peek();
            if current < dur {
                elapsed.set((current + 0.1).min(dur));
            }
        }
    });

    let pct = (elapsed() / duration().max(0.001) * 100.0).min(100.0);

    rsx! {
        div { class: "timer-root",
            style { {CSS} }
            div { class: "timer-card",
                // Progress bar
                div { class: "progress-track",
                    div { class: "progress-fill", style: "width: {pct:.1}%;" }
                }
                p { class: "timer-elapsed", "Elapsed: {elapsed():.1}s" }
                div { class: "timer-slider-row",
                    label { class: "timer-label", "Duration: " }
                    input {
                        r#type: "range",
                        min: "0",
                        max: "30",
                        step: "0.5",
                        value: "{duration()}",
                        class: "timer-slider",
                        oninput: move |evt| {
                            if let Ok(v) = evt.value().parse::<f64>() {
                                duration.set(v);
                            }
                        }
                    }
                    span { class: "timer-duration-label", "{duration():.1}s" }
                }
                button {
                    class: "timer-reset-btn",
                    onclick: move |_| elapsed.set(0.0),
                    "Reset"
                }
            }
        }
    }
}

const CSS: &str = r#"
.timer-root {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    background-color: #f5f5f5;
    font-family: sans-serif;
}

.timer-card {
    display: flex;
    flex-direction: column;
    gap: 16px;
    background: #ffffff;
    border: 1px solid #d0d0d0;
    border-radius: 8px;
    padding: 28px 36px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
    min-width: 320px;
}

.progress-track {
    width: 100%;
    height: 20px;
    background-color: #e0e0e0;
    border-radius: 10px;
    overflow: hidden;
}

.progress-fill {
    height: 100%;
    background-color: #4a6cf7;
    border-radius: 10px;
}

.timer-elapsed {
    font-size: 18px;
    font-weight: 600;
    color: #1a1a1a;
    margin: 0;
    text-align: center;
}

.timer-slider-row {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 12px;
}

.timer-label {
    font-size: 15px;
    color: #444444;
    white-space: nowrap;
}

.timer-slider {
    flex: 1;
    accent-color: #4a6cf7;
}

.timer-duration-label {
    font-size: 15px;
    font-weight: 600;
    color: #1a1a1a;
    min-width: 40px;
    text-align: right;
}

.timer-reset-btn {
    font-size: 16px;
    font-weight: 500;
    color: #ffffff;
    background-color: #4a6cf7;
    border: none;
    border-radius: 6px;
    padding: 10px 24px;
    cursor: pointer;
    align-self: flex-start;
}

.timer-reset-btn:hover {
    background-color: #3a5ce5;
}

.timer-reset-btn:active {
    background-color: #2a4cd3;
}
"#;
