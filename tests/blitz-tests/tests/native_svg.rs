//! Native inline SVG (`DocumentConfig::native_svg`): the `<svg>` subtree stays
//! live DOM nodes styled by Stylo and is painted straight from the DOM, instead
//! of being serialized and re-parsed with usvg.

use std::sync::Arc;

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{CopyStyleGetters, DocumentConfig, LocalName, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};

const W: u32 = 100;
const H: u32 = 100;

const RED: [u8; 3] = [255, 0, 0];
const LIME: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];

fn load(body: &str, native_svg: bool) -> HtmlDocument {
    let html = format!(r#"<html><body style="margin:0; background:#00f">{body}</body></html>"#);
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(W, H, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            native_svg: Some(native_svg),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn render(doc: &mut HtmlDocument) -> Vec<u8> {
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc.as_mut(), 1.0, W, H, 0, 0),
        W,
        H,
    )
}

fn pixel(buffer: &[u8], x: usize, y: usize) -> [u8; 3] {
    let idx = (y * W as usize + x) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

fn attr(local: &str) -> QualName {
    QualName {
        prefix: None,
        ns: ns!(),
        local: LocalName::from(local),
    }
}

/// A 100x100 svg with a 50x50 red square in its top-left quadrant.
const SQUARE: &str = r#"<svg id="icon" width="100" height="100"><rect id="r" width="50" height="50" fill="red"/></svg>"#;

#[test]
fn native_svg_keeps_the_subtree_in_the_dom_and_skips_usvg() {
    let doc = load(SQUARE, true);
    let svg = doc.query_selector("#icon").unwrap().unwrap();
    let el = doc.get_node(svg).unwrap().element_data().unwrap();
    assert!(
        el.svg_data().is_none(),
        "native mode must not parse with usvg"
    );
    assert!(el.svg_root_data().is_some());
    let rect = doc.query_selector("#r").unwrap().unwrap();
    let rect = doc.get_node(rect).unwrap();
    assert!(
        rect.primary_styles().is_some(),
        "svg descendants must be styled by stylo"
    );
}

#[test]
fn usvg_fallback_is_still_used_when_disabled() {
    let mut doc = load(SQUARE, false);
    let svg = doc.query_selector("#icon").unwrap().unwrap();
    let el = doc.get_node(svg).unwrap().element_data().unwrap();
    assert!(el.svg_data().is_some(), "legacy mode must parse with usvg");
    assert!(el.svg_root_data().is_none());
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), RED);
    assert_eq!(pixel(&buffer, 75, 75), BLUE);
}

#[test]
fn native_svg_paints_a_rect() {
    let mut doc = load(SQUARE, true);
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), RED);
    assert_eq!(
        pixel(&buffer, 75, 25),
        BLUE,
        "the svg itself is transparent"
    );
    assert_eq!(pixel(&buffer, 75, 75), BLUE);
}

