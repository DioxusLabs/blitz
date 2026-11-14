//! Rendering for the CSS `mask` properties (`mask-image`, `mask-position`,
//! `mask-size`, `mask-repeat`, `mask-clip`, `mask-origin`).
//!
//! Masks are applied by:
//!
//!  1. Pushing an isolation layer (clipped to the mask painting area) before the
//!     element is painted
//!  2. Painting the element (and its descendants) as normal
//!  3. Pushing a mask layer and drawing the mask image layers into it, which
//!     multiplies the alpha of the already-painted element by the mask
//!  4. Popping both layers
//!
//! A mask with a single image layer uses that layer's `mask-mode` for the mask
//! layer. With several image layers, the mask layer is an alpha mask and each
//! `luminance` layer is converted to alpha before it is composited with the
//! layers below it.
//!
//! The mask image layers themselves are positioned/sized/repeated using the same
//! code as `background-image` layers (see `background.rs`), as the `mask-*` and
//! `background-*` properties share computed value types.
use crate::render::background::get_cyclic;

use super::background::ImageLayerStyles;
use super::{ALMOST_OPAQUE, ElementCx};
use anyrender::{MaskMode, PaintScene};
use peniko::{BlendMode, Color, Compose, Fill, Mix};
use style::properties::generated::longhands::mask_composite::single_value::computed_value::T as StyloMaskComposite;
use style::properties::generated::longhands::mask_mode::single_value::computed_value::T as StyloMaskMode;
use style::values::generics::image::GenericImage;

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
            Fill::NonZero,
            Mix::Normal,
            ALMOST_OPAQUE,
            self.transform,
            &self.frame.border_box_shape(),
            None,
            None,
        );
        true
    }

    /// Apply the CSS mask to the content drawn since the corresponding
    /// [`maybe_push_css_mask_layer`](Self::maybe_push_css_mask_layer) by drawing the
    /// mask image layers into a mask layer, then pop the isolation layer.
    pub(super) fn maybe_pop_css_mask_layer(&self, scene: &mut impl PaintScene, layer_pushed: bool) {
        if !layer_pushed {
            return;
        }

        let svg_styles = self.style.get_svg();
        let single_layer = svg_styles.mask_image.0.len() == 1;
        let mode = if single_layer {
            self.css_mask_mode(0)
        } else {
            MaskMode::Alpha
        };

        scene.push_mask_layer(
            Fill::NonZero,
            mode,
            self.transform,
            &self.frame.border_box_shape(),
        );
        self.draw_css_mask(scene, !single_layer);
        scene.pop_layer(); // Mask layer
        scene.pop_layer(); // Isolation layer
    }

    /// The mask mode of the mask image layer at `idx`
    fn css_mask_mode(&self, idx: usize) -> MaskMode {
        match get_cyclic(&self.style.get_svg().mask_mode.0, idx) {
            StyloMaskMode::Luminance => MaskMode::Luminance,
            // `match-source` is `alpha` for images. `<mask>` element references
            // (which would use the mask's `mask-type`) aren't supported.
            StyloMaskMode::Alpha | StyloMaskMode::MatchSource => MaskMode::Alpha,
        }
    }

    /// Draw the mask image layers (analogous to `draw_background` for `background-image`).
    ///
    /// If `convert_luminance_layers` is set, `luminance` layers are converted to alpha
    /// so that they can be composited with the other layers into an alpha mask.
    fn draw_css_mask(&self, scene: &mut impl PaintScene, convert_luminance_layers: bool) {
        let svg_styles = self.style.get_svg();
        let image_data = &self.element.mask_images;
        let layer_count = svg_styles.mask_image.0.len();

        for idx in (0..layer_count).rev() {
            let layer = ImageLayerStyles::from_svg(svg_styles, image_data, idx);
            let mask_clip_path = self.box_path(layer.clip);

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
                Fill::NonZero,
                BlendMode::new(Mix::Normal, compose),
                1.0,
                self.transform,
                &mask_clip_path,
                None,
                None,
            );
            if convert_luminance_layers && self.css_mask_mode(idx) == MaskMode::Luminance {
                // Mask an opaque fill with the luminance of the layer. The bottommost
                // layer's (SrcOver) layer may not be isolated, but it has nothing
                // below it for the luminance mask layer to erase.
                scene.fill(
                    Fill::NonZero,
                    self.transform,
                    Color::WHITE,
                    None,
                    &mask_clip_path,
                );
                scene.push_mask_layer(
                    Fill::NonZero,
                    MaskMode::Luminance,
                    self.transform,
                    &mask_clip_path,
                );
                self.draw_image_layer(scene, &layer);
                scene.pop_layer();
            } else {
                self.draw_image_layer(scene, &layer);
            }
            scene.pop_layer();
        }
    }
}
