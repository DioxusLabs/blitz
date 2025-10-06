use super::ElementCx;
use crate::{
    color::{Color, ToColorColor as _},
    layers::maybe_with_layer,
};
use anyrender::PaintScene;
use kurbo::{Rect, Vec2};

impl ElementCx<'_> {
    pub(super) fn draw_outset_box_shadow(&self, scene: &mut impl PaintScene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;

        // TODO: Only apply clip if element has transparency
        let has_outset_shadow = box_shadow.iter().any(|s| !s.inset);
        if !has_outset_shadow {
            return;
        }

        let current_color = self.style.clone_color();
        let max_shadow_rect = box_shadow.iter().fold(Rect::ZERO, |prev, shadow| {
            let x = shadow.base.horizontal.px() as f64 * self.scale;
            let y = shadow.base.vertical.px() as f64 * self.scale;
            let blur = shadow.base.blur.px() as f64 * self.scale;
            let spread = shadow.spread.px() as f64 * self.scale;
            let offset = spread + blur * 2.5;

            let rect = self.frame.border_box.inflate(offset, offset) + Vec2::new(x, y);

            prev.union(rect)
        });

        maybe_with_layer(
            scene,
            has_outset_shadow,
            1.0,
            self.transform,
            &self.frame.shadow_clip(max_shadow_rect),
            |scene| {
                for shadow in box_shadow.iter().filter(|s| !s.inset).rev() {
                    let shadow_color = shadow
                        .base
                        .color
                        .resolve_to_absolute(&current_color)
                        .as_srgb_color();

                    let alpha = shadow_color.components[3];
                    if alpha != 0.0 {
                        let transform = self.transform.then_translate(Vec2 {
                            x: shadow.base.horizontal.px() as f64 * self.scale,
                            y: shadow.base.vertical.px() as f64 * self.scale,
                        });

                        // TODO draw shadows with matching individual radii instead of averaging
                        let radius = self.frame.border_radii.average();

                        let spread = shadow.spread.px() as f64 * self.scale;
                        let rect = self.frame.border_box.inflate(spread, spread);

                        // Fill the color
                        scene.draw_box_shadow(
                            transform,
                            rect,
                            shadow_color,
                            radius,
                            shadow.base.blur.px() as f64,
                        );
                    }
                }
            },
        )
    }

    pub(super) fn draw_inset_box_shadow(&self, scene: &mut impl PaintScene) {
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
            &self.frame.padding_box_path(),
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
                        let radius = self.frame.border_radii.average();

                        // Fill the color
                        scene.draw_box_shadow(
                            transform,
                            self.frame.border_box,
                            shadow_color,
                            radius,
                            shadow.base.blur.px() as f64 * self.scale,
                        );
                    }
                }
            },
        );
    }
}
