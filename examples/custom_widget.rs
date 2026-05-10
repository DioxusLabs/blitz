use anyrender::PaintScene as _;
use blitz_dom::node::ComputedStyles;
use color::parse_color;
use dioxus_native::CustomWidgetAttr;
use dioxus_native::Widget;
use dioxus_native::prelude::*;
use peniko::Color;
use peniko::Fill;
use peniko::kurbo::Affine;
use peniko::kurbo::Rect;
use peniko::kurbo::Vec2;
use std::f64::consts::TAU;
use std::time::Instant;

pub fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    let mut show_cube = use_signal(|| true);

    let color_str = use_signal(|| String::from("red"));

    // use_effect(move || println!("{:?}", color().components));

    rsx!(
        style { {STYLES} }
        div { id: "overlay",
            h2 { "Control Panel" }
            button { onclick: move |_| *show_cube.write() = !show_cube(),
                if show_cube() {
                    "Hide cube"
                } else {
                    "Show cube"
                }
            }
            br {}
            ColorControl { label: "Color:", color_str }
            p {
                "This overlay demonstrates that the custom WGPU content can be rendered beneath layers of HTML content"
            }
        }
        div { id: "underlay",
            h2 { "Underlay" }
            p {
                "This underlay demonstrates that the custom WGPU content can be rendered above layers and blended with the content underneath"
            }
        }
        header {
            h2 { "Blitz Custom Widget Demo" }
        }
        if show_cube() {
            SpinningCube { color: color_str }
        }
    )
}

#[component]
fn ColorControl(label: &'static str, color_str: Signal<String>) -> Element {
    rsx!(
        div { class: "color-control",
            {label}
            input {
                value: color_str(),
                oninput: move |evt| { *color_str.write() = evt.value() },
            }
        }
    )
}

#[component]
fn SpinningCube(color: Signal<String>) -> Element {
    let custom_widget = use_memo(|| CustomWidgetAttr::new(DemoWidget::new()));
    rsx!(
        div { id: "canvas-container",
            object { "data": custom_widget, "color": color() }
        }
    )
}

pub struct DemoWidget {
    start_time: std::time::Instant,
    color: Color,
}

impl DemoWidget {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            color: color::palette::css::BLACK,
        }
    }
}

impl Widget for DemoWidget {
    fn connected(&mut self) {}
    fn disconnected(&mut self) {}
    fn can_create_surfaces(&mut self, _render_ctx: &mut dyn anyrender::RenderContext) {}
    fn destroy_surfaces(&mut self) {}

    fn attribute_changed(&mut self, name: &str, _old_value: Option<&str>, new_value: Option<&str>) {
        if name == "color" {
            self.color = new_value
                .and_then(|color_str| parse_color(color_str).ok())
                .map(|c| c.to_alpha_color())
                .unwrap_or(color::palette::css::BLACK)
        }
    }

    fn handle_event(&mut self, event: &blitz_traits::events::UiEvent) {
        let _ = event;
    }

    fn paint(
        &mut self,
        render_ctx: &mut dyn anyrender::RenderContext,
        _styles: &ComputedStyles,
        width: u32,
        height: u32,
        scale: f64,
    ) -> anyrender::Scene {
        let _ = (render_ctx, width, height, scale);
        let mut scene = anyrender::Scene::new();

        let w = (width.min(height) / 2) as f64;
        let h = w;
        let x = (width as f64 - w) / 2.0;
        let y = (height as f64 - h) / 2.0;

        let ms = Instant::now().duration_since(self.start_time).as_millis();
        let angle = (ms as f64 / 400.0) % TAU;
        let rotation =
            Affine::rotate_about(angle, (w / 2.0, h / 2.0)).then_translate(Vec2 { x, y });

        scene.fill(
            Fill::NonZero,
            rotation,
            self.color.clone(),
            None,
            &Rect::from_origin_size((0.0, 0.0), (w, h)),
        );

        scene
    }
}

const STYLES: &str = "
* {
    box-sizing: border-box;
}

html, body, main {
    height: 100%;
    font-family: system-ui, sans;
    margin: 0;
}

main {
    display: grid;
    grid-template-rows: 100px 1fr;
    grid-template-columns: 100%;
    background: #f4e8d2;
}

#canvas-container {
    display: grid;
    opacity: 0.8;
}

header {
    padding: 10px 40px;
    background-color: white;
    z-index: 100;
}

#overlay {
    position: absolute;
    width: 33%;
    height: 100%;
    right: 0;
    z-index: 10;
    background-color: rgba(0, 0, 0, 0.5);
    padding-top: 40%;
    padding-inline: 20px;
    color: white;
}

#underlay {
    position: absolute;
    width: 33%;
    height: 100%;
    z-index: -10;
    background-color: black;
    padding-top: 40%;
    padding-inline: 20px;
    color: white;
}

.color-control {
    display: flex;
    gap: 12px;

    > input {
        width: 150px;
        color: black;
    }
}
";