#[test]
fn native_svg_viewbox_scales_user_units_to_the_viewport() {
    // 10x10 user units mapped onto 100x100 CSS px: a 5x5 rect covers the
    // top-left quadrant.
    let mut doc = load(
        r#"<svg width="100" height="100" viewBox="0 0 10 10"><rect width="5" height="5" fill="red"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), RED);
    assert_eq!(pixel(&buffer, 75, 75), BLUE);
}

#[test]
fn native_svg_intrinsic_size_comes_from_attributes() {
    let doc = load(r#"<svg id="icon" width="40" height="20"></svg>"#, true);
    let svg = doc.query_selector("#icon").unwrap().unwrap();
    let size = doc.get_node(svg).unwrap().final_layout().size;
    assert_eq!((size.width, size.height), (40.0, 20.0));

    // viewBox only: an intrinsic 2:1 aspect ratio and no intrinsic size, so
    // the default 300px width is capped by the 100px containing block.
    let doc = load(r#"<svg id="icon" viewBox="0 0 200 100"></svg>"#, true);
    let svg = doc.query_selector("#icon").unwrap().unwrap();
    let size = doc.get_node(svg).unwrap().final_layout().size;
    assert_eq!((size.width, size.height), (100.0, 50.0));
}

#[test]
fn stylesheets_style_svg_elements() {
    // An author rule beats the `fill` presentation attribute.
    let mut doc = load(
        &format!(r#"<style>rect {{ fill: lime }}</style>{SQUARE}"#),
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), LIME);
}

#[test]
fn svg_elements_are_in_the_svg_namespace() {
    let mut doc = load(
        &format!(
            r#"<style>@namespace svg url(http://www.w3.org/2000/svg); svg|rect {{ fill: lime }}</style>{SQUARE}"#
        ),
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), LIME);

    // Does not match elements in the HTML namespace.
    let mut doc = load(
        &format!(
            r#"<style>@namespace html url(http://www.w3.org/1999/xhtml); html|rect {{ fill: lime }}</style>{SQUARE}"#
        ),
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), RED);
}

#[test]
fn presentation_attributes_map_to_css() {
    let doc = load(
        r#"<svg width="100" height="100"><circle id="c" cx="50" cy="50" r="40" fill="red" stroke="blue" stroke-width="2" fill-opacity="0.5" transform="translate(10 20) scale(2)"/></svg>"#,
        true,
    );
    let circle = doc.query_selector("#c").unwrap().unwrap();
    let node = doc.get_node(circle).unwrap();
    let style = node.primary_styles().unwrap();
    let svg = style.get_inherited_svg();
    assert!(matches!(
        svg.fill.kind,
        style::values::computed::svg::SVGPaintKind::Color(_)
    ));
    assert!(matches!(
        svg.stroke.kind,
        style::values::computed::svg::SVGPaintKind::Color(_)
    ));
    assert_eq!(
        svg.fill_opacity,
        style::values::computed::svg::SVGOpacity::Opacity(0.5)
    );
    let geometry = style.get_svg();
    let px = |lp: &style::values::computed::LengthPercentage| {
        lp.resolve(style::values::computed::Length::new(0.0)).px()
    };
    assert_eq!(px(&geometry.cx), 50.0);
    assert_eq!(px(&geometry.r.0), 40.0);
    assert!(
        !style.get_box().transform.0.is_empty(),
        "svg transform attribute maps to CSS transform"
    );
}

#[test]
fn native_svg_transform_attribute_moves_content() {
    let mut doc = load(
        r#"<svg width="100" height="100"><rect width="50" height="50" fill="red" transform="translate(50 50)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), BLUE);
    assert_eq!(pixel(&buffer, 75, 75), RED);
}

#[test]
fn native_svg_paints_paths_and_groups() {
    let mut doc = load(
        r#"<svg width="100" height="100"><g fill="red"><path d="M50 50 H100 V100 H50 Z"/></g><defs><rect width="100" height="100" fill="lime"/></defs></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 75, 75), RED, "path inherits fill from <g>");
    assert_eq!(
        pixel(&buffer, 25, 25),
        BLUE,
        "<defs> content is not rendered"
    );
}

#[test]
fn native_svg_attribute_change_repaints_without_reconstruction() {
    let mut doc = load(SQUARE, true);
    let rect = doc.query_selector("#r").unwrap().unwrap();
    doc.mutate().set_attribute(rect, attr("fill"), "lime");
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), LIME);

    doc.mutate().set_attribute(rect, attr("width"), "100");
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 75, 25), LIME);
}

#[test]
fn native_svg_current_color_follows_parent() {
    let mut doc = load(
        r#"<div id="p" style="color: red"><svg width="100" height="100"><rect width="100" height="100" fill="currentColor"/></svg></div>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 50, 50), RED);

    let parent = doc.query_selector("#p").unwrap().unwrap();
    doc.mutate()
        .set_attribute(parent, attr("style"), "color: lime");
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 50, 50), LIME);
}

