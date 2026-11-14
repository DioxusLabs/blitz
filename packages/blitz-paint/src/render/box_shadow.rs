use super::ElementCx;
use super::background::get_cyclic;
use crate::color::{Color, ToColorColor as _};
use anyrender::{BoxShadowKind, NonUniformRoundedRect, PaintScene};
use blitz_dom::CopyStyleGetters;
use kurbo::{Rect, Vec2};
use style::properties::generated::longhands::background_clip::single_value::computed_value::T as StyloBackgroundClip;
use style::values::computed::BoxShadow;

impl ElementCx<'_, '_> {
    pub(super) fn outset_box_shadow_bounds(&self) -> Option<Rect> {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        if !box_shadow.iter().any(|s| !s.inset) {
            return None;
        }

        let current_color = self.style.clone_color();
        let border_box = self.frame.border_box_shape();
        let mut bounds: Option<Rect> = None;

        for shadow in box_shadow.iter().filter(|s| !s.inset) {
            let shadow_color = shadow
                .base
                .color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            if shadow_color.components[3] == 0.0 {
                continue;
            }

            let offset = Vec2::new(
                shadow.base.horizontal.px() as f64,
                shadow.base.vertical.px() as f64,
            ) * self.scale;
            let spread = shadow.spread.px() as f64 * self.scale;
            let std_dev = (shadow.base.blur.px() as f64 * self.scale / 2.0).max(0.0);
            let blur_extent = if std_dev > 0.0 {
                3.0 * std_dev + 1.0
            } else {
                0.0
            };

            let extent =
                (border_box.spread(spread).rect + offset).inflate(blur_extent, blur_extent);
            bounds = Some(bounds.map_or(extent, |acc| acc.union(extent)));
        }

        bounds
    }

    pub(super) fn draw_outset_box_shadow(&self, scene: &mut impl PaintScene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        if !box_shadow.iter().any(|s| !s.inset) {
            return;
        }

        let current_color = self.style.clone_color();
        let bg_styles = self.style.get_background();
        let bg_color = bg_styles
            .background_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();
        // The background color is clipped by the clip of the last background layer
        let layer_count = bg_styles.background_image.0.len();
        let bg_clip = *get_cyclic(&bg_styles.background_clip.0, layer_count - 1);
        let bg_covers_border_box = bg_clip == StyloBackgroundClip::BorderBox;
        // Only paint the shadow outside of the border box if it could otherwise be seen
        // through the element
        let bg_is_opaque = bg_color.components[3] >= 1.0;
        let clip_to_box = !bg_is_opaque || !bg_covers_border_box;

        let border_box = self.frame.border_box_shape();
        for shadow in box_shadow.iter().filter(|s| !s.inset).rev() {
            self.draw_box_shadow(
                scene,
                shadow,
                &border_box,
                if clip_to_box {
                    BoxShadowKind::Outset
                } else {
                    BoxShadowKind::OutsetUnclipped
                },
            );
        }
    }

    pub(super) fn draw_inset_box_shadow(&self, scene: &mut impl PaintScene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        if !box_shadow.iter().any(|s| s.inset) {
            return;
        }

        let padding_box = self.frame.padding_box_shape();
        for shadow in box_shadow.iter().filter(|s| s.inset).rev() {
            self.draw_box_shadow(scene, shadow, &padding_box, BoxShadowKind::Inset);
        }
    }

    fn draw_box_shadow(
        &self,
        scene: &mut impl PaintScene,
        shadow: &BoxShadow,
        box_shape: &NonUniformRoundedRect,
        kind: BoxShadowKind,
    ) {
        let shadow_color = shadow
            .base
            .color
            .resolve_to_absolute(&self.style.clone_color())
            .as_srgb_color();
        if shadow_color.components[3] == 0.0 {
            return;
        }
        let shadow_color: Color = shadow_color;

        let offset = Vec2::new(
            shadow.base.horizontal.px() as f64,
            shadow.base.vertical.px() as f64,
        ) * self.scale;
        let spread = shadow.spread.px() as f64 * self.scale;
        // The standard deviation of a CSS shadow's blur is half of its blur radius
        let std_dev = shadow.base.blur.px() as f64 * self.scale / 2.0;

        scene.draw_box_shadow(
            self.transform,
            box_shape,
            offset,
            spread,
            std_dev,
            shadow_color,
            kind,
        );
    }
}
