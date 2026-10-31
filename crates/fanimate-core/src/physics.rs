//! Secondary motion: a bone that follows its keyed angle on a spring, and is
//! thrown about by the turning of the bones above it, so a tail, an ear or a
//! scarf swings and settles by itself.
//!
//! The simulation is a [`Stepper`]: where a bone is on frame 80 is a pure
//! function of its settings and the keyed motion up to frame 80, so the
//! playhead may jump anywhere and show the same thing. Its result is meant
//! to be **baked to keys** ([`bake_spring`]): ordinary keys on the bone's
//! angle, one a frame, that the animator can then edit, retime or delete. A
//! baked file needs nothing from this module to play, so projects stay
//! readable by every program that reads bones.

use crate::anim::{Frame, Interpolation, set_key_in};
use crate::bones::Skeleton;
use crate::stateful::{StateCache, Stepper};

/// The longest range baked in one go, in frames (about 14 minutes at 24 a second).
pub const MAX_BAKE_FRAMES: Frame = 20_000;

/// The most substeps a frame is divided into, to keep a stiff spring stable.
const MAX_SUBSTEPS: u32 = 256;

/// How a spring behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringParams {
    /// How hard the bone is pulled back to its keyed angle, in 1/s squared.
    /// 100 is a loose tail, 1000 a stiff one.
    pub stiffness: f32,
    /// How fast the swing dies away, in 1/s. About twice the square root of
    /// the stiffness is the least that stops it overshooting.
    pub damping: f32,
    /// How much the turning of the bones above throws it: 0 none, 1 as a
    /// free weight would be, more for an exaggerated whip.
    pub inertia: f32,
    /// How hard gravity pulls the bone down the canvas, as a turning
    /// acceleration in 1/s squared: a bone pointing sideways sags towards
    /// hanging, one already hanging is not pulled. 0 is none; 10 droops a
    /// loose tail gently.
    pub gravity: f32,
    /// How hard a steady wind blows the bone to the right (negative, to the
    /// left), in the same units: a bone hanging down is swung out sideways
    /// until the spring holds it.
    pub wind: f32,
}

impl Default for SpringParams {
    fn default() -> Self {
        Self {
            stiffness: 120.0,
            damping: 8.0,
            inertia: 1.0,
            gravity: 0.0,
            wind: 0.0,
        }
    }
}

impl SpringParams {
    pub const STIFFNESS_RANGE: (f32, f32) = (0.0, 10_000.0);
    pub const DAMPING_RANGE: (f32, f32) = (0.0, 1_000.0);
    pub const INERTIA_RANGE: (f32, f32) = (0.0, 10.0);
    pub const GRAVITY_RANGE: (f32, f32) = (0.0, 200.0);
    pub const WIND_RANGE: (f32, f32) = (-200.0, 200.0);

    /// Whether every value is a number within its range.
    pub fn is_sound(&self) -> bool {
        let within = |v: f32, (lo, hi): (f32, f32)| v.is_finite() && (lo..=hi).contains(&v);
        within(self.stiffness, Self::STIFFNESS_RANGE)
            && within(self.damping, Self::DAMPING_RANGE)
            && within(self.inertia, Self::INERTIA_RANGE)
            && within(self.gravity, Self::GRAVITY_RANGE)
            && within(self.wind, Self::WIND_RANGE)
    }

    /// The same, with every value held in its range (a value that is not a
    /// number takes the default).
    pub fn held(self) -> Self {
        let d = Self::default();
        let hold = |v: f32, default: f32, (lo, hi): (f32, f32)| {
            if v.is_finite() { v.clamp(lo, hi) } else { default }
        };
        Self {
            stiffness: hold(self.stiffness, d.stiffness, Self::STIFFNESS_RANGE),
            damping: hold(self.damping, d.damping, Self::DAMPING_RANGE),
            inertia: hold(self.inertia, d.inertia, Self::INERTIA_RANGE),
            gravity: hold(self.gravity, d.gravity, Self::GRAVITY_RANGE),
            wind: hold(self.wind, d.wind, Self::WIND_RANGE),
        }
    }
}

/// Where a spring is and how fast it is going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringState {
    pub angle: f32,
    pub velocity: f32,
}

