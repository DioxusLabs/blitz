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

#[test]
fn switch_renders_only_the_first_matching_child() {
    let mut doc = load(
        r#"<svg width="100" height="100"><switch>
            <rect width="100" height="100" fill="red" requiredExtensions="http://example.com"/>
            <rect width="100" height="100" fill="lime"/>
            <rect width="100" height="100" fill="red"/>
        </switch></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 50, 50), LIME);
}

#[test]
fn stroke_dasharray_leaves_gaps() {
    let mut doc = load(
        r#"<svg width="100" height="100"><line x1="0" y1="50" x2="100" y2="50" stroke="lime" stroke-width="20" stroke-dasharray="25"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 10, 50), LIME);
    assert_eq!(pixel(&buffer, 35, 50), BLUE);
    assert_eq!(pixel(&buffer, 60, 50), LIME);
}

#[test]
fn path_length_scales_dashes() {
    // pathLength="4" makes a dash of 1 cover a quarter of the 100px line.
    let mut doc = load(
        r#"<svg width="100" height="100"><path d="M 0 50 H 100" pathLength="4" stroke="lime" stroke-width="20" stroke-dasharray="1"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 10, 50), LIME);
    assert_eq!(pixel(&buffer, 35, 50), BLUE);
    assert_eq!(pixel(&buffer, 60, 50), LIME);
}

#[test]
fn nested_svg_overflow_clips_to_its_viewport() {
    let mut doc = load(
        r#"<svg width="100" height="100"><svg width="50" height="50"><rect width="100" height="100" fill="lime"/></svg></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), LIME);
    assert_eq!(pixel(&buffer, 75, 75), BLUE);
}

#[test]
fn mix_blend_mode_blends_with_content_below() {
    let mut doc = load(
        r#"<svg width="100" height="100"><rect width="100" height="100" fill="red"/><rect width="100" height="100" fill="lime" style="mix-blend-mode: lighten"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 50, 50), [255, 255, 0]);
}

#[test]
fn layout_records_object_bounding_boxes() {
    let doc = load(
        r#"<svg width="100" height="100"><g id="g" transform="translate(10 0)"><rect x="5" y="5" width="20" height="10"/><circle cx="50" cy="50" r="10" transform="scale(2)"/></g></svg>"#,
        true,
    );
    let g = doc.query_selector("#g").unwrap().unwrap();
    let layout = doc
        .get_node(g)
        .unwrap()
        .element_data()
        .unwrap()
        .svg_layout()
        .unwrap();
    let bbox = layout.bbox.unwrap();
    assert!(
        (bbox.x0 - 5.0).abs() < 1e-3 && (bbox.y0 - 5.0).abs() < 1e-3,
        "{bbox:?}"
    );
    assert!(
        (bbox.x1 - 120.0).abs() < 1e-3 && (bbox.y1 - 120.0).abs() < 1e-3,
        "{bbox:?}"
    );
}

#[test]
fn non_css_geometry_attribute_change_relayouts() {
    let mut doc = load(
        r#"<svg width="100" height="100"><g><path id="p" d="M 0 50 H 100" stroke="lime" stroke-width="20" stroke-dasharray="1"/></g></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 30, 50), LIME);
    let path = doc.query_selector("#p").unwrap().unwrap();
    doc.mutate().set_attribute(path, attr("pathLength"), "4");
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    // Dashes are now 25 user units long: [0, 25) on, [25, 50) off.
    assert_eq!(pixel(&buffer, 10, 50), LIME);
    assert_eq!(pixel(&buffer, 30, 50), BLUE);
    assert_eq!(pixel(&buffer, 60, 50), LIME);
}

fn svg_layout_addr(doc: &HtmlDocument, selector: &str) -> usize {
    let id = doc.query_selector(selector).unwrap().unwrap();
    let layout = doc
        .get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .svg_layout()
        .unwrap();
    std::ptr::from_ref(layout) as usize
}

#[test]
fn svg_layout_reuses_undamaged_elements() {
    let mut doc = load(
        r#"<svg width="100" height="100"><g id="g"><rect id="a" width="50" height="50" fill="red"/><rect id="b" x="50" width="50" height="50" fill="lime"/></g><svg id="n" width="100" height="100"><rect id="c" y="50" width="50" height="50" fill="lime"/></svg></svg>"#,
        true,
    );
    let ids = ["#a", "#b", "#c", "#n"];
    let before = ids.map(|sel| svg_layout_addr(&doc, sel));
    let a = doc.query_selector("#a").unwrap().unwrap();
    doc.mutate().set_attribute(a, attr("fill"), "lime");
    doc.resolve(0.0);
    let after = ids.map(|sel| svg_layout_addr(&doc, sel));
    // `#a`'s own layout may land at the same address once it is rebuilt.
    assert_eq!(before[1..], after[1..], "only #a is laid out again");
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), LIME);

    doc.mutate().set_attribute(a, attr("x"), "50");
    doc.mutate().set_attribute(a, attr("y"), "50");
    doc.resolve(0.0);
    let moved = ids.map(|sel| svg_layout_addr(&doc, sel));
    assert_eq!(after[1..], moved[1..], "only #a is laid out again");
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), BLUE);
    assert_eq!(pixel(&buffer, 75, 75), LIME);
}

#[test]
fn outermost_svg_isolates_blending() {
    let mut doc = load(
        r#"<svg width="100" height="100"><rect width="100" height="100" fill="lime" style="mix-blend-mode: difference"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 50, 50), LIME);
}

#[test]
fn nested_svg_overflow_clips_each_axis() {
    let mut doc = load(
        r#"<svg width="100" height="100"><svg width="50" height="50" style="overflow-x: clip; overflow-y: visible"><rect width="100" height="100" fill="lime"/></svg></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 75), LIME, "y is not clipped");
    assert_eq!(pixel(&buffer, 75, 25), BLUE, "x is clipped");
}

#[test]
fn nested_svg_overflow_auto_clips() {
    let mut doc = load(
        r#"<svg width="100" height="100"><svg width="50" height="50" style="overflow: auto"><rect width="100" height="100" fill="lime"/></svg></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), LIME);
    assert_eq!(pixel(&buffer, 75, 75), BLUE);
}

#[test]
fn unchanged_svg_skips_layout() {
    let mut doc = load(
        r#"<svg id="s1" width="50" height="100"><rect id="a" width="50" height="100" fill="red"/></svg><svg width="50" height="100"><g id="g"><rect id="b" width="50" height="100" fill="lime"/></g></svg>"#,
        true,
    );
    let before = ["#g", "#b"].map(|sel| svg_layout_addr(&doc, sel));
    let a = doc.query_selector("#a").unwrap().unwrap();
    doc.mutate().set_attribute(a, attr("width"), "25");
    doc.resolve(0.0);
    let after = ["#g", "#b"].map(|sel| svg_layout_addr(&doc, sel));
    assert_eq!(before, after, "the second <svg> is not laid out again");
}

fn assert_near(actual: [u8; 3], expected: [u8; 3]) {
    let close = actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 8);
    assert!(close, "expected about {expected:?}, got {actual:?}");
}

#[test]
fn linear_gradient_uses_computed_stop_styles() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <style>#s1 { stop-color: lime }</style>
            <linearGradient id="g"><stop id="s0" offset="0" stop-color="red"/><stop id="s1" offset="1" stop-color="red"/></linearGradient>
            <rect width="100" height="100" fill="url(#g)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_near(pixel(&buffer, 1, 50), RED);
    assert_near(pixel(&buffer, 98, 50), LIME);
    assert_near(pixel(&buffer, 50, 50), [128, 128, 0]);
}

#[test]
fn stop_opacity_applies_to_the_stop_color() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="g"><stop offset="0" stop-color="red" stop-opacity="0"/><stop offset="1" stop-color="red" stop-opacity="0"/></linearGradient>
            <rect width="100" height="100" fill="url(#g)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 50, 50), BLUE);
}

#[test]
fn object_bounding_box_gradients_use_the_shape_bbox() {
    // Pad spreads each end stop beyond the 50x100 rect's bbox.
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="g" x1="0.4" x2="0.6"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="lime"/></linearGradient>
            <rect x="50" width="50" height="100" fill="url(#g)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 50), BLUE);
    assert_eq!(pixel(&buffer, 60, 50), RED);
    assert_eq!(pixel(&buffer, 90, 50), LIME);
}

#[test]
fn user_space_gradients_ignore_the_bbox_and_apply_gradient_transform() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" x2="10" spreadMethod="repeat" gradientTransform="scale(5)">
                <stop offset="0.5" stop-color="red"/><stop offset="0.5" stop-color="lime"/></linearGradient>
            <rect x="10" width="90" height="100" fill="url(#g)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    // Bands of 25 user units, repeating every 50.
    assert_eq!(pixel(&buffer, 15, 50), RED);
    assert_eq!(pixel(&buffer, 35, 50), LIME);
    assert_eq!(pixel(&buffer, 65, 50), RED);
    assert_eq!(pixel(&buffer, 85, 50), LIME);
}

