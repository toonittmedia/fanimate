//! Bones: forward and inverse kinematics, angle limits, and the weights
//! that let one point follow several bones.
//!
//! A bone layer owns a [`Skeleton`]. Shapes on vector layers inside the bone
//! layer follow its bones, either rigidly (one bone, `Shape::bone`) or
//! smoothly (several bones blended per point, `Shape::skin`).

use glam::{Affine2, Vec2};
use serde::{Deserialize, Serialize};

use crate::anim::{Channel, Frame, Interpolation};

/// One bone. Its local space has the origin at the joint and +x running
/// along the bone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    /// Version 7: names the bone for good within its skeleton. The list index
    /// changes when a bone is reparented or removed; the id never does, so a
    /// script or a driver can hold on to a bone. 0 means not given yet: files
    /// from before version 7 get theirs on load (see [`Skeleton::ensure_ids`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub id: u32,
    pub name: String,
    /// Index of the parent bone. Parents always come before children.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// Joint position in the parent's bone space, or in the bone layer's
    /// space for a root bone.
    pub origin: Vec2,
    pub length: f32,
    /// Rotation relative to the parent, in radians. The channel's `base`
    /// value is the rest pose.
    pub angle: Channel<f32>,
    /// Lowest and highest angle the bone may turn to, relative to its
    /// parent, in radians. Kinematics never take it outside them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<[f32; 2]>,
    /// A pinned bone stays put when a bone further down its chain is
    /// dragged: inverse kinematics stops there, as at a hip or shoulder.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    /// How far, in document units, the bone reaches when weights are made
    /// automatically. `None` uses half the bone's length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f32>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// The highest bone id a sound skeleton holds, leaving room above it.
pub const MAX_BONE_ID: u32 = 1 << 30;

impl Bone {
    pub fn new(
        name: impl Into<String>,
        parent: Option<usize>,
        origin: Vec2,
        length: f32,
        angle: f32,
    ) -> Self {
        Self {
            id: 0,
            name: name.into(),
            parent,
            origin,
            length,
            angle: Channel::new(angle),
            limits: None,
            pinned: false,
            strength: None,
        }
    }

    pub fn local(&self, angle: f32) -> Affine2 {
        Affine2::from_angle_translation(angle, self.origin)
    }

    /// `angle` kept within the bone's limits.
    pub fn clamp(&self, angle: f32) -> f32 {
        match self.limits {
            Some([lo, hi]) if lo <= hi => angle.clamp(lo, hi),
            _ => angle,
        }
    }

    /// The reach used for automatic weights.
    pub fn influence(&self) -> f32 {
        self.strength
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or((self.length.abs() * 0.5).max(1.0))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
    /// Version 7: the next bone id to hand out. Ids are not reused, so a bone
    /// that was removed never has its id given to another.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next_bone_id: u32,
    /// Actions and Smart Bone actions (see [`crate::actions`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<crate::actions::RigAction>,
    /// Master controllers: sliders and pads that play actions (see [`crate::actions::Controller`]).
    /// A project that has any requires `controllers@1`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controllers: Vec<crate::actions::Controller>,
}

impl Skeleton {
    /// Add a bone and return its index.
    ///
    /// # Panics
    /// If `parent` does not refer to an existing bone.
    pub fn add(
        &mut self,
        name: impl Into<String>,
        parent: Option<usize>,
        origin: Vec2,
        length: f32,
        rest_angle: f32,
    ) -> usize {
        if let Some(p) = parent {
            assert!(p < self.bones.len(), "parent bone {p} does not exist");
        }
        let mut bone = Bone::new(name, parent, origin, length, rest_angle);
        self.ensure_ids();
        bone.id = self.next_bone_id;
        self.next_bone_id = self.next_bone_id.saturating_add(1);
        self.bones.push(bone);
        self.bones.len() - 1
    }

    /// The index of the bone with this id, if there is one.
    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.bones.iter().position(|b| b.id == id)
    }

    /// Whether every bone has its own id in range and the counter is above
    /// them all, as [`ensure_ids`](Self::ensure_ids) leaves it.
    pub fn ids_are_sound(&self) -> bool {
        let mut seen = std::collections::HashSet::new();
        let mut top = 0;
        for b in &self.bones {
            if b.id == 0 || b.id > MAX_BONE_ID || !seen.insert(b.id) {
                return false;
            }
            top = top.max(b.id);
        }
        self.next_bone_id > top && self.next_bone_id <= MAX_BONE_ID + 1
    }

