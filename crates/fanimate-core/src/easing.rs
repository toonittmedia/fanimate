//! Easing: how a value moves from one key to the next.
//!
//! Smooth, Linear and Step (in [`crate::anim::Interpolation`]) cover most
//! keys. Animators also reach for a ball that bounces to rest, a pop that
//! overshoots and settles, or a curve drawn by hand in the graph editor.
//! Those are here: a handful of named [`Easing`]s, and [`Bezier`], the cubic
//! curve the graph editor edits.
//!
//! Both turn the linear progress `t` between two keys (0 to 1) into the
//! eased progress the value is mixed by. The result may leave 0..=1 (that is
//! what overshoot is), so a value that cannot go past its ends, such as a
//! colour, is held within its range where it is mixed.

use serde::{Deserialize, Serialize};

/// A named easing. Each starts at 0 and ends at exactly 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    /// Starts slowly, ends at speed.
    EaseIn,
    /// Starts at speed, settles gently.
    EaseOut,
    /// Slow at both ends.
    EaseInOut,
    /// Goes a little past the end, then settles back.
    Overshoot,
    /// Falls to the end and bounces back up a few times, each smaller.
    Bounce,
    /// Swings past the end and back, like a spring, each swing smaller.
    Elastic,
}

impl Easing {
    pub const ALL: [Easing; 6] = [
        Self::EaseIn,
        Self::EaseOut,
        Self::EaseInOut,
        Self::Overshoot,
        Self::Bounce,
        Self::Elastic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::EaseIn => "Ease in",
            Self::EaseOut => "Ease out",
            Self::EaseInOut => "Ease in and out",
            Self::Overshoot => "Overshoot",
            Self::Bounce => "Bounce",
            Self::Elastic => "Elastic",
        }
    }

    /// The eased progress for linear progress `t`. `t` outside 0..=1, or not a
    /// number, counts as the nearest end.
    pub fn apply(self, t: f32) -> f32 {
        let t = unit(t);
        if t <= 0.0 || t >= 1.0 {
            return t;
        }
        match self {
            Self::EaseIn => Bezier([420, 0, 1000, 1000]).apply(t),
            Self::EaseOut => Bezier([0, 0, 580, 1000]).apply(t),
            Self::EaseInOut => Bezier([420, 0, 580, 1000]).apply(t),
            Self::Overshoot => Bezier([340, 1560, 640, 1000]).apply(t),
            Self::Bounce => bounce(t),
            Self::Elastic => {
                let c = std::f32::consts::TAU / 3.0;
                2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c).sin() + 1.0
            }
        }
    }
}

/// `t` held within 0..=1; NaN is 0.
fn unit(t: f32) -> f32 {
    if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) }
}

/// A ball dropped on the end value: parabolas that meet it, each bounce lower.
fn bounce(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984_375
    }
}

/// A cubic Bézier easing curve from (0, 0) to (1, 1) with control points
/// (x1, y1) and (x2, y2), kept in thousandths as `[x1, y1, x2, y2]` so a
/// curve can be compared, hashed and saved exactly. The x values stay within
/// 0 to 1000, which keeps time running forward; the y values may go past the
/// ends, for overshoot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bezier(pub [i16; 4]);

impl Bezier {
    /// The lowest and highest y a control point may have, in thousandths.
    pub const Y_RANGE: (i16, i16) = (-2000, 3000);

