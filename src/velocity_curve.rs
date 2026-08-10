//! Lock-free velocity mapping for physical MIDI keyboard input.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// A normalized input/output point in a MIDI velocity curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VelocityPoint {
    pub input: f64,
    pub output: f64,
}

impl VelocityPoint {
    pub const fn new(input: f64, output: f64) -> Self {
        Self { input, output }
    }
}

pub fn default_velocity_points() -> Vec<VelocityPoint> {
    vec![
        VelocityPoint::new(0.0, 0.0),
        VelocityPoint::new(0.5, 0.5),
        VelocityPoint::new(1.0, 1.0),
    ]
}

/// A 128-entry lookup table which can be read without locking on the MIDI
/// callback thread and rebuilt safely by the GTK thread.
#[derive(Clone)]
pub struct VelocityCurve {
    lookup: Arc<[AtomicU8; 128]>,
}

impl Default for VelocityCurve {
    fn default() -> Self {
        let curve = Self {
            lookup: Arc::new(std::array::from_fn(|index| AtomicU8::new(index as u8))),
        };
        curve.set_points(&default_velocity_points());
        curve
    }
}

impl VelocityCurve {
    pub fn map(&self, velocity: u8) -> u8 {
        self.lookup[velocity.min(127) as usize].load(Ordering::Relaxed)
    }

    pub fn set_points(&self, points: &[VelocityPoint]) {
        if points.len() < 2 {
            return;
        }

        let mut points = points.to_vec();
        points.sort_by(|left, right| left.input.total_cmp(&right.input));
        for point in &mut points {
            point.input = point.input.clamp(0.0, 1.0);
            point.output = point.output.clamp(0.0, 1.0);
        }

        for input_velocity in 0..=127 {
            let input = input_velocity as f64 / 127.0;
            let right = points.partition_point(|point| point.input < input);
            let output = if right == 0 {
                points[0].output
            } else if right == points.len() {
                points[points.len() - 1].output
            } else {
                let left = points[right - 1];
                let right = points[right];
                let width = right.input - left.input;
                let amount = if width <= f64::EPSILON {
                    1.0
                } else {
                    (input - left.input) / width
                };
                left.output + (right.output - left.output) * amount
            };
            let mapped = (output.clamp(0.0, 1.0) * 127.0).round() as u8;
            self.lookup[input_velocity].store(mapped, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_curve_is_linear() {
        let curve = VelocityCurve::default();
        for velocity in 0..=127 {
            assert_eq!(curve.map(velocity), velocity);
        }
    }

    #[test]
    fn interpolates_custom_control_points() {
        let curve = VelocityCurve::default();
        curve.set_points(&[
            VelocityPoint::new(0.0, 0.0),
            VelocityPoint::new(0.5, 1.0),
            VelocityPoint::new(1.0, 1.0),
        ]);

        assert_eq!(curve.map(0), 0);
        assert!((curve.map(32) as i16 - 64).abs() <= 1);
        assert_eq!(curve.map(64), 127);
        assert_eq!(curve.map(127), 127);
    }
}
