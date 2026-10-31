//! State that depends on every frame before it: springs, followers, physics.
//!
//! Almost everything Fanimate evaluates is a function of the frame alone, so
//! frames can be asked for in any order. A spring-driven tail is not: where
//! it is on frame 80 depends on where it was on frame 79. To keep the
//! playhead free to jump anywhere, the state of a [`Stepper`] is a pure
//! function of its settings and of the frames from the first one up to the
//! one asked for, and a [`StateCache`] remembers it at intervals, so
//! scrubbing starts from the nearest remembered frame, and playing forward
//! costs one step per frame.
//!
//! The same machinery serves the node graph's stateful nodes (M11), which is
//! why it lives here, with no idea of bones or shapes.

use std::collections::BTreeMap;

use crate::anim::Frame;

/// The most frames after the first that are ever stepped. A playhead or a
/// damaged file asking for frame two billion gets the state at this frame
/// instead of a hang. About two and a half hours at 24 frames a second.
pub const MAX_STEPS: Frame = 200_000;

/// The most remembered states. When there would be more, every other one is
/// dropped and the interval doubles, so a long scene's memory stays bounded.
pub const MAX_MARKS: usize = 4096;

/// Something whose state is built up one frame at a time.
pub trait Stepper {
    type State: Clone;

    /// The state before the first frame.
    fn initial(&self) -> Self::State;

    /// The state after `frame`, given the state after the frame before it.
    /// Must be a pure function: the same inputs give the same state on
    /// every run and platform, or scrubbing would show different things.
    fn step(&self, before: &Self::State, frame: Frame) -> Self::State;
}

/// Remembers a [`Stepper`]'s state at intervals.
#[derive(Clone, Debug)]
pub struct StateCache<S> {
    /// What the cached states were made from: the caller's hash of every
    /// setting that changes them. A different key empties the cache.
    key: u64,
    first: Frame,
    every: Frame,
    /// The state at the frame last asked for, so playing forward steps once
    /// per frame.
    last: Option<(Frame, S)>,
    marks: BTreeMap<Frame, S>,
}

impl<S: Clone> Default for StateCache<S> {
    fn default() -> Self {
        Self::new(24)
    }
}

impl<S: Clone> StateCache<S> {
    /// A cache that remembers a state every `every` frames (at least 1).
    pub fn new(every: Frame) -> Self {
        Self {
            key: 0,
            first: 0,
            every: every.max(1),
            last: None,
            marks: BTreeMap::new(),
        }
    }

    /// How many states are remembered at intervals.
    pub fn marks(&self) -> usize {
        self.marks.len()
    }

    /// Forget everything.
    pub fn clear(&mut self) {
        self.last = None;
        self.marks.clear();
    }

    /// The state after `frame`, with `first` the first frame that is stepped.
    /// `key` stands for every setting `stepper` was made from; when it
    /// differs from the last call's, what was remembered is thrown away.
    /// A frame before `first` gets the initial state.
    pub fn state_at<T: Stepper<State = S>>(
        &mut self,
        stepper: &T,
        key: u64,
        first: Frame,
        frame: Frame,
    ) -> S {
        if key != self.key || first != self.first {
            self.clear();
            self.key = key;
            self.first = first;
        }
        let frame = frame.min(first.saturating_add(MAX_STEPS));
        if frame < first {
            return stepper.initial();
        }
        // start from the nearest state at or before the frame
        let from_last = self.last.as_ref().filter(|(f, _)| *f <= frame);
        let from_mark = self.marks.range(..=frame).next_back();
        let (mut at, mut state) = match (from_last, from_mark) {
            (Some((a, s)), Some((b, t))) => {
                if a >= b {
                    (*a, s.clone())
                } else {
                    (*b, t.clone())
                }
            }
            (Some((a, s)), None) => (*a, s.clone()),
            (None, Some((b, t))) => (*b, t.clone()),
            (None, None) => (first - 1, stepper.initial()),
        };
        while at < frame {
            at += 1;
            state = stepper.step(&state, at);
            if at % self.every == 0 {
                self.marks.entry(at).or_insert_with(|| state.clone());
                if self.marks.len() > MAX_MARKS {
                    self.thin();
                }
            }
        }
        self.last = Some((frame, state.clone()));
        state
    }

