//! Paint a first-party inline `<svg>` fragment.

use anyrender::PaintScene;
use blitz_dom::svg::SvgNodeKind;
use kurbo::{Affine, Shape, Stroke};
use peniko::{Color, Fill};

use crate::color::ToColorColor;

use super::ElementCx;

impl ElementCx<'_, '_> {
    pub(super) fn draw_svg_fragment(&self, scene: &mut impl PaintScene) {
        let Some(ctx) = self.svg_root else {
            return;
        };

        let base = self.transform * Affine::scale(self.scale);
        let doc = self.node;

        for node in ctx.nodes.iter() {
            // Group/Use/Image are geometry-less containers; nothing to paint for the container itself,
            // so skip straight to the next node rather than opening a layer around an empty paint.
            let (SvgNodeKind::Shape(_) | SvgNodeKind::Text(_) | SvgNodeKind::ForeignObject) =
                &node.kind
            else {
                continue;
            };

            let dom_node = doc.with(node.dom_id);
            let ctm = base * node.ctm;

            let Some(style) = dom_node.primary_styles() else {
                continue;
            };
            use style::properties::generated::longhands::visibility::computed_value::T as StyloVisibility;
            if style.get_inherited_box().visibility != StyloVisibility::Visible {
                continue;
            }

            // CSS `opacity` does not inherit, but a group's opacity must still visually apply to everything
            // painted inside it. Since painting is a flat linear scan with no nested layer stack, approximate
            // group compositing by multiplying this leaf's own opacity by every ancestor `SvgNode`'s opacity
            // along the `parent` chain.
            let opacity = style.get_effects().opacity * self.ancestor_opacity(ctx, node.parent);
            if opacity <= 0.0 {
                continue;
            }

            let attrs = dom_node.attrs().unwrap_or(&[]);
            let current_color = resolve_current_color(&style);

            let needs_layer = opacity < 1.0;
            if needs_layer {
                let bbox = ctm * node.bbox.to_path(0.1);
                scene.push_layer(
                    peniko::Mix::Normal,
                    opacity,
                    Affine::IDENTITY,
                    &bbox,
                    None,
                    None,
                );
            }

            match &node.kind {
                SvgNodeKind::Shape(path) => {
                    paint_shape(scene, ctm, path, attrs, current_color);
                }
                SvgNodeKind::Text(run) => {
                    self.draw_svg_text(scene, ctm, run, current_color);
                }
                SvgNodeKind::ForeignObject => {
                    let unbounded = kurbo::Rect::new(f64::MIN, f64::MIN, f64::MAX, f64::MAX);
                    for &child in dom_node.children.iter() {
                        self.context.render_node(scene, child, ctm, unbounded);
                    }
                }
                SvgNodeKind::Group | SvgNodeKind::Use { .. } | SvgNodeKind::Image => {
                    unreachable!("filtered out above")
                }
            }

            if needs_layer {
                scene.pop_layer();
            }
        }
    }

    /// Product of `opacity` for every ancestor `SvgNode` above `parent` in the flat node list.
    /// Elements with no computed style.
    fn ancestor_opacity(&self, ctx: &blitz_dom::svg::SvgContext, parent: Option<u32>) -> f32 {
        let mut opacity = 1.0f32;
        let mut cur = parent;
        while let Some(i) = cur {
            let ancestor = &ctx.nodes[i as usize];
            if let Some(style) = self.node.with(ancestor.dom_id).primary_styles() {
                opacity *= style.get_effects().opacity;
            }
            cur = ancestor.parent;
        }
        opacity
    }

