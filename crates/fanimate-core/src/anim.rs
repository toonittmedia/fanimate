//! Animation channels: a value that can change over time through keyframes.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::Color;
use crate::easing::{Bezier, Easing};

/// A frame number on the timeline. Frame 0 is the rest pose (how a thing is
/// drawn and set up) and frame 1 is the first frame of the animation;
/// keyframes on negative frames are invalid (see the validator in
/// `fanimate-fnm`).
pub type Frame = i32;

/// The frame that shows the rest pose, before the animation starts.
pub const REST_FRAME: Frame = 0;

/// What a channel does after its last key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopMode {
    /// Hold the last value.
    #[default]
    Off,
    /// Repeat the motion between the first and last key, the same values each time.
    Absolute,
    /// Repeat the motion, adding the change from first to last key each time
    /// round, so a walk keeps moving forward.
    Relative,
}

impl LoopMode {
    pub const ALL: [LoopMode; 3] = [Self::Off, Self::Absolute, Self::Relative];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Absolute => "Repeat",
            Self::Relative => "Repeat and offset",
        }
    }

    pub fn is_off(&self) -> bool {
        *self == Self::Off
    }
}

/// How a channel moves from one keyframe to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    /// Hold the value until the next key.
    Step,
    /// Straight-line change.
    Linear,
    /// Ease in and out of each key.
    #[default]
    Smooth,
    /// A named easing: ease in or out, overshoot, bounce, elastic.
    Ease(Easing),
    /// A curve drawn in the graph editor.
    Bezier(Bezier),
}

impl Interpolation {
    /// The choices the interpolation menus offer. A custom curve is not one:
    /// it is drawn in the graph editor.
    pub const ALL: [Interpolation; 9] = [
        Self::Smooth,
        Self::Linear,
        Self::Step,
        Self::Ease(Easing::EaseIn),
        Self::Ease(Easing::EaseOut),
        Self::Ease(Easing::EaseInOut),
        Self::Ease(Easing::Overshoot),
        Self::Ease(Easing::Bounce),
        Self::Ease(Easing::Elastic),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Smooth => "Smooth",
            Self::Linear => "Linear",
            Self::Step => "Step",
            Self::Ease(e) => e.label(),
            Self::Bezier(_) => "Custom curve",
        }
    }

    fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Remap linear progress `t` (0..=1) through this interpolation.
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Self::Step => 0.0,
            Self::Linear => t,
            Self::Smooth => t * t * (3.0 - 2.0 * t),
            Self::Ease(e) => e.apply(t),
            Self::Bezier(b) => b.apply(t),
        }
    }
}

/// How an edit is recorded on the timeline.
///
/// The **base frame** (frame 0, the rest pose) is where a thing is drawn
/// and set up: editing there changes the thing itself. On any later frame an
/// edit is animation, so it makes a keyframe, and a key holding the value the
/// channel had there is placed on the base frame first (when it has none) so
/// the rest pose does not change. `interp` is how the keys made move on to
/// the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keying {
    pub base: Frame,
    pub interp: Interpolation,
}

impl Keying {
    pub fn new(base: Frame, interp: Interpolation) -> Self {
        Self { base, interp }
    }
}

impl Default for Keying {
    fn default() -> Self {
        Self::new(0, Interpolation::Smooth)
    }
}

/// A value that can be interpolated between keyframes.
pub trait Animatable: Copy + PartialEq {
    fn lerp(a: Self, b: Self, t: f32) -> Self;

    /// Component-wise sum, used to carry a repeating motion forward.
    fn plus(a: Self, b: Self) -> Self;

    /// Component-wise difference.
    fn minus(a: Self, b: Self) -> Self;

    /// Whether every component is a finite number.
    fn is_finite(&self) -> bool;
}

impl Animatable for f32 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
    fn plus(a: Self, b: Self) -> Self {
        a + b
    }
    fn minus(a: Self, b: Self) -> Self {
        a - b
    }
    fn is_finite(&self) -> bool {
        f32::is_finite(*self)
    }
}

impl Animatable for Vec2 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a.lerp(b, t)
    }
    fn plus(a: Self, b: Self) -> Self {
        a + b
    }
    fn minus(a: Self, b: Self) -> Self {
        a - b
    }
    fn is_finite(&self) -> bool {
        Vec2::is_finite(*self)
    }
}

