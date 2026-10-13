use super::ElementCx;
use crate::color::{Color, ToColorColor as _};
use anyrender::PaintScene;
use blitz_dom::CopyStyleGetters;
use blitz_dom::local_name;
use kurbo::{Affine, BezPath, Cap, Circle, Join, Point, Rect, RoundedRect, Stroke, Vec2};
use peniko::Fill;
use style::dom::TElement as _;
use style::values::specified::box_::Appearance;

impl ElementCx<'_, '_> {
    pub(super) fn draw_input(&self, scene: &mut impl PaintScene) {
        match &**self.node.local_name() {
            "input" => {}
            "progress" => return self.draw_progress(scene),
            "meter" => return self.draw_meter(scene),
            _ => return,
        }
        match self.node.attr(local_name!("type")) {
            Some("range") => return self.draw_range(scene),
            Some("color") => return self.draw_color_well(scene),
            _ => {}
        }
        let Some(checked) = self.element.checkbox_input_checked() else {
            return;
        };

        let type_attr = self.node.attr(local_name!("type"));
        let disabled = self.node.attr(local_name!("disabled")).is_some();

        // TODO this should be coming from css accent-color, but I couldn't find how to retrieve it
        let accent_color = if disabled {
            Color::from_rgba8(209, 209, 209, 255)
        } else {
            self.style.slow_clone_color().as_srgb_color()
        };

        let width = self.frame.border_box.width();
        let height = self.frame.border_box.height();
        let min_dimension = width.min(height);
        let scale = (min_dimension - 4.0).max(0.0) / 16.0;

        let frame = self.frame.border_box.to_rounded_rect(scale * 2.0);

        match type_attr {
            Some("checkbox") => {
                draw_checkbox(scene, checked, frame, self.transform, accent_color, scale);
            }
            Some("radio") => {
                let center = frame.center();
                draw_radio_button(scene, checked, center, self.transform, accent_color, scale);
            }
            _ => {}
        }
    }
}

fn draw_checkbox(
    scene: &mut impl PaintScene,
    checked: bool,
    frame: RoundedRect,
    transform: Affine,
    accent_color: Color,
    scale: f64,
) {
    if checked {
        scene.fill(Fill::NonZero, transform, accent_color, None, &frame);
        //Tick code derived from masonry
        let mut path = BezPath::new();
        path.move_to((2.0, 9.0));
        path.line_to((6.0, 13.0));
        path.line_to((14.0, 2.0));

        path.apply_affine(Affine::translate(Vec2 { x: 2.0, y: 1.0 }).then_scale(scale));

        let style = Stroke {
            width: 2.0 * scale,
            join: Join::Round,
            miter_limit: 10.0,
            start_cap: Cap::Round,
            end_cap: Cap::Round,
            dash_pattern: Default::default(),
            dash_offset: 0.0,
        };

        scene.stroke(&style, transform, Color::WHITE, None, &path);
    } else {
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &frame);
        scene.stroke(&Stroke::default(), transform, accent_color, None, &frame);
    }
}

fn draw_radio_button(
    scene: &mut impl PaintScene,
    checked: bool,
    center: Point,
    transform: Affine,
    accent_color: Color,
    scale: f64,
) {
    let outer_ring = Circle::new(center, 8.0 * scale);
    let gap = Circle::new(center, 6.0 * scale);
    let inner_circle = Circle::new(center, 4.0 * scale);
    if checked {
        scene.fill(Fill::NonZero, transform, accent_color, None, &outer_ring);
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &gap);
        scene.fill(Fill::NonZero, transform, accent_color, None, &inner_circle);
    } else {
        const GRAY: Color = color::palette::css::GRAY;
        scene.fill(Fill::NonZero, transform, GRAY, None, &outer_ring);
        scene.fill(Fill::NonZero, transform, Color::WHITE, None, &gap);
    }
}

/// The blue of a native progress bar's value and a slider's thumb and fill, as Chrome draws them.
const ACCENT: Color = Color::from_rgb8(0, 117, 255);
/// The fill of a native bar's empty track.
const TRACK: Color = Color::from_rgb8(239, 239, 239);
/// The border of a native progress bar's or slider's empty track.
const TRACK_BORDER: Color = Color::from_rgb8(178, 178, 178);
/// The border of a native meter's empty track.
const METER_TRACK_BORDER: Color = Color::from_rgb8(203, 203, 203);

/// An element's content box laid along its inline axis.
struct InlineAxis {
    /// The rectangle the axis runs along.
    rect: Rect,
    /// Whether the inline axis is vertical.
    vertical: bool,
    /// Whether the inline axis runs against the physical axis (right to left, or bottom to top).
    reversed: bool,
}

