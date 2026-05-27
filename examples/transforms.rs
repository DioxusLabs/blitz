use dioxus_native::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Viewport {
    pub pan: [f32; 2],
    pub zoom: f32,
}

impl std::fmt::Display for Viewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "translate({}px, {}px) scale({})",
            self.pan[0], self.pan[1], self.zoom
        )
    }
}

impl Viewport {
    pub fn onscroll(&mut self, sx: f32, sy: f32, factor: f32) {
        let new_zoom = (self.zoom * factor).clamp(0.05, 50.0);

        let wx = (sx - self.pan[0]) / self.zoom;
        let wy = (sy - self.pan[1]) / self.zoom;

        self.pan[0] = sx - wx * new_zoom;
        self.pan[1] = sy - wy * new_zoom;
        self.zoom = new_zoom;
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: [50.0, 50.0],
            zoom: 2.0,
        }
    }
}

fn app() -> Element {
    let mut border = use_signal(|| false);
    let mut viewport = use_store(Viewport::default);

    let onwheel = move |e: Event<WheelData>| {
        let position = e.client_coordinates();
        let sx = position.x as f32;
        let sy = position.y as f32;

        let sensitivity = 0.01;

        let delta = match e.delta() {
            dioxus_elements::geometry::WheelDelta::Pixels(data) => data.y as f32 * sensitivity,
            dioxus_elements::geometry::WheelDelta::Lines(data) => {
                data.y as f32 * sensitivity * 16.0
            }
            dioxus_elements::geometry::WheelDelta::Pages(data) => {
                data.y as f32 * sensitivity * 400.0
            }
        };

        let factor = (1.0 + delta).clamp(0.8, 1.2);

        viewport.write().onscroll(sx, sy, factor);
    };

    rsx! {
        style { {CSS} }
        div {
            class: "container",

            div { class: "black",
                div {
                    class: "blue",
                    style: "transform: translateY(-20px);"
                }
            }

            div { class: "black",
                div {
                    class: "blue",
                    style: "transform: scale(0.8); transform-origin: center;"
                }
            }

            div { class: "black",
                div {
                    class: "blue",
                    style: "transform: rotate(20deg); transform-origin: center;"
                }
            }
        }

        div {
            position: "absolute",
            top: 0,
            left: 0,
            width: "100vw",
            height: "100vh",

            onwheel,

            div {
                position: "absolute",
                right: 0,
                "{viewport}"
            }
            div {
                width: 0,
                height: 0,
                border: "1px solid black",
                position: "absolute",
                top: 0,
                left: 0,
                transform_origin: "0 0",
                transform: "{viewport}",

                div {
                    font_size: "20px",
                    top: "50px",
                    left: "50px",
                    position: "absolute",
                    "ITEM 50x50"
                }
                button {
                    position: "absolute",
                    top: "150px",
                    left: "150px",
                    border: if border() { "1px solid black"},
                    onclick: move |_| border.toggle(),
                    "{border} 150x150"
                }
                div {
                    position: "absolute",
                    top: "0px",
                    left: "0px",
                    div {
                        "testing"
                        div {
                            "testing"
                        }
                    }
                }


            }
        }



    }
}

const CSS: &str = r#"
* {
    box-sizing: border-box;
}
html,
body,
main {
    width: 100vw;
    height: 100vh;
    margin: 0;
}
.container {
    margin: 20px;
}
.container div { width: 50px; height: 50px }
body { background: white }
.black { position: relative; z-index: 0; display: flex; border: 2px solid black; margin: 20px 0; }
.blue { background: rgba(0, 0, 255, 0.5); }
.blue:hover { background: rgba(255, 0, 0, 0.5); }

"#;
