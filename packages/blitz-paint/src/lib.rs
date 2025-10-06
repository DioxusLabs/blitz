//! Paint a [`blitz_dom::BaseDocument`] by pushing [`anyrender`] drawing commands into
//! an impl [`anyrender::PaintScene`].

mod color;
mod debug_overlay;
mod gradient;
mod kurbo_css;
mod layers;
mod render;
mod sizing;
mod text;

use anyrender::PaintScene;
use blitz_dom::BaseDocument;
use layers::reset_layer_stats;
use render::BlitzDomPainter;

/// Paint a [`blitz_dom::BaseDocument`] by pushing drawing commands into
/// an impl [`anyrender::PaintScene`].
///
/// This function assumes that the styles and layout in the [`BaseDocument`] are already
/// resolved. Please ensure that this is the case before trying to paint.
///
/// The implementation of [`PaintScene`] is responsible for handling the commands that are pushed into it.
/// Generally this will involve executing them to draw a rasterized image/texture. But in some cases it may choose to
/// transform them to a vector format (e.g. SVG/PDF) or serialize them in raw form for later use.
pub fn paint_scene(
    scene: &mut impl PaintScene,
    dom: &BaseDocument,
    scale: f64,
    width: u32,
    height: u32,
) {
    reset_layer_stats();

    let devtools = *dom.devtools();
    let generator = BlitzDomPainter {
        dom,
        scale,
        width,
        height,
        devtools,
    };
    generator.paint_scene(scene);

    // println!(
    //     "Rendered using {} clips (depth: {}) (wanted: {})",
    //     CLIPS_USED.load(atomic::Ordering::SeqCst),
    //     CLIP_DEPTH_USED.load(atomic::Ordering::SeqCst),
    //     CLIPS_WANTED.load(atomic::Ordering::SeqCst)
    // );
}
