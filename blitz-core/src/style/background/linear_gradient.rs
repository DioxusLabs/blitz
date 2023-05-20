use std::f64::consts::PI;
use taffy::prelude::Size;
use vello::kurbo::Point;

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct LinearGradient {
    angle_radians: f64,
}

impl LinearGradient {
    pub fn new(angle: f64) -> Self {
        Self {
            angle_radians: angle.to_radians(),
        }
    }

    pub fn center_offset(&self, size: Size<f32>) -> Point {
        angle_to_center_offset(self.angle_radians, size)
    }
}

// https://developer.mozilla.org/en-US/docs/Web/CSS/gradient/linear-gradient#composition_of_a_linear_gradient
// Graphed visualization: https://www.desmos.com/calculator/7vfcr5kczy
fn half_length_q1(width: f64, height: f64, angle: f64) -> f64 {
    ((height / width).atan() - angle).cos() * (width.powi(2) + height.powi(2)).sqrt()
}

fn angle_to_center_offset(full_angle: f64, size: Size<f32>) -> Point {
    let x = size.width as f64 / 2.;
    let y = size.height as f64 / 2.;
    let full_angle = full_angle % (2. * PI);
    let angle = full_angle % (PI / 2.);
    // Q1
    if (0.0..PI / 2.).contains(&full_angle) {
        let length = half_length_q1(x, y, angle);
        (angle.cos() * length, angle.sin() * length).into()
    }
    // Q2
    else if ((PI / 2.)..PI).contains(&full_angle) {
        let length = half_length_q1(y, x, angle);
        (-angle.sin() * length, angle.cos() * length).into()
    }
    // Q3
    else if (PI..3. * PI / 2.).contains(&full_angle) {
        let length = half_length_q1(x, y, angle);
        (-angle.cos() * length, -angle.sin() * length).into()
    }
    // Q4
    else {
        let length = half_length_q1(y, x, angle);
        (angle.sin() * length, -angle.cos() * length).into()
    }
}

#[test]
fn gradient_offset() {
    // Check that when the angle points dirrectly to a midpoint of a side the offset is correct
    assert_eq!(
        angle_to_center_offset(
            0.0,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(50., 0.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            PI / 2.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(0., 50.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            PI,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(-50., 0.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            3. * PI / 2.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(0., -50.).round()
    );

    // Check that when the angle points to a corner or midpoint of a side the offset is correct
    assert_eq!(
        angle_to_center_offset(
            PI / 4.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(50., 50.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            3. * PI / 4.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(-50., 50.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            5. * PI / 4.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(-50., -50.).round()
    );
    assert_eq!(
        angle_to_center_offset(
            7. * PI / 4.,
            Size {
                width: 100.,
                height: 100.,
            }
        )
        .round(),
        Point::new(50., -50.).round()
    );
}