#[test]
fn href_inherits_stops_and_attributes() {
    let mut doc = load(
        r##"<svg width="100" height="100">
            <linearGradient id="base" x1="0.5" x2="0.5" y2="1" spreadMethod="pad"><stop offset="0.5" stop-color="red"/><stop offset="0.5" stop-color="lime"/></linearGradient>
            <linearGradient id="mid" href="#base"/>
            <linearGradient id="g" xlink:href="#mid"/>
            <rect width="100" height="100" fill="url(#g)"/></svg>"##,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 20, 20), RED);
    assert_eq!(pixel(&buffer, 20, 80), LIME);
}

#[test]
fn radial_geometry_is_not_inherited_from_linear_gradients() {
    let mut doc = load(
        r##"<svg width="100" height="100">
            <linearGradient id="base" x1="0.5" gradientUnits="userSpaceOnUse"><stop offset="0.5" stop-color="lime"/><stop offset="0.5" stop-color="red"/></linearGradient>
            <radialGradient id="g" href="#base" cx="0" cy="0" r="100"/>
            <rect width="100" height="100" fill="url(#g)"/></svg>"##,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 20, 20), LIME);
    assert_eq!(pixel(&buffer, 90, 90), RED);
}

#[test]
fn radial_gradient_with_focal_point() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <radialGradient id="g" fx="0.25" fy="0.5" r="0.5"><stop offset="0.1" stop-color="lime"/><stop offset="0.1" stop-color="red"/></radialGradient>
            <rect width="100" height="100" fill="url(#g)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 50), LIME);
    assert_eq!(pixel(&buffer, 50, 50), RED);
}

#[test]
fn invalid_references_use_the_fallback() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <rect id="notgradient" width="1" height="1" fill="none"/>
            <linearGradient id="g"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="red"/></linearGradient>
            <rect width="50" height="50" fill="url(#missing) lime"/>
            <rect x="50" width="50" height="50" fill="url(#notgradient) lime"/>
            <path d="M 0 75 H 100" stroke="url(#g) lime" stroke-width="10"/>
            <rect y="50" width="50" height="10" fill="url(#missing)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), LIME);
    assert_eq!(pixel(&buffer, 75, 25), LIME);
    // A horizontal line has an empty bbox height, so the objectBoundingBox
    // gradient is invalid for it.
    assert_eq!(pixel(&buffer, 50, 75), LIME);
    // A missing reference without a fallback paints nothing.
    assert_eq!(pixel(&buffer, 25, 55), BLUE);
}

#[test]
fn single_stop_paints_solid_and_no_stops_paint_nothing() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="one"><stop stop-color="lime"/></linearGradient>
            <linearGradient id="none"/>
            <rect width="50" height="100" fill="url(#one) red"/>
            <rect x="50" width="50" height="100" fill="url(#none) red"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 50), LIME);
    assert_eq!(pixel(&buffer, 75, 50), BLUE);
}

#[test]
fn stop_changes_repaint_the_gradient() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="g"><stop offset="0" stop-color="red"/><stop id="s" offset="1" stop-color="red"/></linearGradient>
            <rect width="100" height="100" fill="url(#g)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_near(pixel(&buffer, 98, 50), RED);
    let stop = doc.query_selector("#s").unwrap().unwrap();
    doc.mutate().set_attribute(stop, attr("stop-color"), "lime");
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    assert_near(pixel(&buffer, 98, 50), LIME);
    assert_near(pixel(&buffer, 50, 50), [128, 128, 0]);
    doc.mutate().set_attribute(stop, attr("offset"), "0");
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    assert_near(pixel(&buffer, 50, 50), LIME);
}

#[test]
fn stops_interpolate_unpremultiplied() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="g"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="lime" stop-opacity="0"/></linearGradient>
            <rect width="100" height="100" fill="url(#g)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    // Half-transparent yellow over blue, not half-transparent red.
    assert_near(pixel(&buffer, 50, 50), [64, 64, 128]);
}

#[test]
fn undisplayed_gradients_use_the_fallback() {
    let mut doc = load(
        r##"<svg width="100" height="100">
            <linearGradient id="hidden" style="display:none"><stop stop-color="red"/><stop offset="1" stop-color="red"/></linearGradient>
            <g style="display:none"><linearGradient id="nested"><stop stop-color="red"/><stop offset="1" stop-color="red"/></linearGradient></g>
            <g style="display:none"><linearGradient id="template"><stop stop-color="red"/><stop offset="1" stop-color="red"/></linearGradient></g>
            <linearGradient id="g" href="#template"/>
            <rect width="50" height="50" fill="url(#hidden) lime"/>
            <rect x="50" width="50" height="50" fill="url(#nested) lime"/>
            <rect y="50" width="50" height="50" fill="url(#g) lime"/></svg>"##,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 25), LIME);
    assert_eq!(pixel(&buffer, 75, 25), LIME);
    assert_eq!(pixel(&buffer, 25, 75), LIME);
}

#[test]
fn references_to_other_documents_use_the_fallback() {
    let mut doc = load(
        r##"<svg width="100" height="100">
            <linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="red"/></linearGradient>
            <linearGradient id="remote" href="http://example.com/o.svg#g"/>
            <rect width="50" height="100" fill="url(http://example.com/o.svg#g) lime"/>
            <rect x="50" width="50" height="100" fill="url(#remote) lime"/></svg>"##,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 25, 50), LIME);
    // The remote template is not followed, so `remote` has no stops.
    assert_eq!(pixel(&buffer, 75, 50), BLUE);
}

#[test]
fn transform_attribute_is_not_a_gradient_transform() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="a" transform="scale(0.1)"><stop offset="0.5" stop-color="red"/><stop offset="0.5" stop-color="lime"/></linearGradient>
            <linearGradient id="b" gradientUnits="userSpaceOnUse" x2="50" gradientTransform="scale(2)" transform="scale(0.1)"><stop offset="0.5" stop-color="red"/><stop offset="0.5" stop-color="lime"/></linearGradient>
            <rect width="100" height="50" fill="url(#a)"/>
            <rect y="50" width="100" height="50" fill="url(#b)"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 40, 25), RED);
    assert_eq!(pixel(&buffer, 60, 25), LIME);
    assert_eq!(pixel(&buffer, 40, 75), RED);
    assert_eq!(pixel(&buffer, 60, 75), LIME);
}

