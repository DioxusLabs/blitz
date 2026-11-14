//! Paint hook: app code called before and after each step of an element's paint sequence.

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::paint_hook::PaintPoint::{After, At, Before};
use blitz_dom::paint_hook::anyrender::{RenderContext, Scene};
use blitz_dom::paint_hook::kurbo::{Affine, Rect, Shape, Stroke, Vec2};
use blitz_dom::paint_hook::peniko::{Color, Fill};
use blitz_dom::paint_hook::{PaintEl, PaintHook, PaintPoint, Step};
use blitz_dom::{BaseDocument, DocumentConfig, local_name};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::{CssBox, create_css_rect, paint_scene};
use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const RED: Color = Color::from_rgb8(255, 0, 0);

/// A 100x100 blue box with a 10px green border, at the top-left of a 200x200 white page.
const PAGE: &str = r#"<html><body style="margin:0; background:#ffffff">
    <div id="box" class="target" style="box-sizing:border-box; width:100px; height:100px;
        background:#0000ff; border:10px solid #00ff00;"></div>
</body></html>"#;

fn fill(scene: &mut Scene, shape: &impl Shape, color: Color) {
    scene.fill(Fill::NonZero, Affine::IDENTITY, color, None, shape);
}

/// The element's boxes in CSS pixels, from the painter's own geometry.
fn boxes(el: &PaintEl<'_>) -> CssBox {
    let node = el.node();
    create_css_rect(&node.primary_styles().unwrap(), node.final_layout(), 1.0)
}

fn id<'a>(el: &PaintEl<'a>) -> Option<&'a str> {
    el.node().attr(local_name!("id"))
}

fn tag<'a>(el: &PaintEl<'a>) -> &'a str {
    el.node()
        .element_data()
        .map_or("", |element| &*element.name.local)
}

/// A hook made from a function.
struct FnHook<F>(F);

impl<F: Fn(PaintPoint, &PaintEl<'_>, &mut Scene) + 'static> PaintHook for FnHook<F> {
    fn paint(&self, point: PaintPoint, el: &PaintEl<'_>, cx: &mut Scene) {
        (self.0)(point, el, cx)
    }
}

/// A hook that draws on `#box` at one point.
fn on_box(point: PaintPoint, draw: impl Fn(&PaintEl<'_>, &mut Scene) + 'static) -> impl PaintHook {
    FnHook(move |at, el: &PaintEl<'_>, cx: &mut Scene| {
        if at == point && id(el) == Some("box") {
            draw(el, cx);
        }
    })
}

/// A hook that does nothing.
fn nothing() -> impl PaintHook {
    FnHook(|_, _: &PaintEl<'_>, _: &mut Scene| {})
}

fn document(html: &str, scale: f64) -> HtmlDocument {
    let size = (200.0 * scale) as u32;
    HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(size, size, scale as f32, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    )
}

/// Paint `doc` at `scale` and return the pixel at a CSS-pixel position.
fn paint(doc: &mut BaseDocument, scale: f64, at: (u32, u32)) -> [u8; 3] {
    let size = (200.0 * scale) as u32;
    doc.resolve(0.0);
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc, scale, size, size, 0, 0),
        size,
        size,
    );
    let x = (at.0 as f64 * scale) as u32;
    let y = (at.1 as f64 * scale) as u32;
    let idx = ((y * size + x) * 4) as usize;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

/// Lay out `html`, install `hook`, paint at `scale` and return the pixel at a CSS-pixel
/// position.
fn pixel(html: &str, scale: f64, at: (u32, u32), hook: impl PaintHook) -> [u8; 3] {
    let mut doc = document(html, scale);
    doc.set_paint_hook(hook);
    paint(&mut doc, scale, at)
}

#[test]
fn without_a_hook_the_page_paints_as_usual() {
    let mut doc = document(PAGE, 1.0);
    assert_eq!(paint(&mut doc, 1.0, (50, 50)), [0, 0, 255]);
    assert_eq!(paint(&mut doc, 1.0, (5, 50)), [0, 255, 0]);
}

#[test]
fn every_point_is_called_in_paint_order() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let record = seen.clone();
    pixel(
        PAGE,
        1.0,
        (50, 50),
        FnHook(move |point, el: &PaintEl<'_>, _: &mut Scene| {
            if tag(el) == "div" {
                record.borrow_mut().push(point);
            }
        }),
    );
    assert_eq!(
        *seen.borrow(),
        [
            Before(Step::Shadow),
            After(Step::Shadow),
            Before(Step::Background),
            After(Step::Background),
            Before(Step::Border),
            After(Step::Border),
            Before(Step::Content),
            After(Step::Content),
        ]
    );
}

#[test]
fn after_background_is_under_the_border() {
    let hook = || {
        on_box(After(Step::Background), |el, cx| {
            fill(cx, &boxes(el).border_box_path(), RED)
        })
    };
    assert_eq!(pixel(PAGE, 1.0, (50, 50), hook()), [255, 0, 0]);
    assert_eq!(pixel(PAGE, 1.0, (5, 50), hook()), [0, 255, 0]);
}

