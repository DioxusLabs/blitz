use dioxus_native::prelude::*;

fn is_leap_year(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(m: u32, y: i32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn parse_date(s: &str) -> Option<(u32, u32, i32)> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let d = parts[0].parse::<u32>().ok()?;
    let m = parts[1].parse::<u32>().ok()?;
    let y = parts[2].parse::<i32>().ok()?;
    if !(1..=12).contains(&m) {
        return None;
    }
    if !(1..=days_in_month(m, y)).contains(&d) {
        return None;
    }
    Some((d, m, y))
}

fn date_le(a: (u32, u32, i32), b: (u32, u32, i32)) -> bool {
    (a.2, a.1, a.0) <= (b.2, b.1, b.0)
}

#[component]
pub fn FlightBooker() -> Element {
    let mut flight_type = use_signal(|| String::from("one-way"));
    let mut start_str = use_signal(|| String::from("01.01.2026"));
    let mut return_str = use_signal(|| String::from("01.01.2026"));
    let mut booked_msg: Signal<Option<String>> = use_signal(|| None);

    let is_return = flight_type() == "return";
    let start_valid = parse_date(&start_str()).is_some();
    let return_valid = !is_return || parse_date(&return_str()).is_some();
    let dates_ok = start_valid
        && return_valid
        && (!is_return || {
            let s = parse_date(&start_str()).unwrap();
            let r = parse_date(&return_str()).unwrap();
            date_le(s, r)
        });

    rsx! {
        div { class: "flight-root",
            style { {CSS} }
            div { class: "flight-card",
                h2 { class: "flight-title", "Flight Booker" }
                div { class: "flight-type-row",
                    button {
                        class: if !is_return { "type-btn type-btn-active" } else { "type-btn" },
                        onclick: move |_| flight_type.set("one-way".into()),
                        "one-way flight"
                    }
                    button {
                        class: if is_return { "type-btn type-btn-active" } else { "type-btn" },
                        onclick: move |_| flight_type.set("return".into()),
                        "return flight"
                    }
                }
                input {
                    class: if start_valid { "date-input" } else { "date-input invalid" },
                    value: "{start_str}",
                    oninput: move |evt| start_str.set(evt.value()),
                }
                input {
                    class: if return_valid { "date-input" } else { "date-input invalid" },
                    disabled: !is_return,
                    value: "{return_str}",
                    oninput: move |evt| return_str.set(evt.value()),
                }
                button {
                    class: "flight-btn",
                    disabled: !dates_ok,
                    onclick: move |_| {
                        let msg = if is_return {
                            format!("Booked return flight: {} \u{2192} {}", start_str(), return_str())
                        } else {
                            format!("Booked one-way flight on {}", start_str())
                        };
                        booked_msg.set(Some(msg));
                    },
                    "Book"
                }
                if let Some(msg) = booked_msg() {
                    p { class: "booked-msg", "{msg}" }
                }
            }
        }
    }
}

const CSS: &str = r#"
.flight-root {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    background-color: #f5f5f5;
    font-family: sans-serif;
}

.flight-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: #ffffff;
    border: 1px solid #d0d0d0;
    border-radius: 8px;
    padding: 28px 32px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
    min-width: 320px;
}

.flight-title {
    margin: 0 0 4px 0;
    font-size: 20px;
    font-weight: 600;
    color: #1a1a1a;
    white-space: nowrap;
}

.flight-type-row {
    display: flex;
    flex-direction: row;
    gap: 0;
    border: 1px solid #c0c0c0;
    border-radius: 6px;
    overflow: hidden;
}

.type-btn {
    flex: 1;
    font-size: 14px;
    padding: 8px 10px;
    border: none;
    background: #fafafa;
    color: #444;
    cursor: pointer;
}

.type-btn-active {
    background: #4a6cf7;
    color: #ffffff;
    font-weight: 600;
}

.date-input {
    font-size: 15px;
    padding: 8px 10px;
    border: 1px solid #c0c0c0;
    border-radius: 6px;
    color: #1a1a1a;
    background: #ffffff;
}

.date-input:focus {
    outline: none;
    border-color: #4a6cf7;
}

.date-input:disabled {
    background: #ececec;
    color: #999999;
    cursor: not-allowed;
}

.date-input.invalid {
    border-color: #e53e3e;
    background: #fff5f5;
    color: #c53030;
}

.flight-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 15px;
    font-weight: 500;
    color: #ffffff;
    background-color: #4a6cf7;
    border: none;
    border-radius: 6px;
    padding: 10px;
    cursor: pointer;
    margin-top: 4px;
    width: 100%;
}

.flight-btn:hover {
    background-color: #3a5ce5;
}

.flight-btn:active {
    background-color: #2a4cd3;
}

.flight-btn:disabled {
    background-color: #a0aec0;
    cursor: not-allowed;
}

.booked-msg {
    margin: 4px 0 0 0;
    padding: 10px 12px;
    background: #ebf8ee;
    border: 1px solid #68d391;
    border-radius: 6px;
    color: #276749;
    font-size: 14px;
}
"#;