impl InlineAxis {
    /// Returns the length of the inline axis.
    fn length(&self) -> f64 {
        if self.vertical {
            self.rect.height()
        } else {
            self.rect.width()
        }
    }

    /// Returns the thickness across the inline axis.
    fn thickness(&self) -> f64 {
        if self.vertical {
            self.rect.width()
        } else {
            self.rect.height()
        }
    }

    /// Returns the band from `start` to `end` along the inline axis, `inset` in from both
    /// sides across it.
    fn band(&self, start: f64, end: f64, inset: f64) -> Rect {
        let r = self.rect;
        let (start, end) = if self.reversed {
            (self.length() - end, self.length() - start)
        } else {
            (start, end)
        };
        if self.vertical {
            Rect::new(r.x0 + inset, r.y0 + start, r.x1 - inset, r.y0 + end)
        } else {
            Rect::new(r.x0 + start, r.y0 + inset, r.x0 + end, r.y1 - inset)
        }
    }

    /// Returns the point `along` the inline axis from its start, centred across it.
    fn point(&self, along: f64) -> Point {
        let r = self.rect;
        let along = if self.reversed {
            self.length() - along
        } else {
            along
        };
        if self.vertical {
            Point::new(r.center().x, r.y0 + along)
        } else {
            Point::new(r.x0 + along, r.center().y)
        }
    }
}

impl ElementCx<'_, '_> {
    /// Returns the element's content box laid along its inline axis.
    fn inline_axis(&self) -> InlineAxis {
        let wm = self.style.writing_mode;
        let vertical = wm.is_vertical();
        InlineAxis {
            rect: self.frame.content_box,
            vertical,
            reversed: if vertical {
                !wm.is_inline_tb()
            } else {
                !wm.is_bidi_ltr()
            },
        }
    }

    /// Returns whether the element draws its platform look rather than only its CSS.
    fn has_native_appearance(&self) -> bool {
        self.style.get_box().slow_clone_appearance() != Appearance::None
    }

