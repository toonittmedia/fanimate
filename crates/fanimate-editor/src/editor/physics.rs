//! Secondary motion: Bake Spring Motion on the selected bone and the bones
//! below it. The simulation is in `fanimate_core::physics`; this is how the
//! user, a script or a tool runs it as one undo step.

use fanimate_core::physics::{self, SpringParams};
use fanimate_core::{Frame, LayerId, LayerKind};

use super::{BoneRef, Editor, StatusKind};

impl Editor {
    /// The bone and every bone below it, as indexes in its skeleton.
    pub fn bone_and_descendants(&self, b: BoneRef) -> Vec<usize> {
        let Some(skeleton) = self.skeleton(b.layer) else {
            return Vec::new();
        };
        if b.bone >= skeleton.bones.len() {
            return Vec::new();
        }
        let mut below = vec![b.bone];
        // parents come before children, so one pass in order finds every descendant
        for (i, bone) in skeleton.bones.iter().enumerate().skip(b.bone + 1) {
            if bone.parent.is_some_and(|p| below.contains(&p)) {
                below.push(i);
            }
        }
        below
    }

    /// Bake Spring Motion in the menu: the selected bone and those below it,
    /// over the whole animation, with the settings in [`Editor::spring`].
    pub(super) fn bake_spring_selected(&mut self) {
        let Some(b) = self.selected_bone() else {
            self.set_status(StatusKind::Info, "Select a bone first.");
            return;
        };
        let ids: Vec<u32> = {
            let Some(skeleton) = self.skeleton(b.layer) else {
                return;
            };
            self.bone_and_descendants(b)
                .into_iter()
                .map(|i| skeleton.bones[i].id)
                .collect()
        };
        let (first, last) = (self.doc.start, self.doc.end);
        let params = self.spring;
        match self.bake_spring(b.layer, &ids, params, first, last) {
            Ok(keys) => self.set_status(
                StatusKind::Success,
                format!("Baked the spring: {keys} keys on {} bone(s)", ids.len()),
            ),
            Err(e) => self.set_status(StatusKind::Error, e),
        }
    }

