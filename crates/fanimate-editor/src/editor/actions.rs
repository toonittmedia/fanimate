//! The Actions panel's commands: making, editing, inserting and deleting
//! actions and Smart Bone actions on a bone layer.

use fanimate_core::actions::RigAction;
use fanimate_core::{Frame, LayerId, LayerKind, Skeleton};

use super::{Editor, StatusKind};

impl Editor {
    /// The bone layer the Actions panel shows: the selected bone layer, or
    /// the one around the selected layer.
    pub fn actions_layer(&self) -> Option<LayerId> {
        let id = self.primary()?;
        match self.doc.find(id)?.kind {
            LayerKind::Bone { .. } => Some(id),
            _ => self.enclosing_rig(id),
        }
    }

    pub fn rig_actions(&self, layer: LayerId) -> &[RigAction] {
        match self.doc.find(layer).map(|l| &l.kind) {
            Some(LayerKind::Bone { skeleton, .. }) => &skeleton.actions,
            _ => &[],
        }
    }

    fn rig_mut(&mut self, layer: LayerId) -> Option<&mut Skeleton> {
        match &mut self.doc.find_mut(layer)?.kind {
            LayerKind::Bone { skeleton, .. } => Some(skeleton),
            _ => None,
        }
    }

    /// The action being edited: its bone layer and index.
    pub fn editing_action(&self) -> Option<(LayerId, usize)> {
        self.doc.action_edit.as_ref().map(|e| (e.layer, e.action))
    }

    /// Whether layer `id` is inside the bone layer whose action is being
    /// edited, so changes to it go into the action.
    pub(super) fn in_edited_action(&self, id: LayerId) -> bool {
        self.editing_action()
            .is_some_and(|(rig, _)| self.doc.is_within(id, rig))
    }

    /// Make a new action and open it. A Smart Bone action is driven by the
    /// selected bone and named after it; it starts with the bone at rest on
    /// frame 0, so turning it on later frames sets the range it plays over.
    pub(super) fn new_action(&mut self, smart: bool) -> bool {
        let Some(layer) = self.actions_layer() else {
            self.set_status(StatusKind::Info, "Select a bone layer to add an action to it");
            return false;
        };
        let driver = if smart {
            match self.selected_bone().filter(|b| b.layer == layer) {
                Some(b) => Some(b.bone),
                None => {
                    self.set_status(
                        StatusKind::Info,
                        "Select the bone that will drive the action first",
                    );
                    return false;
                }
            }
        } else {
            None
        };
        let length = (self.doc.fps.round() as Frame).max(1);
        let name = match driver.and_then(|d| self.bone(super::BoneRef { layer, bone: d })) {
            Some(b) => b.name.clone(),
            None => format!("Action {}", self.rig_actions(layer).len() + 1),
        };
        self.checkpoint(if smart {
            "New Smart Bone action"
        } else {
            "New action"
        });
        self.doc.leave_action();
        let Some(skeleton) = self.rig_mut(layer) else {
            return false;
        };
        let mut act = RigAction::new(name, driver, length);
        if let Some(d) = driver {
            let rest = skeleton.bones[d].angle.base;
            act.bones.push(fanimate_core::actions::BoneKeys {
                bone: d,
                angle: fanimate_core::Channel::new(rest).with_key(
                    0,
                    rest,
                    fanimate_core::Interpolation::Linear,
                ),
            });
        }
        skeleton.actions.push(act);
        let index = skeleton.actions.len() - 1;
        self.doc.enter_action(layer, index);
        self.go_to(0);
        self.selection = vec![layer];
        self.set_status(
            StatusKind::Info,
            if smart {
                "Editing the Smart Bone action: turn its bone on a later frame, then reshape what it drives"
            } else {
                "Editing the action: animate it, then choose Finish Editing Action"
            },
        );
        true
    }

    pub(super) fn edit_action(&mut self, index: usize) -> bool {
        let Some(layer) = self.actions_layer() else {
            return false;
        };
        if index >= self.rig_actions(layer).len() || self.editing_action() == Some((layer, index)) {
            return false;
        }
        self.checkpoint("Edit action");
        self.doc.enter_action(layer, index);
        self.go_to(0);
        true
    }

    pub(super) fn finish_action(&mut self) -> bool {
        if self.editing_action().is_none() {
            return false;
        }
        self.checkpoint("Finish editing action");
        self.doc.leave_action();
        true
    }