#[test]
fn small_invertible_gradient_transforms_are_valid() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="g" gradientUnits="userSpaceOnUse" x2="1e9" gradientTransform="scale(1e-7)"><stop offset="0.5" stop-color="red"/><stop offset="0.5" stop-color="lime"/></linearGradient>
            <rect width="100" height="100" fill="url(#g) blue"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 40, 50), RED);
    assert_eq!(pixel(&buffer, 60, 50), LIME);
}

#[test]
fn single_stop_gradients_are_invalid_for_an_empty_bbox() {
    let mut doc = load(
        r#"<svg width="100" height="100">
            <linearGradient id="g"><stop stop-color="red"/></linearGradient>
            <path d="M 0 50 H 100" stroke="url(#g) lime" stroke-width="10"/></svg>"#,
        true,
    );
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 50, 50), LIME);
}

#[test]
fn references_resolve_against_the_document_url() {
    let html = r##"<html><body style="margin:0; background:#00f"><svg width="100" height="100">
        <linearGradient id="g"><stop stop-color="lime"/><stop offset="1" stop-color="lime"/></linearGradient>
        <rect width="100" height="100" fill="url(#g) red"/></svg></body></html>"##;
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            base_url: Some("http://example.com/dir/doc.html".to_string()),
            viewport: Some(Viewport::new(W, H, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            native_svg: Some(true),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    assert_eq!(pixel(&buffer, 50, 50), LIME);
}

fn count(buffer: &[u8], color: [u8; 3], xs: std::ops::Range<usize>) -> usize {
    (0..H as usize)
        .flat_map(|y| xs.clone().map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(buffer, x, y) == color)
        .count()
}

const TEXT: &str = r#"<svg width="100" height="100"><text id="t" x="5" y="80" font-size="70" fill="red">H<tspan fill="lime">H</tspan></text></svg>"#;

#[test]
fn native_svg_paints_text_and_tspan_fills() {
    let mut doc = load(TEXT, true);
    let buffer = render(&mut doc);
    assert!(
        count(&buffer, RED, 0..50) > 100,
        "the text's glyph is filled"
    );
    assert!(
        count(&buffer, LIME, 40..100) > 100,
        "the tspan's glyph follows it"
    );
    assert_eq!(count(&buffer, LIME, 0..30), 0);
}

#[test]
fn native_svg_text_position_attributes_relayout() {
    let mut doc = load(TEXT, true);
    let text = doc.query_selector("#t").unwrap().unwrap();
    doc.mutate().set_attribute(text, attr("x"), "200");
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    assert_eq!(count(&buffer, RED, 0..100), 0);

    doc.mutate().set_attribute(text, attr("x"), "100");
    doc.mutate().set_attribute(text, attr("text-anchor"), "end");
    doc.resolve(0.0);
    let buffer = render(&mut doc);
    assert!(count(&buffer, RED, 0..60) > 100, "anchored at its end");
}

fn assert_same_render(a: &str, b: &str) {
    let a = render(&mut load(a, true));
    let b = render(&mut load(b, true));
    assert!(
        count(&a, [0, 0, 0], 0..W as usize) > 0,
        "the text is painted"
    );
    assert!(a == b, "renders differ");
}

#[test]
fn text_without_x_starts_at_zero() {
    let svg = |x: &str| {
        format!(
            r#"<svg width="100" height="100"><g transform="translate(50,0)" font-size="20">
            <text {x} y="25">&#x5d0;&#x5d1; ab</text>
            <text {x} y="55" text-anchor="middle">&#x5d0;&#x5d1; ab</text>
            <text {x} y="85" direction="rtl">&#x202b;ab cd&#x202c;</text>
            </g></svg>"#
        )
    };
    assert_same_render(&svg(""), &svg(r#"x="0""#));
}

#[test]
fn text_length_glyph_scaling_keeps_explicit_x() {
    assert_same_render(
        r#"<svg width="100" height="100"><text x="0 50" y="80" font-size="40" textLength="60" lengthAdjust="spacingAndGlyphs">HH</text></svg>"#,
        r#"<svg width="100" height="100"><text x="0" y="80" font-size="40" textLength="30" lengthAdjust="spacingAndGlyphs">H</text><text x="50" y="80" font-size="40" textLength="30" lengthAdjust="spacingAndGlyphs">H</text></svg>"#,
    );
}

fn element_at(doc: &HtmlDocument, x: f32, y: f32) -> Option<String> {
    let id = doc.element_from_point(x, y)?;
    let node = doc.get_node(id).unwrap();
    let element = node.element_data().unwrap();
    Some(
        element
            .id
            .as_ref()
            .map_or_else(|| element.name.local.to_string(), |id| id.to_string()),
    )
}

#[track_caller]
fn assert_hits(doc: &HtmlDocument, points: &[(f32, f32, &str)]) {
    for &(x, y, expected) in points {
        assert_eq!(
            element_at(doc, x, y).as_deref(),
            Some(expected),
            "element at ({x}, {y})"
        );
    }
}

#[test]
fn hit_testing_targets_shapes_by_fill_and_stroke() {
    let doc = load(
        r#"<svg id="svg" width="100" height="100">
        <rect id="rect" x="10" y="10" width="30" height="30" fill="red" stroke="blue" stroke-width="10"/>
        <circle id="circle" cx="70" cy="70" r="20" fill="none" stroke="blue" stroke-width="4"/>
        <path id="open" d="M 50 10 L 90 10 L 90 40" fill="red"/>
        </svg>"#,
        true,
    );
    assert_hits(
        &doc,
        &[
            (25.0, 25.0, "rect"),
            (40.0, 25.0, "rect"),
            (7.0, 7.0, "rect"),
            (2.0, 2.0, "svg"),
            // A `fill: none` circle is hit on its stroke only.
            (70.0, 70.0, "svg"),
            (70.0, 51.0, "circle"),
            // An open subpath is filled as if closed.
            (85.0, 20.0, "open"),
            (55.0, 30.0, "svg"),
        ],
    );
}

#[test]
fn hit_testing_follows_pointer_events_and_visibility() {
    let doc = load(
        r#"<svg id="svg" width="100" height="100">
        <rect id="fill-only" width="40" height="40" fill="none" pointer-events="visibleFill"/>
        <rect id="under" x="50" width="40" height="40" fill="red"/>
        <rect id="none" x="50" width="40" height="40" fill="red" pointer-events="none"/>
        <rect id="hidden" y="50" width="40" height="40" fill="red" visibility="hidden"/>
        <rect id="all" x="50" y="50" width="40" height="40" fill="none" visibility="hidden" pointer-events="all"/>
        <rect id="stroke" x="50" y="50" width="20" height="20" fill="red" stroke-width="10" pointer-events="stroke"/>
        </svg>"#,
        true,
    );
    assert_hits(
        &doc,
        &[
            (20.0, 20.0, "fill-only"),
            (70.0, 20.0, "under"),
            (20.0, 70.0, "svg"),
            (60.0, 60.0, "all"),
            (50.0, 60.0, "stroke"),
            (85.0, 85.0, "all"),
        ],
    );
}

#[test]
fn hit_testing_maps_points_through_transforms_and_viewports() {
    let doc = load(
        r#"<svg id="svg" width="100" height="100" viewBox="0 0 50 50">
        <g transform="translate(25 0)"><rect id="rect" width="10" height="10"/></g>
        <svg x="0" y="25" width="10" height="10" viewBox="0 0 1 1"><rect id="nested" x="-1" width="4" height="1"/></svg>
        </svg>"#,
        true,
    );
    assert_hits(
        &doc,
        &[
            (60.0, 10.0, "rect"),
            (40.0, 10.0, "svg"),
            (10.0, 60.0, "nested"),
            // The nested viewport clips its content.
            (30.0, 60.0, "svg"),
        ],
    );
}

#[test]
fn hit_testing_text_targets_the_topmost_tspan() {
    let doc = load(
        r#"<svg id="svg" width="100" height="100" font-size="40">
        <text id="text" y="50">AA<tspan id="over" x="0">BB</tspan></text>
        <text id="hidden" y="95" visibility="hidden">CC</text>
        </svg>"#,
        true,
    );
    assert_hits(&doc, &[(10.0, 40.0, "over"), (10.0, 85.0, "svg")]);
}

#[test]
fn hovering_an_svg_shape_restyles_it() {
    let mut doc = load(
        r#"<style>rect:hover { fill: lime }</style>
        <svg id="svg" width="100" height="100"><rect id="rect" width="50" height="50" fill="red"/></svg>"#,
        true,
    );
    let rect = doc.query_selector("#rect").unwrap().unwrap();
    let svg = doc.query_selector("#svg").unwrap().unwrap();
    assert_eq!(pixel(&render(&mut doc), 25, 25), RED);

    doc.set_hover_to(25.0, 25.0);
    doc.resolve(0.0);
    assert!(doc.get_node(rect).unwrap().is_hovered());
    assert!(doc.get_node(svg).unwrap().is_hovered());
    assert_eq!(pixel(&render(&mut doc), 25, 25), LIME);

    doc.set_hover_to(75.0, 75.0);
    doc.resolve(0.0);
    assert!(!doc.get_node(rect).unwrap().is_hovered());
    assert_eq!(pixel(&render(&mut doc), 25, 25), RED);
}

#[test]
fn usvg_svg_is_one_hit_target() {
    let doc = load(SQUARE, false);
    assert_hits(&doc, &[(25.0, 25.0, "icon")]);
}

#[test]
fn hit_testing_skips_nodes_removed_since_layout() {
    let mut doc = load(
        r#"<svg id="svg" width="100" height="100" font-size="20">
        <switch id="switch"><rect id="rect" width="50" height="50"/></switch>
        <text id="text" y="90">AA<tspan id="tspan">BB</tspan></text>
        </svg>"#,
        true,
    );
    let tspan_point = (0..100)
        .map(|x| (x as f32, 85.0))
        .find(|&(x, y)| element_at(&doc, x, y).as_deref() == Some("tspan"))
        .expect("a point on the tspan");
    assert_hits(&doc, &[(25.0, 25.0, "rect")]);

    // Hit testing before the next resolve sees layout that names the removed nodes.
    for id in ["#rect", "#tspan"] {
        let node = doc.query_selector(id).unwrap().unwrap();
        doc.mutate().remove_and_drop_node(node);
    }
    assert_hits(
        &doc,
        &[(25.0, 25.0, "svg"), (tspan_point.0, tspan_point.1, "svg")],
    );
}

#[test]
fn hit_testing_reaches_miter_joins_and_square_caps() {
    let doc = load(
        r#"<svg id="svg" width="100" height="100">
        <path id="miter" d="M10 80 L50 20 L90 80" fill="none" stroke="red" stroke-width="10"/>
        </svg>"#,
        true,
    );
    // The miter tip is 9px above the corner, past half the stroke width.
    assert_hits(&doc, &[(50.0, 13.0, "miter"), (50.0, 10.0, "svg")]);

    let doc = load(
        r#"<svg id="svg" width="100" height="100">
        <path id="bevel" d="M10 80 L50 20 L90 80" fill="none" stroke="red" stroke-width="10" stroke-miterlimit="1.5"/>
        </svg>"#,
        true,
    );
    assert_hits(&doc, &[(50.0, 13.0, "svg"), (50.0, 18.0, "bevel")]);

    let doc = load(
        r#"<svg id="svg" width="100" height="100">
        <path id="square" d="M30 30 L70 70" stroke="red" stroke-width="20" stroke-linecap="square"/>
        </svg>"#,
        true,
    );
    // A corner of the end cap, about 14px from the end point.
    assert_hits(&doc, &[(83.0, 70.0, "square"), (86.0, 70.0, "svg")]);
}

#[test]
fn hit_testing_finds_visible_children_of_a_hidden_svg() {
    let doc = load(
        r#"<svg id="svg" width="100" height="100" visibility="hidden">
        <rect id="visible" width="50" height="50" visibility="visible"/>
        <rect id="hidden" x="50" y="50" width="50" height="50"/>
        </svg>"#,
        true,
    );
    assert_hits(&doc, &[(25.0, 25.0, "visible"), (75.0, 75.0, "body")]);
}

#[test]
fn hit_testing_finds_content_overflowing_a_visible_svg() {
    let svg = |overflow: &str| {
        load(
            &format!(
                r#"<div id="div" style="height: 100px">
                <svg id="svg" width="50" height="50" style="display: block; overflow: {overflow}">
                <rect id="outside" x="60" y="60" width="20" height="20"/>
                </svg></div>"#
            ),
            true,
        )
    };
    assert_hits(&svg("visible"), &[(70.0, 70.0, "outside")]);
    assert_hits(&svg("hidden"), &[(70.0, 70.0, "div")]);
}

#[test]
fn clip_path_clips_to_the_union_of_its_children() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <clipPath id="c"><rect width="30" height="30"/><rect x="70" y="70" width="30" height="30"/></clipPath>
        <rect width="100" height="100" fill="red" clip-path="url(#c)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 10, 10), RED);
    assert_eq!(pixel(&buf, 90, 90), RED);
    assert_eq!(pixel(&buf, 50, 50), BLUE);
    assert_eq!(pixel(&buf, 90, 10), BLUE);
}

#[test]
fn clip_path_object_bounding_box_units_and_clip_rule() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <clipPath id="c" clipPathUnits="objectBoundingBox">
          <path clip-rule="evenodd" d="M0 0H1V1H0Z M0.25 0.25H0.75V0.75H0.25Z"/>
        </clipPath>
        <rect x="20" y="20" width="80" height="80" fill="red" clip-path="url(#c)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED);
    assert_eq!(pixel(&buf, 60, 60), BLUE, "the evenodd hole");
    assert_eq!(pixel(&buf, 10, 10), BLUE, "outside the bounding box");
}

#[test]
fn clip_path_on_a_clip_path_intersects() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <clipPath id="a"><rect width="50" height="100"/></clipPath>
        <clipPath id="b" clip-path="url(#a)"><rect width="100" height="50"/></clipPath>
        <rect width="100" height="100" fill="red" clip-path="url(#b)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED);
    assert_eq!(pixel(&buf, 75, 25), BLUE);
    assert_eq!(pixel(&buf, 25, 75), BLUE);
}

#[test]
fn recursive_clip_paths_ignore_the_cycle() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <clipPath id="c" clip-path="url(#c)"><rect width="50" height="50" clip-path="url(#c)"/></clipPath>
        <rect width="100" height="100" fill="red" clip-path="url(#c)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED);
    assert_eq!(pixel(&buf, 75, 75), BLUE);
}

