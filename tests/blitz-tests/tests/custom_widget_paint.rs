use anyrender::recording::{FillCommand, RenderCommand};
use anyrender::{AntiAliasing, RenderContext, Scene};
use blitz_dom::{Widget, util::Color};
use blitz_test_harness::{Harness, HarnessOptions};
use blitz_traits::events::UiEvent;
use std::sync::{Arc, Mutex};

const MARKER: Color = Color::from_rgb8(1, 2, 3);

#[derive(Default)]
struct State {
    sizes: Vec<(u32, u32)>,
    points: Vec<[(f32, f32); 3]>,
}

struct Probe(Arc<Mutex<State>>);

impl Widget for Probe {
    fn paint(
        &mut self,
        _: &mut dyn RenderContext,
        _: &style::properties::ComputedValues,
        width: u32,
        height: u32,
        _: f64,
    ) -> Scene {
        self.0.lock().unwrap().sizes.push((width, height));
        let mut scene = Scene::new();
        scene.commands.push(RenderCommand::Fill(FillCommand {
            fill: Default::default(),
            transform: Default::default(),
            brush: MARKER.into(),
            brush_transform: None,
            shape: Default::default(),
            anti_aliasing: AntiAliasing::Enabled,
        }));
        scene
    }

    fn handle_event(&mut self, event: &UiEvent) {
        if let UiEvent::PointerDown(event) = event {
            self.0.lock().unwrap().points.push([
                (event.page_x(), event.page_y()),
                (event.client_x(), event.client_y()),
                (event.element_x(), event.element_y()),
            ]);
        }
    }
}

fn widget(style: &str, scale: f32) -> (Harness, Arc<Mutex<State>>) {
    let mut harness = Harness::from_html_with(
        &format!(
            "<style>body{{margin:0}}</style><canvas id=widget style='display:block;box-sizing:content-box;width:100px;height:60px;padding:12px;border:3px solid black;{style}'></canvas>"
        ),
        HarnessOptions {
            scale,
            ..Default::default()
        },
    );
    let state = Arc::new(Mutex::new(State::default()));
    let id = harness.node("#widget");
    harness
        .base_mut()
        .mutate()
        .set_custom_widget(id, Box::new(Probe(state.clone())));
    harness.pump();
    (harness, state)
}

fn paint(harness: &mut Harness, scale: f64) -> Scene {
    let mut scene = Scene::new();
    blitz_paint::paint_scene(&mut scene, &mut harness.base_mut(), scale, 800, 600, 0, 0);
    scene
}

#[test]
fn widget_paints_only_its_content_box() {
    for scale in [1.0, 2.0] {
        let (mut harness, state) = widget("", scale);
        paint(&mut harness, scale as f64);
        assert_eq!(
            state.lock().unwrap().sizes,
            [(100 * scale as u32, 60 * scale as u32)]
        );
    }
}

#[test]
fn content_origin_is_translated_before_the_element_transform() {
    let (mut harness, _) = widget(
        "transform-origin:0 0;transform:rotate(30deg) scale(1.4,.8)",
        1.0,
    );
    let scene = paint(&mut harness, 1.0);
    let [a, b, c, d, x, y] = scene
        .commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::Fill(fill) if fill.brush == MARKER.into() => {
                Some(fill.transform.as_coeffs())
            }
            _ => None,
        })
        .unwrap();
    assert!((x - (a * 15.0 + c * 15.0)).abs() < 0.001);
    assert!((y - (b * 15.0 + d * 15.0)).abs() < 0.001);
}

#[test]
fn pointer_coordinates_use_the_transformed_content_box() {
    for scale in [1.0, 2.0] {
        let (mut harness, state) = widget(
            "transform-origin:0 0;transform:rotate(30deg) scale(1.4,.8)",
            scale,
        );
        let angle = 30.0_f32.to_radians();
        let x = 1.4 * 35.0 * angle.cos() - 0.8 * 25.0 * angle.sin();
        let y = 1.4 * 35.0 * angle.sin() + 0.8 * 25.0 * angle.cos();
        assert_eq!(
            harness.base().hit(x, y).unwrap().node_id,
            harness.node("#widget")
        );
        harness.click_at(x, y);
        let state = state.lock().unwrap();
        for &(local_x, local_y) in state.points.last().unwrap() {
            assert!((local_x - 20.0).abs() < 0.001, "{local_x}");
            assert!((local_y - 10.0).abs() < 0.001, "{local_y}");
        }
    }
}

#[test]
fn offset_widget_uses_content_box_local_coordinates() {
    for scale in [1.0, 2.0] {
        let (mut harness, state) = widget("position:absolute;left:70px;top:40px", scale);
        let layout = *harness
            .base()
            .get_node(harness.node("#widget"))
            .unwrap()
            .final_layout();
        assert_eq!(layout.location.x, 70.0);
        assert_eq!(layout.location.y, 40.0);
        let scene = paint(&mut harness, scale as f64);
        let origin = scene
            .commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::Fill(fill) if fill.brush == MARKER.into() => {
                    Some(fill.transform.translation())
                }
                _ => None,
            })
            .unwrap();
        assert!((origin.x - 85.0 * scale as f64).abs() < 0.001);
        assert!((origin.y - 55.0 * scale as f64).abs() < 0.001);
        harness.click_at(105.0, 65.0);
        for &(x, y) in state.lock().unwrap().points.last().unwrap() {
            assert!((x - 20.0).abs() < 0.001, "{x}");
            assert!((y - 10.0).abs() < 0.001, "{y}");
        }
    }
}

#[test]
fn empty_content_box_is_not_painted() {
    let (mut harness, state) = widget("width:0;height:0", 1.0);
    paint(&mut harness, 1.0);
    assert!(state.lock().unwrap().sizes.is_empty());
}

#[test]
fn pointer_outside_widget_is_not_forwarded() {
    let (mut harness, state) = widget("", 1.0);
    harness.click_at(700.0, 500.0);
    assert!(state.lock().unwrap().points.is_empty());
}
