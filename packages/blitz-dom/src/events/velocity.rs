//! Velocity estimation adapted from Flutter's BSD-licensed VelocityTracker and
//! LeastSquaresSolver (Copyright 2014 The Flutter Authors).

use std::collections::VecDeque;
use web_time::Instant;

use crate::util::Point;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct VelocityTracker {
    samples: VecDeque<(Instant, Point<f64>)>,
    position: Point<f64>,
}

impl VelocityTracker {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            samples: VecDeque::from([(now, Point::ZERO)]),
            position: Point::ZERO,
        }
    }

    pub(super) fn record(&mut self, now: Instant, dx: f64, dy: f64) {
        self.position.x += dx;
        self.position.y += dy;
        if self.samples.len() == 20 {
            self.samples.pop_front();
        }
        self.samples.push_back((now, self.position));
    }

    pub(super) fn velocity(&self, now: Instant) -> Option<Point<f64>> {
        let (newest, _) = self.samples.back()?;
        if now.duration_since(*newest).as_secs_f64() > 0.040 {
            return None;
        }
        let mut previous = *newest;
        let mut times = Vec::new();
        let mut x = Vec::new();
        let mut y = Vec::new();
        for (time, position) in self.samples.iter().rev() {
            let age = newest.duration_since(*time).as_secs_f64() * 1000.0;
            if age > 100.0 || previous.duration_since(*time).as_secs_f64() > 0.040 {
                break;
            }
            previous = *time;
            times.push(-age);
            x.push(position.x);
            y.push(position.y);
        }
        if times.len() < 2 {
            return None;
        }
        let estimate = |values: &[f64]| {
            if times.len() == 2 {
                let dt = times[0] - times[1];
                (dt > 0.0).then(|| (values[0] - values[1]) / dt)
            } else {
                quadratic_velocity(&times, values)
            }
        };
        Some(Point {
            x: estimate(&x)? * 1000.0,
            y: estimate(&y)? * 1000.0,
        })
    }
}

fn quadratic_velocity(times: &[f64], values: &[f64]) -> Option<f64> {
    let n = times.len();
    let a = [
        vec![1.0; n],
        times.to_vec(),
        times.iter().map(|t| t * t).collect(),
    ];
    let mut q = a.clone();
    let mut r = [[0.0; 3]; 3];
    for j in 0..3 {
        for i in 0..j {
            let dot: f64 = q[j].iter().zip(&q[i]).map(|(a, b)| a * b).sum();
            let (before, current) = q.split_at_mut(j);
            for (value, basis) in current[0].iter_mut().zip(&before[i]) {
                *value -= dot * basis;
            }
        }
        let norm = q[j].iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm < 1e-10 {
            return None;
        }
        for v in &mut q[j] {
            *v /= norm;
        }
        for (i, row) in a.iter().enumerate().skip(j) {
            r[j][i] = q[j].iter().zip(row).map(|(a, b)| a * b).sum();
        }
    }
    let mut coefficients = [0.0; 3];
    for i in (0..3).rev() {
        let rhs: f64 = q[i].iter().zip(values).map(|(a, b)| a * b).sum();
        let remainder: f64 = ((i + 1)..3).map(|j| r[i][j] * coefficients[j]).sum();
        coefficients[i] = (rhs - remainder) / r[i][i];
    }
    Some(coefficients[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use web_time::Duration;

    #[test]
    fn constant_velocity_and_acceleration() {
        let start = Instant::now();
        for acceleration in [0.0, 2000.0] {
            let mut tracker = VelocityTracker::new(start);
            let mut previous = 0.0;
            for i in 1..=10 {
                let t = i as f64 * 0.0085;
                let position = 300.0 * t + 0.5 * acceleration * t * t;
                tracker.record(
                    start + Duration::from_secs_f64(t),
                    position - previous,
                    -(position - previous) * 2.0,
                );
                previous = position;
            }
            let velocity = tracker.velocity(start + Duration::from_millis(85)).unwrap();
            assert!((velocity.x - (300.0 + acceleration * 0.085)).abs() < 1e-6);
            assert!((velocity.y + 2.0 * velocity.x).abs() < 1e-6);
        }
    }

    #[test]
    fn two_points_and_stopped_release() {
        let start = Instant::now();
        let mut tracker = VelocityTracker::new(start);
        assert!(tracker.velocity(start).is_none());
        tracker.record(start + Duration::from_millis(10), 10.0, 0.0);
        assert_eq!(
            tracker
                .velocity(start + Duration::from_millis(50))
                .unwrap()
                .x,
            1000.0
        );
        assert!(
            tracker
                .velocity(start + Duration::from_micros(50_001))
                .is_none()
        );
    }

    #[test]
    fn gaps_horizon_capacity_and_degenerate_samples() {
        let start = Instant::now();
        let mut tracker = VelocityTracker::new(start);
        tracker.record(start + Duration::from_millis(50), 100.0, 0.0);
        assert!(
            tracker
                .velocity(start + Duration::from_millis(50))
                .is_none()
        );
        for i in 1..=30 {
            tracker.record(start + Duration::from_millis(50 + i * 10), 1.0, 0.0);
        }
        assert_eq!(tracker.samples.len(), 20);
        assert!(
            (tracker
                .velocity(start + Duration::from_millis(350))
                .unwrap()
                .x
                - 100.0)
                .abs()
                < 1e-6
        );
        let mut tracker = VelocityTracker::new(start);
        tracker.record(start, 1.0, 0.0);
        tracker.record(start, 1.0, 0.0);
        assert!(tracker.velocity(start).is_none());
    }
}