    fn draw_svg_text(
        &self,
        scene: &mut impl PaintScene,
        ctm: Affine,
        run: &blitz_dom::svg::TextRun,
        current_color: Color,
    ) {
        use blitz_dom::svg::text::TextAnchor;
        let full_width = run.layout.full_width();
        let anchor_dx = match run.anchor {
            TextAnchor::Start => 0.0,
            TextAnchor::Middle => -full_width / 2.0,
            TextAnchor::End => -full_width,
        } as f64;
        let text_transform = ctm * Affine::translate((anchor_dx, 0.0));

        for line in run.layout.lines() {
            for item in line.items() {
                let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run_ref = glyph_run.run();
                let font = run_ref.font();
                let font_size = run_ref.font_size();
                let glyph_xform: Option<kurbo::Affine> = None;
                let coords = run_ref.normalized_coords();

                let mut x = glyph_run.offset() as f64;
                let y = glyph_run.baseline() as f64;
                let glyphs = glyph_run.glyphs().map(move |g| {
                    let gx = x + g.x as f64;
                    x += g.advance as f64;
                    anyrender::Glyph {
                        id: g.id,
                        x: gx as f32,
                        y: y as f32,
                    }
                });

                scene.draw_glyphs(
                    font,
                    font_size,
                    true,
                    coords,
                    kurbo::Vec2::ZERO,
                    Fill::NonZero,
                    current_color,
                    1.0,
                    text_transform,
                    glyph_xform,
                    glyphs,
                );
            }
        }
    }
}

fn paint_shape(
    scene: &mut impl PaintScene,
    ctm: Affine,
    path: &kurbo::BezPath,
    attrs: &[blitz_dom::node::Attribute],
    current_color: Color,
) {
    use blitz_dom::svg::attrs::raw_attr;

    let fill = raw_attr(attrs, "fill");
    let fill_rule = match raw_attr(attrs, "fill-rule") {
        Some("evenodd") => Fill::EvenOdd,
        _ => Fill::NonZero,
    };
    let fill_opacity = raw_attr(attrs, "fill-opacity")
        .and_then(parse_opacity)
        .unwrap_or(1.0);

    // Unset `fill` defaults to black (SVG initial value); `fill: none` paints nothing.
    if fill != Some("none") {
        if let Some(mut color) = parse_paint(fill, current_color) {
            color = color.multiply_alpha(fill_opacity);
            scene.fill(fill_rule, ctm, color, None, path);
        }
    }

    let stroke = raw_attr(attrs, "stroke");
    if let Some(mut color) = stroke.and_then(|s| parse_paint(Some(s), current_color)) {
        let stroke_opacity = raw_attr(attrs, "stroke-opacity")
            .and_then(parse_opacity)
            .unwrap_or(1.0);
        let width = raw_attr(attrs, "stroke-width")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(1.0);
        // Stroke-width 0 skips the stroke phase entirely.
        if width > 0.0 {
            color = color.multiply_alpha(stroke_opacity);
            let stroke_style = Stroke::new(width);
            scene.stroke(&stroke_style, ctm, color, None, path);
        }
    }
}

/// Resolve `fill`/`stroke` presentation-attribute *paint* values that this pass supports: `none` (caller-handled),
/// a solid color keyword/hex/rgb(), or `currentColor`. `url(#id)` paint-server references are not resolved here yet,
/// degrades to `None` (phase skipped), which is the "unresolvable -> fallback colour if given, else none" behaviour,
/// minus the fallback-color-after-`url()` parsing (`fill="url(#g) red"`) which is also not implemented yet.
fn parse_paint(value: Option<&str>, current_color: Color) -> Option<Color> {
    let value = value?.trim();
    if value.is_empty() || value == "none" {
        return None;
    }
    if value.starts_with("url(") {
        return None;
    }
    parse_css_color(value, current_color)
}

fn parse_opacity(value: &str) -> Option<f32> {
    let value = value.trim();
    if let Some(pct) = value.strip_suffix('%') {
        return pct
            .trim()
            .parse::<f32>()
            .ok()
            .map(|p| (p / 100.0).clamp(0.0, 1.0));
    }
    value.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
}

/// Resolve the SVG paint value `currentColor` against the element's computed CSS `color`.
fn resolve_current_color(style: &style::properties::ComputedValues) -> Color {
    style.get_inherited_text().color.as_srgb_color()
}