    /// Give every bone an id of its own. Bones that have a good id keep it;
    /// those with none (0, as in files from before version 7), one out of
    /// range or one already taken get the next free ones, in list order, so
    /// the same file always gets the same ids. Returns whether anything
    /// changed.
    pub fn ensure_ids(&mut self) -> bool {
        if self.ids_are_sound() {
            return false;
        }
        let mut seen = std::collections::HashSet::new();
        let mut top = 0;
        let mut needy = Vec::new();
        for (i, b) in self.bones.iter().enumerate() {
            if b.id == 0 || b.id > MAX_BONE_ID || !seen.insert(b.id) {
                needy.push(i);
            } else {
                top = top.max(b.id);
            }
        }
        let mut next = self.next_bone_id.max(top + 1);
        if next > MAX_BONE_ID {
            next = top + 1;
        }
        for i in needy {
            self.bones[i].id = next;
            next = next.saturating_add(1);
        }
        self.next_bone_id = next;
        true
    }

    /// Remove bone `index`. Its children join its parent, keeping their
    /// rest position. Returns the new index of every old bone (`None` for
    /// the removed one), to renumber references to bones.
    pub fn remove(&mut self, index: usize) -> Vec<Option<usize>> {
        let n = self.bones.len();
        if index >= n {
            return (0..n).map(Some).collect();
        }
        let mut rest = Vec::new();
        self.rest_pose(&mut rest);
        let gone = self.bones[index].clone();
        for (i, bone) in self.bones.iter_mut().enumerate() {
            if bone.parent == Some(index) {
                // the child's rest transform, re-expressed under the new parent
                let parent_world = gone
                    .parent
                    .and_then(|p| rest.get(p))
                    .copied()
                    .unwrap_or(Affine2::IDENTITY);
                let local = parent_world.inverse() * rest[i];
                bone.parent = gone.parent;
                bone.origin = local.translation;
                let angle = local.matrix2.x_axis.to_angle();
                // keep its animation relative to the new rest angle
                let shift = angle - bone.angle.base;
                bone.angle.base = angle;
                for k in &mut bone.angle.keys {
                    k.value += shift;
                }
                if let Some(l) = &mut bone.limits {
                    *l = [l[0] + shift, l[1] + shift];
                }
            }
        }
        self.bones.remove(index);
        let map: Vec<Option<usize>> = (0..n)
            .map(|i| match i.cmp(&index) {
                std::cmp::Ordering::Less => Some(i),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(i - 1),
            })
            .collect();
        for bone in &mut self.bones {
            bone.parent = bone.parent.and_then(|p| map.get(p).copied().flatten());
        }
        self.actions.retain_mut(|a| a.renumber(&map));
        map
    }

    /// Every bone's angle at `frame`, limits applied.
    pub fn angles(&self, frame: f32, out: &mut Vec<f32>) {
        out.clear();
        out.extend(self.bones.iter().map(|b| b.clamp(b.angle.sample(frame))));
    }

    /// World (bone-layer space) transforms for the given angles.
    pub fn pose_from(&self, angles: &[f32], out: &mut Vec<Affine2>) {
        self.solve(out, |i, b| angles.get(i).copied().unwrap_or(b.angle.base));
    }

    /// The bones a drag on `bone` moves: it and its ancestors, up to and
    /// not including the first pinned one. Nearest first.
    pub fn chain(&self, bone: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut current = Some(bone);
        while let Some(b) = current.filter(|&b| b < self.bones.len()) {
            if self.bones[b].pinned && b != bone {
                break;
            }
            out.push(b);
            if self.bones[b].pinned {
                break;
            }
            current = self.bones[b].parent.filter(|&p| p < b);
        }
        out
    }

    /// Inverse kinematics: turn the bones in [`chain`](Self::chain) of
    /// `bone` so its tip reaches towards `target` (bone-layer space),
    /// starting from `angles` and changing them in place. Limits hold.
    /// Cyclic coordinate descent: stable, cheap, and moves the bones
    /// nearest the tip most, which reads naturally for limbs.
    pub fn solve_ik(&self, angles: &mut [f32], bone: usize, target: Vec2) {
        if bone >= self.bones.len() || !target.is_finite() || angles.len() != self.bones.len() {
            return;
        }
        let chain = self.chain(bone);
        let mut poses = Vec::with_capacity(self.bones.len());
        for _ in 0..24 {
            for &j in &chain {
                self.pose_from(angles, &mut poses);
                let tip = self.tip(&poses, bone);
                let joint = poses[j].transform_point2(Vec2::ZERO);
                let (to_tip, to_target) = (tip - joint, target - joint);
                if to_tip.length_squared() < 1e-8 || to_target.length_squared() < 1e-8 {
                    continue;
                }
                let turn = to_tip.angle_to(to_target);
                if turn.is_finite() {
                    angles[j] = self.bones[j].clamp(angles[j] + turn);
                }
            }
            self.pose_from(angles, &mut poses);
            if self.tip(&poses, bone).distance(target) < 0.01 {
                break;
            }
        }
    }

