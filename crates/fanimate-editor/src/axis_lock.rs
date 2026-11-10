//! Holding Shift while dragging keeps the movement on a straight horizontal or vertical line.
//!
//! The rule is here, not in a widget, so it is tested without a window. The line is chosen once
//! the pointer has moved a little from where the drag began (whichever of across and down it has
//! gone further), and then kept for as long as Shift is held, so a drag that wobbles does not
//! flip between the two. Letting go of Shift frees the movement; pressing it again chooses anew.

use fanimate_core::Vec2;

/// Which straight line a locked drag follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Along the line through the start, left and right.
    Horizontal,
    /// Along the line through the start, up and down.
    Vertical,
}

/// The state of one drag's axis lock. Make a fresh one (or call [`AxisLock::reset`]) when a drag
/// begins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AxisLock {
    axis: Option<Axis>,
}

impl AxisLock {
    /// Forget the chosen line, as at the start of a drag.
    pub fn reset(&mut self) {
        self.axis = None;
    }

    /// The line currently chosen, if Shift has been held long enough for one.
    pub fn axis(&self) -> Option<Axis> {
        self.axis
    }

    /// Where the drag is, given the pointer at `to` and the drag having begun at `from`.
    ///
    /// Without `shift` this is `to`, and any chosen line is forgotten. With it, the point is
    /// pulled onto the horizontal or vertical line through `from`. Until the pointer is more than
    /// `dead_zone` away (in the same units as the points) no line is chosen and the point stays at
    /// `from`, so a small shake at the start does not pick a direction by accident. A point that is
    /// not a number is returned as it came.
    pub fn apply(&mut self, from: Vec2, to: Vec2, shift: bool, dead_zone: f32) -> Vec2 {
        if !shift {
            self.axis = None;
            return to;
        }
        if !(from.is_finite() && to.is_finite()) {
            return to;
        }
        let d = to - from;
        if self.axis.is_none() {
            let far = d.abs().max_element();
            if far <= dead_zone.max(0.0) {
                return from;
            }
            self.axis = Some(if d.x.abs() >= d.y.abs() {
                Axis::Horizontal
            } else {
                Axis::Vertical
            });
        }
        match self.axis {
            Some(Axis::Horizontal) => Vec2::new(to.x, from.y),
            Some(Axis::Vertical) => Vec2::new(from.x, to.y),
            None => from,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Vec2 = Vec2::new(100.0, 100.0);

    #[test]
    fn without_shift_the_pointer_is_left_alone_and_the_choice_forgotten() {
        let mut lock = AxisLock::default();
        let to = Vec2::new(130.0, 107.0);
        assert_eq!(lock.apply(ORIGIN, to, false, 4.0), to);
        // chosen with shift...
        lock.apply(ORIGIN, to, true, 4.0);
        assert_eq!(lock.axis(), Some(Axis::Horizontal));
        // ...and gone when shift is let go
        assert_eq!(lock.apply(ORIGIN, to, false, 4.0), to);
        assert_eq!(lock.axis(), None);
    }

    #[test]
    fn with_shift_the_movement_stays_on_the_line_it_started_along() {
        let mut lock = AxisLock::default();
        // mostly across: horizontal, and the up-and-down part is dropped
        assert_eq!(
            lock.apply(ORIGIN, Vec2::new(150.0, 112.0), true, 4.0),
            Vec2::new(150.0, 100.0)
        );
        // it stays horizontal even when the pointer later goes mostly down
        assert_eq!(
            lock.apply(ORIGIN, Vec2::new(140.0, 300.0), true, 4.0),
            Vec2::new(140.0, 100.0)
        );
        // and a fresh drag going mostly down is vertical
        lock.reset();
        assert_eq!(
            lock.apply(ORIGIN, Vec2::new(108.0, 160.0), true, 4.0),
            Vec2::new(100.0, 160.0)
        );
        assert_eq!(lock.axis(), Some(Axis::Vertical));
    }

    #[test]
    fn a_small_shake_at_the_start_chooses_nothing_and_does_not_move() {
        let mut lock = AxisLock::default();
        assert_eq!(lock.apply(ORIGIN, Vec2::new(102.0, 101.0), true, 4.0), ORIGIN);
        assert_eq!(lock.axis(), None);
        // the first move past the dead zone chooses
        assert_eq!(
            lock.apply(ORIGIN, Vec2::new(90.0, 103.0), true, 4.0),
            Vec2::new(90.0, 100.0)
        );
        assert_eq!(lock.axis(), Some(Axis::Horizontal));
    }

    #[test]
    fn a_diagonal_tie_goes_horizontal_and_nonsense_is_passed_through() {
        let mut lock = AxisLock::default();
        assert_eq!(
            lock.apply(ORIGIN, Vec2::new(120.0, 120.0), true, 4.0),
            Vec2::new(120.0, 100.0)
        );
        let mut lock = AxisLock::default();
        let nan = Vec2::new(f32::NAN, 5.0);
        assert!(lock.apply(ORIGIN, nan, true, 4.0).x.is_nan());
        assert_eq!(lock.axis(), None);
        // a negative dead zone is no dead zone, never a panic
        let mut lock = AxisLock::default();
        assert_eq!(
            lock.apply(ORIGIN, Vec2::new(100.5, 100.0), true, -3.0),
            Vec2::new(100.5, 100.0)
        );
    }
}
