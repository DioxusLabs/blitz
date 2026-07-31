//! Tests for paint-command recording and text serialization.

use anyrender::recording::RenderCommand;
use blitz_test_harness::Harness;

const HTML: &str = r#"<html><body style="margin:0">
    <div style="width:50px; height:40px; background:rgb(255,0,0);">text</div>
</body></html>"#;

#[test]
fn record_paint_captures_background_fill() {
    let mut harness = Harness::from_html(HTML);
    let scene = harness.record_paint();

    let red_fill = scene.commands.iter().find_map(|cmd| match cmd {
        RenderCommand::Fill(fill) => {
            let bbox = fill
                .transform
                .transform_rect_bbox(peniko::kurbo::Shape::bounding_box(&fill.shape));
            (bbox.width() == 50.0 && bbox.height() == 40.0).then_some(fill)
        }
        _ => None,
    });
    let red_fill = red_fill.expect("expected a 50x40 fill command");
    match &red_fill.brush {
        anyrender::Paint::Solid(color) => {
            let rgba = color.to_rgba8();
            assert_eq!((rgba.r, rgba.g, rgba.b), (255, 0, 0));
        }
        other => panic!("expected solid brush, got {other:?}"),
    }

    // The div's text should paint as a glyph run
    assert!(
        scene
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::GlyphRun(_)))
    );
}

#[test]
fn paint_string_is_greppable() {
    let mut harness = Harness::from_html(HTML);
    let paint = harness.paint_string();

    assert!(
        paint.contains("fill (0.0,0.0) 50.0x40.0 brush=rgba(255,0,0,255)"),
        "expected red fill in paint string:\n{paint}"
    );
    assert!(paint.contains("glyph_run"));
}
