//! The pose library: Save Pose and Mirror Pose. A pose is a one-frame action
//! (see `fanimate_core::poses`), so it is listed in the Actions panel and
//! applied with Insert Action Here at the playhead, like any other action.

use fanimate_core::actions::RigAction;
use fanimate_core::poses;
use fanimate_core::retarget;
use fanimate_core::{LayerId, LayerKind};

use super::{Editor, StatusKind};

impl Editor {
    /// Save Pose in the Actions panel: the whole rig as it is on the playhead
    /// frame, named "Pose N".
    pub(super) fn save_pose(&mut self) -> bool {
        let Some(layer) = self.actions_layer() else {
            self.set_status(StatusKind::Info, "Select a bone layer to save a pose of it");
            return false;
        };
        match self.save_pose_named(layer, "") {
            Ok(index) => {
                let name = self.rig_actions(layer)[index].name.clone();
                self.set_status(StatusKind::Success, format!("Saved \"{name}\""));
                true
            }
            Err(e) => {
                self.set_status(StatusKind::Info, e);
                false
            }
        }
    }

    /// Save the rig of `layer` as it is on the playhead frame as a pose named
    /// `name` (or "Pose N" when empty; a name already used gets a number).
    /// One undo step. Returns the new action's index.
    pub fn save_pose_named(&mut self, layer: LayerId, name: &str) -> Result<usize, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to save a pose.".to_owned());
        }
        if self.editing_action().is_some() {
            return Err("Finish editing the action first.".to_owned());
        }
        let frame = self.frame;
        let Some(LayerKind::Bone { skeleton, .. }) = self.doc.find(layer).map(|l| &l.kind) else {
            return Err("That is not a bone layer.".to_owned());
        };
        if skeleton.bones.is_empty() {
            return Err("The rig has no bones to save a pose of.".to_owned());
        }
        if skeleton.actions.len() >= MAX_ACTIONS {
            return Err(format!("A rig has at most {MAX_ACTIONS} actions and poses."));
        }
        let taken: Vec<&str> = skeleton.actions.iter().map(|a| a.name.as_str()).collect();
        let name = unique_name(name.trim(), &taken);
        let pose = poses::snapshot(skeleton, name, frame);
        self.checkpoint("Save pose");
        let Some(LayerKind::Bone { skeleton, .. }) = self.doc.find_mut(layer).map(|l| &mut l.kind) else {
            return Err("That is not a bone layer.".to_owned());
        };
        skeleton.actions.push(pose);
        Ok(skeleton.actions.len() - 1)
    }

    /// Mirror Pose in the Actions panel's menu: a new pose that is the pose
    /// turned left to right.
    pub(super) fn mirror_pose_action(&mut self, index: usize) -> bool {
        let Some(layer) = self.actions_layer() else {
            return false;
        };
        match self.mirror_pose_at(layer, index) {
            Ok((new, summary)) => {
                let name = self.rig_actions(layer)[new].name.clone();
                self.set_status(StatusKind::Success, format!("Made \"{name}\"{summary}"));
                true
            }
            Err(e) => {
                self.set_status(StatusKind::Info, e);
                false
            }
        }
    }

    /// Make the mirror of pose `index` of `layer` as a new pose after it. One
    /// undo step. Returns the new pose's index and a note about bones that
    /// could not be mirrored (empty when all could).
    pub fn mirror_pose_at(&mut self, layer: LayerId, index: usize) -> Result<(usize, String), String> {
        if self.editing_blocked() {
            return Err("Pause the animation to mirror a pose.".to_owned());
        }
        if self.editing_action().is_some() {
            return Err("Finish editing the action first.".to_owned());
        }
        let Some(LayerKind::Bone { skeleton, .. }) = self.doc.find(layer).map(|l| &l.kind) else {
            return Err("That is not a bone layer.".to_owned());
        };
        let Some(pose) = skeleton.actions.get(index).filter(|a| RigAction::is_pose(a)) else {
            return Err("That action is not a pose: only a pose can be mirrored.".to_owned());
        };
        if skeleton.actions.len() >= MAX_ACTIONS {
            return Err(format!("A rig has at most {MAX_ACTIONS} actions and poses."));
        }
        let mirrored = poses::mirror_pose(skeleton, pose);
        if mirrored.pose.bones.is_empty() {
            return Err("None of the bones in that pose could be mirrored.".to_owned());
        }
        let taken: Vec<&str> = skeleton.actions.iter().map(|a| a.name.as_str()).collect();
        let mut new = mirrored.pose;
        new.name = unique_name(&new.name, &taken);
        let note = if mirrored.unmatched > 0 {
            format!(
                " ({} bone(s) with a side in their name had no partner and were left out)",
                mirrored.unmatched
            )
        } else {
            String::new()
        };
        self.checkpoint("Mirror pose");
        let Some(LayerKind::Bone { skeleton, .. }) = self.doc.find_mut(layer).map(|l| &mut l.kind) else {
            return Err("That is not a bone layer.".to_owned());
        };
        skeleton.actions.insert(index + 1, new);
        Ok((index + 1, note))
    }
}

