//! First-party inline SVG (`svg-native`): the `<svg>` below is parsed straight into
//! Blitz's DOM, not painted as an opaque external image, so ordinary CSS including
//! `:hover` applies to elements inside it.
use dioxus_native::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    rsx! {
        style { r#"
            body {{ font-family: sans-serif; margin: 24px; }}
            .icon-btn rect {{ transition: fill 0.15s; }}
            .icon-btn:hover rect {{ fill: #ff5533; }}
        "# }
        h1 { "svg-native demo" }

        div { style: "display: flex; gap: 24px; align-items: center;",

            svg { width: "80", height: "80", "viewBox": "0 0 80 80",
                g { fill: "#3366ff",
                    rect { x: "10", y: "10", width: "60", height: "60", rx: "8" }
                }
            }

            svg { width: "80", height: "80", "viewBox": "0 0 80 80", style: "color: #22aa55;",
                circle { cx: "40", cy: "40", r: "30", fill: "currentColor" }
            }

            svg { class: "icon-btn", width: "80", height: "80", "viewBox": "0 0 80 80",
                rect { x: "10", y: "10", width: "60", height: "60", rx: "8", fill: "#8888aa" }
            }
        }
    }
}
