//! 2D transforms: position, rotation and scale, static or animated, about a
//! pivot.

use glam::{Affine2, Mat2, Vec2};
use serde::{Deserialize, Serialize};

use crate::anim::Channel;

/// A transform at one moment. Rotation is in radians; because document space
/// has y pointing down, a positive rotation turns clockwise on screen.
///
/// Rotation and scale are about `pivot`, a point in the layer's own space,
/// and the pivot stays where it is: the layer's `position` moves the whole
/// layer, pivot included. With the pivot at the layer's origin (the
/// default, and how every file saved before pivots was drawn) this is the
/// plain "scale, rotate, then move" transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub position: Vec2,
    pub rotation: f32,
    pub scale: Vec2,
    pub pivot: Vec2,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        position: Vec2::ZERO,
        rotation: 0.0,
        scale: Vec2::ONE,
        pivot: Vec2::ZERO,
    };

    /// Layer space to parent space: a point `p` goes to
    /// `position + pivot + (rotate, scale)(p - pivot)`.
    pub fn to_affine(self) -> Affine2 {
        let about_origin = Affine2::from_scale_angle_translation(self.scale, self.rotation, self.position);
        if self.pivot == Vec2::ZERO {
            return about_origin;
        }
        Affine2::from_translation(self.position + self.pivot)
            * Affine2::from_scale_angle_translation(self.scale, self.rotation, Vec2::ZERO)
            * Affine2::from_translation(-self.pivot)
    }

    /// The transform, with this one's pivot, that does what `affine` does
    /// (layer space to parent space), or `None` when no rotation and scale
    /// can: the affine shears (a rotated layer stretched sideways), or is
    /// flat or not a number. `near` is the transform it replaces, so that
    /// an angle stays within half a turn of the old one (an animation does
    /// not wind up) and a flip stays the way it was.
    pub fn from_affine(affine: Affine2, pivot: Vec2, near: &Transform) -> Option<Transform> {
        if !affine.is_finite() {
            return None;
        }
        let (x, y) = (affine.matrix2.x_axis, affine.matrix2.y_axis);
        let (lx, ly) = (x.length(), y.length());
        if lx < 1e-9 || ly < 1e-9 || (x.dot(y) / (lx * ly)).abs() > 1e-3 {
            return None;
        }
        let flipped = if x.perp_dot(y) < 0.0 { -1.0 } else { 1.0 };
        let angle = x.y.atan2(x.x);
        // a flip of x can be read as a turn of half a turn and flips of both:
        // take the reading closer to what was there
        let wrap =
            |a: f32| (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let direct = wrap(angle - near.rotation);
        let turned = wrap(angle + std::f32::consts::PI - near.rotation);
        let (rotation, scale) = if direct.abs() <= turned.abs() {
            (near.rotation + direct, Vec2::new(lx, flipped * ly))
        } else {
            (near.rotation + turned, Vec2::new(-lx, -flipped * ly))
        };
        let position = affine.transform_point2(pivot) - pivot;
        Some(Transform {
            position,
            rotation,
            scale,
            pivot,
        })
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A layer's transform, with each part animatable. The pivot is not
/// animated: it is the layer's centre of rotation and scale, set once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimTransform {
    pub position: Channel<Vec2>,
    pub rotation: Channel<f32>,
    pub scale: Channel<Vec2>,
    /// The pivot in the layer's own space. Without one, the layer's origin
    /// (version 4; older files have none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot: Option<Vec2>,
}

impl AnimTransform {
    pub fn at(position: Vec2) -> Self {
        Self {
            position: Channel::new(position),
            ..Self::default()
        }
    }

    pub fn sample(&self, frame: f32) -> Transform {
        Transform {
            position: self.position.sample(frame),
            rotation: self.rotation.sample(frame),
            scale: self.scale.sample(frame),
            pivot: self.pivot.unwrap_or(Vec2::ZERO),
        }
    }

    pub fn is_animated(&self) -> bool {
        self.position.is_animated() || self.rotation.is_animated() || self.scale.is_animated()
    }

    /// Whether moving the pivot can keep the layer exactly where it is on
    /// every frame: it can unless the rotation or scale is animated.
    pub fn pivot_moves_cleanly(&self) -> bool {
        !self.rotation.is_animated() && !self.scale.is_animated()
    }

    /// Move the pivot to `pivot` (layer space) without moving the artwork.
    /// The position shifts to make up for it: on `frame`, and on every
    /// frame when [`pivot_moves_cleanly`](Self::pivot_moves_cleanly).
    /// Returns whether it was clean.
    pub fn set_pivot_keeping_look(&mut self, pivot: Vec2, frame: f32) -> bool {
        let old = self.pivot.unwrap_or(Vec2::ZERO);
        let m = Mat2::from_scale_angle(self.scale.sample(frame), self.rotation.sample(frame));
        // position' = position + (M - I)(new - old), so the artwork stays put
        let shift = m * (pivot - old) - (pivot - old);
        self.position.base += shift;
        for key in &mut self.position.keys {
            key.value += shift;
        }
        self.pivot = (pivot != Vec2::ZERO).then_some(pivot);
        self.pivot_moves_cleanly()
    }
}

impl Default for AnimTransform {
    fn default() -> Self {
        Self {
            position: Channel::new(Vec2::ZERO),
            rotation: Channel::new(0.0),
            scale: Channel::new(Vec2::ONE),
            pivot: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-3
    }

    fn t(position: Vec2, rotation: f32, scale: Vec2, pivot: Vec2) -> Transform {
        Transform {
            position,
            rotation,
            scale,
            pivot,
        }
    }

    #[test]
    fn without_a_pivot_it_is_the_old_transform() {
        let tr = t(Vec2::new(10.0, 5.0), 0.5, Vec2::new(2.0, 3.0), Vec2::ZERO);
        let old = Affine2::from_scale_angle_translation(tr.scale, tr.rotation, tr.position);
        assert_eq!(tr.to_affine(), old);
    }

    #[test]
    fn the_pivot_stays_where_the_layer_is_while_it_turns_and_scales() {
        let pivot = Vec2::new(100.0, 50.0);
        let position = Vec2::new(7.0, -3.0);
        for (rotation, scale) in [(0.0, Vec2::ONE), (1.2, Vec2::ONE), (0.7, Vec2::new(2.0, 0.5))] {
            let a = t(position, rotation, scale, pivot).to_affine();
            assert!(
                close(a.transform_point2(pivot), pivot + position),
                "{rotation} {scale}"
            );
        }
        // a point one unit to the right of the pivot turns about it
        let a = t(Vec2::ZERO, std::f32::consts::FRAC_PI_2, Vec2::ONE, pivot).to_affine();
        assert!(close(a.transform_point2(pivot + Vec2::X), pivot + Vec2::Y));
    }

    #[test]
    fn moving_the_pivot_leaves_the_artwork_where_it_was() {
        let mut anim = AnimTransform::at(Vec2::new(40.0, 10.0));
        anim.rotation = Channel::new(0.9);
        anim.scale = Channel::new(Vec2::new(1.5, 0.75));
        let before = anim.sample(0.0).to_affine();
        assert!(anim.set_pivot_keeping_look(Vec2::new(30.0, 20.0), 0.0));
        let after = anim.sample(0.0).to_affine();
        for p in [Vec2::ZERO, Vec2::new(10.0, -4.0), Vec2::new(120.0, 80.0)] {
            assert!(
                close(before.transform_point2(p), after.transform_point2(p)),
                "{p}"
            );
        }
        assert_eq!(anim.pivot, Some(Vec2::new(30.0, 20.0)));
        // back to the origin: no pivot at all, as in an old file
        assert!(anim.set_pivot_keeping_look(Vec2::ZERO, 0.0));
        assert_eq!(anim.pivot, None);
        assert!(close(
            anim.sample(0.0).to_affine().transform_point2(Vec2::ONE),
            before.transform_point2(Vec2::ONE)
        ));
    }

    #[test]
    fn moving_the_pivot_shifts_every_position_key_when_only_position_is_animated() {
        let mut anim = AnimTransform::at(Vec2::ZERO);
        anim.rotation = Channel::new(0.6);
        anim.position
            .set_key(0, Vec2::new(0.0, 0.0), crate::Interpolation::Linear);
        anim.position
            .set_key(10, Vec2::new(100.0, 0.0), crate::Interpolation::Linear);
        let before: Vec<Affine2> = [0.0, 4.0, 10.0].map(|f| anim.sample(f).to_affine()).to_vec();
        assert!(anim.set_pivot_keeping_look(Vec2::new(20.0, 30.0), 4.0));
        for (f, b) in [0.0, 4.0, 10.0].into_iter().zip(before) {
            let a = anim.sample(f).to_affine();
            assert!(close(
                a.transform_point2(Vec2::new(5.0, 5.0)),
                b.transform_point2(Vec2::new(5.0, 5.0))
            ));
        }
    }

    #[test]
    fn an_animated_rotation_is_only_kept_on_the_frame_asked_for() {
        let mut anim = AnimTransform::default();
        anim.rotation.set_key(0, 0.0, crate::Interpolation::Linear);
        anim.rotation.set_key(10, 1.5, crate::Interpolation::Linear);
        assert!(!anim.pivot_moves_cleanly());
        let probe = Vec2::new(30.0, 10.0);
        let at4 = anim.sample(4.0).to_affine().transform_point2(probe);
        assert!(!anim.set_pivot_keeping_look(Vec2::new(50.0, 0.0), 4.0));
        assert!(close(anim.sample(4.0).to_affine().transform_point2(probe), at4));
    }

    #[test]
    fn an_affine_reads_back_as_the_transform_that_made_it() {
        let pivot = Vec2::new(12.0, -8.0);
        for tr in [
            t(Vec2::new(3.0, 4.0), 0.4, Vec2::new(1.5, 2.0), pivot),
            t(Vec2::ZERO, -2.5, Vec2::new(-1.0, 1.0), pivot),
            t(Vec2::new(-9.0, 2.0), 3.0, Vec2::new(1.0, -2.0), Vec2::ZERO),
            t(Vec2::ZERO, 0.0, Vec2::ONE, pivot),
        ] {
            let back = Transform::from_affine(tr.to_affine(), tr.pivot, &tr).unwrap();
            assert!(close(back.position, tr.position), "{tr:?} {back:?}");
            assert!((back.rotation - tr.rotation).abs() < 1e-3, "{tr:?} {back:?}");
            assert!(close(back.scale, tr.scale), "{tr:?} {back:?}");
        }
    }

    #[test]
    fn a_turn_past_half_a_circle_does_not_wind_the_animation() {
        let near = t(Vec2::ZERO, 3.0, Vec2::ONE, Vec2::ZERO);
        // 3.3 rad is the same as -2.98 rad: stay beside 3.0
        let a = Affine2::from_scale_angle_translation(Vec2::ONE, 3.3, Vec2::ZERO);
        let back = Transform::from_affine(a, Vec2::ZERO, &near).unwrap();
        assert!((back.rotation - 3.3).abs() < 1e-3, "{}", back.rotation);
    }

    #[test]
    fn shear_and_flat_affines_have_no_transform() {
        let tr = Transform::IDENTITY;
        let shear = Affine2::from_mat2(Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(0.5, 1.0)));
        assert!(Transform::from_affine(shear, Vec2::ZERO, &tr).is_none());
        let flat = Affine2::from_scale_angle_translation(Vec2::new(1.0, 0.0), 0.0, Vec2::ZERO);
        assert!(Transform::from_affine(flat, Vec2::ZERO, &tr).is_none());
        let nan = Affine2::from_translation(Vec2::NAN);
        assert!(Transform::from_affine(nan, Vec2::ZERO, &tr).is_none());
    }

    #[test]
    fn pivots_are_saved_only_when_set() {
        let plain = AnimTransform::default();
        assert!(!serde_json::to_string(&plain).unwrap().contains("pivot"));
        let mut set = plain.clone();
        set.pivot = Some(Vec2::new(1.0, 2.0));
        let text = serde_json::to_string(&set).unwrap();
        assert_eq!(serde_json::from_str::<AnimTransform>(&text).unwrap(), set);
        // a version 3 layer has none
        let old = r#"{"position":{"base":[0.0,0.0]},"rotation":{"base":0.0},"scale":{"base":[1.0,1.0]}}"#;
        assert_eq!(serde_json::from_str::<AnimTransform>(old).unwrap().pivot, None);
    }
}
