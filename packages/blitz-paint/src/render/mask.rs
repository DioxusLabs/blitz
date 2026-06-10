//! Rendering for the CSS `mask` properties (`mask-image`, `mask-position`,
//! `mask-size`, `mask-repeat`, `mask-clip`, `mask-origin`).
//!
//! Masks are applied by:
//!
//!  1. Pushing an isolation layer (clipped to the mask painting area) before the
//!     element is painted
//!  2. Painting the element (and its descendants) as normal
//!  3. Pushing a `Compose::DestIn` layer and drawing the mask image layers into it,
//!     which multiplies the alpha of the already-painted element by the alpha of
//!     the mask
//!  4. Popping both layers
//!
//! The mask image layers themselves are positioned/sized/repeated using the same
//! code as `background-image` layers (see `background.rs`), as the `mask-*` and
//! `background-*` properties share computed value types.
use crate::render::background::get_cyclic;

use super::ElementCx;
use super::background::ImageLayerStyles;
use anyrender::PaintScene;
use peniko::{BlendMode, Compose, Mix};
use style::properties::generated::longhands::mask_composite::single_value::computed_value::T as StyloMaskComposite;
use style::values::generics::image::GenericImage;

#[cfg(feature = "tracing")]
use tracing::warn;

/// An opacity just below 1.0, used to force the renderer to allocate an isolated
/// buffer for the mask isolation layer. Renderers (e.g. vello_cpu) may optimize
/// layers with `Mix::Normal`/`Compose::SrcOver` and an opacity of exactly 1.0 into
/// plain non-isolated clip layers, in which case the `Compose::DestIn` mask
/// compositing would erase the backdrop behind the element rather than just the
/// element's own content.
const ALMOST_OPAQUE: f32 = 1.0 - f32::EPSILON;

impl ElementCx<'_, '_> {
    /// Whether the element has any CSS `mask-image` layers
    pub(super) fn has_css_mask(&self) -> bool {
        self.style
            .get_svg()
            .mask_image
            .0
            .iter()
            .any(|image| !matches!(image, GenericImage::None))
    }

    /// If the element has a CSS mask, push an isolation layer for the masked content
    /// to be drawn into. Returns whether a layer was pushed, which should later be
    /// passed to [`maybe_pop_css_mask_layer`](Self::maybe_pop_css_mask_layer).
    pub(super) fn maybe_push_css_mask_layer(&self, scene: &mut impl PaintScene) -> bool {
        if !self.has_css_mask() {
            return false;
        }

        // Content outside of the mask painting area (at largest the border box) has
        // a mask alpha of 0, so we can clip the isolation layer to the border box.
        scene.push_layer(
            Mix::Normal,
            ALMOST_OPAQUE,
            self.transform,
            &self.frame.border_box_path(),
            None,
            None,
        );
        true
    }

    /// Apply the CSS mask to the content drawn since the corresponding
    /// [`maybe_push_css_mask_layer`](Self::maybe_push_css_mask_layer) by drawing the
    /// mask image layers with `Compose::DestIn`, then pop the isolation layer.
    pub(super) fn maybe_pop_css_mask_layer(&self, scene: &mut impl PaintScene, layer_pushed: bool) {
        if !layer_pushed {
            return;
        }

        scene.push_layer(
            BlendMode::new(Mix::Normal, Compose::DestIn),
            1.0,
            self.transform,
            &self.frame.border_box_path(),
            None,
            None,
        );
        self.draw_css_mask(scene);
        scene.pop_layer(); // Mask (DestIn) layer
        scene.pop_layer(); // Isolation layer
    }

    /// Draw the mask image layers (analogous to `draw_background` for `background-image`)
    fn draw_css_mask(&self, scene: &mut impl PaintScene) {
        let svg_styles = self.style.get_svg();
        let image_data = &self.element.mask_images;
        let layer_count = svg_styles.mask_image.0.len();

        for idx in (0..layer_count).rev() {
            let layer = ImageLayerStyles::from_svg(svg_styles, image_data, idx);
            let mask_clip_path = self.box_path(layer.clip);

            // TODO: support `mask-mode: luminance` (luminance masks are currently
            // rendered as alpha masks).
            #[cfg(feature = "tracing")]
            {
                use style::properties::generated::longhands::mask_mode::single_value::computed_value::T as StyloMaskMode;
                let mask_mode = &svg_styles.mask_mode.0;
                if matches!(get_cyclic(&mask_mode, idx), StyloMaskMode::Luminance) {
                    warn!("mask-mode: luminance is not supported (falling back to alpha)");
                }
            }

            // Each mask layer is composited with the (already drawn) layers below it
            // using the Porter-Duff operator given by its `mask-composite` value. The
            // bottommost layer has no layers below it and is always drawn with SrcOver.
            let compose = if idx == layer_count - 1 {
                Compose::SrcOver
            } else {
                let composite_list = &svg_styles.mask_composite.0;
                match get_cyclic(composite_list, idx) {
                    StyloMaskComposite::Add => Compose::SrcOver,
                    StyloMaskComposite::Subtract => Compose::SrcOut,
                    StyloMaskComposite::Intersect => Compose::SrcIn,
                    StyloMaskComposite::Exclude => Compose::Xor,
                }
            };

            // The layer is pushed unconditionally (even for `mask-image: none` layers,
            // which draw nothing) as compositing a transparent black layer with e.g.
            // `intersect` clears the mask built up so far.
            scene.push_layer(
                BlendMode::new(Mix::Normal, compose),
                1.0,
                self.transform,
                &mask_clip_path,
                None,
                None,
            );
            self.draw_image_layer(scene, &layer);
            scene.pop_layer();
        }
    }
}
