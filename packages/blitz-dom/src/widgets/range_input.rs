use anyrender::{PaintScene as _, Scene};
use blitz_traits::events::{MouseEventButton, MouseEventButtons, UiEvent};
use keyboard_types::Key;
use kurbo::{Affine, Circle, RoundedRect};
use peniko::Fill;

use crate::node::{
    ComputedStyles, Widget, WidgetEventContext, WidgetIntrinsicSize, WidgetPaintContext,
};
use crate::qual_name;
use crate::util::{Color, ToColorColor as _};

/// A [`Widget`] implementing "slider" functionality for `<input type="range">` elements.
///
/// The slider's state is controlled by the standard `min`, `max`, `step`, `value` and
/// `disabled` attributes on the element. When the user changes the value by dragging the
/// slider thumb (or using the arrow keys while the input is focused), the widget writes
/// the new value back to the element's `value` attribute and dispatches a DOM "input" event.
pub struct RangeInputWidget {
    min: f64,
    max: f64,
    /// The stepping interval. `None` represents `step="any"`.
    step: Option<f64>,
    /// The current value. `None` until either the `value` attribute is set or the user
    /// interacts with the slider, in which case the value defaults to the midpoint of
    /// `min` and `max`.
    value: Option<f64>,
    disabled: bool,
    /// Whether the slider thumb is currently being dragged
    dragging: bool,
}

impl Default for RangeInputWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl RangeInputWidget {
    pub fn new() -> Self {
        Self {
            min: 0.0,
            max: 100.0,
            step: Some(1.0),
            value: None,
            disabled: false,
            dragging: false,
        }
    }

    /// The current value, clamped to the `[min, max]` range
    pub fn value(&self) -> f64 {
        self.value
            .unwrap_or(self.min + (self.max - self.min) / 2.0)
            .clamp(self.min, self.max.max(self.min))
    }

    /// The current value as fraction of the `[min, max]` range (between 0 and 1)
    fn fraction(&self) -> f64 {
        if self.max > self.min {
            (self.value() - self.min) / (self.max - self.min)
        } else {
            0.0
        }
    }

    /// The amount that arrow keys change the value by
    fn keyboard_step(&self) -> f64 {
        self.step.unwrap_or((self.max - self.min) / 100.0)
    }

    /// Snap a value to the nearest step and clamp it to the `[min, max]` range
    fn snap_and_clamp(&self, value: f64) -> f64 {
        let stepped = match self.step {
            Some(step) => self.min + ((value - self.min) / step).round() * step,
            None => value,
        };
        // Round to avoid floating point artifacts (e.g. 0.30000000000000004) leaking
        // into the value attribute
        let rounded = (stepped * 1e6).round() / 1e6;
        rounded.clamp(self.min, self.max.max(self.min))
    }

    /// Update the value, writing it back to the element's `value` attribute and
    /// dispatching a DOM "input" event if it changed.
    fn set_value(&mut self, new_value: f64, ctx: &mut WidgetEventContext) {
        let new_value = self.snap_and_clamp(new_value);
        if self.value != Some(new_value) {
            self.value = Some(new_value);
            let value_str = format!("{new_value}");
            ctx.set_attribute(qual_name!("value"), value_str.clone());
            ctx.dispatch_input_event(value_str);
            ctx.request_redraw();
        }
    }

    /// Set the value from the x coordinate of a pointer event (relative to the widget's box)
    fn set_value_from_position(&mut self, x: f32, ctx: &mut WidgetEventContext) {
        let width = ctx.width as f64;
        let height = ctx.height as f64;
        let thumb_radius = thumb_radius(width, height, 1.0);
        let track_width = (width - thumb_radius * 2.0).max(0.0);
        let fraction = if track_width > 0.0 {
            ((x as f64 - thumb_radius) / track_width).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.set_value(self.min + fraction * (self.max - self.min), ctx);
    }
}

impl Widget for RangeInputWidget {
    fn attribute_changed(&mut self, name: &str, _old_value: Option<&str>, new_value: Option<&str>) {
        match name {
            "min" => self.min = new_value.and_then(parse_float).unwrap_or(0.0),
            "max" => self.max = new_value.and_then(parse_float).unwrap_or(100.0),
            "step" => {
                self.step = match new_value {
                    Some(value) if value.trim().eq_ignore_ascii_case("any") => None,
                    Some(value) => {
                        Some(parse_float(value).filter(|step| *step > 0.0).unwrap_or(1.0))
                    }
                    None => Some(1.0),
                }
            }
            "value" => self.value = new_value.and_then(parse_float),
            "disabled" => self.disabled = new_value.is_some(),
            _ => {}
        }
    }

    fn intrinsic_size(&mut self) -> WidgetIntrinsicSize {
        WidgetIntrinsicSize {
            width: Some(160.0),
            height: Some(16.0),
            aspect_ratio: None,
        }
    }

