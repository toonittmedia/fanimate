//! Rigging: the bone tools, binding artwork to bones, and weight painting.
//!
//! As in Moho, the bone tools work on the selected bone layer. Bones are
//! shown and picked where they are posed on the current frame. Posing a
//! bone always sets keyframes, even on a bone with none yet: a bone angle's
//! unkeyed value is its rest pose, which bound artwork is measured from, so
//! posing must never change it. The Select Bone tool edits the rest pose.

use std::collections::BTreeSet;

use fanimate_core::bones::{self, Skeleton};
use fanimate_core::{Affine2, Bone, LayerId, LayerKind, Shape, Vec2};

use super::{Editor, StatusKind, labelled};

/// The selected bone: its bone layer and index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoneRef {
    pub layer: LayerId,
    pub bone: usize,
}

/// A bone gesture in progress.
pub(super) enum BoneDrag {
    /// Drawing a new bone from its joint; the tip follows the pointer.
    Add { bone: BoneRef },
    /// Editing the rest pose: moving the joint, or the tip.
    Rest { bone: BoneRef, tip: bool },
    /// Posing: the angles when the drag began, and whether only the one
    /// bone turns (forward kinematics) or its chain follows (inverse).
    Pose {
        bone: BoneRef,
        start: Vec<f32>,
        grab: Vec2,
        fk: bool,
    },
    /// Painting weights for the selected bone.
    Paint { recorded: bool },
}

impl Editor {
    /// The bone layer the bone tools work on: the selected one.
    pub fn bone_layer(&self) -> Option<LayerId> {
        let id = self.primary()?;
        matches!(self.doc.find(id)?.kind, LayerKind::Bone { .. }).then_some(id)
    }

    fn skeleton(&self, layer: LayerId) -> Option<&Skeleton> {
        match &self.doc.find(layer)?.kind {
            LayerKind::Bone { skeleton, .. } => Some(skeleton),
            _ => None,
        }
    }

    fn skeleton_mut(&mut self, layer: LayerId) -> Option<&mut Skeleton> {
        match &mut self.doc.find_mut(layer)?.kind {
            LayerKind::Bone { skeleton, .. } => Some(skeleton),
            _ => None,
        }
    }

    /// The selected bone, if it still exists.
    pub fn selected_bone(&self) -> Option<BoneRef> {
        let b = self.bone?;
        (self.skeleton(b.layer)?.bones.len() > b.bone).then_some(b)
    }

    /// A bone, for the Properties panel.
    pub fn bone(&self, b: BoneRef) -> Option<&Bone> {
        self.skeleton(b.layer)?.bones.get(b.bone)
    }

    /// Change a bone's settings. Does not checkpoint; callers checkpoint
    /// once per gesture.
    pub fn edit_bone(&mut self, b: BoneRef, f: impl FnOnce(&mut Bone)) {
        if let Some(bone) = self.skeleton_mut(b.layer).and_then(|s| s.bones.get_mut(b.bone)) {
            f(bone);
            self.dirty = true;
        }
    }

