use super::ElementCx;
use crate::{
    color::{Color, ToColorColor as _},
    layers::maybe_with_layer,
};
use kurbo::Vec2;

impl ElementCx<'_> {
    pub(super) fn draw_outset_box_shadow(&self, scene: &mut impl anyrender::Scene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let current_color = self.style.clone_color();

        // TODO: Only apply clip if element has transparency
        let has_outset_shadow = box_shadow.iter().any(|s| !s.inset);
        maybe_with_layer(
            scene,
            has_outset_shadow,
            1.0,
            self.transform,
            &self.frame.shadow_clip(),
            |scene| {
                for shadow in box_shadow.iter().filter(|s| !s.inset) {
                    let shadow_color = shadow
                        .base
                        .color
                        .resolve_to_absolute(&current_color)
                        .as_srgb_color();
                    if shadow_color != Color::TRANSPARENT {
                        let transform = self.transform.then_translate(Vec2 {
                            x: shadow.base.horizontal.px() as f64,
                            y: shadow.base.vertical.px() as f64,
                        });

                        //TODO draw shadows with matching individual radii instead of averaging
                        let radius = (self.frame.border_top_left_radius_height
                            + self.frame.border_bottom_left_radius_width
                            + self.frame.border_bottom_left_radius_height
                            + self.frame.border_bottom_left_radius_width
                            + self.frame.border_bottom_right_radius_height
                            + self.frame.border_bottom_right_radius_width
                            + self.frame.border_top_right_radius_height
                            + self.frame.border_top_right_radius_width)
                            / 8.0;

                        // Fill the color
                        scene.draw_box_shadow(
                            transform,
                            self.frame.border_box,
                            shadow_color,
                            radius,
                            shadow.base.blur.px() as f64,
                        );
                    }
                }
            },
        )
    }

    pub(super) fn draw_inset_box_shadow(&self, scene: &mut impl anyrender::Scene) {
        let current_color = self.style.clone_color();
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let has_inset_shadow = box_shadow.iter().any(|s| s.inset);
        if !has_inset_shadow {
            return;
        }

        maybe_with_layer(
            scene,
            has_inset_shadow,
            1.0,
            self.transform,
            &self.frame.frame(),
            |scene| {
                for shadow in box_shadow.iter().filter(|s| s.inset) {
                    let shadow_color = shadow
                        .base
                        .color
                        .resolve_to_absolute(&current_color)
                        .as_srgb_color();
                    if shadow_color != Color::TRANSPARENT {
                        let transform = self.transform.then_translate(Vec2 {
                            x: shadow.base.horizontal.px() as f64,
                            y: shadow.base.vertical.px() as f64,
                        });

                        //TODO draw shadows with matching individual radii instead of averaging
                        let radius = (self.frame.border_top_left_radius_height
                            + self.frame.border_bottom_left_radius_width
                            + self.frame.border_bottom_left_radius_height
                            + self.frame.border_bottom_left_radius_width
                            + self.frame.border_bottom_right_radius_height
                            + self.frame.border_bottom_right_radius_width
                            + self.frame.border_top_right_radius_height
                            + self.frame.border_top_right_radius_width)
                            / 8.0;

                        // Fill the color
                        scene.draw_box_shadow(
                            transform,
                            self.frame.border_box,
                            shadow_color,
                            radius,
                            shadow.base.blur.px() as f64,
                        );
                    }
                }
            },
        );
    }
}