/// One bone's angle on a spring. `target` is the keyed angle and `drive` the
/// angular acceleration of the bones above it, both one value per frame from
/// the frame before the first (index 0) to the frame after the last.
pub struct AngleSpring<'a> {
    pub params: SpringParams,
    pub fps: f32,
    /// The first frame that is stepped; `target[0]` is the frame before it.
    pub first: Frame,
    pub target: &'a [f32],
    pub drive: &'a [f32],
    /// The world angle of the bone's parent on every frame (the turning of everything above it),
    /// the same length as `target`: with the bone's own angle it says which way the bone points,
    /// which is what gravity and wind push against.
    pub above: &'a [f32],
    /// The angles the bone may not leave, relative to its parent.
    pub limits: Option<[f32; 2]>,
}

impl AngleSpring<'_> {
    fn at(values: &[f32], index: usize) -> f32 {
        values.get(index).or(values.last()).copied().unwrap_or(0.0)
    }
}

impl Stepper for AngleSpring<'_> {
    type State = SpringState;

    fn initial(&self) -> SpringState {
        SpringState {
            angle: Self::at(self.target, 0),
            velocity: 0.0,
        }
    }

    fn step(&self, before: &SpringState, frame: Frame) -> SpringState {
        let p = self.params.held();
        let fps = if self.fps.is_finite() {
            self.fps.clamp(1.0, 480.0)
        } else {
            24.0
        };
        let index = (i64::from(frame) - i64::from(self.first) + 1).max(1) as usize;
        let (t0, t1) = (Self::at(self.target, index - 1), Self::at(self.target, index));
        let (d0, d1) = (Self::at(self.drive, index - 1), Self::at(self.drive, index));
        let (w0, w1) = (Self::at(self.above, index - 1), Self::at(self.above, index));
        let frame_dt = 1.0 / fps;
        // fast springs get shorter steps, so they stay stable
        let fastest = p.stiffness.sqrt().max(p.damping);
        let substeps = ((fastest * frame_dt / 0.25).ceil() as u32).clamp(1, MAX_SUBSTEPS);
        let dt = frame_dt / substeps as f32;
        let (mut x, mut v) = (before.angle, before.velocity);
        for s in 0..substeps {
            let along = (s as f32 + 1.0) / substeps as f32;
            let target = t0 + (t1 - t0) * along;
            let drive = d0 + (d1 - d0) * along;
            // gravity pulls down the canvas (y grows downwards) and wind to the right: each pushes
            // across the bone, so it turns the bone by the part of the force at right angles to it
            let world = w0 + (w1 - w0) * along + x;
            let field = p.gravity * world.cos() - p.wind * world.sin();
            let a = p.stiffness * (target - x) - p.damping * v - p.inertia * drive + field;
            v += a * dt;
            x += v * dt;
            if let Some([lo, hi]) = self.limits
                && lo <= hi
            {
                if x < lo {
                    (x, v) = (lo, v.max(0.0));
                } else if x > hi {
                    (x, v) = (hi, v.min(0.0));
                }
            }
        }
        if x.is_finite() && v.is_finite() {
            SpringState {
                angle: x,
                velocity: v,
            }
        } else {
            // cannot happen with the clamps above; if it ever did, follow the key
            SpringState {
                angle: t1,
                velocity: 0.0,
            }
        }
    }
}

/// Why a bake could not be done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BakeError {
    /// A bone that is not in the skeleton.
    NoSuchBone(usize),
    /// The same bone twice.
    RepeatedBone(usize),
    /// The last frame is before the first, or the range is longer than [`MAX_BAKE_FRAMES`].
    BadRange,
    /// The settings are not numbers within their ranges.
    BadSettings,
    /// Nothing to bake.
    NoBones,
}

impl std::fmt::Display for BakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchBone(i) => write!(f, "there is no bone {i}"),
            Self::RepeatedBone(i) => write!(f, "bone {i} was named twice"),
            Self::BadRange => write!(
                f,
                "the frames must run from the first to a later one, at most {MAX_BAKE_FRAMES} of them"
            ),
            Self::BadSettings => write!(f, "the spring settings must be numbers within their ranges"),
            Self::NoBones => write!(f, "choose a bone to bake"),
        }
    }
}

impl std::error::Error for BakeError {}