    fn handle_event(&mut self, event: &UiEvent, ctx: &mut WidgetEventContext) {
        if self.disabled {
            self.dragging = false;
            return;
        }

        match event {
            UiEvent::PointerDown(evt) if evt.button == MouseEventButton::Main => {
                self.dragging = true;
                // Capture the pointer so that the drag continues to work even when
                // the pointer moves outside of the widget's bounds
                ctx.set_pointer_capture(evt.id);
                self.set_value_from_position(evt.coords.page_x, ctx);
            }
            UiEvent::PointerMove(evt) if self.dragging => {
                if evt.buttons.contains(MouseEventButtons::Primary) {
                    self.set_value_from_position(evt.coords.page_x, ctx);
                } else {
                    // The primary button is no longer pressed
                    self.dragging = false;
                }
            }
            UiEvent::PointerUp(_) | UiEvent::PointerCancel(_) => self.dragging = false,
            UiEvent::KeyDown(evt) if evt.state.is_pressed() => {
                let value = self.value();
                match evt.key {
                    Key::ArrowLeft | Key::ArrowDown => {
                        self.set_value(value - self.keyboard_step(), ctx)
                    }
                    Key::ArrowRight | Key::ArrowUp => {
                        self.set_value(value + self.keyboard_step(), ctx)
                    }
                    Key::Home => self.set_value(self.min, ctx),
                    Key::End => self.set_value(self.max, ctx),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn paint(
        &mut self,
        _render_ctx: &mut dyn anyrender::RenderContext,
        styles: &ComputedStyles,
        width: u32,
        height: u32,
        scale: f64,
        _ctx: &mut WidgetPaintContext,
    ) -> Scene {
        let mut scene = Scene::new();

        let width = width as f64;
        let height = height as f64;
        if width <= 0.0 || height <= 0.0 {
            return scene;
        }

        let accent_color = if self.disabled {
            Color::from_rgba8(209, 209, 209, 255)
        } else {
            styles.clone_color().as_color_color()
        };
        let track_color = if self.disabled {
            Color::from_rgba8(227, 227, 227, 255)
        } else {
            Color::from_rgba8(205, 205, 205, 255)
        };

        let thumb_radius = thumb_radius(width, height, scale);
        let track_height = (4.0 * scale).min(height);
        let track_y0 = (height - track_height) / 2.0;
        let track_radius = track_height / 2.0;
        let track_width = (width - thumb_radius * 2.0).max(0.0);
        let thumb_x = thumb_radius + self.fraction() * track_width;

        // Track
        let track = RoundedRect::new(0.0, track_y0, width, track_y0 + track_height, track_radius);
        scene.fill(Fill::NonZero, Affine::IDENTITY, track_color, None, &track);

        // Filled (active) portion of the track
        let active_track = RoundedRect::new(
            0.0,
            track_y0,
            thumb_x,
            track_y0 + track_height,
            track_radius,
        );
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            accent_color,
            None,
            &active_track,
        );

        // Thumb
        let thumb = Circle::new((thumb_x, height / 2.0), thumb_radius);
        scene.fill(Fill::NonZero, Affine::IDENTITY, accent_color, None, &thumb);

        scene
    }
}

/// The radius of the slider thumb (for a box of the given size)
fn thumb_radius(width: f64, height: f64, scale: f64) -> f64 {
    (8.0 * scale).min(height / 2.0).min(width / 2.0)
}

fn parse_float(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn defaults() {
        let widget = RangeInputWidget::new();
        assert_eq!(widget.value(), 50.0);
        assert_eq!(widget.fraction(), 0.5);
    }

    #[test]
    fn attributes_update_state() {
        let mut widget = RangeInputWidget::new();
        widget.attribute_changed("min", None, Some("10"));
        widget.attribute_changed("max", None, Some("20"));
        assert_eq!(widget.value(), 15.0);

        widget.attribute_changed("value", None, Some("18"));
        assert_eq!(widget.value(), 18.0);
        assert_eq!(widget.fraction(), 0.8);

        // Values are clamped to the [min, max] range
        widget.attribute_changed("value", None, Some("200"));
        assert_eq!(widget.value(), 20.0);

        // Removing the value attribute reverts to the default (midpoint) value
        widget.attribute_changed("value", None, None);
        assert_eq!(widget.value(), 15.0);
    }

    #[test]
    fn snaps_to_step() {
        let mut widget = RangeInputWidget::new();
        assert_eq!(widget.snap_and_clamp(41.7), 42.0);

        widget.attribute_changed("step", None, Some("10"));
        assert_eq!(widget.snap_and_clamp(44.9), 40.0);
        assert_eq!(widget.snap_and_clamp(45.1), 50.0);

        widget.attribute_changed("step", None, Some("any"));
        assert_eq!(widget.snap_and_clamp(41.7), 41.7);
    }
}
