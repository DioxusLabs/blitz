//! Empty text inputs paint their `placeholder` attribute in the `::placeholder` style.
use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 40;
const FIELD: &str = "display:block;width:300px;height:40px;margin:0;padding:0 8px;border:0;\
                     font:20px sans-serif;background:white";

fn document(styles: &str, input: &str) -> HtmlDocument {
    let html = format!(
        "<html><head><style>{styles}</style></head>\
         <body style='margin:0;background:white'>{input}</body></html>"
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn render(styles: &str, input: &str) -> Vec<u8> {
    let mut doc = document(styles, input);
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, WIDTH, HEIGHT, 0, 0),
        WIDTH,
        HEIGHT,
    )
}

fn has_pixel(pixels: &[u8], matches: impl Fn([u8; 3]) -> bool) -> bool {
    pixels
        .chunks(4)
        .any(|pixel| matches([pixel[0], pixel[1], pixel[2]]))
}

fn placeholder_color(doc: &HtmlDocument) -> Option<[u8; 3]> {
    let id = doc.query_selector("input, textarea").unwrap().unwrap();
    let placeholder = doc
        .get_node(id)?
        .element_data()?
        .text_input_data()?
        .placeholder
        .as_deref()?;
    let [r, g, b, _] = placeholder
        .color
        .to_color_space(style::color::ColorSpace::Srgb)
        .raw_components()
        .map(|c| (c * 255.0).round() as u8);
    Some([r, g, b])
}

#[test]
fn empty_input_paints_its_placeholder_in_the_ua_colour() {
    let input = format!("<input placeholder='Your email address' style='{FIELD}'>");
    assert_eq!(
        placeholder_color(&document("", &input)),
        Some([0x75, 0x75, 0x75])
    );
    let pixels = render("", &input);
    assert!(
        has_pixel(&pixels, |[r, g, b]| r == g
            && g == b
            && (100..=140).contains(&r)),
        "no grey placeholder text painted"
    );
}

#[test]
fn placeholder_pseudo_element_sets_the_colour() {
    let styles = "input::placeholder{color:rgb(255,0,0)}";
    let input = format!("<input placeholder='Your email address' style='{FIELD}'>");
    assert_eq!(
        placeholder_color(&document(styles, &input)),
        Some([255, 0, 0])
    );
    assert!(has_pixel(&render(styles, &input), |rgb| rgb == [255, 0, 0]));
}

#[test]
fn input_with_a_value_does_not_paint_its_placeholder() {
    let with = render(
        "",
        &format!("<input value='a' placeholder='Your email address' style='{FIELD}'>"),
    );
    let without = render("", &format!("<input value='a' style='{FIELD}'>"));
    assert_eq!(with, without);
}

#[test]
fn textarea_keeps_placeholder_line_breaks_and_input_strips_them() {
    let lines = |doc: &HtmlDocument| {
        let id = doc.query_selector("input, textarea").unwrap().unwrap();
        let input = doc.get_node(id).unwrap().element_data().unwrap();
        input
            .text_input_data()
            .unwrap()
            .placeholder
            .as_ref()
            .unwrap()
            .layout
            .len()
    };
    let text = "first&#10;second";
    assert_eq!(
        lines(&document(
            "",
            &format!("<textarea placeholder='{text}'></textarea>")
        )),
        2
    );
    assert_eq!(
        lines(&document("", &format!("<input placeholder='{text}'>"))),
        1
    );
}

#[test]
fn input_without_a_placeholder_has_none() {
    assert_eq!(placeholder_color(&document("", "<input>")), None);
    assert_eq!(
        placeholder_color(&document("", "<input placeholder=''>")),
        None
    );
}
