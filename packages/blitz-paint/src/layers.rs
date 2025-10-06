use anyrender::PaintScene;
use kurbo::{Affine, Shape};
use peniko::Mix;
use std::sync::atomic::{AtomicUsize, Ordering};

const LAYER_LIMIT: usize = 1024;

static LAYERS_USED: AtomicUsize = AtomicUsize::new(0);
static LAYER_DEPTH: AtomicUsize = AtomicUsize::new(0);
static LAYER_DEPTH_USED: AtomicUsize = AtomicUsize::new(0);
static LAYERS_WANTED: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn reset_layer_stats() {
    LAYERS_USED.store(0, Ordering::SeqCst);
    LAYERS_WANTED.store(0, Ordering::SeqCst);
    LAYER_DEPTH.store(0, Ordering::SeqCst);
    LAYER_DEPTH_USED.store(0, Ordering::SeqCst);
}

pub(crate) fn maybe_with_layer<S: PaintScene, F: FnOnce(&mut S)>(
    scene: &mut S,
    condition: bool,
    opacity: f32,
    transform: Affine,
    shape: &impl Shape,
    paint_layer: F,
) {
    let layer_used = maybe_push_layer(scene, condition, opacity, transform, shape);
    paint_layer(scene);
    maybe_pop_layer(scene, layer_used);
}

pub(crate) fn maybe_push_layer(
    scene: &mut impl PaintScene,
    condition: bool,
    opacity: f32,
    transform: Affine,
    shape: &impl Shape,
) -> bool {
    if !condition {
        return false;
    }
    LAYERS_WANTED.fetch_add(1, Ordering::SeqCst);

    // Check if clips are above limit
    let layers_available = LAYERS_USED.load(Ordering::SeqCst) <= LAYER_LIMIT;
    if !layers_available {
        return false;
    }
    let blend_mode = if opacity == 1.0 {
        #[allow(deprecated)]
        Mix::Clip
    } else {
        Mix::Normal
    };

    // Actually push the clip layer
    scene.push_layer(blend_mode, opacity, transform, shape);

    // Update accounting
    LAYERS_USED.fetch_add(1, Ordering::SeqCst);
    let depth = LAYER_DEPTH.fetch_add(1, Ordering::SeqCst) + 1;
    LAYER_DEPTH_USED.fetch_max(depth, Ordering::SeqCst);

    true
}

pub(crate) fn maybe_pop_layer(scene: &mut impl PaintScene, condition: bool) {
    if condition {
        scene.pop_layer();
        LAYER_DEPTH.fetch_sub(1, Ordering::SeqCst);
    }
}