impl Editor {
    /// The bone layers of the document with their names, other than `except`:
    /// where an action can be copied to.
    pub fn other_bone_layers(&self, except: LayerId) -> Vec<(LayerId, String)> {
        let mut out = Vec::new();
        self.doc.walk(|l| {
            if l.id != except && matches!(l.kind, LayerKind::Bone { .. }) {
                out.push((l.id, l.name.clone()));
            }
        });
        out
    }

    /// Copy action `index` of rig `from` onto rig `to` by bone names, keeping
    /// each bone's turn from its own rest angle (see `fanimate_core::retarget`).
    /// One undo step. Returns the new action's index in `to` and a note about
    /// what was left out (empty when nothing was).
    pub fn retarget_action_to(
        &mut self,
        from: LayerId,
        index: usize,
        to: LayerId,
    ) -> Result<(usize, String), String> {
        if self.editing_blocked() {
            return Err("Pause the animation to copy an action.".to_owned());
        }
        if self.editing_action().is_some() {
            return Err("Finish editing the action first.".to_owned());
        }
        if from == to {
            return Err("Choose another rig: an action already plays on its own.".to_owned());
        }
        let skeleton_of = |ed: &Self, id: LayerId| match ed.doc.find(id).map(|l| &l.kind) {
            Some(LayerKind::Bone { skeleton, .. }) => Some(skeleton.clone()),
            _ => None,
        };
        let (Some(src), Some(dst)) = (skeleton_of(self, from), skeleton_of(self, to)) else {
            return Err("That is not a bone layer.".to_owned());
        };
        let Some(action) = src.actions.get(index) else {
            return Err("There is no such action.".to_owned());
        };
        if dst.actions.len() >= MAX_ACTIONS {
            return Err(format!("A rig has at most {MAX_ACTIONS} actions and poses."));
        }
        let made = retarget::retarget(&src, action, &dst)?;
        let taken: Vec<&str> = dst.actions.iter().map(|a| a.name.as_str()).collect();
        let mut new = made.action;
        new.name = unique_name(&new.name, &taken);
        let mut notes = Vec::new();
        if !made.unmatched.is_empty() {
            notes.push(format!(
                "{} bone(s) had no match ({})",
                made.unmatched.len(),
                made.unmatched.join(", ")
            ));
        }
        if made.shapes_dropped > 0 {
            notes.push(format!(
                "{} shape(s) with point keys were left out",
                made.shapes_dropped
            ));
        }
        self.checkpoint("Copy action to rig");
        let Some(LayerKind::Bone { skeleton, .. }) = self.doc.find_mut(to).map(|l| &mut l.kind) else {
            return Err("That is not a bone layer.".to_owned());
        };
        skeleton.actions.push(new);
        let note = if notes.is_empty() {
            String::new()
        } else {
            format!(" ({})", notes.join("; "))
        };
        Ok((skeleton.actions.len() - 1, note))
    }

    /// Copy to Rig in the Actions panel's menu.
    pub(super) fn retarget_action_command(&mut self, index: usize, to: LayerId) {
        let Some(from) = self.actions_layer() else {
            return;
        };
        match self.retarget_action_to(from, index, to) {
            Ok((new, note)) => {
                let name = self
                    .rig_actions(to)
                    .get(new)
                    .map_or(String::new(), |a| a.name.clone());
                let rig = self.doc.find(to).map_or(String::new(), |l| l.name.clone());
                self.set_status(
                    StatusKind::Success,
                    format!("Copied to \"{rig}\" as \"{name}\"{note}"),
                );
            }
            Err(e) => self.set_status(StatusKind::Info, e),
        }
    }
}

/// The most actions and poses a rig takes through these commands.
const MAX_ACTIONS: usize = 1_000;