impl Animatable for Color {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Color::lerp(a, b, t)
    }
    fn plus(a: Self, b: Self) -> Self {
        Color::rgba(a.r + b.r, a.g + b.g, a.b + b.b, a.a + b.a)
    }
    fn minus(a: Self, b: Self) -> Self {
        Color::rgba(a.r - b.r, a.g - b.g, a.b - b.b, a.a - b.a)
    }
    fn is_finite(&self) -> bool {
        [self.r, self.g, self.b, self.a].iter().all(|c| c.is_finite())
    }
}

/// One keyframe. `interp` controls the motion from this key to the next.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key<T> {
    pub frame: Frame,
    pub value: T,
    #[serde(default, skip_serializing_if = "Interpolation::is_default")]
    pub interp: Interpolation,
}

/// An animatable value.
///
/// `base` is the value when there are no keys. For bones it is also the rest
/// pose. Keys are kept sorted by frame with at most one key per frame; the
/// editing methods maintain that, and the file validator checks it on load.
/// `loop_mode` says what happens after the last key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Channel<T> {
    pub base: T,
    #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<Key<T>>,
    #[serde(default, skip_serializing_if = "LoopMode::is_off")]
    pub loop_mode: LoopMode,
}

impl<T: Animatable> Channel<T> {
    pub fn new(base: T) -> Self {
        Self {
            base,
            keys: Vec::new(),
            loop_mode: LoopMode::Off,
        }
    }

    pub fn is_animated(&self) -> bool {
        !self.keys.is_empty()
    }

    /// The first and last key frames when the channel repeats (looping is on
    /// and there are at least two keys at different frames); the timeline
    /// draws its repeat bar from this.
    pub fn loop_span(&self) -> Option<(Frame, Frame)> {
        if self.loop_mode.is_off() {
            return None;
        }
        let (first, last) = (self.keys.first()?, self.keys.last()?);
        (last.frame > first.frame).then_some((first.frame, last.frame))
    }

    /// Insert a key, replacing any existing key on the same frame.
    pub fn set_key(&mut self, frame: Frame, value: T, interp: Interpolation) {
        set_key_in(&mut self.keys, frame, value, interp);
    }

    /// Change the value at `frame` the way an edit is recorded (see
    /// [`Keying`]): on a later frame than the base frame it gets a key, and
    /// a key on the base frame holding the value the channel had there is
    /// added first if there is none; on the base frame a channel without keys
    /// changes outright, and one with keys gets a key.
    pub fn write(&mut self, frame: Frame, value: T, keying: Keying) {
        write_in(&mut self.base, &mut self.keys, frame, value, keying);
    }

    /// Builder form of [`set_key`](Self::set_key).
    pub fn with_key(mut self, frame: Frame, value: T, interp: Interpolation) -> Self {
        self.set_key(frame, value, interp);
        self
    }

    pub fn remove_key(&mut self, frame: Frame) -> Option<Key<T>> {
        let i = self.keys.binary_search_by_key(&frame, |k| k.frame).ok()?;
        Some(self.keys.remove(i))
    }

    /// The value at `frame`, which may fall between whole frames, repeating
    /// after the last key when the channel loops.
    pub fn sample(&self, frame: f32) -> T {
        sample_loop_in(self.base, &self.keys, self.loop_mode, frame)
    }
}

/// Insert a key into a list kept sorted by frame, replacing any key on the
/// same frame.
pub fn set_key_in<T>(keys: &mut Vec<Key<T>>, frame: Frame, value: T, interp: Interpolation) {
    let key = Key { frame, value, interp };
    match keys.binary_search_by_key(&frame, |k| k.frame) {
        Ok(i) => keys[i] = key,
        Err(i) => keys.insert(i, key),
    }
}

/// The value at `frame` (which may fall between whole frames) of a value
/// and the keys that animate it: `base` with no keys, and the first and last
/// key's value outside them.
pub fn sample_in<T: Animatable>(base: T, keys: &[Key<T>], frame: f32) -> T {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
        return base;
    };
    // a NaN frame holds the first value rather than reaching the search below
    if frame.is_nan() || frame <= first.frame as f32 {
        return first.value;
    }
    if frame >= last.frame as f32 {
        return last.value;
    }
    // index of the first key after `frame`; always 1..len here
    let next = keys
        .partition_point(|k| k.frame as f32 <= frame)
        .clamp(1, keys.len() - 1);
    let (a, b) = (&keys[next - 1], &keys[next]);
    let t = (frame - a.frame as f32) / (b.frame - a.frame) as f32;
    T::lerp(a.value, b.value, a.interp.apply(t))
}