    /// A curve from control points given as fractions, rounded to the nearest
    /// thousandth and held within what a curve may be.
    pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        let q = |v: f32| if v.is_finite() { (v * 1000.0).round() } else { 0.0 };
        Self([q(x1) as i16, q(y1) as i16, q(x2) as i16, q(y2) as i16]).repaired()
    }

    /// Whether the control points are within what a curve may be.
    pub fn is_sound(&self) -> bool {
        let [x1, y1, x2, y2] = self.0;
        let (lo, hi) = Self::Y_RANGE;
        (0..=1000).contains(&x1)
            && (0..=1000).contains(&x2)
            && (lo..=hi).contains(&y1)
            && (lo..=hi).contains(&y2)
    }

    /// The same curve with each control point held within range.
    pub fn repaired(self) -> Self {
        let [x1, y1, x2, y2] = self.0;
        let (lo, hi) = Self::Y_RANGE;
        Self([
            x1.clamp(0, 1000),
            y1.clamp(lo, hi),
            x2.clamp(0, 1000),
            y2.clamp(lo, hi),
        ])
    }

    /// The control points as fractions: (x1, y1, x2, y2).
    pub fn points(&self) -> [f32; 4] {
        self.0.map(|v| f32::from(v) / 1000.0)
    }

    /// The eased progress for linear progress `t`: the curve's y where its x
    /// is `t`. `t` outside 0..=1, or not a number, counts as the nearest end.
    pub fn apply(&self, t: f32) -> f32 {
        let t = unit(t);
        if t <= 0.0 || t >= 1.0 {
            return t;
        }
        let [x1, y1, x2, y2] = self.repaired().points();
        // x(s) rises with s, because x1 and x2 are within 0..=1: find the s
        // where x(s) = t, by Newton's method, and by bisection if it stalls
        let point = |s: f32, a: f32, b: f32| {
            let u = 1.0 - s;
            3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
        };
        let slope = |s: f32| {
            let u = 1.0 - s;
            3.0 * u * u * x1 + 6.0 * u * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2)
        };
        let mut s = t;
        for _ in 0..8 {
            let err = point(s, x1, x2) - t;
            if err.abs() < 1e-6 {
                return point(s, y1, y2);
            }
            let d = slope(s);
            if d.abs() < 1e-6 {
                break;
            }
            s = (s - err / d).clamp(0.0, 1.0);
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        s = t;
        for _ in 0..40 {
            if point(s, x1, x2) < t {
                lo = s;
            } else {
                hi = s;
            }
            s = 0.5 * (lo + hi);
        }
        point(s, y1, y2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> impl Iterator<Item = f32> {
        (0..=100).map(|i| i as f32 / 100.0)
    }

    #[test]
    fn every_easing_starts_at_zero_and_ends_at_exactly_one() {
        for e in Easing::ALL {
            assert_eq!(e.apply(0.0), 0.0, "{e:?}");
            assert_eq!(e.apply(1.0), 1.0, "{e:?}");
            for t in steps() {
                assert!(e.apply(t).is_finite(), "{e:?} at {t}");
            }
        }
    }

    #[test]
    fn input_that_is_not_a_progress_counts_as_the_nearest_end() {
        for e in Easing::ALL {
            assert_eq!(e.apply(-3.0), 0.0);
            assert_eq!(e.apply(9.0), 1.0);
            assert_eq!(e.apply(f32::NAN), 0.0);
            assert_eq!(e.apply(f32::INFINITY), 1.0);
            assert_eq!(e.apply(f32::NEG_INFINITY), 0.0);
        }
        assert_eq!(Bezier([250, 100, 750, 900]).apply(f32::NAN), 0.0);
    }

    #[test]
    fn ease_in_starts_slow_ease_out_starts_fast_and_both_only_rise() {
        assert!(Easing::EaseIn.apply(0.25) < 0.25);
        assert!(Easing::EaseOut.apply(0.25) > 0.25);
        let mid = Easing::EaseInOut.apply(0.5);
        assert!((mid - 0.5).abs() < 0.01, "{mid}");
        assert!(Easing::EaseInOut.apply(0.2) < 0.2 && Easing::EaseInOut.apply(0.8) > 0.8);
        for e in [Easing::EaseIn, Easing::EaseOut, Easing::EaseInOut] {
            let mut last = 0.0;
            for t in steps() {
                let v = e.apply(t);
                assert!(v >= last - 1e-5, "{e:?} fell from {last} to {v} at {t}");
                last = v;
            }
        }
    }

    #[test]
    fn overshoot_and_elastic_go_past_one_and_bounce_never_does() {
        let top = |e: Easing| steps().map(|t| e.apply(t)).fold(f32::MIN, f32::max);
        assert!(top(Easing::Overshoot) > 1.05, "{}", top(Easing::Overshoot));
        assert!(top(Easing::Elastic) > 1.05, "{}", top(Easing::Elastic));
        assert!(top(Easing::Bounce) <= 1.0 + 1e-6);
        // the bounce touches the end value several times on the way
        let touches = steps()
            .filter(|&t| (Easing::Bounce.apply(t) - 1.0).abs() < 0.02)
            .count();
        assert!(touches >= 3, "{touches}");
        // and none of them go below the start
        for e in Easing::ALL {
            assert!(steps().all(|t| e.apply(t) > -0.05), "{e:?}");
        }
    }

    #[test]
    fn a_straight_bezier_is_linear() {
        let line = Bezier([0, 0, 1000, 1000]);
        for t in steps() {
            assert!((line.apply(t) - t).abs() < 1e-3, "{t}: {}", line.apply(t));
        }
    }

    #[test]
    fn a_bezier_is_solved_for_time_not_for_its_parameter() {
        // check by running the curve forward: the x it reaches is the t asked for
        for curve in [
            [420, 0, 580, 1000],
            [0, 0, 580, 1000],
            [1000, 0, 0, 1000],
            [250, 100, 750, 900],
            [0, 1000, 1000, 0],
        ] {
            let b = Bezier(curve);
            let [x1, y1, x2, y2] = b.points();
            for t in steps() {
                let y = b.apply(t);
                // find s with y(s) = y and x(s) = t by scanning s finely
                let best = (0..=2000)
                    .map(|i| i as f32 / 2000.0)
                    .map(|s| {
                        let u = 1.0 - s;
                        let x = 3.0 * u * u * s * x1 + 3.0 * u * s * s * x2 + s * s * s;
                        let yy = 3.0 * u * u * s * y1 + 3.0 * u * s * s * y2 + s * s * s;
                        (x - t).abs() + (yy - y).abs()
                    })
                    .fold(f32::MAX, f32::min);
                assert!(best < 2e-3, "{curve:?} at {t}: off by {best}");
            }
        }
    }

    #[test]
    fn a_curve_is_quantised_clamped_and_never_unsound_after_repair() {
        let b = Bezier::new(0.4251, -0.5, 0.58, 1.5);
        assert_eq!(b.0, [425, -500, 580, 1500]);
        assert!(b.is_sound());
        let wild = Bezier::new(-3.0, 99.0, 9.0, f32::NAN);
        assert!(wild.is_sound(), "{wild:?}");
        let raw = Bezier([-5, i16::MIN, 2000, i16::MAX]);
        assert!(!raw.is_sound());
        assert!(raw.repaired().is_sound());
        // an unsound curve still evaluates, as its repaired self
        assert_eq!(raw.apply(0.5), raw.repaired().apply(0.5));
    }

    #[test]
    fn easings_and_curves_are_saved_as_short_readable_names() {
        use crate::anim::Interpolation;
        for (interp, json) in [
            (Interpolation::Smooth, "\"smooth\""),
            (Interpolation::Ease(Easing::Bounce), r#"{"ease":"bounce"}"#),
            (
                Interpolation::Ease(Easing::EaseInOut),
                r#"{"ease":"ease_in_out"}"#,
            ),
            (
                Interpolation::Bezier(Bezier([250, 100, 250, 1000])),
                r#"{"bezier":[250,100,250,1000]}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&interp).unwrap(), json);
            assert_eq!(serde_json::from_str::<Interpolation>(json).unwrap(), interp);
        }
    }
}