    /// Copy a regular action's keys into the timeline, starting at the
    /// current frame.
    pub(super) fn insert_action(&mut self, index: usize) -> bool {
        if self.editing_action().is_some() {
            self.set_status(StatusKind::Info, "Finish editing the action first");
            return false;
        }
        let Some(layer) = self.actions_layer() else {
            return false;
        };
        let at = self.current_frame();
        match self.insert_action_on(layer, index, at) {
            Ok(name) => {
                self.set_status(StatusKind::Info, format!("Inserted \"{name}\" at frame {at}"));
                true
            }
            Err(e) => {
                self.set_status(StatusKind::Info, e);
                false
            }
        }
    }

    /// Copy the keys of action `index` of the bone layer `layer` into the
    /// timeline starting at frame `at`. One undo step. Returns the action's
    /// name. A Smart Bone action is refused: it plays by itself when its bone
    /// turns.
    pub fn insert_action_on(&mut self, layer: LayerId, index: usize, at: Frame) -> Result<String, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to insert an action.".to_owned());
        }
        if self.editing_action().is_some() {
            return Err("Finish editing the action first.".to_owned());
        }
        if at < 0 {
            return Err("A frame is 0 or later.".to_owned());
        }
        let Some(act) = self.rig_actions(layer).get(index).cloned() else {
            return Err("There is no such action.".to_owned());
        };
        if act.driver.is_some() {
            return Err("Smart Bone actions play by themselves when their bone turns".to_owned());
        }
        self.checkpoint("Insert action");
        if let Some(skeleton) = self.rig_mut(layer) {
            for b in &act.bones {
                if let Some(bone) = skeleton.bones.get_mut(b.bone) {
                    for k in &b.angle.keys {
                        bone.angle.set_key(at.saturating_add(k.frame), k.value, k.interp);
                    }
                }
            }
        }
        for s in &act.shapes {
            if let Some(shape) = self.point_shape_mut((s.layer, s.shape)) {
                for k in &s.path_keys {
                    if k.value.same_structure(&shape.path) {
                        shape.key_path(at.saturating_add(k.frame), k.value.clone(), k.interp);
                    }
                }
            }
        }
        Ok(act.name)
    }

    pub(super) fn delete_action(&mut self, index: usize) -> bool {
        let Some(layer) = self.actions_layer() else {
            return false;
        };
        if index >= self.rig_actions(layer).len() {
            return false;
        }
        self.checkpoint("Delete action");
        if self.editing_action().is_some_and(|(l, _)| l == layer) {
            self.doc.leave_action();
        }
        if let Some(s) = self.rig_mut(layer) {
            s.actions.remove(index);
            // master controllers that held it let go; those holding later actions follow them up
            for c in &mut s.controllers {
                c.links.retain(|l| l.action != index);
                for l in &mut c.links {
                    if l.action > index {
                        l.action -= 1;
                    }
                }
            }
        }
        true
    }

    pub fn rename_action(&mut self, index: usize, name: &str) {
        let name = name.trim();
        let Some(layer) = self.actions_layer() else { return };
        if name.is_empty() || self.rig_actions(layer).get(index).is_none_or(|a| a.name == name) {
            return;
        }
        self.checkpoint("Rename action");
        if let Some(a) = self.rig_mut(layer).and_then(|s| s.actions.get_mut(index)) {
            a.name = name.to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::BoneRef;
    use crate::{Action, Tool};
    use fanimate_core::{Document, Vec2, evaluate};

    /// A bone layer with one bone along x, and a vector layer with a line
    /// whose tip a Smart Bone will move.
    fn rig() -> (Editor, LayerId, LayerId) {
        let mut doc = Document::new("t", 400.0, 400.0, 24.0);
        doc.end = 47;
        let line = fanimate_core::Shape::new(
            "mouth",
            fanimate_core::Path::new()
                .move_to(Vec2::new(0.0, 200.0))
                .line_to(Vec2::new(100.0, 200.0)),
            fanimate_core::Style::default().with_stroke(fanimate_core::Color::BLACK, 2.0),
        );
        let art = doc.new_layer("face", LayerKind::vector(vec![line]));
        let art_id = art.id;
        let mut skeleton = Skeleton::default();
        skeleton.add("head", None, Vec2::new(100.0, 100.0), 50.0, 0.0);
        let rig = doc.new_layer(
            "rig",
            LayerKind::Bone {
                skeleton,
                children: vec![art],
            },
        );
        let rig_id = rig.id;
        doc.layers.push(rig);
        let mut ed = Editor::new(doc);
        ed.selection = vec![rig_id];
        (ed, rig_id, art_id)
    }

    #[test]
    fn a_smart_bone_action_is_made_by_posing_and_reshaping() {
        let (mut ed, rig, art) = rig();
        assert!(!ed.new_action(true), "needs a bone");
        ed.select_bone(Some(BoneRef { layer: rig, bone: 0 }));
        ed.run(Action::NewSmartBoneAction);
        assert_eq!(ed.editing_action(), Some((rig, 0)));
        assert_eq!(ed.rig_actions(rig)[0].name, "head");
        // frame 24: turn the head a quarter and drop the mouth's tip
        ed.go_to(24);
        ed.tool = Tool::ManipulateBones;
        assert!(ed.begin_pose(Vec2::new(140.0, 100.0), 3.0, true));
        ed.drag_bone(Vec2::new(100.0, 140.0));
        ed.end_bone_drag();
        ed.selection = vec![art];
        ed.tool = Tool::SelectPoints;
        let tip = ed.point_at(Vec2::new(100.0, 200.0), 2.0).unwrap();
        ed.select_point(tip, crate::SelectMode::Replace);
        assert!(ed.begin_point_drag());
        ed.drag_points(Vec2::new(0.0, 30.0));
        ed.run(Action::FinishAction);
        assert_eq!(ed.editing_action(), None);
        // the timeline is untouched: no keys anywhere
        assert!(ed.key_frames(rig).is_empty() && ed.key_frames(art).is_empty());
        // turning the head halfway in the timeline drops the mouth halfway
        ed.selection = vec![rig];
        ed.go_to(10);
        ed.tool = Tool::ManipulateBones;
        assert!(ed.begin_pose(Vec2::new(140.0, 100.0), 3.0, true));
        ed.drag_bone(Vec2::new(
            100.0 + 40.0 * std::f32::consts::FRAC_1_SQRT_2,
            100.0 + 40.0 * std::f32::consts::FRAC_1_SQRT_2,
        ));
        ed.end_bone_drag();
        let items = evaluate(&ed.doc, 10.0);
        let tip = items[0].shape.path.on_curve_points()[1];
        assert!((tip.y - 215.0).abs() < 1.5, "{tip:?}");
        // saved without the editing state, and clean
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
    }

    #[test]
    fn actions_survive_undo_while_editing() {
        let (mut ed, rig, _) = rig();
        ed.run(Action::NewAction);
        assert!(ed.editing_action().is_some());
        ed.run(Action::Undo);
        assert_eq!(ed.editing_action(), None);
        assert!(ed.rig_actions(rig).is_empty());
        ed.run(Action::Redo);
        assert_eq!(ed.editing_action(), Some((rig, 0)));
    }

    #[test]
    fn a_regular_action_is_inserted_into_the_timeline() {
        let (mut ed, rig, _) = rig();
        ed.run(Action::NewAction);
        ed.select_bone(Some(BoneRef { layer: rig, bone: 0 }));
        ed.tool = Tool::ManipulateBones;
        for f in [0, 6] {
            ed.go_to(f);
            assert!(ed.begin_pose(Vec2::new(140.0, 100.0), 3.0, true));
            ed.drag_bone(Vec2::new(140.0, 102.0 + f as f32));
            ed.end_bone_drag();
        }
        ed.run(Action::FinishAction);
        ed.go_to(30);
        ed.run(Action::InsertAction(0));
        assert_eq!(ed.key_frames(rig), [30, 36]);
        ed.run(Action::DeleteAction(0));
        assert!(ed.rig_actions(rig).is_empty());
        ed.rename_action(0, "x");
    }

    #[test]
    fn saving_while_editing_writes_the_timeline() {
        let (mut ed, rig, _) = rig();
        ed.select_bone(Some(BoneRef { layer: rig, bone: 0 }));
        ed.run(Action::NewSmartBoneAction);
        let dir = std::env::temp_dir().join(format!("fanimate-action-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.fnm");
        ed.save_to(&path).unwrap();
        let (loaded, report) = fanimate_fnm::load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded.action_edit, None);
        assert_eq!(loaded, *ed.doc.mainline());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
