//! Setting how layers are mixed with the others: blend mode, mask role,
//! unmasked, clip and flatten (see `fanimate_core::compositing`). Each command
//! acts on every selected layer, is one undo step, and does nothing (and
//! records nothing) when every one of them is as asked already.

use fanimate_core::{Blend, Composite, LayerId, MaskRole};

use super::Editor;

impl Editor {
    /// How the selected layers' pictures mix with what is under them.
    pub fn set_blend(&mut self, blend: Blend) -> bool {
        self.change_composite("Change blend mode", |c| c.blend = blend)
    }

    /// Make the selected layers part of their group's mask, or not.
    pub fn set_mask_role(&mut self, role: MaskRole) -> bool {
        self.change_composite("Change mask", |c| c.mask = role)
    }

    /// Keep the selected layers out of their group's mask, or put them under
    /// it again: on unless every one of them is already.
    pub fn toggle_unmasked(&mut self) -> bool {
        self.toggle_composite("Change unmasked", |c| &mut c.unmasked)
    }

    /// Clip the selected layers to the layer below them, or let them go.
    pub fn toggle_clip(&mut self) -> bool {
        self.toggle_composite("Change clip", |c| &mut c.clip)
    }

    /// Draw the selected groups as one picture, or as separate layers again.
    pub fn toggle_flatten(&mut self) -> bool {
        self.toggle_composite("Change flatten", |c| &mut c.flatten)
    }

    /// Give one layer these compositing settings, whether or not it is
    /// selected. One undo step; returns whether anything changed.
    pub fn set_layer_composite(&mut self, id: LayerId, composite: Composite) -> bool {
        if self.doc.find(id).is_none_or(|l| l.composite == composite) || self.editing_blocked() {
            return false;
        }
        self.checkpoint("Change layer compositing");
        if let Some(l) = self.doc.find_mut(id) {
            l.composite = composite;
        }
        true
    }

    /// The selected layers, with the compositing settings each has.
    fn selected_composites(&self) -> Vec<(LayerId, Composite)> {
        self.selection
            .iter()
            .filter_map(|id| Some((*id, self.doc.find(*id)?.composite)))
            .collect()
    }

    /// Apply `change` to every selected layer it changes. Returns whether any changed.
    fn change_composite(&mut self, label: &str, change: impl Fn(&mut Composite)) -> bool {
        let changing: Vec<LayerId> = self
            .selected_composites()
            .into_iter()
            .filter(|(_, c)| {
                let mut after = *c;
                change(&mut after);
                after != *c
            })
            .map(|(id, _)| id)
            .collect();
        if changing.is_empty() || self.editing_blocked() {
            return false;
        }
        self.checkpoint(label);
        for id in changing {
            if let Some(l) = self.doc.find_mut(id) {
                change(&mut l.composite);
            }
        }
        true
    }

    /// Turn a setting on for every selected layer, or off when they all have it.
    fn toggle_composite(&mut self, label: &str, field: fn(&mut Composite) -> &mut bool) -> bool {
        let on = self
            .selected_composites()
            .into_iter()
            .any(|(_, mut c)| !*field(&mut c));
        self.change_composite(label, |c| *field(c) = on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::SelectMode;
    use crate::{Action, Editor};
    use fanimate_core::LayerKind;

    /// Three layers, the top one selected.
    fn three() -> (Editor, [LayerId; 3]) {
        let mut ed = Editor::untitled();
        let ids = [1, 2, 3].map(|n| {
            let l = ed.doc.new_layer(format!("l{n}"), LayerKind::vector(vec![]));
            let id = l.id;
            ed.doc.layers.push(l);
            id
        });
        ed.select(ids[2], SelectMode::Replace);
        (ed, ids)
    }

    fn composite(ed: &Editor, id: LayerId) -> Composite {
        ed.doc.find(id).unwrap().composite
    }

    #[test]
    fn each_setting_changes_the_selected_layers_in_one_undo_step() {
        let (mut ed, [a, b, c]) = three();
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        ed.run(Action::SetBlend(Blend::Multiply));
        assert_eq!(composite(&ed, a).blend, Blend::Multiply);
        assert_eq!(composite(&ed, b).blend, Blend::Multiply);
        assert!(composite(&ed, c).is_plain(), "the one not selected is left alone");
        ed.run(Action::Undo);
        assert!(composite(&ed, a).is_plain() && composite(&ed, b).is_plain());
        ed.run(Action::SetMaskRole(MaskRole::Subtract));
        assert_eq!(composite(&ed, a).mask, MaskRole::Subtract);
        ed.run(Action::ToggleClip);
        ed.run(Action::ToggleUnmasked);
        ed.run(Action::ToggleFlatten);
        let after = composite(&ed, a);
        assert!(after.clip && after.unmasked && after.flatten);
    }

    #[test]
    fn a_toggle_turns_on_unless_every_selected_layer_has_it_and_a_no_op_records_nothing() {
        let (mut ed, [a, b, _]) = three();
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        ed.run(Action::ToggleClip);
        assert!(composite(&ed, a).clip && composite(&ed, b).clip);
        // one of them lets go by itself: the next toggle puts the clip back on both
        ed.doc.find_mut(a).unwrap().composite.clip = false;
        ed.run(Action::ToggleClip);
        assert!(composite(&ed, a).clip && composite(&ed, b).clip);
        // all have it: off
        ed.run(Action::ToggleClip);
        assert!(!composite(&ed, a).clip && !composite(&ed, b).clip);
        // asking for what is so records nothing
        let before = ed.history.undo_label().map(str::to_owned);
        assert!(!ed.set_blend(Blend::Normal));
        assert!(!ed.set_mask_role(MaskRole::None));
        assert_eq!(ed.history.undo_label().map(str::to_owned), before);
    }

    #[test]
    fn nothing_changes_while_the_animation_plays_or_with_nothing_selected() {
        let (mut ed, [_, _, c]) = three();
        ed.playing = true;
        assert!(!ed.set_blend(Blend::Add));
        assert!(composite(&ed, c).is_plain());
        ed.playing = false;
        ed.selection.clear();
        assert!(!ed.can_run(Action::SetBlend(Blend::Add)));
        assert!(!ed.set_blend(Blend::Add));
    }
}