/// `wanted` (or "Pose N" when empty), with a number added until no action has it.
fn unique_name(wanted: &str, taken: &[&str]) -> String {
    let base = if wanted.is_empty() { "Pose" } else { wanted };
    let exists = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !wanted.is_empty() && !exists(base) {
        return base.to_owned();
    }
    (1..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !exists(n))
        .unwrap_or_else(|| base.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;
    use fanimate_core::{Document, Interpolation, Skeleton, Vec2};

    fn rig() -> (Editor, LayerId) {
        let mut doc = Document::new("t", 800.0, 600.0, 24.0);
        let mut skeleton = Skeleton::default();
        let spine = skeleton.add("spine", None, Vec2::ZERO, 80.0, 0.0);
        let left = skeleton.add("arm L", Some(spine), Vec2::new(0.0, 0.0), 60.0, 0.0);
        skeleton.add("arm R", Some(spine), Vec2::new(0.0, 0.0), 60.0, 0.0);
        skeleton.bones[left].angle.set_key(1, 0.0, Interpolation::Linear);
        skeleton.bones[left].angle.set_key(13, 1.2, Interpolation::Linear);
        let layer = doc.new_layer(
            "Rig",
            LayerKind::Bone {
                skeleton,
                children: vec![],
            },
        );
        let id = layer.id;
        doc.layers.push(layer);
        let mut ed = Editor::new(doc);
        ed.selection = vec![id];
        (ed, id)
    }

    fn angle(ed: &Editor, layer: LayerId, bone: usize, frame: f32) -> f32 {
        ed.skeleton(layer).unwrap().bones[bone].angle.sample(frame)
    }

    #[test]
    fn saving_a_pose_snapshots_the_rig_on_the_playhead_and_inserting_it_poses_the_rig_there() {
        let (mut ed, layer) = rig();
        ed.go_to(13);
        assert!(ed.can_run(Action::SavePose));
        ed.run(Action::SavePose);
        let poses = ed.rig_actions(layer);
        assert_eq!(poses.len(), 1);
        assert_eq!(poses[0].name, "Pose 1");
        assert!(poses[0].is_pose());
        // pose the rig somewhere else on the timeline with the ordinary Insert Action
        ed.go_to(40);
        ed.run(Action::InsertAction(0));
        assert!((angle(&ed, layer, 1, 40.0) - 1.2).abs() < 1e-5);
        // two poses in a row get their own names, and saving is one undo step
        ed.run(Action::SavePose);
        assert_eq!(ed.rig_actions(layer)[1].name, "Pose 2");
        ed.run(Action::Undo);
        assert_eq!(ed.rig_actions(layer).len(), 1);
    }

    #[test]
    fn a_named_pose_keeps_its_name_unless_taken() {
        let (mut ed, layer) = rig();
        assert_eq!(ed.save_pose_named(layer, "  Wave  ").unwrap(), 0);
        assert_eq!(ed.rig_actions(layer)[0].name, "Wave");
        ed.save_pose_named(layer, "wave").unwrap();
        assert_eq!(ed.rig_actions(layer)[1].name, "wave 1");
    }

    #[test]
    fn mirroring_makes_a_new_pose_with_the_sides_swapped_and_leaves_the_original() {
        let (mut ed, layer) = rig();
        ed.go_to(13);
        ed.save_pose_named(layer, "Reach L").unwrap();
        assert!(ed.can_run(Action::MirrorPose(0)));
        ed.run(Action::MirrorPose(0));
        let actions = ed.rig_actions(layer);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[1].name, "Reach R");
        assert_eq!(actions[0].name, "Reach L");
        // inserted, the mirrored pose raises the other arm the other way
        ed.go_to(50);
        ed.run(Action::InsertAction(1));
        assert!(
            (angle(&ed, layer, 2, 50.0) + 1.2).abs() < 1e-5,
            "{}",
            angle(&ed, layer, 2, 50.0)
        );
        // undo takes back the insert, then the mirroring in one step
        ed.run(Action::Undo);
        assert_eq!(ed.rig_actions(layer).len(), 2);
        ed.run(Action::Undo);
        assert_eq!(ed.rig_actions(layer).len(), 1);
    }

    #[test]
    fn things_that_are_not_poses_or_cannot_be_done_are_refused_and_change_nothing() {
        let (mut ed, layer) = rig();
        let revision = ed.revision();
        assert!(ed.mirror_pose_at(layer, 0).is_err(), "no such pose");
        assert!(!ed.can_run(Action::MirrorPose(0)));
        ed.run(Action::NewAction);
        ed.run(Action::FinishAction);
        let revision_with_action = ed.revision();
        assert!(
            !ed.can_run(Action::MirrorPose(0)),
            "an empty action is not a pose"
        );
        assert!(ed.mirror_pose_at(layer, 0).unwrap_err().contains("not a pose"));
        // a rig with no bones, and a layer that is not a rig
        let empty = ed.doc.new_layer(
            "Empty",
            LayerKind::Bone {
                skeleton: Skeleton::default(),
                children: vec![],
            },
        );
        let empty_id = empty.id;
        ed.doc.layers.push(empty);
        assert!(ed.save_pose_named(empty_id, "x").is_err());
        let vector = ed.doc.new_layer("v", LayerKind::vector(vec![]));
        let vector_id = vector.id;
        ed.doc.layers.push(vector);
        assert!(ed.save_pose_named(vector_id, "x").is_err());
        assert_eq!(ed.revision(), revision_with_action);
        assert!(revision_with_action >= revision);
        // nothing while playing
        ed.playing = true;
        assert!(ed.save_pose_named(layer, "x").is_err());
    }

    #[test]
    fn a_pose_with_no_partner_for_a_side_says_so() {
        let mut doc = Document::new("t", 800.0, 600.0, 24.0);
        let mut skeleton = Skeleton::default();
        skeleton.add("spine", None, Vec2::ZERO, 80.0, 0.0);
        skeleton.add("wing L", Some(0), Vec2::ZERO, 60.0, 0.0);
        let layer = doc.new_layer(
            "Rig",
            LayerKind::Bone {
                skeleton,
                children: vec![],
            },
        );
        let id = layer.id;
        doc.layers.push(layer);
        let mut ed = Editor::new(doc);
        ed.save_pose_named(id, "Flap").unwrap();
        let (_, note) = ed.mirror_pose_at(id, 0).unwrap();
        assert!(note.contains("no partner"), "{note}");
    }

    /// A second bone layer with the same bones named another way and another rest pose.
    fn second_rig(ed: &mut Editor) -> LayerId {
        let mut skeleton = Skeleton::default();
        let spine = skeleton.add("Spine", None, Vec2::ZERO, 40.0, 0.0);
        skeleton.add("Arm_L", Some(spine), Vec2::ZERO, 30.0, -0.5);
        skeleton.add("tail", Some(spine), Vec2::ZERO, 30.0, 0.0);
        let layer = ed.doc.new_layer(
            "Small rig",
            LayerKind::Bone {
                skeleton,
                children: vec![],
            },
        );
        let id = layer.id;
        ed.doc.layers.push(layer);
        id
    }

    #[test]
    fn an_action_is_copied_to_another_rig_by_bone_names_in_one_undo_step() {
        let (mut ed, tall) = rig();
        ed.go_to(13);
        ed.save_pose_named(tall, "Reach").unwrap();
        let small = second_rig(&mut ed);
        assert_eq!(ed.other_bone_layers(tall), [(small, "Small rig".to_owned())]);
        assert!(ed.other_bone_layers(small).iter().all(|(id, _)| *id == tall));
        let before = ed.revision();
        assert!(ed.can_run(Action::RetargetAction(0, small.0)));
        ed.run(Action::RetargetAction(0, small.0));
        assert_eq!(ed.revision(), before + 1);
        let copied = &ed.rig_actions(small)[0];
        assert_eq!(copied.name, "Reach");
        // the arm turned 1.2 from a rest of 0 on the tall rig: 1.2 from -0.5 on this one
        assert_eq!(
            copied.bones.len(),
            2,
            "the spine and the arm; arm R has no partner"
        );
        let arm = copied.bones.iter().find(|b| b.bone == 1).unwrap();
        assert!(
            (arm.angle.keys[0].value - 0.7).abs() < 1e-5,
            "{:?}",
            arm.angle.keys
        );
        // the original is untouched, a second copy gets its own name
        assert_eq!(ed.rig_actions(tall).len(), 1);
        ed.run(Action::RetargetAction(0, small.0));
        assert_eq!(ed.rig_actions(small)[1].name, "Reach 1");
        ed.run(Action::Undo);
        ed.run(Action::Undo);
        assert!(ed.rig_actions(small).is_empty(), "each copy was one step");
    }

    #[test]
    fn copies_that_cannot_be_made_are_refused_and_change_nothing() {
        let (mut ed, tall) = rig();
        ed.save_pose_named(tall, "Pose").unwrap();
        let small = second_rig(&mut ed);
        let before = ed.doc.clone();
        assert!(ed.retarget_action_to(tall, 0, tall).is_err(), "not onto itself");
        assert!(ed.retarget_action_to(tall, 5, small).is_err(), "no such action");
        assert!(
            ed.retarget_action_to(tall, 0, LayerId(999)).is_err(),
            "no such rig"
        );
        assert!(!ed.can_run(Action::RetargetAction(0, tall.0)), "the rig it is on");
        assert!(!ed.can_run(Action::RetargetAction(3, small.0)));
        assert!(!ed.can_run(Action::RetargetAction(0, 999)));
        ed.playing = true;
        assert!(!ed.can_run(Action::RetargetAction(0, small.0)));
        assert!(ed.retarget_action_to(tall, 0, small).is_err());
        assert_eq!(ed.doc, before);
    }
}
