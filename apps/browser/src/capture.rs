//! Utility functions for capturing screenshots

use anyrender::PaintScene;
use blitz_paint::paint_scene;
use peniko::Fill;
use peniko::kurbo::Rect;

use anyrender::render_to_buffer;
#[cfg(feature = "screenshot")]
use anyrender_vello_cpu::VelloCpuImageRenderer;
use std::path::{Path, PathBuf};

#[cfg(feature = "capture")]
use anyrender_serialize::{SceneArchive, SerializeConfig};

#[derive(Copy, Clone)]
pub(crate) enum RenderSize {
    /// Render the scene at the size of the
    Viewport,
    #[allow(unused)]
    /// Render the scene using the size of the full document height
    FullDocumentHeight,
}

impl RenderSize {
    fn resolve(&self, doc: &blitz_dom::BaseDocument) -> (u32, u32) {
        match self {
            RenderSize::Viewport => doc.viewport().window_size,
            RenderSize::FullDocumentHeight => {
                let root_element_size = doc.root_element().final_layout.size;
                (
                    root_element_size.width as u32,
                    root_element_size.height as u32,
                )
            }
        }
    }
}

/// Capture a screenshot as PNG and write it to the specified path
#[cfg(feature = "screenshot")]
pub(crate) fn capture_screenshot(doc: &blitz_dom::BaseDocument, path: &Path) {
    let size = RenderSize::Viewport;
    let (render_width, render_height) = size.resolve(doc);

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            render_scene(doc, scene, size);
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

/// Capture a scene as an AnyRender serialized scene
#[cfg(feature = "capture")]
pub(crate) fn capture_anyrender_scene(doc: &blitz_dom::BaseDocument, path: &Path) {
    let mut scene = anyrender::Scene::new();
    render_scene(doc, &mut scene, RenderSize::Viewport);

    let config = SerializeConfig::new()
        .with_woff2_fonts(true)
        .with_subset_fonts(true);
    let archive = SceneArchive::from_scene(&scene, &config).unwrap();

    let mut file = std::fs::File::create(path).unwrap();
    archive.serialize(&mut file).unwrap();
}

fn render_scene(
    doc: &blitz_dom::BaseDocument,
    scene: &mut impl PaintScene,
    size: RenderSize,
) -> (u32, u32) {
    let scale = doc.viewport().scale_f64();
    let (render_width, render_height) = size.resolve(doc);

    scene.fill(
        Fill::NonZero,
        Default::default(),
        blitz_dom::util::Color::WHITE,
        Default::default(),
        &Rect::new(0.0, 0.0, render_width as f64, render_height as f64),
    );
    paint_scene(scene, doc, scale, render_width, render_height, 0, 0);

    (render_width, render_height)
}

/// Open an RFD file dialog to get a path to save a file to
pub(crate) async fn try_get_save_path(file_type_name: &str, ext: &str) -> Option<PathBuf> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let default_name = format!("blitz-screenshot-{timestamp}.{ext}");

    #[cfg(any(target_os = "android", target_os = "ios"))]
    let path = Some(std::path::PathBuf::from(&default_name));

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let path = rfd::AsyncFileDialog::new()
        .set_file_name(&default_name)
        .add_filter(file_type_name, &[ext])
        .save_file()
        .await
        .map(|file| file.path().to_owned());

    path
}
