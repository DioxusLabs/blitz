//! Paint-command inspection: record the document's paint into a command list for
//! assertions like "this element painted a red rect at this position", which are much
//! more robust than pixel comparison for many paint bugs.

use anyrender::Paint;
use anyrender::recording::{RenderCommand, Scene};
use blitz_dom::Document;
use blitz_paint::paint_scene;
use peniko::kurbo::Shape;

use crate::Harness;

fn brush_string(brush: &Paint) -> String {
    match brush {
        Paint::Solid(color) => {
            let rgba = color.to_rgba8();
            format!("rgba({},{},{},{})", rgba.r, rgba.g, rgba.b, rgba.a)
        }
        Paint::Gradient(_) => "gradient".to_string(),
        Paint::Image(_) | Paint::Resource(_) => "image".to_string(),
        Paint::Custom(_) => "custom".to_string(),
    }
}

fn rect_string(rect: peniko::kurbo::Rect) -> String {
    format!(
        "({:.1},{:.1}) {:.1}x{:.1}",
        rect.x0,
        rect.y0,
        rect.width(),
        rect.height()
    )
}

/// One line of text per command: command kind, device-space bounding box, and brush
pub fn paint_command_string(command: &RenderCommand) -> String {
    match command {
        RenderCommand::PushLayer(cmd) => {
            let bbox = cmd.transform.transform_rect_bbox(cmd.clip.bounding_box());
            format!("push_layer clip={} alpha={}", rect_string(bbox), cmd.alpha)
        }
        RenderCommand::PushClipLayer(cmd) => {
            let bbox = cmd.transform.transform_rect_bbox(cmd.clip.bounding_box());
            format!("push_clip clip={}", rect_string(bbox))
        }
        RenderCommand::PopLayer => "pop_layer".to_string(),
        RenderCommand::Stroke(cmd) => {
            let bbox = cmd.transform.transform_rect_bbox(cmd.shape.bounding_box());
            format!(
                "stroke {} width={} brush={}",
                rect_string(bbox),
                cmd.style.width,
                brush_string(&cmd.brush)
            )
        }
        RenderCommand::Fill(cmd) => {
            let bbox = cmd.transform.transform_rect_bbox(cmd.shape.bounding_box());
            format!(
                "fill {} brush={}",
                rect_string(bbox),
                brush_string(&cmd.brush)
            )
        }
        RenderCommand::GlyphRun(cmd) => {
            format!(
                "glyph_run glyphs={} size={} brush={}",
                cmd.glyphs.len(),
                cmd.font_size,
                brush_string(&cmd.brush)
            )
        }
        RenderCommand::BoxShadow(cmd) => {
            let bbox = cmd.transform.transform_rect_bbox(cmd.rect);
            let rgba = cmd.brush.to_rgba8();
            format!(
                "box_shadow {} radius={} std_dev={} color=rgba({},{},{},{})",
                rect_string(bbox),
                cmd.radius,
                cmd.std_dev,
                rgba.r,
                rgba.g,
                rgba.b,
                rgba.a
            )
        }
    }
}

impl<D: Document> Harness<D> {
    /// Paint the document into a recorded [`Scene`], returning the full command list
    /// (`scene.commands`) for structural assertions on painting.
    pub fn record_paint(&mut self) -> Scene {
        self.pump();
        let mut doc = self.doc.inner_mut();
        let (width, height) = doc.get_viewport().window_size;
        let scale = doc.get_viewport().scale_f64();
        let mut scene = Scene::new();
        paint_scene(&mut scene, &mut doc, scale, width, height, 0, 0);
        scene
    }

    /// A text serialization of the document's paint commands (one per line):
    /// command kind, device-space bounding box, and brush.
    pub fn paint_string(&mut self) -> String {
        let scene = self.record_paint();
        let mut out = String::new();
        for command in &scene.commands {
            out.push_str(&paint_command_string(command));
            out.push('\n');
        }
        out
    }
}