    pub fn key_angle(&mut self, bone: usize, frame: Frame, angle: f32, interp: Interpolation) {
        self.bones[bone].angle.set_key(frame, angle, interp);
    }

    /// World (bone-layer space) transform of every bone at `frame`.
    pub fn pose(&self, frame: f32, out: &mut Vec<Affine2>) {
        self.solve(out, |_, b| b.clamp(b.angle.sample(frame)));
    }

    /// World transform of every bone in the rest pose.
    pub fn rest_pose(&self, out: &mut Vec<Affine2>) {
        self.solve(out, |_, b| b.angle.base);
    }

    fn solve(&self, out: &mut Vec<Affine2>, angle: impl Fn(usize, &Bone) -> f32) {
        out.clear();
        out.reserve(self.bones.len());
        for (i, bone) in self.bones.iter().enumerate() {
            let local = bone.local(angle(i, bone));
            // Parents precede children (enforced by `add`, checked by the
            // validator), so the parent's world transform is already known.
            // A bad parent index is treated as a root rather than panicking.
            let world = match bone.parent.and_then(|p| out.get(p)) {
                Some(parent) => *parent * local,
                None => local,
            };
            out.push(world);
        }
    }

    /// Where a bone's tip is, given world transforms from [`pose`](Self::pose).
    pub fn tip(&self, poses: &[Affine2], bone: usize) -> Vec2 {
        poses[bone].transform_point2(Vec2::new(self.bones[bone].length, 0.0))
    }
}

/// Per-point bone weights: for each point, (bone, weight) pairs.
pub type Skin = Vec<Vec<(usize, f32)>>;

/// Smooth binding: for each point, the bones it follows and how much,
/// made from the rest pose. A point follows the bones whose reach covers it,
/// more strongly the closer it is, and the nearest bone when none does.
/// At most `max` bones per point; weights add up to 1.
pub fn auto_weights(skeleton: &Skeleton, points: &[Vec2], max: usize) -> Skin {
    let mut rest = Vec::new();
    skeleton.rest_pose(&mut rest);
    let segments: Vec<(Vec2, Vec2, f32)> = skeleton
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| {
            (
                rest[i].transform_point2(Vec2::ZERO),
                skeleton.tip(&rest, i),
                b.influence(),
            )
        })
        .collect();
    points
        .iter()
        .map(|&p| {
            let mut near: Vec<(usize, f32, f32)> = segments
                .iter()
                .enumerate()
                .map(|(i, &(a, b, reach))| {
                    let d = segment_distance(p, a, b);
                    let w = (1.0 - d / reach).max(0.0);
                    (i, d, w * w)
                })
                .collect();
            let covered = near.iter().any(|n| n.2 > 0.0);
            if !covered {
                return near
                    .iter()
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|n| vec![(n.0, 1.0)])
                    .unwrap_or_default();
            }
            near.retain(|n| n.2 > 0.0);
            near.sort_by(|a, b| b.2.total_cmp(&a.2));
            near.truncate(max.max(1));
            let total: f32 = near.iter().map(|n| n.2).sum();
            near.iter().map(|n| (n.0, n.2 / total)).collect()
        })
        .collect()
}