/// Like [`sample_in`], but after the last key the motion between the first
/// and last key repeats according to `mode`. Needs two keys on different
/// frames; with fewer, or for a frame that is not a finite number, it is
/// [`sample_in`]. Frames before the first key hold the first value.
///
/// `Absolute` plays the same values each time round. `Relative` adds, for
/// every completed cycle, the change from the first key to the last, so a
/// walk keeps moving forward.
pub fn sample_loop_in<T: Animatable>(base: T, keys: &[Key<T>], mode: LoopMode, frame: f32) -> T {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
        return base;
    };
    let period = (last.frame as i64 - first.frame as i64) as f32;
    if mode.is_off() || keys.len() < 2 || period <= 0.0 || !frame.is_finite() || frame <= last.frame as f32 {
        return sample_in(base, keys, frame);
    }
    let since = frame - first.frame as f32;
    let cycles = (since / period).floor();
    let local = first.frame as f32 + (since - cycles * period).clamp(0.0, period);
    let value = sample_in(base, keys, local);
    match mode {
        LoopMode::Relative => {
            // net * cycles, written with lerp because the trait has no scaling:
            // lerp(first, last, n) = first + (last - first) * n
            let carried = T::lerp(first.value, last.value, cycles);
            T::plus(value, T::minus(carried, first.value))
        }
        _ => value,
    }
}

/// Change a value at `frame` the way an edit is recorded (see [`Keying`]),
/// for a value and its keys held apart: on a later frame than the base frame
/// a key goes on `frame`, after one on the base frame holding the value the
/// channel had there (when it has none); on the base frame a channel without
/// keys changes outright and one with keys gets a key.
pub fn write_in<T: Animatable>(base: &mut T, keys: &mut Vec<Key<T>>, frame: Frame, value: T, keying: Keying) {
    if frame > keying.base {
        if keys.binary_search_by_key(&keying.base, |k| k.frame).is_err() {
            let seed = sample_in(*base, keys, keying.base as f32);
            set_key_in(keys, keying.base, seed, keying.interp);
        }
        set_key_in(keys, frame, value, keying.interp);
    } else if !keys.is_empty() {
        set_key_in(keys, frame, value, keying.interp);
    } else {
        *base = value;
    }
}

/// Keyframe editing that does not depend on the value type, so the timeline
/// can retime or change the interpolation of every channel of a layer
/// alike: numbers, vectors, colours and point keys.
pub trait KeyList {
    /// Push the frame of every key onto `out`.
    fn frames_into(&self, out: &mut Vec<Frame>);

    /// The interpolation of the key on `frame`, if there is one.
    fn interp_at(&self, frame: Frame) -> Option<Interpolation>;

    /// Move the key on `from` to `to`, replacing any key already there.
    /// Returns whether there was a key to move.
    fn move_key(&mut self, from: Frame, to: Frame) -> bool;

    /// Set the interpolation of the key on `frame`. Returns whether it
    /// changed.
    fn set_interp(&mut self, frame: Frame, interp: Interpolation) -> bool;

    /// Remove the key on `frame`. Returns whether there was one.
    fn delete_key(&mut self, frame: Frame) -> bool;
}

impl<T> KeyList for Vec<Key<T>> {
    fn frames_into(&self, out: &mut Vec<Frame>) {
        out.extend(self.iter().map(|k| k.frame));
    }

    fn interp_at(&self, frame: Frame) -> Option<Interpolation> {
        let i = self.binary_search_by_key(&frame, |k| k.frame).ok()?;
        Some(self[i].interp)
    }

    fn move_key(&mut self, from: Frame, to: Frame) -> bool {
        let Ok(i) = self.binary_search_by_key(&from, |k| k.frame) else {
            return false;
        };
        let mut key = self.remove(i);
        key.frame = to;
        match self.binary_search_by_key(&to, |k| k.frame) {
            Ok(j) => self[j] = key,
            Err(j) => self.insert(j, key),
        }
        true
    }

    fn set_interp(&mut self, frame: Frame, interp: Interpolation) -> bool {
        match self.binary_search_by_key(&frame, |k| k.frame) {
            Ok(i) if self[i].interp != interp => {
                self[i].interp = interp;
                true
            }
            _ => false,
        }
    }

    fn delete_key(&mut self, frame: Frame) -> bool {
        match self.binary_search_by_key(&frame, |k| k.frame) {
            Ok(i) => {
                self.remove(i);
                true
            }
            Err(_) => false,
        }
    }
}

