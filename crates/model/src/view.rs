//! Where a model is seen from: six numbers a person can read and change in the text.

use std::fmt;

use crate::ModelFacts;

/// Where a model is seen from. The eye is `distance` from `target`, turned `yaw` degrees round
/// the model's up axis (y) from the front, positive to the right (towards x), and raised `pitch`
/// degrees above the ground, looking at `target`. From the front, yaw and pitch 0, the eye
/// looks along -z, as a model's front faces z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    /// In the model's own units, always more than nothing.
    pub distance: f32,
    pub target: [f32; 3],
}

/// How far above the ground the framing view looks from, in degrees.
const FRAMING_PITCH: f32 = 20.0;

/// How much room the framing view leaves round a model's sphere: a tenth.
const FRAMING_ROOM: f32 = 1.1;

impl View {
    /// How much is seen across the picture's shorter side, in degrees.
    pub const FIELD_OF_VIEW: f32 = 30.0;

    /// The whole model in sight, from the front and a little above, looking at its middle.
    ///
    /// A point cloud's `facts` gives a box of nothing, its header not saying where its points
    /// lie: give this the bounds from `read(...).facts` for a cloud. A box of nothing, or one
    /// that isn't made of numbers, is framed as a sphere of radius 1 round its middle.
    pub fn framing(facts: &ModelFacts) -> View {
        let (low, high) = facts.bounds;
        let mut target = [0, 1, 2].map(|i| (low[i] + high[i]) / 2.0);
        if !target.iter().all(|v| v.is_finite()) {
            target = [0.0; 3];
        }
        let half = [0, 1, 2].map(|i| (high[i] - low[i]) / 2.0);
        let radius = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt();
        let radius = if radius.is_finite() && radius > f32::MIN_POSITIVE * 1e6 { radius } else { 1.0 };
        let distance = radius / (Self::FIELD_OF_VIEW / 2.0).to_radians().sin() * FRAMING_ROOM;
        View { yaw: 0.0, pitch: FRAMING_PITCH, distance, target }
    }

    /// Reads the six numbers `yaw pitch distance x y z`, as `Display` writes them. Every number
    /// must be finite and the distance more than nothing.
    pub fn parse(text: &str) -> Option<View> {
        let mut numbers = [0.0f32; 6];
        let mut words = text.split_whitespace();
        for number in &mut numbers {
            *number = words.next()?.parse().ok()?;
            if !number.is_finite() {
                return None;
            }
        }
        if words.next().is_some() || numbers[2] <= 0.0 {
            return None;
        }
        let [yaw, pitch, distance, x, y, z] = numbers;
        Some(View { yaw, pitch, distance, target: [x, y, z] })
    }

    /// The view `t` of the way from `a` to `b`, `t` from 0 to 1: starting and ending gently,
    /// turning the short way round, and zooming evenly however far it goes.
    pub fn between(a: View, b: View, t: f32) -> View {
        if t.is_nan() || t <= 0.0 {
            return a;
        }
        if t >= 1.0 {
            return b;
        }
        let s = t * t * (3.0 - 2.0 * t);
        let turn = (b.yaw - a.yaw + 180.0).rem_euclid(360.0) - 180.0;
        let distance = a.distance * (b.distance / a.distance).powf(s);
        View {
            yaw: a.yaw + turn * s,
            pitch: a.pitch + (b.pitch - a.pitch) * s,
            distance: if distance.is_finite() && distance > 0.0 { distance } else { a.distance + (b.distance - a.distance) * s },
            target: [0, 1, 2].map(|i| a.target[i] + (b.target[i] - a.target[i]) * s),
        }
    }

    /// Where the eye is.
    pub fn eye(&self) -> [f32; 3] {
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.clamp(-MOST_PITCH, MOST_PITCH).to_radians());
        let t = self.target;
        let d = self.distance;
        [t[0] + d * pitch.cos() * yaw.sin(), t[1] + d * pitch.sin(), t[2] + d * pitch.cos() * yaw.cos()]
    }
}

/// How far up or down the eye is raised when drawing: straight up or down, the way up can't be
/// told.
pub(crate) const MOST_PITCH: f32 = 89.9;

/// Writes the six numbers, each as briefly as reads back the same, with no minus on a nothing.
impl fmt::Display for View {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let numbers = [self.yaw, self.pitch, self.distance, self.target[0], self.target[1], self.target[2]];
        for (i, number) in numbers.into_iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            // Adding nothing makes a minus nothing plain nothing.
            write!(f, "{}", number + 0.0)?;
        }
        Ok(())
    }
}