    /// Set the parent of bone `b` to `new_parent` (or to root if `None`).
    /// Refuses cycles (a bone cannot be its own ancestor). One undo step.
    pub fn set_bone_parent(&mut self, b: BoneRef, new_parent: Option<usize>) -> bool {
        // refuse if new_parent is b itself or a descendant of b
        if let Some(np) = new_parent {
            if np == b.bone {
                return false;
            }
            // walk up from np: if we reach b.bone, it's a cycle
            let Some(sk) = self.skeleton(b.layer) else {
                return false;
            };
            let mut cur = Some(np);
            loop {
                match cur {
                    Some(i) if i == b.bone => return false,
                    Some(i) => cur = sk.bones.get(i).and_then(|bn| bn.parent),
                    None => break,
                }
            }
        }
        self.checkpoint("Reparent bone");
        if let Some(bone) = self.skeleton_mut(b.layer).and_then(|s| s.bones.get_mut(b.bone)) {
            bone.parent = new_parent;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Move bone at `from` to index `to` within its layer's skeleton, shifting
    /// all parent indices accordingly. No-op if `from == to` or out of range.
    /// One undo step.
    pub fn reorder_bone(&mut self, layer: LayerId, from: usize, to: usize) {
        if from == to {
            return;
        }
        let Some(skeleton) = self.skeleton_mut(layer) else {
            return;
        };
        let n = skeleton.bones.len();
        if from >= n || to >= n {
            return;
        }
        self.checkpoint("Reorder bone");
        // Build the renumber map: after moving `from` to `to`, each old index
        // maps to its new position.
        // Removing `from` shifts everything above it down by 1; then inserting
        // at `to` shifts everything at and above `to` up by 1.
        let map: Vec<Option<usize>> = (0..n)
            .map(|i| {
                Some(if i == from {
                    to
                } else if from < to {
                    // e.g. move 1→3: old 0→0, 1→3, 2→1, 3→2, 4→4
                    if i > from && i <= to { i - 1 } else { i }
                } else {
                    // e.g. move 3→1: old 0→0, 1→2, 2→3, 3→1, 4→4
                    if i >= to && i < from { i + 1 } else { i }
                })
            })
            .collect();

        // Reorder the bones vec
        let skeleton = self.skeleton_mut(layer).expect("checked above");
        let bone = skeleton.bones.remove(from);
        skeleton.bones.insert(to, bone);
        // Patch parent refs inside the skeleton
        for b in &mut skeleton.bones {
            b.parent = b.parent.and_then(|p| map.get(p).copied().flatten());
        }
        // Patch any action references
        skeleton.actions.retain_mut(|a| a.renumber(&map));
        // Patch child-layer (bound artwork) bone refs
        if let Some(layer_node) = self.doc.find_mut(layer)
            && let LayerKind::Bone { children, .. } = &mut layer_node.kind
        {
            children.iter_mut().for_each(|c| renumber_bones(c, &map));
        }
        // Update the selected bone index
        if let Some(sel) = &mut self.bone
            && sel.layer == layer
            && let Some(Some(new_i)) = map.get(sel.bone)
        {
            sel.bone = *new_i;
        }
        self.dirty = true;
    }

    /// Each bone of `layer` as posed now: (index, joint, tip) in document
    /// space.
    pub fn bone_positions(&self, layer: LayerId) -> Vec<(usize, Vec2, Vec2)> {
        let Some(skeleton) = self.skeleton(layer) else {
            return Vec::new();
        };
        let world = self.world(layer);
        let mut poses = Vec::new();
        skeleton.pose(self.current_frame() as f32, &mut poses);
        (0..skeleton.bones.len())
            .map(|i| {
                let joint = world.transform_point2(poses[i].transform_point2(Vec2::ZERO));
                (i, joint, world.transform_point2(skeleton.tip(&poses, i)))
            })
            .collect()
    }

    /// The bone of the bone layer nearest `p` (document space), within
    /// `tolerance`.
    pub fn bone_at(&self, p: Vec2, tolerance: f32) -> Option<BoneRef> {
        let layer = self.bone_layer()?;
        self.bone_positions(layer)
            .into_iter()
            .map(|(i, a, b)| (i, segment_distance(p, a, b)))
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(bone, _)| BoneRef { layer, bone })
    }

    pub fn select_bone(&mut self, bone: Option<BoneRef>) {
        self.bone = bone;
    }

    /// Document space to the bone layer's space, now.
    fn to_rig(&self, layer: LayerId) -> Affine2 {
        self.world(layer).inverse()
    }

    // ------------------------------------------------------------------
    // Add Bone

    /// Add Bone, drag start at `at`: a new bone there, child of the selected
    /// bone, with no length yet. With no bone layer selected, a new one is
    /// made. One undo step with the drag.
    pub fn begin_add_bone(&mut self, at: Vec2) -> bool {
        if !at.is_finite() {
            return false;
        }
        self.checkpoint("Add bone");
        let layer = match self.bone_layer() {
            Some(l) => l,
            None => {
                let name = self.unused_name("Bones");
                let new = labelled(self.doc.new_layer(
                    name,
                    LayerKind::Bone {
                        skeleton: Skeleton::default(),
                        children: vec![],
                    },
                ));
                let id = new.id;
                let (parent, index) = self.insertion_point();
                self.doc.insert(parent, index, new);
                self.selection = vec![id];
                self.bone = None;
                id
            }
        };
        let local = self.to_rig(layer).transform_point2(at);
        if !local.is_finite() {
            return false;
        }
        let parent = self.selected_bone().filter(|b| b.layer == layer).map(|b| b.bone);
        let frame = self.current_frame() as f32;
        let Some(skeleton) = self.skeleton_mut(layer) else {
            return false;
        };
        let mut poses = Vec::new();
        skeleton.pose(frame, &mut poses);
        let parent_world = parent.map_or(Affine2::IDENTITY, |p| poses[p]);
        let origin = parent_world.inverse().transform_point2(local);
        let n = skeleton.bones.len() + 1;
        let index = skeleton.add(format!("Bone {n}"), parent, origin, 0.0, 0.0);
        let bone = BoneRef { layer, bone: index };
        self.bone = Some(bone);
        self.bone_drag = Some(BoneDrag::Add { bone });
        true
    }

    // ------------------------------------------------------------------
    // Select Bone: the rest pose

    /// Select Bone, drag start at `at`: grab the selected bone's tip (to
    /// change its length and rest angle) or joint (to move it). Returns
    /// false when neither is within `tolerance`.
    pub fn begin_bone_edit(&mut self, at: Vec2, tolerance: f32) -> bool {
        let Some(bone) = self.selected_bone() else {
            return false;
        };
        let Some(&(_, joint, tip)) = self.bone_positions(bone.layer).get(bone.bone) else {
            return false;
        };
        let on_tip = tip.distance(at) <= tolerance;
        if !on_tip && joint.distance(at) > tolerance {
            return false;
        }
        self.checkpoint("Edit bone");
        self.bone_drag = Some(BoneDrag::Rest { bone, tip: on_tip });
        true
    }

    // ------------------------------------------------------------------
    // Manipulate Bones: posing

    /// Manipulate Bones, drag start at `at` on a bone: select it and start
    /// posing. `fk` turns only that bone.
    pub fn begin_pose(&mut self, at: Vec2, tolerance: f32, fk: bool) -> bool {
        let Some(bone) = self.bone_at(at, tolerance) else {
            return false;
        };
        let Some(skeleton) = self.skeleton(bone.layer) else {
            return false;
        };
        let mut start = Vec::new();
        skeleton.angles(self.current_frame() as f32, &mut start);
        self.checkpoint("Pose bones");
        self.bone = Some(bone);
        let grab = self.to_rig(bone.layer).transform_point2(at);
        self.bone_drag = Some(BoneDrag::Pose {
            bone,
            start,
            grab,
            fk,
        });
        true
    }

    /// Continue the bone gesture with the pointer at `to` (document space).
    pub fn drag_bone(&mut self, to: Vec2) {
        let Some(drag) = self.bone_drag.take() else { return };
        match &drag {
            BoneDrag::Add { bone } | BoneDrag::Rest { bone, tip: true } => self.set_tip(*bone, to),
            BoneDrag::Rest { bone, tip: false } => self.set_joint(*bone, to),
            BoneDrag::Pose {
                bone,
                start,
                grab,
                fk,
            } => self.pose(*bone, start, *grab, to, *fk),
            BoneDrag::Paint { .. } => {}
        }
        self.bone_drag = Some(drag);
    }

    /// End the bone gesture. A new bone left with no length is removed.
    pub fn end_bone_drag(&mut self) {
        if let Some(BoneDrag::Add { bone }) = self.bone_drag.take()
            && self.bone(bone).is_some_and(|b| b.length < 0.5)
        {
            self.remove_bone(bone);
            self.bone = bone_parent(self, bone);
        }
    }

    pub fn is_dragging_bone(&self) -> bool {
        self.bone_drag.is_some()
    }

    /// During an AddBone drag, the joint (origin) of the bone being drawn,
    /// in document space. Used for angle snapping.
    pub fn bone_drag_joint(&self) -> Option<Vec2> {
        if let Some(BoneDrag::Add { bone }) = self.bone_drag {
            let positions = self.bone_positions(bone.layer);
            positions
                .into_iter()
                .find(|(i, _, _)| *i == bone.bone)
                .map(|(_, joint, _)| joint)
        } else {
            None
        }
    }

    /// Point bone `b`'s tip at `to`: length and rest angle.
    fn set_tip(&mut self, b: BoneRef, to: Vec2) {
        let local = self.to_rig(b.layer).transform_point2(to);
        let frame = self.current_frame() as f32;
        let Some(skeleton) = self.skeleton_mut(b.layer) else {
            return;
        };
        let mut poses = Vec::new();
        skeleton.pose(frame, &mut poses);
        if b.bone >= poses.len() {
            return;
        }
        // in the bone's parent space, where its origin and angle are
        let parent = poses_parent(skeleton, &poses, b.bone).unwrap_or(Affine2::IDENTITY);
        let v = parent.inverse().transform_point2(local) - skeleton.bones[b.bone].origin;
        if !v.is_finite() || v.length() < 1e-4 {
            return;
        }
        let bone = &mut skeleton.bones[b.bone];
        bone.length = v.length();
        bone.angle.base = bone.clamp(v.to_angle());
        self.dirty = true;
    }

    fn set_joint(&mut self, b: BoneRef, to: Vec2) {
        let local = self.to_rig(b.layer).transform_point2(to);
        let frame = self.current_frame() as f32;
        let Some(skeleton) = self.skeleton_mut(b.layer) else {
            return;
        };
        let mut poses = Vec::new();
        skeleton.pose(frame, &mut poses);
        let parent = poses_parent(skeleton, &poses, b.bone).unwrap_or(Affine2::IDENTITY);
        let origin = parent.inverse().transform_point2(local);
        if origin.is_finite() {
            skeleton.bones[b.bone].origin = origin;
            self.dirty = true;
        }
    }

    fn pose(&mut self, b: BoneRef, start: &[f32], grab: Vec2, to: Vec2, fk: bool) {
        let local = self.to_rig(b.layer).transform_point2(to);
        let frame = self.current_frame();
        let interp = self.key_interp;
        let Some(skeleton) = self.skeleton_mut(b.layer) else {
            return;
        };
        let mut angles = start.to_vec();
        if fk {
            let mut poses = Vec::new();
            skeleton.pose_from(start, &mut poses);
            let joint = poses[b.bone].transform_point2(Vec2::ZERO);
            let turn = (grab - joint).angle_to(local - joint);
            if turn.is_finite() {
                angles[b.bone] = skeleton.bones[b.bone].clamp(start[b.bone] + turn);
            }
        } else {
            skeleton.solve_ik(&mut angles, b.bone, local);
        }
        for (i, bone) in skeleton.bones.iter_mut().enumerate() {
            if (angles[i] - start[i]).abs() > 1e-6 || bone.angle.keys.iter().any(|k| k.frame == frame) {
                bone.angle.set_key(frame, angles[i], interp);
            }
        }
        self.dirty = true;
    }

    /// Remove a bone: its children join its parent, and artwork bound to
    /// it is released. Smart Bone actions it drove are removed too.
    pub(super) fn remove_bone(&mut self, b: BoneRef) {
        let Some(skeleton) = self.skeleton_mut(b.layer) else {
            return;
        };
        let map = skeleton.remove(b.bone);
        if let Some(layer) = self.doc.find_mut(b.layer) {
            renumber_bones(layer, &map);
        }
    }

    /// Delete the selected bone, as one undo step.
    pub(super) fn delete_selected_bone(&mut self) -> bool {
        let Some(b) = self.selected_bone() else {
            return false;
        };
        let name = self.bone(b).map(|b| b.name.clone()).unwrap_or_default();
        self.checkpoint("Delete bone");
        self.bone = bone_parent(self, b);
        self.remove_bone(b);
        self.set_status(StatusKind::Info, format!("Deleted bone \"{name}\""));
        true
    }

    // ------------------------------------------------------------------
    // Binding

    /// The shapes a bind command acts on: those with selected points, or
    /// every shape of the selected vector layers. With the bone layer
    /// enclosing each.
    fn bind_targets(&self) -> Vec<(LayerId, LayerId, Vec<usize>)> {
        let mut out = Vec::new();
        let with_points: BTreeSet<(LayerId, usize)> =
            self.points.iter().map(|p| (p.layer, p.shape)).collect();
        for &id in &self.selection {
            let Some(shapes) = self.layer_shapes(id) else {
                continue;
            };
            let Some(rig) = self.enclosing_rig(id) else {
                continue;
            };
            let picked: Vec<usize> = if with_points.iter().any(|(l, _)| *l == id) {
                with_points
                    .iter()
                    .filter(|(l, _)| *l == id)
                    .map(|(_, s)| *s)
                    .collect()
            } else {
                (0..shapes.len()).collect()
            };
            out.push((id, rig, picked));
        }
        out
    }

    /// Selected image layers inside a bone layer, with that bone layer.
    fn image_targets(&self) -> Vec<(LayerId, LayerId)> {
        self.selection
            .iter()
            .filter(|id| matches!(self.doc.find(**id).map(|l| &l.kind), Some(LayerKind::Image(_))))
            .filter_map(|id| Some((*id, self.enclosing_rig(*id)?)))
            .collect()
    }

    fn image_mut(&mut self, id: LayerId) -> Option<&mut fanimate_core::Image> {
        match &mut self.doc.find_mut(id)?.kind {
            LayerKind::Image(img) => Some(img),
            _ => None,
        }
    }

    /// The nearest bone layer around `id`.
    pub fn enclosing_rig(&self, id: LayerId) -> Option<LayerId> {
        let mut current = self.doc.locate(id).and_then(|(p, _)| p);
        while let Some(c) = current {
            if matches!(self.doc.find(c)?.kind, LayerKind::Bone { .. }) {
                return Some(c);
            }
            current = self.doc.locate(c).and_then(|(p, _)| p);
        }
        None
    }

    /// The transform from layer `id`'s space to its bone layer's space,
    /// now. Bound points are stored as they would be in the rest pose.
    fn layer_to_rig(&self, id: LayerId, rig: LayerId) -> Affine2 {
        self.world(rig).inverse() * self.world(id)
    }

    /// Bind the target shapes rigidly to the selected bone. They stay where
    /// they are on screen.
    pub(super) fn bind_to_bone(&mut self) -> bool {
        let Some(bone) = self.selected_bone() else {
            self.set_status(StatusKind::Info, "Select a bone first, with the Select Bone tool");
            return false;
        };
        let targets: Vec<_> = self
            .bind_targets()
            .into_iter()
            .filter(|t| t.1 == bone.layer)
            .collect();
        let images: Vec<LayerId> = self
            .image_targets()
            .into_iter()
            .filter(|t| t.1 == bone.layer)
            .map(|t| t.0)
            .collect();
        if targets.is_empty() && images.is_empty() {
            self.set_status(
                StatusKind::Info,
                "Select a vector or image layer inside the bone layer, or points on its shapes",
            );
            return false;
        }
        let frame = self.current_frame() as f32;
        let Some(skeleton) = self.skeleton(bone.layer) else {
            return false;
        };
        let (mut pose, mut rest) = (Vec::new(), Vec::new());
        skeleton.pose(frame, &mut pose);
        skeleton.rest_pose(&mut rest);
        let skin = pose[bone.bone] * rest[bone.bone].inverse();
        let moves: Vec<(LayerId, Vec<usize>, Affine2)> = targets
            .into_iter()
            .map(|(id, rig, shapes)| {
                let rel = self.layer_to_rig(id, rig);
                // stored so the bone's movement puts them where they are now
                (id, shapes, rel.inverse() * skin.inverse() * rel)
            })
            .collect();
        self.checkpoint("Bind to bone");
        // pictures cannot be re-drawn in the rest pose, so they bind as
        // they are in it
        for id in images {
            if let Some(img) = self.image_mut(id) {
                img.bone = Some(bone.bone);
                img.skin = None;
            }
        }
        let mut n = 0;
        for (id, shapes, to_rest) in moves {
            for_shapes(self, id, &shapes, |s| {
                if to_rest.is_finite() {
                    s.path.transform(to_rest);
                    for k in &mut s.path_keys {
                        k.value.transform(to_rest);
                    }
                }
                s.bone = Some(bone.bone);
                s.skin = None;
                n += 1;
            });
        }
        self.set_status(
            StatusKind::Info,
            format!("Bound {n} shape{} to the bone", super::plural(n)),
        );
        true
    }

    /// Bind the target shapes smoothly: each point follows the bones near
    /// it in the rest pose, with weights made automatically.
    pub(super) fn bind_smoothly(&mut self) -> bool {
        let targets = self.bind_targets();
        let images = self.image_targets();
        if targets.is_empty() && images.is_empty() {
            self.set_status(
                StatusKind::Info,
                "Select a vector or image layer inside a bone layer, or points on its shapes",
            );
            return false;
        }
        // pictures bend on a mesh: 8 by 8 cells unless set already
        let mut image_weights: Vec<(LayerId, [u32; 2], bones::Skin)> = Vec::new();
        for (id, rig) in &images {
            let Some(skeleton) = self.skeleton(*rig) else {
                continue;
            };
            let Some(LayerKind::Image(img)) = self.doc.find(*id).map(|l| &l.kind) else {
                continue;
            };
            if skeleton.bones.is_empty() {
                continue;
            }
            let mut img = img.clone();
            if img.cells() == [1, 1] {
                img.grid = [8, 8];
            }
            let rel = self.layer_to_rig(*id, *rig);
            let pts: Vec<Vec2> = img
                .vertices()
                .iter()
                .map(|(p, _)| rel.transform_point2(*p))
                .collect();
            image_weights.push((*id, img.cells(), bones::auto_weights(skeleton, &pts, 3)));
        }
        let mut weights: Vec<(LayerId, usize, bones::Skin)> = Vec::new();
        for (id, rig, shapes) in &targets {
            let Some(skeleton) = self.skeleton(*rig) else {
                continue;
            };
            if skeleton.bones.is_empty() {
                continue;
            }
            let rel = self.layer_to_rig(*id, *rig);
            for &i in shapes {
                let Some(shape) = self.point_shape((*id, i)) else {
                    continue;
                };
                let pts: Vec<Vec2> = shape
                    .path
                    .on_curve_points()
                    .iter()
                    .map(|p| rel.transform_point2(*p))
                    .collect();
                weights.push((*id, i, bones::auto_weights(skeleton, &pts, 3)));
            }
        }
        if weights.is_empty() && image_weights.is_empty() {
            self.set_status(StatusKind::Info, "The bone layer has no bones yet");
            return false;
        }
        self.checkpoint("Bind smoothly");
        let n = weights.len() + image_weights.len();
        for (id, grid, w) in image_weights {
            if let Some(img) = self.image_mut(id) {
                img.grid = grid;
                img.skin = Some(w);
                img.bone = None;
            }
        }
        for (id, i, w) in weights {
            if let Some(s) = self.point_shape_mut((id, i)) {
                s.skin = Some(w);
                s.bone = None;
            }
        }
        self.set_status(
            StatusKind::Info,
            format!("Bound {n} shape{} to the bones around them", super::plural(n)),
        );
        true
    }

    pub(super) fn unbind(&mut self) -> bool {
        let targets = self.bind_targets();
        let images = self.image_targets();
        let bound = targets.iter().any(|(id, _, shapes)| {
            shapes.iter().any(|&i| {
                self.point_shape((*id, i))
                    .is_some_and(|s| s.bone.is_some() || s.skin.is_some())
            })
        }) || images.iter().any(|(id, _)| {
            matches!(self.doc.find(*id).map(|l| &l.kind), Some(LayerKind::Image(i)) if i.bone.is_some() || i.skin.is_some())
        });
        if !bound {
            return false;
        }
        self.checkpoint("Release from bones");
        for (id, _) in images {
            if let Some(img) = self.image_mut(id) {
                img.bone = None;
                img.skin = None;
            }
        }
        for (id, _, shapes) in targets {
            for_shapes(self, id, &shapes, |s| {
                s.bone = None;
                s.skin = None;
            });
        }
        true
    }

    pub(super) fn can_bind(&self) -> bool {
        !self.bind_targets().is_empty() || !self.image_targets().is_empty()
    }

    // ------------------------------------------------------------------
    // Paint Weights

    pub fn begin_paint_weights(&mut self) -> bool {
        if self.selected_bone().is_none() {
            self.set_status(StatusKind::Info, "Select a bone first, with the Select Bone tool");
            return false;
        }
        if self
            .point_layer()
            .is_none_or(|l| Some(l) != self.points_layer_in_rig())
        {
            self.set_status(
                StatusKind::Info,
                "Select a vector layer inside the selected bone's layer",
            );
            return false;
        }
        self.bone_drag = Some(BoneDrag::Paint { recorded: false });
        true
    }

    /// The selected vector layer, if it is inside the selected bone's layer.
    fn points_layer_in_rig(&self) -> Option<LayerId> {
        let layer = self.point_layer()?;
        let bone = self.bone?;
        (self.enclosing_rig(layer) == Some(bone.layer)).then_some(layer)
    }

    /// Brush weights at `at` (document space): points within `radius` follow
    /// the selected bone more (or less, with `away`) by up to `amount`.
    pub fn paint_weights(&mut self, at: Vec2, radius: f32, amount: f32, away: bool) {
        let Some(bone) = self.selected_bone() else { return };
        let Some(layer) = self.points_layer_in_rig() else {
            return;
        };
        let touched: Vec<(usize, usize, f32)> = self
            .point_positions()
            .into_iter()
            .filter_map(|(r, p)| {
                let d = p.distance(at);
                (d < radius).then(|| (r.shape, r.point, amount * (1.0 - d / radius)))
            })
            .collect();
        if touched.is_empty() {
            return;
        }
        if let Some(BoneDrag::Paint { recorded }) = &mut self.bone_drag
            && !*recorded
        {
            *recorded = true;
            self.checkpoint("Paint weights");
        }
        for (shape, point, strength) in touched {
            let Some(s) = self.point_shape_mut((layer, shape)) else {
                continue;
            };
            let n = s.path.on_curve_count();
            if s.valid_skin().is_none() {
                // start from what the shape follows now
                let from = s.bone.map_or_else(Vec::new, |b| vec![(b, 1.0)]);
                s.skin = Some(vec![from; n]);
                s.bone = None;
            }
            if let Some(skin) = s.skin.as_mut()
                && let Some(w) = skin.get_mut(point)
            {
                paint(w, bone.bone, strength, away);
            }
        }
        self.dirty = true;
    }

    pub fn end_paint_weights(&mut self) {
        if matches!(self.bone_drag, Some(BoneDrag::Paint { .. })) {
            self.bone_drag = None;
        }
    }

    /// How much each point of the point layer follows the selected bone, for
    /// drawing: (position, weight 0 to 1).
    pub fn bone_weights(&self) -> Vec<(Vec2, f32)> {
        let Some(bone) = self.selected_bone() else {
            return Vec::new();
        };
        let Some(layer) = self.points_layer_in_rig() else {
            return Vec::new();
        };
        self.point_positions()
            .into_iter()
            .map(|(r, p)| {
                let w = self
                    .point_shape((layer, r.shape))
                    .map_or(0.0, |s| match s.valid_skin() {
                        Some(skin) => {
                            let ws = &skin[r.point];
                            let total: f32 = ws.iter().map(|x| x.1).sum();
                            let mine: f32 = ws.iter().filter(|x| x.0 == bone.bone).map(|x| x.1).sum();
                            if total > 0.0 { mine / total } else { 0.0 }
                        }
                        None => f32::from(s.bone == Some(bone.bone)),
                    });
                (p, w)
            })
            .collect()
    }
}

/// Add `strength` of `bone` to a point's weights (or take it away), keeping
/// the total at 1.
fn paint(weights: &mut Vec<(usize, f32)>, bone: usize, strength: f32, away: bool) {
    let total: f32 = weights.iter().map(|w| w.1).sum();
    if total > 0.0 {
        for w in weights.iter_mut() {
            w.1 /= total;
        }
    }
    let current = weights.iter().find(|w| w.0 == bone).map_or(0.0, |w| w.1);
    let target = if away {
        (current - strength).max(0.0)
    } else {
        (current + strength).min(1.0)
    };
    let others = 1.0 - current;
    let left = 1.0 - target;
    for w in weights.iter_mut() {
        if w.0 != bone {
            w.1 = if others > 0.0 { w.1 / others * left } else { 0.0 };
        }
    }
    match weights.iter_mut().find(|w| w.0 == bone) {
        Some(w) => w.1 = target,
        None => weights.push((bone, target)),
    }
    // a point with no other bone keeps following this one
    if weights.iter().all(|w| w.1 <= 0.0) {
        for w in weights.iter_mut() {
            if w.0 == bone {
                w.1 = 1.0;
            }
        }
    }
    weights.retain(|w| w.1 > 1e-4);
}

fn poses_parent(skeleton: &Skeleton, poses: &[Affine2], bone: usize) -> Option<Affine2> {
    skeleton.bones[bone].parent.and_then(|p| poses.get(p)).copied()
}

fn bone_parent(ed: &Editor, b: BoneRef) -> Option<BoneRef> {
    let p = ed.bone(b)?.parent?;
    Some(BoneRef {
        layer: b.layer,
        bone: p,
    })
}

fn for_shapes(ed: &mut Editor, id: LayerId, shapes: &[usize], mut f: impl FnMut(&mut Shape)) {
    if let Some(all) = ed.layer_shapes_mut(id) {
        for &i in shapes {
            if let Some(s) = all.get_mut(i) {
                f(s);
            }
        }
    }
}

fn renumber_shape(s: &mut Shape, map: &[Option<usize>]) {
    s.bone = s.bone.and_then(|b| map.get(b).copied().flatten());
    if let Some(skin) = &mut s.skin {
        for point in skin.iter_mut() {
            point.retain_mut(|(b, _)| match map.get(*b).copied().flatten() {
                Some(n) => {
                    *b = n;
                    true
                }
                None => false,
            });
        }
    }
}

/// Renumber bone references in the shapes inside a bone layer after a bone
/// is removed; references to the removed bone are dropped.
fn renumber_bones(layer: &mut fanimate_core::Layer, map: &[Option<usize>]) {
    fn go(layer: &mut fanimate_core::Layer, map: &[Option<usize>]) {
        match &mut layer.kind {
            LayerKind::Frames { cels } => {
                for s in cels.iter_mut().flat_map(|c| c.shapes_mut()) {
                    renumber_shape(s, map);
                }
            }
            LayerKind::Vector { shapes } => {
                for s in shapes {
                    s.bone = s.bone.and_then(|b| map.get(b).copied().flatten());
                    if let Some(skin) = &mut s.skin {
                        for point in skin.iter_mut() {
                            point.retain_mut(|(b, _)| match map.get(*b).copied().flatten() {
                                Some(n) => {
                                    *b = n;
                                    true
                                }
                                None => false,
                            });
                        }
                    }
                }
            }
            LayerKind::Image(img) => {
                img.bone = img.bone.and_then(|b| map.get(b).copied().flatten());
                if let Some(skin) = &mut img.skin {
                    for point in skin.iter_mut() {
                        point.retain_mut(|(b, _)| match map.get(*b).copied().flatten() {
                            Some(n) => {
                                *b = n;
                                true
                            }
                            None => false,
                        });
                    }
                }
            }
            // a nested bone layer's shapes refer to its own bones
            LayerKind::Bone { .. } | LayerKind::Audio(_) => {}
            LayerKind::Group { children } | LayerKind::Switch { children, .. } => {
                children.iter_mut().for_each(|c| go(c, map))
            }
        }
    }
    if let LayerKind::Bone { children, .. } = &mut layer.kind {
        children.iter_mut().for_each(|c| go(c, map));
    }
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
    use crate::{Action, Tool};
    use fanimate_core::{Document, evaluate};

    /// A bone layer with a two-bone arm along x from (100, 100): 50 + 50,
    /// holding an empty vector layer.
    fn rig() -> (Editor, LayerId, LayerId) {
        let mut doc = Document::new("t", 400.0, 400.0, 24.0);
        let art = doc.new_layer("art", LayerKind::vector(vec![]));
        let art_id = art.id;
        let rig = doc.new_layer(
            "rig",
            LayerKind::Bone {
                skeleton: Skeleton::default(),
                children: vec![art],
            },
        );
        let rig_id = rig.id;
        doc.layers.push(rig);
        let mut ed = Editor::new(doc);
        ed.selection = vec![rig_id];
        ed.tool = Tool::AddBone;
        for (from, to) in [(100.0, 150.0), (150.0, 200.0)] {
            assert!(ed.begin_add_bone(Vec2::new(from, 100.0)));
            ed.drag_bone(Vec2::new(to, 100.0));
            ed.end_bone_drag();
        }
        (ed, rig_id, art_id)
    }

    fn skeleton(ed: &Editor, id: LayerId) -> Skeleton {
        ed.skeleton(id).unwrap().clone()
    }

    #[test]
    fn add_bone_builds_a_chain_from_the_selected_bone() {
        let (mut ed, rig, _) = rig();
        let s = skeleton(&ed, rig);
        assert_eq!(s.bones.len(), 2);
        assert_eq!(s.bones[1].parent, Some(0));
        assert!((s.bones[0].length - 50.0).abs() < 1e-3);
        let pos = ed.bone_positions(rig);
        assert!(pos[1].2.abs_diff_eq(Vec2::new(200.0, 100.0), 1e-3));
        // a click with no drag leaves no bone
        assert!(ed.begin_add_bone(Vec2::new(300.0, 300.0)));
        ed.end_bone_drag();
        assert_eq!(skeleton(&ed, rig).bones.len(), 2);
        // each bone is one undo step
        ed.run(Action::Undo);
        ed.run(Action::Undo);
        assert_eq!(skeleton(&ed, rig).bones.len(), 1);
    }

    #[test]
    fn add_bone_with_no_bone_layer_makes_one() {
        let mut ed = Editor::untitled();
        assert!(ed.begin_add_bone(Vec2::ZERO));
        ed.drag_bone(Vec2::new(0.0, 40.0));
        ed.end_bone_drag();
        let rig = ed.bone_layer().unwrap();
        assert_eq!(skeleton(&ed, rig).bones.len(), 1);
    }

    #[test]
    fn posing_with_ik_keys_the_chain_and_fk_turns_one_bone() {
        let (mut ed, rig, _) = rig();
        ed.tool = Tool::ManipulateBones;
        ed.go_to(10);
        assert!(ed.begin_pose(Vec2::new(190.0, 100.0), 3.0, false));
        ed.drag_bone(Vec2::new(150.0, 160.0));
        ed.end_bone_drag();
        let pos = ed.bone_positions(rig);
        assert!(pos[1].2.distance(Vec2::new(150.0, 160.0)) < 1.0, "{:?}", pos[1].2);
        let s = skeleton(&ed, rig);
        assert!(s.bones.iter().all(|b| b.angle.keys.iter().any(|k| k.frame == 10)));
        // the rest pose is untouched
        assert!(s.bones.iter().all(|b| b.angle.base == 0.0));
        ed.run(Action::Undo);
        assert!(skeleton(&ed, rig).bones.iter().all(|b| b.angle.keys.is_empty()));

        assert!(ed.begin_pose(Vec2::new(190.0, 100.0), 3.0, true));
        ed.drag_bone(Vec2::new(150.0, 150.0));
        ed.end_bone_drag();
        let s = skeleton(&ed, rig);
        assert!(s.bones[0].angle.keys.is_empty());
        assert!(!s.bones[1].angle.keys.is_empty());
    }

    #[test]
    fn select_bone_edits_the_rest_pose() {
        let (mut ed, rig, _) = rig();
        ed.select_bone(Some(BoneRef { layer: rig, bone: 1 }));
        assert!(!ed.begin_bone_edit(Vec2::new(175.0, 100.0), 3.0));
        assert!(ed.begin_bone_edit(Vec2::new(200.0, 100.0), 3.0));
        ed.drag_bone(Vec2::new(150.0, 180.0));
        ed.end_bone_drag();
        let b = &skeleton(&ed, rig).bones[1];
        assert!((b.length - 80.0).abs() < 1e-3);
        assert!((b.angle.base - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        assert!(b.angle.keys.is_empty());
    }

    #[test]
    fn deleting_a_bone_releases_what_it_held() {
        let (mut ed, rig, art) = rig();
        ed.tool = Tool::SelectBone;
        ed.select_bone(Some(BoneRef { layer: rig, bone: 0 }));
        if let Some(LayerKind::Vector { shapes }) = ed.doc.find_mut(art).map(|l| &mut l.kind) {
            let mut a = Shape::new(
                "a",
                fanimate_core::Path::circle(Vec2::ZERO, 1.0),
                Default::default(),
            );
            a.bone = Some(0);
            let mut b = a.clone();
            b.bone = Some(1);
            shapes.extend([a, b]);
        }
        ed.run(Action::Delete);
        assert_eq!(skeleton(&ed, rig).bones.len(), 1);
        let shapes = match &ed.doc.find(art).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.clone(),
            _ => unreachable!(),
        };
        assert_eq!(shapes[0].bone, None);
        assert_eq!(shapes[1].bone, Some(0));
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
    }

    #[test]
    fn binding_keeps_artwork_in_place_and_it_then_follows() {
        let (mut ed, rig, art) = rig();
        ed.selection = vec![art];
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::new(160.0, 90.0),
            Vec2::new(190.0, 110.0),
            false,
            false,
            1.0,
        );
        ed.select_bone(Some(BoneRef { layer: rig, bone: 1 }));
        ed.run(Action::BindToBone);
        let before: Vec<Vec2> = evaluate(&ed.doc, 0.0)[0].shape.path.on_curve_points();
        assert_eq!(before[0], Vec2::new(160.0, 90.0));
        // pose the lower arm: the rectangle turns with it
        ed.selection = vec![rig];
        ed.go_to(5);
        assert!(ed.begin_pose(Vec2::new(195.0, 100.0), 3.0, true));
        ed.drag_bone(Vec2::new(150.0, 150.0));
        ed.end_bone_drag();
        let items = evaluate(&ed.doc, 5.0);
        let p = items[0]
            .transform
            .transform_point2(items[0].shape.path.on_curve_points()[0]);
        assert!(p.distance(Vec2::new(160.0, 90.0)) > 5.0);
    }

    #[test]
    fn smooth_binding_and_weight_painting() {
        let (mut ed, rig, art) = rig();
        ed.selection = vec![art];
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::new(110.0, 90.0),
            Vec2::new(190.0, 110.0),
            false,
            false,
            1.0,
        );
        ed.run(Action::BindSmoothly);
        let skin = |ed: &Editor| ed.point_shape((art, 0)).unwrap().skin.clone().unwrap();
        assert_eq!(skin(&ed).len(), 4);
        ed.select_bone(Some(BoneRef { layer: rig, bone: 0 }));
        ed.tool = Tool::PaintWeights;
        assert!(ed.begin_paint_weights());
        let before = ed.bone_weights();
        ed.paint_weights(Vec2::new(190.0, 90.0), 5.0, 0.5, false);
        ed.end_paint_weights();
        let after = ed.bone_weights();
        let i = after
            .iter()
            .position(|(p, _)| p.distance(Vec2::new(190.0, 90.0)) < 1.0)
            .unwrap();
        assert!(after[i].1 > before[i].1);
        for point in skin(&ed) {
            let total: f32 = point.iter().map(|w| w.1).sum();
            assert!((total - 1.0).abs() < 1e-4);
        }
        assert_eq!(ed.history.undo_label(), Some("Paint weights"));
        ed.run(Action::Unbind);
        assert!(ed.point_shape((art, 0)).unwrap().skin.is_none());
    }