impl<T: Animatable + Default> Default for Channel<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> Channel<f32> {
        Channel::new(5.0)
            .with_key(10, 0.0, Interpolation::Linear)
            .with_key(20, 100.0, Interpolation::Linear)
    }

    #[test]
    fn unkeyed_channel_returns_base() {
        assert_eq!(Channel::new(5.0_f32).sample(42.0), 5.0);
    }

    #[test]
    fn an_edit_on_the_base_frame_changes_the_value_itself() {
        let keying = Keying::default();
        let mut ch = Channel::new(5.0_f32);
        ch.write(0, 9.0, keying);
        assert!(!ch.is_animated());
        assert_eq!(ch.base, 9.0);
        assert_eq!(ch.sample(30.0), 9.0);
    }

    #[test]
    fn an_edit_on_a_later_frame_starts_animating_from_the_base_value() {
        let keying = Keying::new(0, Interpolation::Linear);
        let mut ch = Channel::new(5.0_f32);
        ch.write(10, 25.0, keying);
        // a key on the base frame holds what the value was, so frame 0
        // does not change, and the motion eases to the new key
        let frames: Vec<_> = ch.keys.iter().map(|k| (k.frame, k.value, k.interp)).collect();
        assert_eq!(
            frames,
            [(0, 5.0, Interpolation::Linear), (10, 25.0, Interpolation::Linear)]
        );
        assert_eq!(ch.sample(0.0), 5.0);
        assert_eq!(ch.sample(5.0), 15.0);
        assert_eq!(ch.sample(99.0), 25.0);
    }

    #[test]
    fn a_channel_with_keys_gets_a_key_wherever_it_is_edited() {
        let keying = Keying::default();
        let mut ch = ramp();
        ch.write(15, 7.0, keying);
        // the rest pose is seeded too, holding what the channel showed there
        assert_eq!(
            ch.keys.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>(),
            [(0, 0.0), (10, 0.0), (15, 7.0), (20, 100.0)]
        );
        // an edit on the base frame of an animated channel keys it too
        ch.write(0, 3.0, keying);
        assert_eq!(ch.keys.first().map(|k| (k.frame, k.value)), Some((0, 3.0)));
        // editing on a frame that has a key replaces it
        ch.write(15, 8.0, keying);
        assert_eq!(ch.keys.len(), 4);
        assert_eq!(ch.sample(15.0), 8.0);
    }

    #[test]
    fn the_rest_pose_is_seeded_when_a_channel_with_keys_is_edited_later() {
        let keying = Keying::new(REST_FRAME, Interpolation::Linear);
        // keys start on frame 10; the value at the rest frame is the first key's
        let mut ch = ramp();
        ch.write(30, 50.0, keying);
        assert_eq!(ch.keys.first().map(|k| (k.frame, k.value)), Some((0, 0.0)));
        assert_eq!(ch.sample(0.0), 0.0);
        assert_eq!(ch.sample(30.0), 50.0);
        // a key already on the base frame is left alone
        let mut ch = ramp().with_key(0, 9.0, Interpolation::Linear);
        ch.write(30, 50.0, keying);
        assert_eq!(ch.keys.first().map(|k| (k.frame, k.value)), Some((0, 9.0)));
        assert_eq!(ch.keys.len(), 4);
    }

    #[test]
    fn the_base_frame_need_not_be_zero() {
        // a project that starts on frame 24: frame 24 is where things are set up
        let keying = Keying::new(24, Interpolation::Smooth);
        let mut ch = Channel::new(1.0_f32);
        ch.write(24, 2.0, keying);
        assert!(!ch.is_animated());
        ch.write(30, 4.0, keying);
        assert_eq!(
            ch.keys.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>(),
            [(24, 2.0), (30, 4.0)]
        );
    }

    fn looped(mode: LoopMode) -> Channel<f32> {
        let mut c = ramp();
        c.loop_mode = mode;
        c
    }

    #[test]
    fn an_absolute_loop_repeats_the_same_values() {
        let c = looped(LoopMode::Absolute);
        assert_eq!(c.sample(20.0), 100.0);
        assert_eq!(c.sample(25.0), 50.0);
        assert_eq!(c.sample(35.0), 50.0);
        assert_eq!(c.sample(30.0), 0.0);
        // before the first key the first value still holds
        assert_eq!(c.sample(3.0), 0.0);
    }

    #[test]
    fn a_relative_loop_keeps_moving_forward() {
        let c = looped(LoopMode::Relative);
        assert_eq!(c.sample(20.0), 100.0);
        assert_eq!(c.sample(25.0), 150.0);
        assert_eq!(c.sample(30.0), 200.0);
        assert_eq!(c.sample(35.0), 250.0);
        assert_eq!(c.sample(40.0), 300.0);
        let mut previous = c.sample(20.0);
        for f in 21..80 {
            let v = c.sample(f as f32);
            assert!(v >= previous, "frame {f}: {v} fell below {previous}");
            previous = v;
        }
    }

    #[test]
    fn a_relative_loop_works_for_vectors_and_colours() {
        let mut v = Channel::new(Vec2::ZERO)
            .with_key(0, Vec2::ZERO, Interpolation::Linear)
            .with_key(10, Vec2::new(4.0, -2.0), Interpolation::Linear);
        v.loop_mode = LoopMode::Relative;
        assert_eq!(v.sample(15.0), Vec2::new(6.0, -3.0));
        let mut c = Channel::new(Color::BLACK)
            .with_key(0, Color::rgba(0.0, 0.0, 0.0, 1.0), Interpolation::Linear)
            .with_key(10, Color::rgba(0.5, 0.0, 0.0, 1.0), Interpolation::Linear);
        c.loop_mode = LoopMode::Relative;
        assert!((c.sample(20.0).r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn looping_needs_two_keys_on_different_frames() {
        let mut one = Channel::new(0.0_f32).with_key(5, 3.0, Interpolation::Linear);
        one.loop_mode = LoopMode::Relative;
        assert_eq!(one.sample(50.0), 3.0);
        assert_eq!(one.loop_span(), None);
        let mut none = Channel::new(7.0_f32);
        none.loop_mode = LoopMode::Absolute;
        assert_eq!(none.sample(50.0), 7.0);
        assert_eq!(none.loop_span(), None);
    }

    #[test]
    fn loop_span_is_the_first_and_last_key_when_looping() {
        assert_eq!(ramp().loop_span(), None);
        assert_eq!(looped(LoopMode::Absolute).loop_span(), Some((10, 20)));
        assert_eq!(looped(LoopMode::Relative).loop_span(), Some((10, 20)));
    }

    #[test]
    fn looping_survives_damaged_input() {
        for mode in LoopMode::ALL {
            let c = looped(mode);
            for f in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, -1e30] {
                let _ = c.sample(f);
            }
            // keys that share a frame or hold NaN must not panic either
            let mut bad = Channel::new(0.0_f32)
                .with_key(5, f32::NAN, Interpolation::Linear)
                .with_key(9, 1.0, Interpolation::Linear);
            bad.loop_mode = mode;
            bad.keys.push(Key {
                frame: 9,
                value: f32::INFINITY,
                interp: Interpolation::Linear,
            });
            for f in [0.0, 5.0, 7.0, 9.0, 40.0, f32::NAN] {
                let _ = bad.sample(f);
            }
            let _ = sample_in(0.0_f32, &bad.keys, f32::NAN);
        }
    }

    #[test]
    fn loop_mode_is_only_written_when_on() {
        let off = serde_json::to_string(&ramp()).unwrap_or_default();
        assert!(!off.contains("loop_mode"));
        let on = serde_json::to_string(&looped(LoopMode::Relative)).unwrap_or_default();
        assert!(on.contains("\"loop_mode\":\"relative\""));
        let back: Result<Channel<f32>, _> = serde_json::from_str(&on);
        assert_eq!(back.ok().map(|c| c.loop_mode), Some(LoopMode::Relative));
        let old: Result<Channel<f32>, _> = serde_json::from_str(&off);
        assert_eq!(old.ok().map(|c| c.loop_mode), Some(LoopMode::Off));
    }

    #[test]
    fn holds_before_first_and_after_last_key() {
        let c = ramp();
        assert_eq!(c.sample(0.0), 0.0);
        assert_eq!(c.sample(99.0), 100.0);
    }

    #[test]
    fn linear_and_subframe_sampling() {
        let c = ramp();
        assert_eq!(c.sample(15.0), 50.0);
        assert_eq!(c.sample(12.5), 25.0);
    }

    #[test]
    fn step_holds_until_next_key() {
        let c =
            Channel::new(0.0)
                .with_key(0, 1.0, Interpolation::Step)
                .with_key(10, 2.0, Interpolation::Step);
        assert_eq!(c.sample(9.9), 1.0);
        assert_eq!(c.sample(10.0), 2.0);
    }

    #[test]
    fn smooth_eases_but_hits_midpoint() {
        let c = Channel::new(0.0)
            .with_key(0, 0.0, Interpolation::Smooth)
            .with_key(10, 1.0, Interpolation::Smooth);
        assert!(c.sample(1.0) < 0.1);
        assert!((c.sample(5.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn set_key_keeps_keys_sorted_and_unique() {
        let mut c = ramp();
        c.set_key(15, 1.0, Interpolation::Linear);
        c.set_key(15, 2.0, Interpolation::Linear);
        c.set_key(5, 3.0, Interpolation::Linear);
        let frames: Vec<_> = c.keys.iter().map(|k| k.frame).collect();
        assert_eq!(frames, [5, 10, 15, 20]);
        assert_eq!(c.sample(15.0), 2.0);
        assert!(c.remove_key(15).is_some());
        assert!(c.remove_key(15).is_none());
    }

    #[test]
    fn key_list_moves_keys_replacing_any_on_the_target() {
        let mut c = ramp();
        assert!(c.keys.move_key(10, 25));
        let frames: Vec<_> = c.keys.iter().map(|k| (k.frame, k.value)).collect();
        assert_eq!(frames, [(20, 100.0), (25, 0.0)]);
        assert!(c.keys.move_key(25, 20));
        assert_eq!(c.keys.len(), 1);
        assert_eq!(c.keys[0].value, 0.0);
        assert!(!c.keys.move_key(3, 4));
        assert!(c.keys.set_interp(20, Interpolation::Step));
        assert!(!c.keys.set_interp(20, Interpolation::Step));
        assert_eq!(c.keys.interp_at(20), Some(Interpolation::Step));
        assert!(c.keys.delete_key(20));
        assert!(!c.is_animated());
    }

    #[test]
    fn eased_keys_sample_through_their_easing() {
        let mut ch = Channel::new(0.0f32);
        ch.set_key(1, 0.0, Interpolation::Ease(Easing::EaseIn));
        ch.set_key(11, 100.0, Interpolation::Smooth);
        assert!(
            ch.sample(6.0) < 50.0,
            "ease in is behind a straight line at the middle"
        );
        assert_eq!(ch.sample(11.0), 100.0);
        // an overshoot goes past the next key's value on the way
        ch.set_key(1, 0.0, Interpolation::Ease(Easing::Overshoot));
        let top = (1..=11).map(|f| ch.sample(f as f32)).fold(f32::MIN, f32::max);
        assert!(top > 105.0, "{top}");
        assert_eq!(ch.sample(11.0), 100.0, "and settles on it");
        // a hand-drawn curve works the same way
        ch.set_key(1, 0.0, Interpolation::Bezier(Bezier([0, 0, 1000, 1000])));
        assert!((ch.sample(6.0) - 50.0).abs() < 0.5);
    }

    #[test]
    fn a_colour_cannot_follow_an_overshoot_out_of_range() {
        let mut ch = Channel::new(Color::BLACK);
        ch.set_key(
            1,
            Color::rgba(0.0, 0.5, 1.0, 1.0),
            Interpolation::Ease(Easing::Elastic),
        );
        ch.set_key(21, Color::rgba(1.0, 1.0, 1.0, 1.0), Interpolation::Smooth);
        for f in 1..=21 {
            let c = ch.sample(f as f32);
            for v in [c.r, c.g, c.b, c.a] {
                assert!((0.0..=1.0).contains(&v), "frame {f}: {c:?}");
            }
        }
        // inside the range nothing changes: a plain mix is a plain mix
        let mix = Color::lerp(Color::BLACK, Color::WHITE, 0.25);
        assert_eq!((mix.r, mix.a), (0.25, 1.0));
    }

    #[test]
    fn every_menu_choice_has_a_distinct_label_and_the_custom_curve_is_not_one() {
        let labels: std::collections::BTreeSet<_> = Interpolation::ALL.iter().map(|i| i.label()).collect();
        assert_eq!(labels.len(), Interpolation::ALL.len());
        assert!(
            !Interpolation::ALL
                .iter()
                .any(|i| matches!(i, Interpolation::Bezier(_)))
        );
        assert_eq!(
            Interpolation::Bezier(Bezier([0, 0, 1000, 1000])).label(),
            "Custom curve"
        );
    }
}