/// The blend of bone transforms for one point's weights: weights are
/// normalised; an empty or zero-weight list gives the identity.
pub fn blend(skins: &[Affine2], weights: &[(usize, f32)]) -> Affine2 {
    let total: f32 = weights
        .iter()
        .filter(|(b, w)| *b < skins.len() && w.is_finite() && *w > 0.0)
        .map(|(_, w)| *w)
        .sum();
    if total <= 0.0 {
        return Affine2::IDENTITY;
    }
    let mut m = glam::Mat2::ZERO;
    let mut t = Vec2::ZERO;
    for &(b, w) in weights {
        if b < skins.len() && w.is_finite() && w > 0.0 {
            let k = w / total;
            m += skins[b].matrix2 * k;
            t += skins[b].translation * k;
        }
    }
    Affine2::from_mat2_translation(m, t)
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn arm() -> Skeleton {
        let mut s = Skeleton::default();
        let upper = s.add("upper", None, Vec2::ZERO, 10.0, 0.0);
        s.add("lower", Some(upper), Vec2::new(10.0, 0.0), 10.0, 0.0);
        s
    }

    #[test]
    fn rest_pose_chains_along_x() {
        let s = arm();
        let mut poses = Vec::new();
        s.rest_pose(&mut poses);
        assert!(s.tip(&poses, 1).abs_diff_eq(Vec2::new(20.0, 0.0), 1e-5));
    }

    #[test]
    fn rotating_parent_carries_child() {
        let mut s = arm();
        s.key_angle(0, 0, FRAC_PI_2, Interpolation::Linear);
        let mut poses = Vec::new();
        s.pose(0.0, &mut poses);
        // upper arm points down (+y), lower arm continues it
        assert!(s.tip(&poses, 1).abs_diff_eq(Vec2::new(0.0, 20.0), 1e-4));
    }

    #[test]
    fn ik_reaches_a_target_within_reach() {
        let s = arm();
        let mut angles = vec![0.0, 0.0];
        let target = Vec2::new(10.0, 10.0);
        s.solve_ik(&mut angles, 1, target);
        let mut poses = Vec::new();
        s.pose_from(&angles, &mut poses);
        assert!(s.tip(&poses, 1).distance(target) < 0.05, "{:?}", s.tip(&poses, 1));
    }

    #[test]
    fn ik_respects_limits_and_pins() {
        let mut s = arm();
        s.bones[1].limits = Some([0.0, 0.5]);
        let mut angles = vec![0.0, 0.0];
        s.solve_ik(&mut angles, 1, Vec2::new(0.0, -20.0));
        assert!((0.0..=0.5).contains(&angles[1]));
        // pinning the elbow keeps the upper arm still
        s.bones[1].pinned = true;
        assert_eq!(s.chain(1), [1]);
        let mut angles = vec![0.0, 0.0];
        s.bones[1].limits = None;
        s.solve_ik(&mut angles, 1, Vec2::new(10.0, 10.0));
        assert_eq!(angles[0], 0.0);
        assert!(angles[1] > 1.0);
    }

    #[test]
    fn removing_a_bone_keeps_its_children_in_place() {
        let mut s = arm();
        let c = s.add("hand", Some(1), Vec2::new(10.0, 0.0), 5.0, 0.3);
        let mut before = Vec::new();
        s.rest_pose(&mut before);
        let map = s.remove(1);
        assert_eq!(map, [Some(0), None, Some(1)]);
        assert_eq!(s.bones[1].parent, Some(0));
        let mut after = Vec::new();
        s.rest_pose(&mut after);
        assert!(
            after[1]
                .transform_point2(Vec2::ZERO)
                .abs_diff_eq(before[c].transform_point2(Vec2::ZERO), 1e-4)
        );
        assert!(s.tip(&after, 1).abs_diff_eq(s_tip(&before, c, 5.0), 1e-4));
    }

    fn s_tip(poses: &[Affine2], i: usize, len: f32) -> Vec2 {
        poses[i].transform_point2(Vec2::new(len, 0.0))
    }

    #[test]
    fn limits_clamp_sampled_angles() {
        let mut s = arm();
        s.bones[0].limits = Some([-0.1, 0.1]);
        s.key_angle(0, 0, 1.0, Interpolation::Linear);
        let mut a = Vec::new();
        s.angles(0.0, &mut a);
        assert_eq!(a[0], 0.1);
    }

    #[test]
    fn auto_weights_blend_near_a_joint_and_add_up() {
        let s = arm();
        let w = auto_weights(
            &s,
            &[Vec2::new(2.0, 0.0), Vec2::new(10.0, 1.0), Vec2::new(500.0, 0.0)],
            2,
        );
        assert_eq!(w[0][0].0, 0);
        assert_eq!(w[1].len(), 2);
        for point in &w {
            let total: f32 = point.iter().map(|x| x.1).sum();
            assert!((total - 1.0).abs() < 1e-5);
        }
        // far from everything: the nearest bone alone
        assert_eq!(w[2], vec![(1, 1.0)]);
    }

    #[test]
    fn blend_averages_transforms() {
        let a = Affine2::from_translation(Vec2::new(10.0, 0.0));
        let b = Affine2::IDENTITY;
        let m = blend(&[a, b], &[(0, 1.0), (1, 1.0)]);
        assert_eq!(m.transform_point2(Vec2::ZERO), Vec2::new(5.0, 0.0));
        assert_eq!(blend(&[a], &[(7, 1.0)]), Affine2::IDENTITY);
    }

    #[test]
    fn bad_parent_index_is_treated_as_root() {
        let mut s = arm();
        s.bones[1].parent = Some(99);
        let mut poses = Vec::new();
        s.rest_pose(&mut poses);
        assert!(s.tip(&poses, 1).abs_diff_eq(Vec2::new(20.0, 0.0), 1e-5));
    }

    fn ids(s: &Skeleton) -> Vec<u32> {
        s.bones.iter().map(|b| b.id).collect()
    }

    fn legacy(n: usize) -> Skeleton {
        let mut s = Skeleton::default();
        for i in 0..n {
            s.bones.push(Bone::new(
                format!("b{i}"),
                i.checked_sub(1),
                Vec2::ZERO,
                10.0,
                0.0,
            ));
        }
        s
    }

    #[test]
    fn bones_get_ids_that_are_never_reused() {
        let mut s = Skeleton::default();
        let a = s.add("a", None, Vec2::ZERO, 10.0, 0.0);
        let b = s.add("b", Some(a), Vec2::ZERO, 10.0, 0.0);
        let c = s.add("c", Some(b), Vec2::ZERO, 10.0, 0.0);
        assert_eq!(ids(&s), [1, 2, 3]);
        assert_eq!(s.index_of(3), Some(c));
        s.remove(c);
        let d = s.add("d", Some(a), Vec2::ZERO, 10.0, 0.0);
        assert_eq!(s.bones[d].id, 4, "a removed bone's id is not given to a new one");
        assert_eq!(s.index_of(3), None);
        // removing a bone in the middle moves the others' indexes, not their ids
        s.remove(a);
        assert_eq!(ids(&s), [2, 4]);
        assert_eq!(s.index_of(4), Some(1));
        assert!(s.ids_are_sound());
    }

    #[test]
    fn bones_without_ids_get_them_in_list_order_and_keep_good_ones() {
        let mut s = legacy(3);
        assert!(!s.ids_are_sound());
        assert!(s.ensure_ids());
        assert_eq!(ids(&s), [1, 2, 3]);
        assert_eq!(s.next_bone_id, 4);
        assert!(!s.ensure_ids(), "doing it again changes nothing");

        // good ids stay; the bone with none gets the next free one
        let mut s = legacy(3);
        s.bones[0].id = 7;
        s.bones[2].id = 5;
        s.ensure_ids();
        assert_eq!(ids(&s), [7, 8, 5]);
        assert_eq!(s.next_bone_id, 9);
    }

    #[test]
    fn duplicate_out_of_range_and_runaway_ids_are_repaired() {
        let mut s = legacy(4);
        s.bones[0].id = 2;
        s.bones[1].id = 2; // taken by the first
        s.bones[2].id = MAX_BONE_ID + 5; // out of range
        s.bones[3].id = 3;
        s.next_bone_id = u32::MAX; // a damaged counter
        assert!(!s.ids_are_sound());
        s.ensure_ids();
        assert!(s.ids_are_sound(), "{:?} next {}", ids(&s), s.next_bone_id);
        assert_eq!(s.bones[0].id, 2, "the first bone keeps a contested id");
        assert_eq!(s.bones[3].id, 3);
        let mut all = ids(&s);
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 4);
        // and it still hands out ids afterwards
        let n = s.add("new", None, Vec2::ZERO, 1.0, 0.0);
        assert!(s.bones[n].id > 3 && s.ids_are_sound());
    }

    #[test]
    fn ids_are_written_only_when_given() {
        let mut s = legacy(1);
        let plain = serde_json::to_string(&s).unwrap();
        assert!(
            !plain.contains("\"id\"") && !plain.contains("next_bone_id"),
            "{plain}"
        );
        s.ensure_ids();
        let given = serde_json::to_string(&s).unwrap();
        assert!(
            given.contains("\"id\":1") && given.contains("\"next_bone_id\":2"),
            "{given}"
        );
        assert_eq!(serde_json::from_str::<Skeleton>(&given).unwrap(), s);
        assert_eq!(serde_json::from_str::<Skeleton>(&plain).unwrap(), legacy(1));
    }
}