#[test]
fn boxes_are_in_css_pixels_at_any_scale() {
    // Fill the padding box only: the border stays green, at 1x and at 2x
    let hook = || {
        on_box(After(Step::Border), |el, cx| {
            let boxes = boxes(el);
            assert_eq!(boxes.border_box, Rect::new(0.0, 0.0, 100.0, 100.0));
            assert_eq!(boxes.padding_box, Rect::new(10.0, 10.0, 90.0, 90.0));
            assert_eq!(boxes.border_radii.top_left, Vec2::ZERO);
            fill(cx, &boxes.padding_box_path(), RED);
        })
    };
    for scale in [1.0, 2.0] {
        assert_eq!(pixel(PAGE, scale, (50, 50), hook()), [255, 0, 0]);
        assert_eq!(pixel(PAGE, scale, (5, 50), hook()), [0, 255, 0]);
        assert_eq!(pixel(PAGE, scale, (95, 50), hook()), [0, 255, 0]);
    }
}

/// The outline of a box is closed, so stroking it draws all four sides.
#[test]
fn stroking_a_box_draws_all_four_sides() {
    let ring = || {
        on_box(After(Step::Border), |el, cx| {
            let outline = &boxes(el).border_box_path();
            cx.stroke(&Stroke::new(4.0), Affine::IDENTITY, RED, None, outline);
        })
    };
    for side in [(50, 1), (98, 50), (50, 98), (1, 50)] {
        assert_eq!(pixel(PAGE, 1.0, side, ring()), [255, 0, 0], "{side:?}");
    }
}

/// A glow is larger than the element. On an element with `opacity`, drawing from the
/// background on is cut to the border box; drawing at the shadow points is not.
#[test]
fn shadow_point_is_not_cut_by_the_opacity_layer() {
    let page = PAGE.replace("background:#0000ff;", "background:#0000ff; opacity:0.5;");
    let halo = Rect::new(-20.0, -20.0, 140.0, 140.0);

    let in_layer = on_box(Before(Step::Background), move |_, cx| fill(cx, &halo, RED));
    assert_eq!(pixel(&page, 1.0, (110, 50), in_layer), [255, 255, 255]);

    let at_shadow = on_box(Before(Step::Shadow), move |_, cx| fill(cx, &halo, RED));
    assert_eq!(pixel(&page, 1.0, (110, 50), at_shadow), [255, 0, 0]);
}