    /// Returns a number attribute of the element, if it holds a finite number.
    fn number_attr(&self, name: blitz_dom::LocalName) -> Option<f64> {
        self.node
            .attr(name)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite())
    }

    /// Draws a progress bar: a rounded track filled from its inline start, or under
    /// `appearance: none` a gray bar filled in green, as Chrome draws them.
    fn draw_progress(&self, scene: &mut impl PaintScene) {
        let max = self
            .number_attr(local_name!("max"))
            .filter(|max| *max > 0.0);
        let max = max.unwrap_or(1.0);
        // A progress element without a value is indeterminate, and shows no value.
        let fraction = self
            .number_attr(local_name!("value"))
            .map(|value| (value / max).clamp(0.0, 1.0));
        if self.has_native_appearance() {
            self.draw_native_bar(scene, fraction.unwrap_or(0.0), ACCENT, TRACK_BORDER);
        } else {
            let axis = self.inline_axis();
            let gray = Color::from_rgb8(128, 128, 128);
            let green = Color::from_rgb8(0, 128, 0);
            scene.fill(Fill::NonZero, self.transform, gray, None, &axis.rect);
            if let Some(fraction) = fraction {
                let value = axis.band(0.0, axis.length() * fraction, 0.0);
                scene.fill(Fill::NonZero, self.transform, green, None, &value);
            }
        }
    }

    /// Draws a meter: a rounded track filled from its inline start, green where the value
    /// lies in the optimum region, yellow next to it and red beyond.
    fn draw_meter(&self, scene: &mut impl PaintScene) {
        // https://html.spec.whatwg.org/#the-meter-element
        let min = self.number_attr(local_name!("min")).unwrap_or(0.0);
        let max = self.number_attr(local_name!("max")).unwrap_or(1.0).max(min);
        let value = self
            .number_attr(local_name!("value"))
            .unwrap_or(0.0)
            .clamp(min, max);
        let low = self
            .number_attr(local_name!("low"))
            .unwrap_or(min)
            .clamp(min, max);
        let high = self
            .number_attr(local_name!("high"))
            .unwrap_or(max)
            .clamp(low, max);
        let optimum = self
            .number_attr(local_name!("optimum"))
            .unwrap_or((min + max) / 2.0)
            .clamp(min, max);
        let region = |v: f64| -> u8 {
            if v < low {
                0
            } else if v > high {
                2
            } else {
                1
            }
        };
        let color = match region(optimum).abs_diff(region(value)) {
            0 => Color::from_rgb8(16, 124, 16),
            1 => Color::from_rgb8(255, 185, 0),
            _ => Color::from_rgb8(209, 52, 56),
        };
        let fraction = if max > min {
            (value - min) / (max - min)
        } else {
            0.0
        };
        self.draw_native_bar(scene, fraction, color, METER_TRACK_BORDER);
    }

    /// Draws a native bar: a track half as thick as the content box, rounded and centred in
    /// it, filled `fraction` of its length from its inline start.
    fn draw_native_bar(
        &self,
        scene: &mut impl PaintScene,
        fraction: f64,
        color: Color,
        border: Color,
    ) {
        let axis = self.inline_axis();
        let inset = axis.thickness() / 4.0;
        let track = axis.band(0.0, axis.length(), inset);
        let radius = track.width().min(track.height()) / 2.0;
        let outline = track.inset(-self.scale / 2.0).to_rounded_rect(radius);
        scene.fill(Fill::NonZero, self.transform, TRACK, None, &outline);
        let stroke = Stroke::new(self.scale);
        scene.stroke(&stroke, self.transform, border, None, &outline);
        if fraction > 0.0 {
            let value = axis.band(0.0, axis.length() * fraction, inset);
            let value = value.to_rounded_rect(radius);
            scene.fill(Fill::NonZero, self.transform, color, None, &value);
        }
    }

    /// Draws a slider: a track filled up to its thumb, which sits at the value along the inline
    /// axis. Under `appearance: none` only the thumb is drawn, as Chrome draws it.
    fn draw_range(&self, scene: &mut impl PaintScene) {
        // https://html.spec.whatwg.org/#range-state-(type=range)
        let min = self.number_attr(local_name!("min")).unwrap_or(0.0);
        let max = self
            .number_attr(local_name!("max"))
            .unwrap_or(100.0)
            .max(min);
        let value = self
            .number_attr(local_name!("value"))
            .unwrap_or(min + (max - min) / 2.0)
            .clamp(min, max);
        let fraction = if max > min {
            (value - min) / (max - min)
        } else {
            0.0
        };
        let axis = self.inline_axis();
        let radius = (8.0 * self.scale)
            .min(axis.thickness() / 2.0)
            .min(axis.length() / 2.0);
        if radius <= 0.0 {
            return;
        }
        let along = radius + (axis.length() - 2.0 * radius) * fraction;
        if self.has_native_appearance() {
            let inset = axis.thickness() / 4.0;
            let track = axis.band(0.0, axis.length(), inset);
            let track_radius = track.width().min(track.height()) / 2.0;
            let outline = track.inset(-self.scale / 2.0).to_rounded_rect(track_radius);
            scene.fill(Fill::NonZero, self.transform, TRACK, None, &outline);
            let stroke = Stroke::new(self.scale);
            scene.stroke(&stroke, self.transform, TRACK_BORDER, None, &outline);
            let filled = axis.band(0.0, along, inset).to_rounded_rect(track_radius);
            scene.fill(Fill::NonZero, self.transform, ACCENT, None, &filled);
        }
        let thumb = Circle::new(axis.point(along), radius);
        scene.fill(Fill::NonZero, self.transform, ACCENT, None, &thumb);
    }

    /// Draws a color well's swatch: the value's color inside the content box, 4px in from its
    /// block-axis sides and 2px from its inline-axis sides, with a gray border.
    fn draw_color_well(&self, scene: &mut impl PaintScene) {
        let color = self
            .node
            .attr(local_name!("value"))
            .and_then(parse_simple_color)
            .unwrap_or(Color::BLACK);
        let content = self.frame.content_box;
        let (inline, block) = (2.0 * self.scale, 4.0 * self.scale);
        let (dx, dy) = if self.style.writing_mode.is_vertical() {
            (block, inline)
        } else {
            (inline, block)
        };
        let swatch = Rect::new(
            content.x0 + dx,
            content.y0 + dy,
            content.x1 - dx,
            content.y1 - dy,
        );
        if swatch.width() <= 0.0 || swatch.height() <= 0.0 {
            return;
        }
        scene.fill(Fill::NonZero, self.transform, color, None, &swatch);
        let border = Color::from_rgb8(119, 119, 119);
        let outline = swatch.inset(-self.scale / 2.0);
        let stroke = Stroke::new(self.scale);
        scene.stroke(&stroke, self.transform, border, None, &outline);
    }
}

/// Parses a valid simple color, `#` and six hex digits.
///
/// https://html.spec.whatwg.org/#valid-simple-colour
fn parse_simple_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Color::from_rgb8(channel(0)?, channel(2)?, channel(4)?))
}
