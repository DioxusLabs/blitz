//! Utility functions for capturing screenshots

#[cfg(feature = "screenshot")]
use anyrender::{PaintScene as _, render_to_buffer};
#[cfg(feature = "screenshot")]
use anyrender_vello_cpu::VelloCpuImageRenderer;
#[cfg(feature = "screenshot")]
use blitz_paint::paint_scene;
#[cfg(feature = "screenshot")]
use peniko::Fill;
#[cfg(feature = "screenshot")]
use peniko::kurbo::Rect;

#[cfg(feature = "screenshot")]
use std::path::{Path, PathBuf};

/// Capture a screenshot as PNG and write it to the specified path
#[cfg(feature = "screenshot")]
pub(crate) fn capture_screenshot(doc: &blitz_dom::BaseDocument, path: &Path) {
    let viewport = doc.viewport();
    let scale = viewport.scale_f64();
    let (render_width, render_height) = viewport.window_size;

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                blitz_dom::util::Color::WHITE,
                Default::default(),
                &Rect::new(0.0, 0.0, render_width as f64, render_height as f64),
            );
            paint_scene(scene, doc, scale, render_width, render_height, 0, 0);
        },
        render_width,
        render_height,
    );

    if let Ok(file) = std::fs::File::create(path) {
        let mut encoder = png::Encoder::new(file, render_width, render_height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = encoder.write_header() {
            if writer.write_image_data(&buffer).is_ok() {
                println!("Screenshot saved to {}", path.display());
            }
        }
    }
}

/// Open an RFD file dialog to get a path to save a file to
#[cfg(feature = "screenshot")]
pub(crate) async fn try_get_save_path() -> Option<PathBuf> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let default_name = format!("blitz-screenshot-{timestamp}.png");

    #[cfg(any(target_os = "android", target_os = "ios"))]
    let path = Some(std::path::PathBuf::from(&default_name));

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let path = rfd::AsyncFileDialog::new()
        .set_file_name(&default_name)
        .add_filter("PNG Image", &["png"])
        .save_file()
        .await
        .map(|file| file.path().to_owned());

    path
}