    /// Keep every other remembered state and double the interval.
    fn thin(&mut self) {
        self.every = self.every.saturating_mul(2);
        let every = self.every;
        self.marks.retain(|f, _| f % every == 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A follower: each frame it moves a tenth of the way to the frame's
    /// target (here, ten times the frame number mod 7), so its state depends
    /// on all the frames before it.
    struct Follow {
        gain: f64,
        steps: Cell<usize>,
    }

    impl Follow {
        fn new(gain: f64) -> Self {
            Self {
                gain,
                steps: Cell::new(0),
            }
        }
    }

    impl Stepper for Follow {
        type State = f64;
        fn initial(&self) -> f64 {
            0.0
        }
        fn step(&self, before: &f64, frame: Frame) -> f64 {
            self.steps.set(self.steps.get() + 1);
            let target = 10.0 * f64::from(frame.rem_euclid(7));
            before + (target - before) * self.gain
        }
    }

    /// The state computed straight through, with no cache.
    fn direct(s: &Follow, first: Frame, frame: Frame) -> f64 {
        let mut state = s.initial();
        for f in first..=frame {
            state = s.step(&state, f);
        }
        state
    }

    #[test]
    fn any_order_of_frames_gives_the_state_a_straight_run_gives() {
        let s = Follow::new(0.1);
        let mut cache = StateCache::new(10);
        // a fixed shuffle of frames, forward, backward and repeated
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut asked = Vec::new();
        for _ in 0..300 {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            asked.push(1 + (x.wrapping_mul(0x9e37_79b9_7f4a_7c15) % 400) as Frame);
        }
        for frame in asked {
            let got = cache.state_at(&s, 1, 1, frame);
            let want = direct(&Follow::new(0.1), 1, frame);
            assert_eq!(got.to_bits(), want.to_bits(), "frame {frame}");
        }
    }

    #[test]
    fn playing_forward_costs_one_step_per_frame() {
        let s = Follow::new(0.1);
        let mut cache = StateCache::default();
        for frame in 1..=200 {
            cache.state_at(&s, 1, 1, frame);
        }
        assert_eq!(s.steps.get(), 200);
        // asking again for the frame it is on costs nothing
        cache.state_at(&s, 1, 1, 200);
        assert_eq!(s.steps.get(), 200);
    }

    #[test]
    fn scrubbing_back_restarts_from_a_remembered_frame_not_the_start() {
        let s = Follow::new(0.1);
        let mut cache = StateCache::new(10);
        cache.state_at(&s, 1, 1, 500);
        let before = s.steps.get();
        let got = cache.state_at(&s, 1, 1, 123);
        assert!(s.steps.get() - before <= 3, "{} steps", s.steps.get() - before);
        assert_eq!(got.to_bits(), direct(&Follow::new(0.1), 1, 123).to_bits());
        // a frame before the start is the initial state, with no stepping
        let steps = s.steps.get();
        assert_eq!(cache.state_at(&s, 1, 1, 0), 0.0);
        assert_eq!(cache.state_at(&s, 1, 1, -50), 0.0);
        assert_eq!(s.steps.get(), steps);
    }

    #[test]
    fn a_change_of_key_or_first_frame_throws_the_cache_away() {
        let mut cache = StateCache::new(10);
        let slow = Follow::new(0.1);
        let fast = Follow::new(0.9);
        let a = cache.state_at(&slow, 1, 1, 100);
        let b = cache.state_at(&fast, 2, 1, 100);
        assert_ne!(a.to_bits(), b.to_bits());
        assert_eq!(b.to_bits(), direct(&Follow::new(0.9), 1, 100).to_bits());
        // the same key but another first frame is another simulation (a slow
        // follower remembers where it started)
        let from_1 = cache.state_at(&slow, 3, 1, 100);
        let from_10 = cache.state_at(&slow, 3, 10, 100);
        assert_eq!(from_10.to_bits(), direct(&Follow::new(0.1), 10, 100).to_bits());
        assert_ne!(from_1.to_bits(), from_10.to_bits());
    }

    #[test]
    fn a_huge_frame_is_held_at_the_limit_instead_of_hanging() {
        let s = Follow::new(0.1);
        let mut cache = StateCache::new(24);
        let far = cache.state_at(&s, 1, 1, i32::MAX);
        assert!(s.steps.get() <= MAX_STEPS as usize + 1, "{} steps", s.steps.get());
        let at_limit = cache.state_at(&s, 1, 1, 1 + MAX_STEPS);
        assert_eq!(far.to_bits(), at_limit.to_bits());
        // a first frame near the top of the range does not overflow
        let mut cache = StateCache::new(24);
        let _ = cache.state_at(&s, 2, i32::MAX - 5, i32::MAX);
    }

    #[test]
    fn memory_stays_bounded_on_a_long_scene() {
        let s = Follow::new(0.1);
        let mut cache = StateCache::new(1);
        let got = cache.state_at(&s, 1, 1, 20_000);
        assert!(cache.marks() <= MAX_MARKS, "{} marks", cache.marks());
        assert_eq!(got.to_bits(), direct(&Follow::new(0.1), 1, 20_000).to_bits());
        // and scrubbing back after thinning is still exact
        let back = cache.state_at(&s, 1, 1, 777);
        assert_eq!(back.to_bits(), direct(&Follow::new(0.1), 1, 777).to_bits());
    }
}