    /// Run the spring on the bones named by id on `layer` over `first..=last`
    /// and write the result as keys, replacing their keys in that range (see
    /// [`physics::bake_spring`]). One undo step; nothing changes when it
    /// fails. Returns how many keys were written.
    pub fn bake_spring(
        &mut self,
        layer: LayerId,
        bone_ids: &[u32],
        params: SpringParams,
        first: Frame,
        last: Frame,
    ) -> Result<usize, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to bake.".to_owned());
        }
        let fps = self.doc.fps;
        // work on a copy, so a failure leaves the document and the undo history alone
        let mut skeleton = match self.skeleton(layer) {
            Some(s) => s.clone(),
            None => return Err("That is not a bone layer.".to_owned()),
        };
        let mut indexes = Vec::with_capacity(bone_ids.len());
        for id in bone_ids {
            match skeleton.index_of(*id) {
                Some(i) => indexes.push(i),
                None => return Err(format!("There is no bone {id} on that layer.")),
            }
        }
        let keys = physics::bake_spring(&mut skeleton, &indexes, params, fps, first, last)
            .map_err(|e| e.to_string())?;
        self.checkpoint("Bake spring motion");
        if let Some(LayerKind::Bone { skeleton: live, .. }) = self.doc.find_mut(layer).map(|l| &mut l.kind) {
            *live = skeleton;
        }
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;
    use fanimate_core::{Document, Interpolation, LayerKind, Skeleton};

    /// A bone layer with a three bone tail whose root swings.
    fn rig() -> (Editor, LayerId, Vec<u32>) {
        let mut doc = Document::new("t", 800.0, 600.0, 24.0);
        let mut skeleton = Skeleton::default();
        let root = skeleton.add("root", None, fanimate_core::Vec2::ZERO, 60.0, 0.0);
        let mid = skeleton.add("mid", Some(root), fanimate_core::Vec2::new(60.0, 0.0), 60.0, 0.0);
        skeleton.add("tip", Some(mid), fanimate_core::Vec2::new(60.0, 0.0), 60.0, 0.0);
        skeleton.bones[root].angle.set_key(1, 0.0, Interpolation::Linear);
        skeleton.bones[root].angle.set_key(10, 1.2, Interpolation::Linear);
        skeleton.bones[root].angle.set_key(40, 1.2, Interpolation::Linear);
        let ids = skeleton.bones.iter().map(|b| b.id).collect();
        let layer = doc.new_layer(
            "Rig",
            LayerKind::Bone {
                skeleton,
                children: vec![],
            },
        );
        let id = layer.id;
        doc.layers.push(layer);
        (Editor::new(doc), id, ids)
    }

    fn keys_of(ed: &Editor, layer: LayerId, bone: usize) -> usize {
        ed.skeleton(layer).unwrap().bones[bone].angle.keys.len()
    }

    #[test]
    fn baking_writes_a_key_a_frame_on_the_chosen_bones_in_one_undo_step() {
        let (mut ed, layer, ids) = rig();
        let before = keys_of(&ed, layer, 1);
        let params = SpringParams::default();
        let written = ed.bake_spring(layer, &ids[1..], params, 1, 48).unwrap();
        assert_eq!(written, 96);
        assert_eq!(keys_of(&ed, layer, 1), 48);
        assert_eq!(keys_of(&ed, layer, 0), 3, "the root keeps its own keys");
        assert!(ed.dirty);
        ed.run(Action::Undo);
        assert_eq!(keys_of(&ed, layer, 1), before);
        ed.run(Action::Redo);
        assert_eq!(keys_of(&ed, layer, 2), 48);
    }

    #[test]
    fn a_bad_request_changes_nothing_and_leaves_no_undo_step() {
        let (mut ed, layer, ids) = rig();
        let revision = ed.revision();
        let p = SpringParams::default();
        assert!(
            ed.bake_spring(layer, &[999], p, 1, 10)
                .unwrap_err()
                .contains("no bone")
        );
        assert!(ed.bake_spring(LayerId(9999), &ids, p, 1, 10).is_err());
        assert!(ed.bake_spring(layer, &ids, p, 20, 10).is_err());
        let bad = SpringParams {
            stiffness: f32::NAN,
            ..p
        };
        assert!(ed.bake_spring(layer, &ids, bad, 1, 10).is_err());
        assert_eq!(ed.revision(), revision);
        ed.playing = true;
        assert!(ed.bake_spring(layer, &ids, p, 1, 10).is_err());
        assert_eq!(keys_of(&ed, layer, 1), 0);
    }

    #[test]
    fn the_menu_command_bakes_the_selected_bone_and_everything_below_it() {
        let (mut ed, layer, _) = rig();
        assert!(!ed.can_run(Action::BakeSpring), "needs a bone");
        ed.bone = Some(BoneRef { layer, bone: 1 });
        assert!(ed.can_run(Action::BakeSpring));
        assert_eq!(ed.bone_and_descendants(BoneRef { layer, bone: 1 }), vec![1, 2]);
        assert_eq!(ed.bone_and_descendants(BoneRef { layer, bone: 0 }), vec![0, 1, 2]);
        assert!(ed.bone_and_descendants(BoneRef { layer, bone: 9 }).is_empty());
        ed.run(Action::BakeSpring);
        assert!(keys_of(&ed, layer, 1) > 40 && keys_of(&ed, layer, 2) > 40);
        assert_eq!(keys_of(&ed, layer, 0), 3, "not the root");
        // the settings in the editor are the ones used: a stiffer spring swings less
        let (mut stiff, layer2, _) = rig();
        stiff.spring = SpringParams {
            stiffness: 3000.0,
            damping: 120.0,
            inertia: 0.2,
            ..SpringParams::default()
        };
        stiff.bone = Some(BoneRef {
            layer: layer2,
            bone: 1,
        });
        stiff.run(Action::BakeSpring);
        let reach = |e: &Editor, l| {
            e.skeleton(l).unwrap().bones[1]
                .angle
                .keys
                .iter()
                .map(|k| k.value.abs())
                .fold(0.0f32, f32::max)
        };
        assert!(reach(&stiff, layer2) < reach(&ed, layer));
    }
}