/// Minimal CSS `<color>` parser covering the SVG-in-the-wild common cases:
/// `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()`, `currentColor`, and a handful of named colors.
/// Full CSS color syntax is handled by the presentation-attribute cascade path (`svg/attrs.rs`)
/// for properties that go through it, this is the raw-attribute fallback used directly by shape painting.
fn parse_css_color(value: &str, current_color: Color) -> Option<Color> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("currentcolor") {
        return Some(current_color);
    }
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(inner) = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
    {
        let inner = inner.strip_suffix(')')?;
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() < 3 {
            return None;
        }
        let component = |s: &str| -> Option<f32> {
            if let Some(pct) = s.strip_suffix('%') {
                Some((pct.trim().parse::<f32>().ok()? / 100.0).clamp(0.0, 1.0))
            } else {
                Some((s.parse::<f32>().ok()? / 255.0).clamp(0.0, 1.0))
            }
        };
        let r = component(parts[0])?;
        let g = component(parts[1])?;
        let b = component(parts[2])?;
        let a = parts
            .get(3)
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        return Some(Color::new([r, g, b, a]));
    }
    named_color(value)
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    let to_f = |b: u8| b as f32 / 255.0;
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            Some(Color::new([to_f(r), to_f(g), to_f(b), 1.0]))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::new([to_f(r), to_f(g), to_f(b), 1.0]))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color::new([to_f(r), to_f(g), to_f(b), to_f(a)]))
        }
        _ => None,
    }
}

fn named_color(name: &str) -> Option<Color> {
    let rgb = match name.to_ascii_lowercase().as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "gray" | "grey" => (128, 128, 128),
        "silver" => (192, 192, 192),
        "maroon" => (128, 0, 0),
        "navy" => (0, 0, 128),
        "teal" => (0, 128, 128),
        "olive" => (128, 128, 0),
        "lime" => (0, 255, 0),
        "aqua" | "cyan" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "pink" => (255, 192, 203),
        "brown" => (165, 42, 42),
        "transparent" => return Some(Color::new([0.0, 0.0, 0.0, 0.0])),
        _ => return None,
    };
    Some(Color::new([
        rgb.0 as f32 / 255.0,
        rgb.1 as f32 / 255.0,
        rgb.2 as f32 / 255.0,
        1.0,
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Color = Color::new([0.0, 0.0, 0.0, 1.0]);

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(
            parse_hex_color("f00"),
            Some(Color::new([1.0, 0.0, 0.0, 1.0]))
        );
        assert_eq!(
            parse_hex_color("ff0000"),
            Some(Color::new([1.0, 0.0, 0.0, 1.0]))
        );
    }

    #[test]
    fn parses_named_colors() {
        assert_eq!(
            parse_css_color("red", BLACK),
            Some(Color::new([1.0, 0.0, 0.0, 1.0]))
        );
        assert_eq!(parse_css_color("black", BLACK), Some(BLACK));
    }

    #[test]
    fn resolves_current_color_keyword() {
        let cc = Color::new([0.2, 0.4, 0.6, 1.0]);
        assert_eq!(parse_css_color("currentColor", cc), Some(cc));
    }

    #[test]
    fn rejects_url_references() {
        assert_eq!(parse_paint(Some("url(#grad)"), BLACK), None);
    }

    #[test]
    fn none_and_unset_paint_are_no_paint() {
        assert_eq!(parse_paint(Some("none"), BLACK), None);
        assert_eq!(parse_paint(None, BLACK), None);
    }

    #[test]
    fn parses_rgb_function() {
        assert_eq!(
            parse_css_color("rgb(255, 0, 0)", BLACK),
            Some(Color::new([1.0, 0.0, 0.0, 1.0]))
        );
    }

    #[test]
    fn opacity_parses_percent_and_clamps() {
        assert_eq!(parse_opacity("50%"), Some(0.5));
        assert_eq!(parse_opacity("2"), Some(1.0));
        assert_eq!(parse_opacity("-1"), Some(0.0));
    }
}