/// `prepare` runs once per paint, before any element, with the renderer's context.
#[test]
fn prepare_runs_once_before_the_elements() {
    struct Trace(Rc<RefCell<Vec<&'static str>>>);
    impl PaintHook for Trace {
        fn prepare(&self, _: &mut dyn RenderContext) {
            self.0.borrow_mut().push("prepare");
        }
        fn begin_element(&self, el: &PaintEl<'_>) -> bool {
            if tag(el) == "html" {
                self.0.borrow_mut().push("html");
            }
            false
        }
        fn paint(&self, _: PaintPoint, _: &PaintEl<'_>, _: &mut Scene) {}
    }
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut doc = document(PAGE, 1.0);
    doc.set_paint_hook(Trace(trace.clone()));
    paint(&mut doc, 1.0, (50, 50));
    paint(&mut doc, 1.0, (50, 50));
    assert_eq!(*trace.borrow(), ["prepare", "html", "prepare", "html"]);
}

/// Children are painted inside their parent, so `begin_element` and `end_element` nest.
#[test]
fn begin_and_end_nest_like_the_elements() {
    struct Trace(Rc<RefCell<Vec<String>>>);
    impl PaintHook for Trace {
        fn begin_element(&self, el: &PaintEl<'_>) -> bool {
            self.0.borrow_mut().push(format!("<{}>", tag(el)));
            false
        }
        fn paint(&self, _: PaintPoint, _: &PaintEl<'_>, _: &mut Scene) {
            panic!("every element was declined");
        }
        fn end_element(&self, el: &PaintEl<'_>) {
            self.0.borrow_mut().push(format!("</{}>", tag(el)));
        }
    }
    let trace = Rc::new(RefCell::new(Vec::new()));
    pixel(PAGE, 1.0, (50, 50), Trace(trace.clone()));
    assert_eq!(
        trace.borrow().join(""),
        "<html><body><div></div></body></html>"
    );
}

/// Each `request_redraw` asks the window for one more frame. A hook that stops asking gets no
/// further frames.
#[test]
fn request_redraw_asks_the_window_for_one_more_frame() {
    #[derive(Default)]
    struct Window(AtomicUsize);
    impl ShellProvider for Window {
        fn request_redraw(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    let mut doc = document(PAGE, 1.0);
    let window = Arc::new(Window::default());
    doc.set_shell_provider(window.clone());

    let frames_left = Rc::new(Cell::new(1));
    let counter = frames_left.clone();
    doc.set_paint_hook(on_box(After(Step::Border), move |el, _| {
        if counter.get() > 0 {
            counter.set(counter.get() - 1);
            el.document().shell_provider.request_redraw();
        }
    }));
    let requests = || window.0.load(Ordering::Relaxed);
    let installed = requests();
    assert_eq!(installed, 1, "installing a hook asks for a frame");

    paint(&mut doc, 1.0, (50, 50));
    assert_eq!(
        requests(),
        installed + 1,
        "the hook asked for another frame"
    );
    paint(&mut doc, 1.0, (50, 50));
    assert_eq!(requests(), installed + 1, "it did not ask again");
}

#[test]
fn the_hook_can_be_removed() {
    let mut doc = document(PAGE, 1.0);
    doc.set_paint_hook(on_box(After(Step::Border), |el, cx| {
        fill(cx, &boxes(el).border_box_path(), RED)
    }));
    assert_eq!(paint(&mut doc, 1.0, (50, 50)), [255, 0, 0]);
    assert!(doc.take_paint_hook().is_some());
    assert_eq!(paint(&mut doc, 1.0, (50, 50)), [0, 0, 255]);
}

/// A hook that takes over one step of `#box`, and may draw its own version of it.
struct Replace {
    step: Step,
    fill_at: Option<PaintPoint>,
}

impl PaintHook for Replace {
    fn replaces(&self, step: Step, el: &PaintEl<'_>) -> bool {
        step == self.step && id(el) == Some("box")
    }

    fn paint(&self, point: PaintPoint, el: &PaintEl<'_>, cx: &mut Scene) {
        if self.fill_at == Some(point) && id(el) == Some("box") {
            fill(cx, &boxes(el).border_box_path(), RED);
        }
    }
}

fn replace(step: Step) -> Replace {
    Replace {
        step,
        fill_at: None,
    }
}

/// A replaced step is left out by the painter; the other steps are still drawn.
#[test]
fn a_replaced_step_is_not_painted() {
    const WHITE: [u8; 3] = [255, 255, 255];
    const BLUE: [u8; 3] = [0, 0, 255];
    const GREEN: [u8; 3] = [0, 255, 0];

    // Background gone: the page shows through, the border stays
    assert_eq!(pixel(PAGE, 1.0, (50, 50), replace(Step::Background)), WHITE);
    assert_eq!(pixel(PAGE, 1.0, (5, 50), replace(Step::Background)), GREEN);

    // Border gone: the background under it shows, as it reaches the border's outer edge
    assert_eq!(pixel(PAGE, 1.0, (5, 50), replace(Step::Border)), BLUE);
    assert_eq!(pixel(PAGE, 1.0, (50, 50), replace(Step::Border)), BLUE);

    // Shadow gone, outer and inset
    let shadow = PAGE.replace(
        "background:#0000ff;",
        "background:#0000ff; box-shadow: 20px 0 0 0 #ff0000, inset 0 0 0 30px #ff0000;",
    );
    assert_eq!(pixel(&shadow, 1.0, (110, 50), nothing()), [255, 0, 0]);
    assert_eq!(pixel(&shadow, 1.0, (20, 50), nothing()), [255, 0, 0]);
    assert_eq!(pixel(&shadow, 1.0, (110, 50), replace(Step::Shadow)), WHITE);
    assert_eq!(pixel(&shadow, 1.0, (20, 50), replace(Step::Shadow)), BLUE);

    // Content gone, children included
    let child = PAGE.replace(
        "></div>",
        r#"><div style="width:40px; height:40px; background:#ff0000"></div></div>"#,
    );
    assert_eq!(pixel(&child, 1.0, (20, 20), nothing()), [255, 0, 0]);
    assert_eq!(pixel(&child, 1.0, (20, 20), replace(Step::Content)), BLUE);
}

/// The hook draws its own version of a replaced step at `At`, and it sits in the same place
/// in the order: a replaced background is still under the border.
#[test]
fn a_replaced_step_is_drawn_by_the_hook_at_its_place() {
    let own_background = || Replace {
        step: Step::Background,
        fill_at: Some(At(Step::Background)),
    };
    assert_eq!(pixel(PAGE, 1.0, (50, 50), own_background()), [255, 0, 0]);
    assert_eq!(pixel(PAGE, 1.0, (5, 50), own_background()), [0, 255, 0]);
}

/// `At` is called between `Before` and `After`, and only for a step the hook takes over.
#[test]
fn at_is_called_for_replaced_steps_only() {
    struct Record(Rc<RefCell<Vec<PaintPoint>>>);
    impl PaintHook for Record {
        fn replaces(&self, step: Step, el: &PaintEl<'_>) -> bool {
            id(el) == Some("box") && matches!(step, Step::Background | Step::Content)
        }
        fn paint(&self, point: PaintPoint, el: &PaintEl<'_>, _: &mut Scene) {
            if id(el) == Some("box") {
                self.0.borrow_mut().push(point);
            }
        }
    }
    let seen = Rc::new(RefCell::new(Vec::new()));
    pixel(PAGE, 1.0, (50, 50), Record(seen.clone()));
    assert_eq!(
        *seen.borrow(),
        [
            Before(Step::Shadow),
            After(Step::Shadow),
            Before(Step::Background),
            At(Step::Background),
            After(Step::Background),
            Before(Step::Border),
            After(Step::Border),
            Before(Step::Content),
            At(Step::Content),
            After(Step::Content),
        ]
    );
}
