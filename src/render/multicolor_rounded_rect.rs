//! A rounded rect closer to the browser
//! Implemented in such a way that splits the border into 4 parts at the midway of each radius

use std::{
    f64::consts::FRAC_PI_4,
    f64::consts::PI,
    f64::consts::{FRAC_PI_2, FRAC_PI_8},
};

use style::properties::longhands::width;
use vello::kurbo::{Arc, ArcAppendIter, PathEl, Point, RoundedRect, RoundedRectRadii, Shape, Vec2};

pub struct SplitRoundedRect {
    pub rect: RoundedRect,
}

pub struct RectArcs {
    pub top: [Arc; 2],
    pub right: [Arc; 2],
    pub bottom: [Arc; 2],
    pub left: [Arc; 2],
}

impl SplitRoundedRect {
    // Split a rounded rect up into propery slices
    pub fn new(rect: RoundedRect) -> Self {
        Self { rect }
    }

    #[rustfmt::skip]
    pub fn arcs(
        &self,
        top_width: f64,
        right_width: f64,
        bottom_width: f64,
        left_width: f64,
    ) -> RectArcs {
        let RoundedRectRadii {
            top_left: tl,
            top_right: tr,
            bottom_right: br,
            bottom_left: bl,
        } = self.rect.radii();
        let width = self.rect.width();
        let height = self.rect.height();

        RectArcs {
            top: [
                self.arc(-FRAC_PI_4, tl, tl, tl),  // start at top left (mid -> end)
                self.arc(0.0, width - tr, tr, tr), // jump to top right arc and (start -> mid)
            ],
            right: [
                self.arc(FRAC_PI_4, width - tr, tr, tr), // jump to top right arc and (start -> mid)
                self.arc(FRAC_PI_2, width - br, height - br, br), // jump to top right arc and (start -> mid)
            ],
            bottom: [
                self.arc(FRAC_PI_2 + FRAC_PI_4, width - br, height - br, br), // jump to top right arc and (start -> mid)
                self.arc(PI, bl, height - bl, bl), // jump to top right arc and (start -> mid)
            ],
            left: [
                self.arc(PI + FRAC_PI_4, bl, height - bl, bl), // jump to top right arc and (start -> mid)
                self.arc(PI + FRAC_PI_2, tl, tl, tl), // jump to top right arc and (start -> mid)
            ],
        }
    }

    pub fn arc(&self, start_angle: f64, x_offset: f64, y_offset: f64, radius: f64) -> Arc {
        Arc {
            // For whatever reason, kurbo starts 0 and the x origin and rotates clockwise
            // Mentally I think of it as starting at the y origin (unit circle)
            x_rotation: PI + FRAC_PI_2,
            center: Point {
                x: self.rect.rect().x0 + x_offset,
                y: self.rect.rect().y0 + y_offset,
            },
            radii: Vec2 {
                x: radius,
                y: radius,
            },
            start_angle,
            sweep_angle: FRAC_PI_4,
        }
    }
}