#[test]
fn references_to_missing_or_other_elements_are_ignored() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <rect id="g" width="0" height="0"/>
        <rect width="50" height="50" fill="red" clip-path="url(#missing)"/>
        <rect x="50" width="50" height="50" fill="red" clip-path="url(#g)"/>
        <rect y="50" width="50" height="50" fill="red" mask="url(#missing)"/>
        <rect x="50" y="50" width="50" height="50" fill="red" mask="url(#g)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    for (x, y) in [(25, 25), (75, 25), (25, 75), (75, 75)] {
        assert_eq!(pixel(&buf, x, y), RED, "({x}, {y})");
    }
}

#[test]
fn luminance_and_alpha_masks() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <mask id="lum" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
          <rect width="50" height="50" fill="white"/>
        </mask>
        <mask id="alpha" style="mask-type: alpha">
          <rect y="50" width="50" height="50" fill="black"/>
        </mask>
        <rect width="100" height="50" fill="red" mask="url(#lum)"/>
        <rect y="50" width="100" height="50" fill="red" mask="url(#alpha)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED, "white mask content");
    assert_eq!(pixel(&buf, 75, 25), BLUE, "no mask content");
    assert_eq!(pixel(&buf, 25, 75), RED, "opaque black alpha");
    assert_eq!(pixel(&buf, 75, 75), BLUE, "transparent alpha");
}