#[test]
fn page_background_shows_outside_the_svg() {
    let mut doc = load(
        r#"<svg width="50" height="50"><rect width="50" height="50" fill="red"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), RED);
    assert_eq!(pixel(&buffer, 75, 75), BLUE);
}

#[test]
fn resource_container_children_are_styled() {
    // Stylo does not style the children of a `display: none` element, so
    // `<defs>`/gradients/`<clipPath>` must stay displayed (and the painter
    // skips them) for `<stop>` and clip shapes to have computed styles.
    let doc = load(
        r#"<svg width="100" height="100">
            <defs>
                <linearGradient id="g"><stop id="s" offset="0" style="stop-opacity: 0.5"/></linearGradient>
                <clipPath id="cp"><rect id="cr" width="50" height="50" fill="lime"/></clipPath>
            </defs>
            <rect width="100" height="100" fill="red"/>
        </svg>"#,
        true,
    );
    for id in ["#g", "#s", "#cp", "#cr"] {
        let node = doc.query_selector(id).unwrap().unwrap();
        let node = doc.get_node(node).unwrap();
        let style = node
            .primary_styles()
            .unwrap_or_else(|| panic!("{id} has no computed style"));
        assert_ne!(
            style.clone_display(),
            style::values::specified::Display::None,
            "{id} must not be display: none"
        );
    }
    let cr = doc.query_selector("#cr").unwrap().unwrap();
    let style = doc.get_node(cr).unwrap().primary_styles().unwrap();
    assert!(matches!(
        style.get_inherited_svg().fill.kind,
        style::values::computed::svg::SVGPaintKind::Color(_)
    ));
}

#[test]
fn resource_containers_are_not_painted() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <defs><rect width="100" height="100" fill="lime"/></defs>
            <clipPath id="c"><rect width="100" height="100" fill="lime"/></clipPath>
            <mask id="m"><rect width="100" height="100" fill="lime"/></mask>
            <pattern id="p"><rect width="100" height="100" fill="lime"/></pattern>
            <symbol id="s"><rect width="100" height="100" fill="lime"/></symbol>
            <marker id="mk"><rect width="100" height="100" fill="lime"/></marker>
            <rect width="50" height="50" fill="red"/>
        </svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED);
    assert_eq!(pixel(&buf, 75, 75), BLUE);
}

#[test]
fn geometry_attributes_are_element_specific() {
    let doc = load(
        r#"<svg width="100" height="100">
            <circle id="c" cx="10" r="5"/>
            <text id="t" x="10" y="20">hi</text>
            <use id="u" x="10" width="30"/>
            <ellipse id="e" r="5" rx="7"/>
        </svg>"#,
        true,
    );
    let resolve = |lp: &style::values::computed::LengthPercentage| {
        lp.resolve(style::values::computed::Length::new(0.0)).px()
    };
    let px = |id: &str, get: &dyn Fn(&style::properties::ComputedValues) -> f32| {
        let node = doc.query_selector(id).unwrap().unwrap();
        let style = doc.get_node(node).unwrap().primary_styles().unwrap();
        get(&style)
    };
    let rx = |s: &style::properties::ComputedValues| {
        s.get_svg()
            .rx
            .non_auto()
            .map(|v| resolve(&v.0))
            .unwrap_or(-1.0)
    };
    assert_eq!(px("#c", &|s| resolve(&s.get_svg().cx)), 10.0);
    assert_eq!(px("#c", &|s| resolve(&s.get_svg().r.0)), 5.0);
    // `x` on <text> is a list of positions, not the CSS `x` property.
    assert_eq!(px("#t", &|s| resolve(&s.get_svg().x)), 0.0);
    assert_eq!(px("#u", &|s| resolve(&s.get_svg().x)), 0.0);
    assert!(px("#u", &|s| s.get_position().width.is_auto() as u8 as f32) == 1.0);
    assert_eq!(px("#e", &|s| resolve(&s.get_svg().r.0)), 0.0);
    assert_eq!(px("#e", &rx), 7.0);
    assert_eq!(px("#c", &rx), -1.0);
}