/// Run the spring on every bone of `bones` (indexes into the skeleton, any
/// order) over frames `first..=last` and write the result as an angle key on
/// every frame, replacing the keys the bone had in that range. A bone follows
/// the bones above it in the chain as they were *after* baking, so a tail of
/// several bones whips along its length.
///
/// The keys the bones had before are the spring's targets, so baking the same
/// range twice would spring the spring; undo the first to bake again with
/// other settings. Returns how many keys were written.
pub fn bake_spring(
    skeleton: &mut Skeleton,
    bones: &[usize],
    params: SpringParams,
    fps: f32,
    first: Frame,
    last: Frame,
) -> Result<usize, BakeError> {
    if bones.is_empty() {
        return Err(BakeError::NoBones);
    }
    if !params.is_sound() || !fps.is_finite() || fps <= 0.0 {
        return Err(BakeError::BadSettings);
    }
    if first < 0 || last < first || last - first >= MAX_BAKE_FRAMES {
        return Err(BakeError::BadRange);
    }
    let mut order: Vec<usize> = Vec::with_capacity(bones.len());
    for &b in bones {
        if b >= skeleton.bones.len() {
            return Err(BakeError::NoSuchBone(b));
        }
        if order.contains(&b) {
            return Err(BakeError::RepeatedBone(b));
        }
        order.push(b);
    }
    // parents come before children in a skeleton, so ascending order bakes a bone after the ones above it
    order.sort_unstable();

    let frames = (last - first + 1) as usize;
    // one value per frame from the one before the first to the one after the last
    let len = frames + 2;
    let frame_of = |i: usize| first - 1 + i as Frame;
    let original = skeleton.clone();
    let keyed = |bone: usize, i: usize| original.bones[bone].angle.sample(frame_of(i) as f32);
    let mut baked: Vec<Option<Vec<f32>>> = vec![None; skeleton.bones.len()];
    let mut keys_written = 0;

    for &bone in &order {
        let target: Vec<f32> = (0..len).map(|i| keyed(bone, i)).collect();
        // the turning of everything above, as one angle on every frame: the parent's world angle
        let mut above = vec![0.0f32; len];
        let mut up = original.bones[bone].parent;
        while let Some(p) = up {
            for (i, a) in above.iter_mut().enumerate() {
                *a += baked[p].as_ref().map_or_else(|| keyed(p, i), |sim| sim[i]);
            }
            up = original.bones[p].parent;
        }
        let fps2 = fps * fps;
        let mut drive = vec![0.0f32; len];
        for i in 1..len - 1 {
            drive[i] = (above[i + 1] - 2.0 * above[i] + above[i - 1]) * fps2;
        }
        drive[0] = drive[1];
        drive[len - 1] = drive[len - 2];

        let spring = AngleSpring {
            params,
            fps,
            first,
            target: &target,
            drive: &drive,
            above: &above,
            limits: original.bones[bone].limits,
        };
        let mut cache = StateCache::new(24);
        let mut sim = vec![0.0f32; len];
        sim[0] = target[0];
        sim[len - 1] = target[len - 1];
        for j in 0..frames {
            let state = cache.state_at(&spring, 0, first, first + j as Frame);
            sim[j + 1] = state.angle;
        }
        // replace the bone's keys in the range with one a frame
        let channel = &mut skeleton.bones[bone].angle;
        channel.keys.retain(|k| k.frame < first || k.frame > last);
        for j in 0..frames {
            set_key_in(
                &mut channel.keys,
                first + j as Frame,
                sim[j + 1],
                Interpolation::Linear,
            );
            keys_written += 1;
        }
        baked[bone] = Some(sim);
    }
    Ok(keys_written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bones::Bone;
    use glam::Vec2;

    fn chain(n: usize) -> Skeleton {
        let mut s = Skeleton::default();
        for i in 0..n {
            let mut b = Bone {
                id: 0,
                name: format!("b{i}"),
                parent: i.checked_sub(1),
                origin: Vec2::new(if i == 0 { 0.0 } else { 50.0 }, 0.0),
                length: 50.0,
                angle: crate::anim::Channel::new(0.0),
                limits: None,
                pinned: false,
                strength: None,
            };
            b.id = 0;
            s.bones.push(b);
        }
        s.ensure_ids();
        s
    }

    fn angle_at(s: &Skeleton, bone: usize, frame: Frame) -> f32 {
        s.bones[bone].angle.sample(frame as f32)
    }

    #[test]
    fn a_bone_on_a_still_key_does_not_move() {
        let mut s = chain(1);
        s.bones[0].angle.base = 0.4;
        bake_spring(&mut s, &[0], SpringParams::default(), 24.0, 1, 48).unwrap();
        for f in 1..=48 {
            assert!((angle_at(&s, 0, f) - 0.4).abs() < 1e-6, "frame {f}");
        }
    }

    #[test]
    fn a_step_in_the_key_is_followed_late_overshoots_and_settles() {
        let mut s = chain(1);
        s.bones[0].angle.set_key(1, 0.0, Interpolation::Step);
        s.bones[0].angle.set_key(11, 1.0, Interpolation::Step);
        let params = SpringParams {
            stiffness: 300.0,
            damping: 6.0,
            inertia: 0.0,
            ..SpringParams::default()
        };
        bake_spring(&mut s, &[0], params, 24.0, 1, 120).unwrap();
        assert!(angle_at(&s, 0, 10) < 0.01, "nothing before the key");
        let peak = (11..60).map(|f| angle_at(&s, 0, f)).fold(f32::MIN, f32::max);
        assert!(peak > 1.05, "a loose spring overshoots: {peak}");
        assert!(
            (angle_at(&s, 0, 120) - 1.0).abs() < 0.01,
            "and settles on the key"
        );
        // a critically damped one does not overshoot
        let mut t = chain(1);
        t.bones[0].angle.set_key(1, 0.0, Interpolation::Step);
        t.bones[0].angle.set_key(11, 1.0, Interpolation::Step);
        let critical = SpringParams {
            stiffness: 100.0,
            damping: 20.0,
            inertia: 0.0,
            ..SpringParams::default()
        };
        bake_spring(&mut t, &[0], critical, 24.0, 1, 120).unwrap();
        let peak = (11..120).map(|f| angle_at(&t, 0, f)).fold(f32::MIN, f32::max);
        assert!(peak <= 1.001, "{peak}");
    }

    #[test]
    fn the_state_after_a_frame_is_the_same_whatever_order_it_is_asked_in() {
        let target: Vec<f32> = (0..62).map(|i| if i > 10 { 1.0 } else { 0.0 }).collect();
        let drive = vec![0.0; 62];
        let above = vec![0.0; 62];
        let spring = AngleSpring {
            params: SpringParams {
                stiffness: 200.0,
                damping: 5.0,
                inertia: 0.0,
                ..SpringParams::default()
            },
            fps: 24.0,
            first: 1,
            target: &target,
            drive: &drive,
            above: &above,
            limits: None,
        };
        let mut straight = StateCache::new(8);
        let in_order: Vec<_> = (1..=60).map(|f| straight.state_at(&spring, 7, 1, f)).collect();
        let mut jumpy = StateCache::new(8);
        for f in [60, 3, 33, 59, 1, 45, 12, 60, 2] {
            assert_eq!(
                jumpy.state_at(&spring, 7, 1, f),
                in_order[(f - 1) as usize],
                "frame {f}"
            );
        }
    }

    #[test]
    fn a_bone_below_a_turning_parent_is_thrown_the_other_way_then_settles() {
        let mut s = chain(2);
        // the parent turns a quarter turn between frames 5 and 15 and stops
        s.bones[0].angle.set_key(1, 0.0, Interpolation::Linear);
        s.bones[0].angle.set_key(5, 0.0, Interpolation::Linear);
        s.bones[0].angle.set_key(15, 1.5, Interpolation::Linear);
        s.bones[0].angle.set_key(60, 1.5, Interpolation::Linear);
        let params = SpringParams {
            stiffness: 150.0,
            damping: 4.0,
            inertia: 1.0,
            ..SpringParams::default()
        };
        bake_spring(&mut s, &[1], params, 24.0, 1, 100).unwrap();
        let swing: Vec<f32> = (1..=100).map(|f| angle_at(&s, 1, f)).collect();
        let (min, max) = swing
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(min < -0.02 || max > 0.02, "the tail swings: {min} {max}");
        assert!(
            swing[99].abs() < 0.02,
            "and settles back on its key: {}",
            swing[99]
        );
        // with no inertia the same parent motion moves it not at all
        let mut calm = chain(2);
        calm.bones[0].angle = s.bones[0].angle.clone();
        bake_spring(
            &mut calm,
            &[1],
            SpringParams {
                inertia: 0.0,
                ..params
            },
            24.0,
            1,
            100,
        )
        .unwrap();
        assert!((1..=100).all(|f| angle_at(&calm, 1, f).abs() < 1e-5));
    }

    #[test]
    fn a_chain_bakes_from_the_top_so_the_whip_runs_down_its_length() {
        let mut s = chain(3);
        s.bones[0].angle.set_key(1, 0.0, Interpolation::Linear);
        s.bones[0].angle.set_key(8, 1.0, Interpolation::Linear);
        s.bones[0].angle.set_key(40, 1.0, Interpolation::Linear);
        let params = SpringParams::default();
        // asked for in the wrong order and with the root left out
        let written = bake_spring(&mut s, &[2, 1], params, 24.0, 1, 72).unwrap();
        assert_eq!(written, 144);
        let reach = |bone: usize| {
            (1..=72)
                .map(|f| angle_at(&s, bone, f).abs())
                .fold(0.0f32, f32::max)
        };
        assert!(reach(1) > 0.01, "the middle bone swings");
        assert!(reach(2) > 0.0, "and the tip follows");
        assert_eq!(s.bones[0].angle.keys.len(), 3, "the root was not touched");
    }

    #[test]
    fn keys_outside_the_range_stay_and_ones_inside_are_replaced_by_one_a_frame() {
        let mut s = chain(1);
        s.bones[0].angle.set_key(1, 0.0, Interpolation::Linear);
        s.bones[0].angle.set_key(30, 0.5, Interpolation::Linear);
        s.bones[0].angle.set_key(90, 0.2, Interpolation::Linear);
        bake_spring(&mut s, &[0], SpringParams::default(), 24.0, 20, 40).unwrap();
        let frames: Vec<Frame> = s.bones[0].angle.keys.iter().map(|k| k.frame).collect();
        assert!(frames.contains(&1) && frames.contains(&90));
        assert_eq!(frames.iter().filter(|f| (20..=40).contains(*f)).count(), 21);
        assert!(
            !s.bones[0]
                .angle
                .keys
                .iter()
                .any(|k| k.frame == 30 && k.interp != Interpolation::Linear)
        );
    }

    #[test]
    fn limits_hold_and_hostile_settings_are_refused_or_held() {
        let mut s = chain(1);
        s.bones[0].limits = Some([-0.3, 0.3]);
        s.bones[0].angle.set_key(1, 0.0, Interpolation::Step);
        s.bones[0].angle.set_key(10, 0.3, Interpolation::Step);
        let wild = SpringParams {
            stiffness: 5000.0,
            damping: 1.0,
            inertia: 0.0,
            ..SpringParams::default()
        };
        bake_spring(&mut s, &[0], wild, 24.0, 1, 100).unwrap();
        assert!((1..=100).all(|f| (-0.3 - 1e-4..=0.3 + 1e-4).contains(&angle_at(&s, 0, f))));
        let bad = [
            SpringParams {
                stiffness: f32::NAN,
                ..SpringParams::default()
            },
            SpringParams {
                damping: -1.0,
                ..SpringParams::default()
            },
            SpringParams {
                inertia: 1.0e9,
                ..SpringParams::default()
            },
        ];
        for p in bad {
            assert_eq!(
                bake_spring(&mut s, &[0], p, 24.0, 1, 10),
                Err(BakeError::BadSettings)
            );
            assert!(p.held().is_sound());
        }
        assert_eq!(
            bake_spring(&mut s, &[], SpringParams::default(), 24.0, 1, 10),
            Err(BakeError::NoBones)
        );
        assert_eq!(
            bake_spring(&mut s, &[5], SpringParams::default(), 24.0, 1, 10),
            Err(BakeError::NoSuchBone(5))
        );
        assert_eq!(
            bake_spring(&mut s, &[0, 0], SpringParams::default(), 24.0, 1, 10),
            Err(BakeError::RepeatedBone(0))
        );
        assert_eq!(
            bake_spring(&mut s, &[0], SpringParams::default(), 24.0, 10, 5),
            Err(BakeError::BadRange)
        );
        assert_eq!(
            bake_spring(&mut s, &[0], SpringParams::default(), 24.0, -3, 5),
            Err(BakeError::BadRange)
        );
        assert_eq!(
            bake_spring(
                &mut s,
                &[0],
                SpringParams::default(),
                24.0,
                0,
                MAX_BAKE_FRAMES + 5
            ),
            Err(BakeError::BadRange)
        );
        assert_eq!(
            bake_spring(&mut s, &[0], SpringParams::default(), 0.0, 1, 5),
            Err(BakeError::BadSettings)
        );
    }

    #[test]
    fn baking_is_deterministic() {
        let make = || {
            let mut s = chain(3);
            s.bones[0].angle.set_key(1, 0.0, Interpolation::Smooth);
            s.bones[0].angle.set_key(20, 2.0, Interpolation::Smooth);
            bake_spring(&mut s, &[1, 2], SpringParams::default(), 30.0, 1, 90).unwrap();
            s
        };
        assert_eq!(make(), make());
    }

    fn held_still(angle: f32) -> Skeleton {
        let mut s = chain(1);
        s.bones[0].angle.base = angle;
        s
    }

    #[test]
    fn gravity_sags_a_bone_that_points_sideways_and_leaves_one_that_hangs() {
        let loose = SpringParams {
            stiffness: 100.0,
            damping: 12.0,
            inertia: 0.0,
            gravity: 30.0,
            ..SpringParams::default()
        };
        let mut sideways = held_still(0.0);
        bake_spring(&mut sideways, &[0], loose, 24.0, 1, 200).unwrap();
        let sag = angle_at(&sideways, 0, 200);
        // the spring balances the pull: about gravity / stiffness, a little less as the bone turns
        assert!(sag > 0.2 && sag < 0.32, "{sag}");
        let mut hanging = held_still(std::f32::consts::FRAC_PI_2);
        bake_spring(&mut hanging, &[0], loose, 24.0, 1, 200).unwrap();
        assert!(
            (angle_at(&hanging, 0, 200) - std::f32::consts::FRAC_PI_2).abs() < 0.01,
            "a hanging bone is not pulled"
        );
        // none is the old behaviour
        let mut none = held_still(0.0);
        bake_spring(
            &mut none,
            &[0],
            SpringParams {
                gravity: 0.0,
                ..loose
            },
            24.0,
            1,
            100,
        )
        .unwrap();
        assert!(angle_at(&none, 0, 100).abs() < 1e-6);
    }

    #[test]
    fn wind_swings_a_hanging_bone_out_to_the_side_it_blows_towards() {
        let hang = std::f32::consts::FRAC_PI_2;
        let calm = SpringParams {
            stiffness: 100.0,
            damping: 12.0,
            inertia: 0.0,
            ..SpringParams::default()
        };
        let blow = |wind: f32| {
            let mut s = held_still(hang);
            bake_spring(&mut s, &[0], SpringParams { wind, ..calm }, 24.0, 1, 200).unwrap();
            angle_at(&s, 0, 200)
        };
        // a wind to the right turns the hanging bone so its tip goes right (a smaller angle),
        // one to the left the other way
        assert!(blow(30.0) < hang - 0.2, "{}", blow(30.0));
        assert!(blow(-30.0) > hang + 0.2, "{}", blow(-30.0));
        assert!((blow(0.0) - hang).abs() < 1e-6);
    }

    #[test]
    fn gravity_and_wind_are_scrub_safe_held_and_refused_when_not_numbers() {
        let params = SpringParams {
            gravity: 20.0,
            wind: -10.0,
            ..SpringParams::default()
        };
        let target: Vec<f32> = (0..62).map(|i| if i > 10 { 0.8 } else { 0.0 }).collect();
        let drive = vec![0.0; 62];
        let above: Vec<f32> = (0..62).map(|i| i as f32 * 0.01).collect();
        let spring = AngleSpring {
            params,
            fps: 24.0,
            first: 1,
            target: &target,
            drive: &drive,
            above: &above,
            limits: None,
        };
        let mut straight = StateCache::new(8);
        let in_order: Vec<_> = (1..=60).map(|f| straight.state_at(&spring, 3, 1, f)).collect();
        let mut jumpy = StateCache::new(8);
        for f in [60, 5, 31, 1, 59, 17] {
            assert_eq!(
                jumpy.state_at(&spring, 3, 1, f),
                in_order[(f - 1) as usize],
                "frame {f}"
            );
        }
        let mut s = held_still(0.0);
        for bad in [
            SpringParams {
                gravity: f32::NAN,
                ..params
            },
            SpringParams {
                gravity: -1.0,
                ..params
            },
            SpringParams {
                wind: 1.0e9,
                ..params
            },
            SpringParams {
                wind: f32::INFINITY,
                ..params
            },
        ] {
            assert_eq!(
                bake_spring(&mut s, &[0], bad, 24.0, 1, 10),
                Err(BakeError::BadSettings)
            );
            assert!(bad.held().is_sound());
        }
    }
}
