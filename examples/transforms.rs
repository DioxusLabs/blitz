use dioxus_native::prelude::*;

const SVG: Asset = asset!("./assets/hello_world.svg");
const IMG: Asset = asset!("./assets/servo-color-negative-no-container.png");
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

    pub fn pan(&mut self, screen: &[f32; 2], last: &[f32; 2]) {
        let dx = (screen[0] - last[0]) / self.zoom;
        let dy = (screen[1] - last[1]) / self.zoom;
        self.pan[0] += dx;
        self.pan[1] += dy;
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
    let mut viewport = use_store(Viewport::default);
    let mut active_pointer = use_signal(|| false);
    let mut last = use_signal(|| [0.0, 0.0]);

    let onpointerdown = move |evt: Event<PointerData>| {
        let coords = evt.client_coordinates();
        if evt
            .held_buttons()
            .contains(dioxus_elements::input_data::MouseButton::Auxiliary)
        {
            active_pointer.set(true);
            last.set([coords.x as f32, coords.y as f32]);
            return;
        }
    };
    let onpointerup = move |_: Event<PointerData>| {
        active_pointer.set(false);
    };

    let onpointermove = move |evt: Event<PointerData>| {
        if active_pointer() {
            let coords = evt.client_coordinates();
            let screen = [coords.x as f32, coords.y as f32];
            viewport.write().pan(&screen, &last());
            last.set(screen);
        }
    };

    let onwheel = move |e: Event<WheelData>| {
        let position = e.client_coordinates();
        let sx = position.x as f32;
        let sy = position.y as f32;

        let sensitivity = 0.001;

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
            position: "absolute",
            top: 0,
            left: 0,
            width: "100vw",
            height: "100vh",
            onpointerdown,
            onpointerup,
            onpointermove,
            onwheel,
            div {
                class: "overlay",
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
                    position: "absolute", left: "400px",
                    width: "800px",
                    height: "800px",
                    img {  position: "absolute",src: SVG }
                    img {  background: "black", top: "200px",  position: "absolute", src: IMG }

                    img {  position: "absolute", transform: "scale(0.5) rotate(45deg)", src: SVG }
                    img { background: "black", top: "200px",  position: "absolute", transform: "scale(0.5) rotate(45deg)", src: IMG }

                    img {  position: "absolute", transform: "scale(0.5) translateY(150px) rotate(45deg)", src: SVG }
                    img { background: "black", top: "200px",  position: "absolute", transform: "scale(0.5) translateY(150px) rotate(45deg)", src: IMG }
                }
                div {
                    position: "absolute",
                     transform: "translate(-500px, -500px) rotate(-45deg) scale(0.7)",
                ToggleBtn {
                    top: "150px",
                    left: "150px",
                    width: "100px",


                }}
                div {
                    font_size: "20px",
                    top: "50px",
                    left: "50px",
                    position: "absolute",
                    "top 50 left 50"
                }
                ToggleBtn {
                    top: "150px",
                    left: "150px",
                    width: "100px",
                }
                input {
                    position: "absolute",
                    top: "200px",
                    width: "500px",
                    transform: "rotate(45deg)"
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
                div {
                    position: "absolute",
                    left: "100px",
                    top: "50px",
                    a {
                        class: "button",
                        display: "inline-flex",
                        href: "#week-schedule",
                        "2026 schedule"
                    }
                }
                div {
                    position: "absolute",
                    left: "300px",
                    top: "300px",
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
                    div { class: "black",
                        div {
                            class: "blue",
                            style: "transform: translateY(-20px);"
                        }
                    }
                }
                div {
                    width: 0,
                    height: 0,
                    position: "absolute",
                    transform_origin: "0 0",
                    transform: "rotate(20deg) translateY(550px)",
                    div {
                        font_size: "20px",
                        top: "50px",
                        left: "50px",
                        position: "absolute",
                        "top 50 left 50"
                    }

                    div {
                        z_index: 10,
                        font_size: "20px",
                        top: "50px",
                        left: "50px",
                        position: "absolute",
                        "top 50 left 50 z-index 10"
                    }

                    ToggleBtn {
                        top: "150px",
                        left: "150px",
                    }
                }
            }
        }
    }
}

#[component]
fn ToggleBtn(#[props(extends=GlobalAttributes)] attributes: Vec<Attribute>) -> Element {
    let mut border = use_signal(|| false);

    rsx! {
        button {
            position: "absolute",
            border: if border() { "1px solid black"},
            onclick: move |_| border.toggle(),
             ..attributes,
            "Toggle"
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
    overflow: none;
}

.overlay {
    position: absolute;
    top: 20px;
    right: 20px;
    z-index: 99;
}

body, .overlay {
    background: white;
}

.button {
    align-items: center;
    appearance: none;
    border-radius: 1.5rem 0;
    cursor: pointer;

    font-size: 1rem;
    font-weight: 700;
    line-height: 1;
    min-height: 3rem;
    padding-inline: 1.5rem;
    text-align: center;
    text-decoration: none;
    user-select: none;
    transition-property: filter, scale;
    transition-duration: 0.15s;
    background-color: #e74310;
    color: #fff;
}

.button:visited {
    color: #fff;
}

.button:hover {
    filter: brightness(90%);
    scale: 1.2;
}
.black {
    position: relative;
    width: 50px;
    height: 50px;
    display: flex;
    border: 2px solid black;
    margin: 50px;
}
.blue {
    width: 50px;
    height: 50px;
    background: rgba(0, 0, 255, 0.5);
}
.blue:hover {
    background: rgba(255, 0, 0, 0.5);
}

"#;
