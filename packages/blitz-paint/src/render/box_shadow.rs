use super::ElementCx;
use crate::color::{Color, ToColorColor as _};
use crate::kurbo_css::{CssBox, NonUniformRoundedRectRadii};
use anyrender::PaintScene;
use blitz_dom::CopyStyleGetters;
use kurbo::{BezPath, Insets, Rect, Vec2};
use peniko::{Compose, Fill, Mix};

impl ElementCx<'_, '_> {
    pub(super) fn draw_outset_box_shadow(&self, scene: &mut impl PaintScene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let has_outset_shadow = box_shadow.iter().any(|s| !s.inset);
        if !has_outset_shadow {
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
        let bg_is_opaque = bg_color.components[3] >= 1.0;
        let needs_clip = opacity < 1.0 || !bg_is_opaque;

        let max_shadow_rect = box_shadow.iter().fold(Rect::ZERO, |prev, shadow| {
            let x = shadow.base.horizontal.px() as f64 * self.scale;
            let y = shadow.base.vertical.px() as f64 * self.scale;
            let blur = shadow.base.blur.px() as f64 * self.scale;
            let spread = shadow.spread.px() as f64 * self.scale;
            let offset = spread + blur * 2.5;

            let rect = self.frame.border_box.inflate(offset, offset) + Vec2::new(x, y);

            prev.union(rect)
        });

        self.context.layer_manager.maybe_with_layer(
            scene,
            needs_clip,
            Mix::Normal.into(),
            1.0,
            self.transform,
            &self.frame.shadow_clip(max_shadow_rect),
            None,
            None,
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

                        let spread = shadow.spread.px() as f64 * self.scale;
                        let blur = shadow.base.blur.px() as f64;

                        // Without blur or spread the shadow is exactly the element's shape,
                        // so fill the border box path directly (preserving individual radii)
                        if blur == 0.0 && spread == 0.0 {
                            scene.fill(
                                Fill::NonZero,
                                transform,
                                shadow_color,
                                None,
                                &self.frame.border_box_path(),
                            );
                            continue;
                        }

                        // TODO draw shadows with matching individual radii instead of averaging
                        let radius = self.frame.border_radii.average();
                        let rect = self.frame.border_box.inflate(spread, spread);

                        // Fill the color
                        scene.draw_box_shadow(transform, rect, shadow_color, radius, blur);
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

        let padding_box = self.frame.padding_box_path();

        for shadow in box_shadow.iter().filter(|s| s.inset) {
            let shadow_color = shadow
                .base
                .color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            if shadow_color == Color::TRANSPARENT {
                return;
            }

            let offset = Vec2 {
                x: shadow.base.horizontal.px() as f64 * self.scale,
                y: shadow.base.vertical.px() as f64 * self.scale,
            };
            let spread = shadow.spread.px() as f64 * self.scale;
            let blur = shadow.base.blur.px() as f64 * self.scale;

            // Without blur the shadow is exactly the padding box minus the "hole" (the
            // padding box shrunk by the spread and moved by the offset), so fill that
            // shape directly: one path, wound so that the hole has a winding number of
            // zero. Going through the blurred-rect shader below instead is both slow
            // (it is evaluated per pixel even with nothing to blur) and inexact:
            // DestOut-ing its antialiased hole edge from the antialiased padding box
            // edge leaves a faint line wherever the two coincide (#1038), whereas the
            // coverage of coincident, opposite edges in one path cancels exactly.
            if blur == 0.0 {
                let mut shape = padding_box.clone();
                if let Some(hole) = self.inset_shadow_hole(spread, offset) {
                    shape.extend(hole);
                }
                scene.fill(Fill::NonZero, self.transform, shadow_color, None, &shape);
                continue;
            }

            // TODO draw shadows with matching individual radii instead of averaging
            let radius = self.frame.border_radii.average();
            let transform = self.transform.then_translate(offset);

            scene.push_layer(
                Fill::NonZero,
                Mix::Normal,
                1.0,
                self.transform,
                &padding_box,
                None,
                None,
            );
            scene.fill(
                Fill::NonZero,
                self.transform,
                shadow_color,
                None,
                &padding_box,
            );

            scene.push_layer(
                Fill::NonZero,
                Compose::DestOut,
                1.0,
                self.transform,
                &padding_box,
                None,
                None,
            );
            // The unshadowed area is the padding box shrunk by the spread radius
            let hole = self.frame.padding_box.inflate(-spread, -spread);
            scene.draw_box_shadow(transform, hole, Color::WHITE, radius, blur);

            scene.pop_layer();
            scene.pop_layer();
        }
    }

    /// The path of the unshadowed area of an inset shadow without blur: the padding
    /// box shrunk by `spread` and moved by `offset`, with the padding box's corner radii
    /// shrunk by `spread` too (sharp corners stay sharp), wound against the padding box
    /// path. `None` if no part of the padding box is left unshadowed.
    ///
    /// The hole is cut down to the padding box, so that the combined path never winds
    /// the other way outside it (which would need a clip, whose antialiased edge would
    /// let some of the shadow through where the hole covers it). A side that is cut
    /// lies on the padding box's edge, and the corners next to it follow the padding
    /// box's corners, concentrically inset to the hole's other side.
    fn inset_shadow_hole(&self, spread: f64, offset: Vec2) -> Option<BezPath> {
        let padding_box = self.frame.padding_box;
        let hole = padding_box.inflate(-spread, -spread) + offset;
        let rect = hole.intersect(padding_box);
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }

        // How far each side of the hole lies inside the padding box, and whether it
        // was cut (i.e. the hole reached past the padding box on that side).
        let inset = Insets {
            x0: rect.x0 - padding_box.x0,
            y0: rect.y0 - padding_box.y0,
            x1: padding_box.x1 - rect.x1,
            y1: padding_box.y1 - rect.y1,
        };
        let cut_x0 = hole.x0 < padding_box.x0;
        let cut_y0 = hole.y0 < padding_box.y0;
        let cut_x1 = hole.x1 > padding_box.x1;
        let cut_y1 = hole.y1 > padding_box.y1;

        let border = self.frame.border_width;
        let radii = self.frame.border_radii;
        // The radii of one corner of the hole, from the corner's outer (border box)
        // radii, the border widths of its two sides, how far those sides of the hole
        // lie inside the padding box and whether either of them was cut.
        let corner = |outer: Vec2, border: (f64, f64), inset: (f64, f64), is_cut: bool| {
            let padding = Vec2::new((outer.x - border.0).max(0.0), (outer.y - border.1).max(0.0));
            if padding.x == 0.0 || padding.y == 0.0 {
                return Vec2::ZERO;
            }
            let shrink = if is_cut {
                Vec2::new(inset.0, inset.1)
            } else {
                Vec2::new(spread, spread)
            };
            Vec2::new(
                (padding.x - shrink.x).max(0.0),
                (padding.y - shrink.y).max(0.0),
            )
        };
        let radii = NonUniformRoundedRectRadii {
            top_left: corner(
                radii.top_left,
                (border.x0, border.y0),
                (inset.x0, inset.y0),
                cut_x0 || cut_y0,
            ),
            top_right: corner(
                radii.top_right,
                (border.x1, border.y0),
                (inset.x1, inset.y0),
                cut_x1 || cut_y0,
            ),
            bottom_right: corner(
                radii.bottom_right,
                (border.x1, border.y1),
                (inset.x1, inset.y1),
                cut_x1 || cut_y1,
            ),
            bottom_left: corner(
                radii.bottom_left,
                (border.x0, border.y1),
                (inset.x0, inset.y1),
                cut_x0 || cut_y1,
            ),
        };

        let hole = CssBox::new(rect, Insets::ZERO, Insets::ZERO, 0.0, radii);
        Some(hole.border_box_path().reverse_subpaths())
    }
}