#[test]
fn mask_content_units_and_masks_on_masks() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <mask id="inner" maskContentUnits="objectBoundingBox">
          <rect width="1" height="0.5" fill="white"/>
        </mask>
        <mask id="outer" maskContentUnits="objectBoundingBox" mask="url(#inner)">
          <rect width="0.5" height="1" fill="white"/>
        </mask>
        <rect width="100" height="100" fill="red" mask="url(#outer)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 25), RED);
    assert_eq!(pixel(&buf, 75, 25), BLUE);
    assert_eq!(pixel(&buf, 25, 75), BLUE);
}

#[test]
fn recursive_masks_ignore_the_cycle() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <mask id="m" mask="url(#m)" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
          <rect width="50" height="100" fill="white" mask="url(#m)"/>
        </mask>
        <rect width="100" height="100" fill="red" mask="url(#m)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 50), RED);
    assert_eq!(pixel(&buf, 75, 50), BLUE);
}

#[test]
fn clip_rule_comes_from_the_computed_style() {
    let mut doc = load(
        r#"<style>#c { clip-rule: evenodd }</style><svg width="100" height="100">
        <clipPath id="c"><path d="M0 0H100V100H0Z M25 25H75V75H25Z"/></clipPath>
        <rect width="100" height="100" fill="red" clip-path="url(#c)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 10, 10), RED);
    assert_eq!(pixel(&buf, 50, 50), BLUE, "the evenodd hole");
}

const BENCH_W: u32 = 1600;
const BENCH_H: u32 = 1200;

/// 100 small rects at 1600x1200, each with `attrs` and the `defs` resources.
fn clipped_rects_doc(defs: &str, attrs: &str, native_svg: bool) -> HtmlDocument {
    let mut rects = String::new();
    for i in 0..100 {
        let (x, y) = ((i % 10) * 150 + 10, (i / 10) * 110 + 10);
        rects.push_str(&format!(
            r#"<rect x="{x}" y="{y}" width="40" height="40" fill="red" {attrs}/>"#
        ));
    }
    let html = format!(
        r#"<html><body style="margin:0"><svg width="{BENCH_W}" height="{BENCH_H}"><defs>{defs}</defs>{rects}</svg></body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(BENCH_W, BENCH_H, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            native_svg: Some(native_svg),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

const BENCH_CASES: [(&str, &str, &str); 5] = [
    (
        "single-rect clipPath",
        r#"<clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="0.5" height="1"/></clipPath>"#,
        r#"clip-path="url(#c)""#,
    ),
    (
        "two-rect clipPath",
        r#"<clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="0.5" height="0.5"/><rect x="0.5" y="0.5" width="0.5" height="0.5"/></clipPath>"#,
        r#"clip-path="url(#c)""#,
    ),
    (
        "overlapping-rect clipPath",
        r#"<clipPath id="c" clipPathUnits="objectBoundingBox"><rect width="0.6" height="0.6"/><rect x="0.4" y="0.4" width="0.6" height="0.6"/></clipPath>"#,
        r#"clip-path="url(#c)""#,
    ),
    (
        "text clipPath",
        r#"<clipPath id="c" clipPathUnits="objectBoundingBox"><text y="0.8" font-size="1">A</text></clipPath>"#,
        r#"clip-path="url(#c)""#,
    ),
    (
        "mask",
        r#"<mask id="m" maskContentUnits="objectBoundingBox"><rect width="0.5" height="1" fill="white"/></mask>"#,
        r#"mask="url(#m)""#,
    ),
];

/// Clip path and mask groups are bounded by what they let through, not the
/// whole viewport, as vello_cpu composites a layer's whole area.
#[test]
fn clip_and_mask_layers_are_bounded() {
    for (name, defs, attrs) in BENCH_CASES {
        let mut doc = clipped_rects_doc(defs, attrs, true);
        let mut scene = anyrender::recording::Scene::new();
        paint_scene(&mut scene, doc.as_mut(), 1.0, BENCH_W, BENCH_H, 0, 0);
        let mut layers = 0;
        for command in &scene.commands {
            let (transform, clip) = match command {
                anyrender::recording::RenderCommand::PushLayer(l) => (l.transform, &l.clip),
                anyrender::recording::RenderCommand::PushMaskLayer(l) => (l.transform, &l.clip),
                _ => continue,
            };
            layers += 1;
            // The clip's control points in device space (no kurbo dependency
            // here, so read them back from its SVG path data).
            let [a, b, c, d, e, f] = transform.as_coeffs();
            let numbers: Vec<f64> = clip
                .to_svg()
                .split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-' || ch == 'e'))
                .filter_map(|n| n.parse().ok())
                .collect();
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for p in numbers.chunks_exact(2) {
                let (x, y) = (a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
            let bounds = (x1 - x0, y1 - y0);
            assert!(
                bounds.0 <= 200.0 && bounds.1 <= 200.0,
                "{name}: a layer covers {bounds:?}"
            );
        }
        assert!(layers > 0 || name != "mask", "{name}");
    }
}

/// Run with: `cargo test -p blitz-tests --release --test all native_svg::clip_and_mask_bench -- --ignored --nocapture`
#[test]
#[ignore]
fn clip_and_mask_bench() {
    use anyrender::ImageRenderer;
    for native_svg in [true, false] {
        for (name, defs, attrs) in BENCH_CASES {
            let mut doc = clipped_rects_doc(defs, attrs, native_svg);
            let mut renderer = VelloCpuImageRenderer::new(BENCH_W, BENCH_H);
            let mut buf = Vec::new();
            let mut frame = |doc: &mut HtmlDocument| {
                buf.clear();
                renderer.reset();
                renderer.render_to_vec(
                    |scene| paint_scene(scene, doc.as_mut(), 1.0, BENCH_W, BENCH_H, 0, 0),
                    &mut buf,
                );
            };
            frame(&mut doc);
            let frames = 10;
            let start = std::time::Instant::now();
            for _ in 0..frames {
                frame(&mut doc);
            }
            let per_frame = start.elapsed() / frames;
            let mode = if native_svg { "native" } else { "usvg" };
            println!("{mode} {name}: {per_frame:?}/frame");
        }
    }
}

#[test]
fn long_reference_chains_are_cut_off() {
    for (tag, property, content) in [
        ("clipPath", "clip-path", ""),
        ("mask", "mask", r#" fill="white""#),
    ] {
        let mut svg = String::from(r#"<svg width="100" height="100">"#);
        for i in 0..10_000 {
            let next = i + 1;
            svg += &format!(
                r#"<{tag} id="c{i}" {property}="url(#c{next})"><rect width="100" height="100"{content}/></{tag}>"#
            );
        }
        svg +=
            &format!(r#"<rect width="100" height="100" fill="red" {property}="url(#c0)"/></svg>"#);
        let mut doc = load(&svg, true);
        let buf = render(&mut doc);
        assert_eq!(pixel(&buf, 50, 50), RED, "{tag}");
    }
}

#[test]
fn branching_reference_graphs_are_bounded() {
    let depth = 20;
    let mut svg = String::from(r#"<svg width="100" height="100">"#);
    for i in 0..depth {
        let next = i + 1;
        svg += &format!(
            r#"<clipPath id="c{i}"><rect width="100" height="100" clip-path="url(#c{next})"/><rect width="100" height="100" clip-path="url(#c{next})"/></clipPath>"#
        );
    }
    svg += &format!(r#"<clipPath id="c{depth}"><rect width="100" height="100"/></clipPath>"#);
    svg += r#"<rect width="100" height="100" fill="red" clip-path="url(#c0)"/></svg>"#;
    let mut doc = load(&svg, true);
    let start = std::time::Instant::now();
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 50), RED);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

/// `<clipPath>` and `<mask>` can be referenced wherever they are, even
/// inside elements that are not rendered.
#[test]
fn resources_inside_unrendered_elements_apply() {
    let containers = [
        (r#"<g style="display:none">"#, "</g>"),
        ("<symbol>", "</symbol>"),
        ("<pattern>", "</pattern>"),
        ("<marker>", "</marker>"),
        (r#"<g systemLanguage="xx">"#, "</g>"),
    ];
    let mut svg = String::from(r#"<svg width="100" height="100">"#);
    for (i, (open, close)) in containers.iter().enumerate() {
        let y = i * 20;
        svg += &format!(
            r#"{open}<clipPath id="c{i}"><rect width="50" height="100"/></clipPath>{close}
            <rect y="{y}" width="100" height="20" fill="red" clip-path="url(#c{i})"/>"#
        );
    }
    svg += "</svg>";
    let mut doc = load(&svg, true);
    let buf = render(&mut doc);
    for i in 0..containers.len() {
        let y = i * 20 + 10;
        assert_eq!(pixel(&buf, 25, y), RED, "{}", containers[i].0);
        assert_eq!(pixel(&buf, 75, y), BLUE, "{}", containers[i].0);
    }
}

#[test]
fn masks_inside_display_none_apply() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <g style="display:none"><g><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
          <rect width="50" height="100" fill="white"/>
        </mask></g></g>
        <rect width="100" height="100" fill="red" mask="url(#m)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 50), RED);
    assert_eq!(pixel(&buf, 75, 50), BLUE);
}

/// `clip-path` and `mask` on a nested `<svg>` apply in the user space its
/// `viewBox` establishes.
#[test]
fn clip_and_mask_on_nested_svg_use_its_user_space() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <clipPath id="c"><rect width="5" height="5"/></clipPath>
        <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="5" height="10">
          <rect width="10" height="10" fill="white"/>
        </mask>
        <svg width="100" height="50" viewBox="0 0 10 10" preserveAspectRatio="none" clip-path="url(#c)">
          <rect width="10" height="10" fill="red"/>
        </svg>
        <svg y="50" width="100" height="50" viewBox="0 0 10 10" preserveAspectRatio="none" mask="url(#m)">
          <rect width="10" height="10" fill="red"/>
        </svg></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 12), RED, "inside the clip path");
    assert_eq!(pixel(&buf, 75, 12), BLUE);
    assert_eq!(pixel(&buf, 25, 37), BLUE);
    assert_eq!(pixel(&buf, 25, 75), RED, "inside the mask region");
    assert_eq!(pixel(&buf, 75, 75), BLUE);
}

/// A reference that closes a cycle is ignored for the whole document (as
/// usvg does), not only where it re-enters a mask being applied: with
/// `mask2`'s own `mask` ignored, each mask applies once.
#[test]
fn circular_mask_references_are_ignored_everywhere() {
    let mut doc = load(
        r#"<svg width="100" height="100">
        <mask id="self1" mask="url(#self2)" style="mask-type: alpha"><rect width="100" height="100" fill-opacity="0.5"/></mask>
        <mask id="self2" mask="url(#self1)" style="mask-type: alpha"><rect width="100" height="100" fill-opacity="0.5"/></mask>
        <mask id="child1" style="mask-type: alpha"><rect width="100" height="100" fill-opacity="0.5" mask="url(#child2)"/></mask>
        <mask id="child2" style="mask-type: alpha"><rect width="100" height="100" fill-opacity="0.5" mask="url(#child1)"/></mask>
        <rect width="50" height="100" fill="red" mask="url(#self2)"/>
        <rect x="50" width="50" height="100" fill="red" mask="url(#child2)"/></svg>"#,
        true,
    );
    let buf = render(&mut doc);
    assert_near(pixel(&buf, 25, 50), [128, 0, 128]);
    assert_near(pixel(&buf, 75, 50), [128, 0, 128]);
}

/// A 2x1 opaque lime PNG.
const LIME_2X1_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 244, 34, 127, 138, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 96, 248, 15, 129, 0, 15,
    249, 3, 253, 216, 57, 113, 65, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// A 1x1 opaque white PNG.
const WHITE_1X1_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 120, 156, 99, 248, 15, 4, 0, 9, 251, 3,
    253, 251, 94, 107, 43, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// Serves every request with the same bytes.
struct StaticNetProvider(&'static [u8]);

impl blitz_traits::net::NetProvider for StaticNetProvider {
    fn fetch(
        &self,
        _doc_id: usize,
        request: blitz_traits::net::Request,
        handler: Box<dyn blitz_traits::net::NetHandler>,
    ) {
        handler.bytes(request.url.to_string(), self.0.into());
    }
}

fn load_with_image(body: &str, image: &'static [u8]) -> HtmlDocument {
    let html = format!(r#"<html><body style="margin:0; background:#00f">{body}</body></html>"#);
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some("http://example.com/".to_string()),
            viewport: Some(Viewport::new(W, H, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            net_provider: Some(Arc::new(StaticNetProvider(image))),
            native_svg: Some(true),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

#[test]
fn image_paints_its_raster_into_its_viewport() {
    let mut doc = load_with_image(
        r#"<svg width="100" height="100"><image href="a.png" width="100" height="100" preserveAspectRatio="none"/></svg>"#,
        LIME_2X1_PNG,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 5, 5), LIME);
    assert_eq!(pixel(&buf, 95, 95), LIME);
}

#[test]
fn image_preserves_its_aspect_ratio() {
    let mut doc = load_with_image(
        r#"<svg width="100" height="100"><image href="a.png" width="100" height="100"/></svg>"#,
        LIME_2X1_PNG,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 50, 10), BLUE);
    assert_eq!(pixel(&buf, 50, 50), LIME);
    assert_eq!(pixel(&buf, 50, 90), BLUE);
}

#[test]
fn image_without_a_size_uses_its_intrinsic_size() {
    let mut doc = load_with_image(
        r#"<svg width="100" height="100"><image x="10" y="10" xlink:href="a.png" transform="scale(2)"/></svg>"#,
        LIME_2X1_PNG,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 21, 20), LIME);
    assert_eq!(pixel(&buf, 25, 20), BLUE);
    assert_eq!(pixel(&buf, 21, 23), BLUE);
}

#[test]
fn image_inside_a_mask_reveals_masked_content() {
    let mut doc = load_with_image(
        r#"<svg width="100" height="100">
            <mask id="m"><g clip-path="url(#c)"><image href="a.png" width="100" height="100"/></g></mask>
            <clipPath id="c"><rect width="50" height="100"/></clipPath>
            <rect width="100" height="100" fill="lime" mask="url(#m)"/>
        </svg>"#,
        WHITE_1X1_PNG,
    );
    let buf = render(&mut doc);
    assert_eq!(pixel(&buf, 25, 50), LIME);
    assert_eq!(pixel(&buf, 75, 50), BLUE);
}

#[test]
fn image_href_change_loads_the_new_image() {
    let mut doc = load_with_image(
        r#"<svg width="100" height="100"><image id="i" width="100" height="100" preserveAspectRatio="none"/></svg>"#,
        LIME_2X1_PNG,
    );
    assert_eq!(pixel(&render(&mut doc), 50, 50), BLUE);
    let id = doc.get_element_by_id("i").unwrap();
    doc.mutate().set_attribute(id, attr("href"), "b.png");
    doc.resolve(0.0);
    assert_eq!(pixel(&render(&mut doc), 50, 50), LIME);
}

/// Records every request and serves the named files, holding the responses
/// until `flush` when `hold` is set.
struct RecordingNet {
    files: Vec<(&'static str, &'static [u8])>,
    hold: bool,
    pending: std::sync::Mutex<Vec<(String, Box<dyn blitz_traits::net::NetHandler>)>>,
    urls: std::sync::Mutex<Vec<String>>,
}

impl RecordingNet {
    fn new(files: &[(&'static str, &'static [u8])], hold: bool) -> Arc<Self> {
        Arc::new(Self {
            files: files.to_vec(),
            hold,
            pending: Default::default(),
            urls: Default::default(),
        })
    }

    fn deliver(&self, url: String, handler: Box<dyn blitz_traits::net::NetHandler>) {
        let name = url.rsplit('/').next().unwrap();
        match self.files.iter().find(|(file, _)| *file == name) {
            Some((_, bytes)) => handler.bytes(url, bytes.to_vec().into()),
            None => handler.error(url, "404".into()),
        }
    }

    fn flush(&self) {
        for (url, handler) in std::mem::take(&mut *self.pending.lock().unwrap()) {
            self.deliver(url, handler);
        }
    }

    fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }
}

struct RecordingNetProvider(Arc<RecordingNet>);

impl blitz_traits::net::NetProvider for RecordingNetProvider {
    fn fetch(
        &self,
        _doc_id: usize,
        request: blitz_traits::net::Request,
        handler: Box<dyn blitz_traits::net::NetHandler>,
    ) {
        let url = request.url.to_string();
        self.0.urls.lock().unwrap().push(url.clone());
        if self.0.hold {
            self.0.pending.lock().unwrap().push((url, handler));
        } else {
            self.0.deliver(url, handler);
        }
    }
}

fn load_with_net(body: &str, net: &Arc<RecordingNet>) -> HtmlDocument {
    let html = format!(r#"<html><body style="margin:0; background:#00f">{body}</body></html>"#);
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some("http://example.com/".to_string()),
            viewport: Some(Viewport::new(W, H, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            net_provider: Some(Arc::new(RecordingNetProvider(net.clone()))),
            native_svg: Some(true),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

#[test]
fn image_paints_once_it_arrives() {
    let net = RecordingNet::new(&[("a.png", LIME_2X1_PNG)], true);
    let mut doc = load_with_net(
        r#"<svg width="100" height="100"><image href="a.png" width="100" height="100" preserveAspectRatio="none"/></svg>"#,
        &net,
    );
    assert_eq!(pixel(&render(&mut doc), 50, 50), BLUE);
    net.flush();
    doc.resolve(0.0);
    assert_eq!(pixel(&render(&mut doc), 50, 50), LIME);
}

#[test]
fn late_image_in_a_mask_inside_display_none_paints() {
    let net = RecordingNet::new(&[("w.png", WHITE_1X1_PNG)], true);
    let mut doc = load_with_net(
        r#"<svg width="100" height="100">
            <g style="display:none"><mask id="m"><image href="w.png" width="100" height="100"/></mask></g>
            <rect width="100" height="100" fill="lime" mask="url(#m)"/>
        </svg>"#,
        &net,
    );
    net.flush();
    doc.resolve(0.0);
    assert_eq!(pixel(&render(&mut doc), 50, 50), LIME);
}

#[test]
fn removing_href_falls_back_to_xlink_href() {
    let net = RecordingNet::new(&[("a.png", LIME_2X1_PNG), ("w.png", WHITE_1X1_PNG)], false);
    let mut doc = load_with_net(
        r#"<svg width="100" height="100"><image id="i" href="a.png" xlink:href="w.png" width="100" height="100" preserveAspectRatio="none"/></svg>"#,
        &net,
    );
    assert_eq!(pixel(&render(&mut doc), 50, 50), LIME);
    let id = doc.get_element_by_id("i").unwrap();
    doc.mutate().clear_attribute(id, attr("href"));
    doc.resolve(0.0);
    assert_eq!(pixel(&render(&mut doc), 50, 50), [255, 255, 255]);
}

#[test]
fn removing_the_only_href_removes_the_image() {
    let net = RecordingNet::new(&[("a.png", LIME_2X1_PNG)], false);
    let mut doc = load_with_net(
        r#"<svg width="100" height="100"><image id="i" href="a.png" width="100" height="100" preserveAspectRatio="none"/></svg>"#,
        &net,
    );
    let id = doc.get_element_by_id("i").unwrap();
    doc.mutate().clear_attribute(id, attr("href"));
    doc.resolve(0.0);
    assert_eq!(pixel(&render(&mut doc), 50, 50), BLUE);
}

#[test]
fn image_ignores_a_fragment_href_and_fetches_each_url_once() {
    let net = RecordingNet::new(&[("a.png", LIME_2X1_PNG)], false);
    load_with_net(
        r##"<svg width="100" height="100"><image href="#x" width="10" height="10"/><image href="" width="10" height="10"/><image href="a.png" width="10" height="10"/><image href="a.png" width="10" height="10"/></svg>"##,
        &net,
    );
    assert_eq!(net.urls(), ["http://example.com/a.png"]);
}

/// A 50x50 SVG image with lime inside its viewport and red outside it.
const OVERFLOWING_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50"><rect width="50" height="50" fill="lime"/><rect x="50" width="50" height="100" fill="red"/></svg>"#;

#[test]
fn svg_image_is_clipped_to_its_own_viewport() {
    for overflow in ["hidden", "visible"] {
        let net = RecordingNet::new(&[("s.svg", OVERFLOWING_SVG)], false);
        let mut doc = load_with_net(
            &format!(
                r#"<svg width="100" height="100"><image href="s.svg" width="50" height="50" style="overflow:{overflow}"/></svg>"#
            ),
            &net,
        );
        let buf = render(&mut doc);
        assert_eq!(pixel(&buf, 25, 25), LIME, "overflow: {overflow}");
        assert_eq!(pixel(&buf, 75, 25), BLUE, "overflow: {overflow}");
    }
}

#[test]
fn sliced_image_is_clipped_unless_overflow_is_visible() {
    for (overflow, outside) in [("hidden", BLUE), ("visible", LIME)] {
        let net = RecordingNet::new(&[("a.png", LIME_2X1_PNG)], false);
        let mut doc = load_with_net(
            &format!(
                r#"<svg width="100" height="100"><image href="a.png" x="25" y="25" width="50" height="50" preserveAspectRatio="xMidYMid slice" style="overflow:{overflow}"/></svg>"#
            ),
            &net,
        );
        let buf = render(&mut doc);
        assert_eq!(pixel(&buf, 50, 50), LIME, "overflow: {overflow}");
        assert_eq!(pixel(&buf, 10, 50), outside, "overflow: {overflow}");
    }
}

#[test]
fn fitted_raster_image_needs_no_clip() {
    let net = RecordingNet::new(&[("a.png", LIME_2X1_PNG)], false);
    let doc = load_with_net(
        r#"<svg width="100" height="100"><image id="i" href="a.png" width="100" height="100"/></svg>"#,
        &net,
    );
    let id = doc.get_element_by_id("i").unwrap();
    let layout = doc
        .get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .svg_layout()
        .unwrap();
    assert!(matches!(
        layout.kind,
        blitz_dom::svg::SvgLayoutKind::Image { clip: None, .. }
    ));
}

/// The bounding box `(x0, y0, x1, y1)` of the pixels that aren't background.
fn ink_box(buffer: &[u8]) -> Option<(usize, usize, usize, usize)> {
    let mut ink: Option<(usize, usize, usize, usize)> = None;
    for y in 0..H as usize {
        for x in 0..W as usize {
            if pixel(buffer, x, y) != BLUE {
                ink = Some(ink.map_or((x, y, x, y), |(x0, y0, x1, y1)| {
                    (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
                }));
            }
        }
    }
    ink
}

/// Asserts `grouped` paints as far as `ungrouped`, allowing a pixel of faint
/// antialiasing lost to the group's opacity.
fn assert_not_clipped(grouped: &str, ungrouped: &str) {
    let grouped = ink_box(&render(&mut load(grouped, true))).expect("grouped ink");
    let ungrouped = ink_box(&render(&mut load(ungrouped, true))).expect("ungrouped ink");
    assert!(
        grouped.0 <= ungrouped.0 + 1
            && grouped.1 <= ungrouped.1 + 1
            && grouped.2 + 1 >= ungrouped.2
            && grouped.3 + 1 >= ungrouped.3,
        "group layer clipped its content: {grouped:?} vs {ungrouped:?}"
    );
}

/// An `a` with 14 stacked acute accents, reaching far above its line.
const STACKED_MARKS: &str = "a\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}";

#[test]
fn group_opacity_does_not_clip_stacked_combining_marks() {
    let svg = |opacity: &str| {
        format!(
            r#"<svg width="100" height="100"><g opacity="{opacity}"><text x="10" y="95" font-size="20" fill="red">{STACKED_MARKS}</text></g></svg>"#
        )
    };
    assert_not_clipped(&svg("0.99"), &svg("1"));
}

#[test]
fn group_opacity_does_not_clip_a_wide_text_stroke() {
    let svg = |opacity: &str| {
        format!(
            r#"<svg width="100" height="100"><text opacity="{opacity}" x="40" y="55" font-size="20" font-style="italic" stroke="red" stroke-width="60" stroke-linejoin="round" fill="none">jf</text></svg>"#
        )
    };
    assert_not_clipped(&svg("0.99"), &svg("1"));
}

#[test]
fn tspan_opacity_does_not_clip_stacked_combining_marks() {
    let svg = |opacity: &str| {
        format!(
            r#"<svg width="100" height="100"><text x="10" y="95" font-size="12" fill="red"><tspan opacity="{opacity}">{STACKED_MARKS}</tspan></text></svg>"#
        )
    };
    assert_not_clipped(&svg("0.99"), &svg("1"));
}

#[test]
fn root_clip_still_clips_text_overflowing_the_svg() {
    let mut doc = load(
        &format!(
            r#"<div style="height:50px"></div><svg width="100" height="50" style="display:block"><text x="40" y="30" font-size="12" fill="red">{STACKED_MARKS}</text></svg>"#
        ),
        true,
    );
    let buf = render(&mut doc);
    let (_, top, ..) = ink_box(&buf).expect("text ink");
    assert!(top >= 50, "text painted above the svg box, from y = {top}");
}

/// Paints `doc` into a recording and counts the layers pushed.
fn layer_count(doc: &mut HtmlDocument) -> usize {
    use anyrender::recording::RenderCommand;
    let mut scene = anyrender::Scene::new();
    paint_scene(&mut scene, doc.as_mut(), 1.0, W, H, 0, 0);
    scene
        .commands
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_)
            )
        })
        .count()
}

#[test]
fn root_clip_is_skipped_only_when_content_fits() {
    let fits = layer_count(&mut load(SQUARE, true));
    let overflows = layer_count(&mut load(
        r#"<svg width="50" height="50"><rect width="100" height="50" fill="red"/></svg>"#,
        true,
    ));
    let text_fits = layer_count(&mut load(
        &format!(
            r#"<svg width="100" height="100"><text x="40" y="95" font-size="20" fill="red">{STACKED_MARKS}</text></svg>"#
        ),
        true,
    ));
    assert_eq!(fits, 0, "content inside the svg box needs no clip");
    assert_eq!(text_fits, 0, "text inside the svg box needs no clip");
    assert_eq!(overflows, 1, "overflowing content needs the root clip");
}

#[test]
fn group_layers_are_bounded_by_their_content() {
    use anyrender::recording::RenderCommand;
    use kurbo::{Rect, Shape};
    for (svg, content) in [
        (
            r#"<svg width="100" height="100"><g opacity="0.5"><rect x="10" y="20" width="30" height="40" fill="red"/></g></svg>"#,
            Rect::new(10.0, 20.0, 40.0, 60.0),
        ),
        (
            r#"<svg width="100" height="100"><g opacity="0.5"><rect x="10" y="20" width="20" height="30" fill="red" stroke="red" stroke-width="10" stroke-linejoin="round"/></g></svg>"#,
            Rect::new(5.0, 15.0, 35.0, 55.0),
        ),
    ] {
        let mut doc = load(svg, true);
        let mut scene = anyrender::Scene::new();
        paint_scene(&mut scene, doc.as_mut(), 1.0, W, H, 0, 0);
        let layers: Vec<_> = scene
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::PushLayer(layer) => Some(
                    layer
                        .transform
                        .transform_rect_bbox(layer.clip.bounding_box()),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(layers.len(), 1, "{svg}");
        let bounds = layers[0];
        assert!(
            bounds.contains_rect(content),
            "layer {bounds:?} clips its content in {svg}"
        );
        assert!(
            content.inflate(2.0, 2.0).contains_rect(bounds),
            "layer {bounds:?} is wider than its content in {svg}"
        );
    }
}