#[test]
fn path_data_is_kept_up_to_the_first_error() {
    // SVG renders the valid prefix of a malformed `d`; `path("...")` as CSS
    // would have dropped the whole attribute.
    let mut doc = load(
        r#"<svg width="100" height="100"><path d="M 0 0 H 50 V 50 H 0 Z garbage" fill="red"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED);
    assert_eq!(pixel(&buf, 75, 75), BLUE);
}

#[test]
fn group_opacity_blends_with_the_page() {
    let mut doc = load(
        r#"<svg width="100" height="100"><g opacity="0.5" transform="translate(50 50)"><rect x="-25" y="-25" width="50" height="50" fill="red"/></g></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    let [r, g, b] = pixel(&buf, 50, 50);
    assert!(
        r > 100 && r < 160 && g == 0 && b > 100 && b < 160,
        "got {:?}",
        [r, g, b]
    );
    assert_eq!(pixel(&buf, 10, 10), BLUE);
}

#[test]
fn nested_svg_clips_to_its_viewport() {
    let mut doc = load(
        r#"<svg width="100" height="100"><svg x="25" y="25" width="50" height="50"><circle cx="25" cy="25" r="60" fill="red"/></svg></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 50), RED);
    assert_eq!(pixel(&buf, 30, 30), RED);
    assert_eq!(pixel(&buf, 20, 50), BLUE, "outside the nested viewport");
    assert_eq!(pixel(&buf, 50, 80), BLUE, "outside the nested viewport");
}

#[test]
fn usvg_mode_leaves_presentation_attributes_to_usvg() {
    let body = r#"<svg id="s" width="50" height="50" transform="translate(10 0)"><rect width="50" height="50" fill="red"/></svg>"#;
    for (native, expect_transform) in [(false, false), (true, true)] {
        let doc = load(body, native);
        let id = doc.query_selector("#s").unwrap().unwrap();
        let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
        assert_eq!(
            !styles.get_box().transform.0.is_empty(),
            expect_transform,
            "native_svg = {native}"
        );
    }
}

#[test]
fn path_arcs_sweep_on_the_right_side() {
    // sweep-flag 1 goes in the positive-angle direction (clockwise with y
    // down), so this half circle from (10,50) to (90,50) bulges upwards.
    let mut doc = load(
        r#"<svg width="100" height="100"><path d="M 10 50 A 40 40 0 0 1 90 50 Z" fill="red"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 30), RED);
    assert_eq!(pixel(&buf, 50, 70), BLUE);
    assert_eq!(pixel(&buf, 15, 45), RED);
}

#[test]
fn non_presentation_attributes_do_not_style_the_box() {
    let doc = load(
        r#"<svg id="s" width="100" height="100" padding="30" background="red" margin-left="200" border="5px solid blue"><rect width="100" height="100" fill="red"/></svg>"#,
        true,
    );
    let id = doc.query_selector("#s").unwrap().unwrap();
    let layout = *doc.get_node(id).unwrap().final_layout();
    assert_eq!((layout.location.x, layout.size.width), (0.0, 100.0));
    assert_eq!(layout.padding.left, 0.0);
}

#[test]
fn degenerate_arcs_do_not_drop_the_path() {
    for d in [
        "M10 10 L50 10 A0 0 0 0 1 90 50 L10 90 Z",
        "M10 10 L50 10 A20 20 0 0 1 50 10 L90 90 L10 90 Z",
    ] {
        let mut doc = load(
            &format!(r#"<svg width="100" height="100"><path d="{d}" fill="red"/></svg>"#),
            true,
        );
        let buf = render(&mut doc);
        assert_eq!(pixel(&buf, 30, 50), RED, "{d}");
    }
}

#[test]
fn native_svg_flex_item_keeps_its_specified_size() {
    let doc = load(
        r#"<div style="display:flex;width:50px"><svg id="s" width="100" height="100"></svg></div>"#,
        true,
    );
    let id = doc.query_selector("#s").unwrap().unwrap();
    assert_eq!(doc.get_node(id).unwrap().final_layout().size.width, 100.0);
}

#[test]
fn switch_paints_its_first_child() {
    let mut doc = load(
        r#"<svg width="100" height="100"><switch><rect width="100" height="100" fill="red"/><rect width="100" height="100" fill="lime"/></switch></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 50), RED);
}

#[test]
fn line_attributes_resolve_percentages() {
    let mut doc = load(
        r#"<svg width="100" height="100"><line x1="50%" y1="0" x2="50%" y2="100%" stroke="red" stroke-width="20"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 50), RED);
    assert_eq!(pixel(&buf, 20, 50), BLUE);
}

#[test]
fn group_opacity_is_not_clipped_when_overflow_is_visible() {
    let mut doc = load(
        r#"<svg width="50" height="50" style="overflow:visible"><g opacity="0.5"><rect width="100" height="50" fill="red"/></g></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_ne!(pixel(&buf, 75, 25), BLUE, "content outside the root box");
}