    #[test]
    fn pictures_bind_on_a_mesh_and_bend() {
        let (mut ed, rig, _) = rig();
        // a picture over the arm, inside the bone layer
        let png = {
            let img = image::RgbaImage::from_pixel(100, 20, image::Rgba([1, 2, 3, 255]));
            let mut b = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png)
                .unwrap();
            b
        };
        ed.selection = vec![rig];
        let id = ed.import_picture("arm.png", png).unwrap();
        ed.doc.find_mut(id).unwrap().transform.position.base = Vec2::new(100.0, 90.0);
        ed.run(Action::BindSmoothly);
        let LayerKind::Image(img) = &ed.doc.find(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(img.cells(), [8, 8]);
        assert!(img.valid_skin().is_some());
        // bend the elbow: the picture's far end moves, its near end stays
        ed.selection = vec![rig];
        ed.go_to(3);
        assert!(ed.begin_pose(Vec2::new(195.0, 100.0), 3.0, true));
        ed.drag_bone(Vec2::new(150.0, 150.0));
        ed.end_bone_drag();
        let items = evaluate(&ed.doc, 3.0);
        let pic = items.iter().find(|i| i.layer == id).unwrap();
        let verts = &pic.image.as_ref().unwrap().vertices;
        let world = |p: Vec2| pic.transform.transform_point2(p);
        assert!(world(verts[0].0).distance(Vec2::new(100.0, 90.0)) < 0.5);
        assert!(world(verts[8].0).distance(Vec2::new(200.0, 90.0)) > 20.0);
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
        ed.selection = vec![id];
        ed.run(Action::Unbind);
        let LayerKind::Image(img) = &ed.doc.find(id).unwrap().kind else {
            panic!()
        };
        assert!(img.skin.is_none());
    }

    #[test]
    fn painting_weights_shifts_and_renormalises() {
        let mut w = vec![(0, 1.0)];
        paint(&mut w, 1, 0.25, false);
        assert_eq!(w, vec![(0, 0.75), (1, 0.25)]);
        paint(&mut w, 1, 1.0, true);
        assert_eq!(w, vec![(0, 1.0)]);
    }
}
