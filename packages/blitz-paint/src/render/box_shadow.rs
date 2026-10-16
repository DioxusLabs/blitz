use super::ElementCx;
use crate::color::{Color, ToColorColor as _};
use anyrender::{BoxShadowKind, NonUniformRoundedRect, PaintScene};
use kurbo::Vec2;
use style::values::computed::BoxShadow;

impl ElementCx<'_, '_> {
    pub(super) fn draw_outset_box_shadow(&self, scene: &mut impl PaintScene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        if !box_shadow.iter().any(|s| !s.inset) {
            return;
        }

        let current_color = self.style.clone_color();
        let opacity = self.style.get_effects().opacity;
        let bg_color = self
            .style
            .get_background()
            .background_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();
        // Only paint the shadow outside of the border box if it could otherwise be seen
        // through the element
        let bg_is_opaque = bg_color.components[3] >= 1.0;
        let clip_to_box = opacity < 1.0 || !bg_is_opaque;

        let border_box = self.frame.border_box_shape();
        for shadow in box_shadow.iter().filter(|s| !s.inset).rev() {
            self.draw_box_shadow(
                scene,
                shadow,
                &border_box,
                BoxShadowKind::Outset { clip_to_box },
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
