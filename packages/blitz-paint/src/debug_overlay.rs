use anyrender::PaintScene;
use blitz_dom::BaseDocument;
use kurbo::{Affine, Rect, Vec2};

use crate::color::Color;

/// Renders a layout debugging overlay which visualises the content size, padding and border
/// of the node with a transparent overlay.
pub(crate) fn render_debug_overlay(
    scene: &mut impl PaintScene,
    dom: &BaseDocument,
    node_id: usize,
    scale: f64,
) {
    let viewport_scroll = dom.as_ref().viewport_scroll();
    let mut node = &dom.as_ref().tree()[node_id];

    let taffy::Layout {
        size,
        border,
        padding,
        margin,
        ..
    } = node.final_layout;
    let taffy::Size { width, height } = size;

    let padding_border = padding + border;
    let scaled_pb = padding_border.map(|v| f64::from(v) * scale);
    let scaled_padding = padding.map(|v| f64::from(v) * scale);
    let scaled_border = border.map(|v| f64::from(v) * scale);
    let scaled_margin = margin.map(|v| f64::from(v) * scale);

    let content_width = width - padding_border.left - padding_border.right;
    let content_height = height - padding_border.top - padding_border.bottom;

    let taffy::Point { x, y } = node.final_layout.location;

    let mut abs_x = x;
    let mut abs_y = y;
    while let Some(parent_id) = node.layout_parent.get() {
        node = &dom.as_ref().tree()[parent_id];
        let taffy::Point { x, y } = node.final_layout.location;
        abs_x += x;
        abs_y += y;
    }

    abs_x -= viewport_scroll.x as f32;
    abs_y -= viewport_scroll.y as f32;

    // Hack: scale factor
    let abs_x = f64::from(abs_x) * scale;
    let abs_y = f64::from(abs_y) * scale;
    let width = f64::from(width) * scale;
    let height = f64::from(height) * scale;
    let content_width = f64::from(content_width) * scale;
    let content_height = f64::from(content_height) * scale;

    // Fill content box blue
    let base_translation = Vec2::new(abs_x, abs_y);
    let transform = Affine::translate(base_translation + Vec2::new(scaled_pb.left, scaled_pb.top));
    let rect = Rect::new(0.0, 0.0, content_width, content_height);
    let fill_color = Color::from_rgba8(66, 144, 245, 128); // blue
    scene.fill(peniko::Fill::NonZero, transform, fill_color, None, &rect);

    let padding_color = Color::from_rgba8(81, 144, 66, 128); // green
    draw_cutout_rect(
        scene,
        base_translation + Vec2::new(scaled_border.left, scaled_border.top),
        Vec2::new(
            content_width + scaled_padding.left + scaled_padding.right,
            content_height + scaled_padding.top + scaled_padding.bottom,
        ),
        scaled_padding.map(f64::from),
        padding_color,
    );

    let border_color = Color::from_rgba8(245, 66, 66, 128); // red
    draw_cutout_rect(
        scene,
        base_translation,
        Vec2::new(width, height),
        scaled_border.map(f64::from),
        border_color,
    );

    let margin_color = Color::from_rgba8(249, 204, 157, 128); // orange
    draw_cutout_rect(
        scene,
        base_translation - Vec2::new(scaled_margin.left, scaled_margin.top),
        Vec2::new(
            width + scaled_margin.left + scaled_margin.right,
            height + scaled_margin.top + scaled_margin.bottom,
        ),
        scaled_margin.map(f64::from),
        margin_color,
    );
}

fn draw_cutout_rect(
    scene: &mut impl PaintScene,
    base_translation: Vec2,
    size: Vec2,
    edge_widths: taffy::Rect<f64>,
    color: Color,
) {
    let mut fill = |pos: Vec2, width: f64, height: f64| {
        scene.fill(
            peniko::Fill::NonZero,
            Affine::translate(pos),
            color,
            None,
            &Rect::new(0.0, 0.0, width, height),
        );
    };

    let right = size.x - edge_widths.right;
    let bottom = size.y - edge_widths.bottom;
    let inner_h = size.y - edge_widths.top - edge_widths.bottom;
    let inner_w = size.x - edge_widths.left - edge_widths.right;

    let bt = base_translation;
    let ew = edge_widths;

    // Corners
    fill(bt, ew.left, ew.top); // top-left
    fill(bt + Vec2::new(0.0, bottom), ew.left, ew.bottom); // bottom-left
    fill(bt + Vec2::new(right, 0.0), ew.right, ew.top); // top-right
    fill(bt + Vec2::new(right, bottom), ew.right, ew.bottom); // bottom-right

    // Sides
    fill(bt + Vec2::new(0.0, ew.top), ew.left, inner_h); // left
    fill(bt + Vec2::new(right, ew.top), ew.right, inner_h); // right
    fill(bt + Vec2::new(ew.left, 0.0), inner_w, ew.top); // top
    fill(bt + Vec2::new(ew.left, bottom), inner_w, ew.bottom); // bottom
}
