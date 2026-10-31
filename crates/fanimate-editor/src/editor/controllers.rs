//! Master controllers: sliders and pads on a bone layer whose values play
//! actions (see `fanimate_core::actions::Controller`). These are the
//! commands that make, link and set them; the value of a controller follows
//! the keying rule like any other animated value.

use fanimate_core::actions::{
    Controller, ControllerAxis, ControllerKind, ControllerLink, MAX_CONTROLLERS, MAX_LINKS,
};
use fanimate_core::{LayerId, LayerKind};

use super::{Editor, StatusKind};

impl Editor {
    /// The master controllers of a bone layer.
    pub fn controllers(&self, layer: LayerId) -> &[Controller] {
        match self.doc.find(layer).map(|l| &l.kind) {
            Some(LayerKind::Bone { skeleton, .. }) => &skeleton.controllers,
            _ => &[],
        }
    }

    /// Whether controllers can be edited now: not while playing and not while
    /// an action is open (its rig shows none).
    pub fn controllers_open(&self) -> bool {
        !self.playing && self.editing_action().is_none()
    }

    /// New Slider and New Pad in the Actions panel.
    pub(super) fn new_controller_action(&mut self, kind: ControllerKind) {
        let Some(layer) = self.actions_layer() else {
            self.set_status(StatusKind::Info, "Select a bone layer to add a controller to it");
            return;
        };
        match self.add_controller(layer, "", kind) {
            Ok(i) => {
                let name = self.controllers(layer)[i].name.clone();
                self.set_status(StatusKind::Success, format!("Added \"{name}\""));
            }
            Err(e) => self.set_status(StatusKind::Info, e),
        }
    }

    pub(super) fn delete_controller_action(&mut self, index: usize) {
        let Some(layer) = self.actions_layer() else {
            return;
        };
        if let Err(e) = self.remove_controller(layer, index) {
            self.set_status(StatusKind::Info, e);
        }
    }

    pub(super) fn link_controller_action(&mut self, controller: usize, action: usize, axis: ControllerAxis) {
        let Some(layer) = self.actions_layer() else {
            return;
        };
        if let Err(e) = self.link_controller(layer, controller, action, axis, 0.0, 1.0) {
            self.set_status(StatusKind::Info, e);
        }
    }

    pub(super) fn unlink_controller_action(&mut self, controller: usize, action: usize) {
        let Some(layer) = self.actions_layer() else {
            return;
        };
        if let Err(e) = self.unlink_controller(layer, controller, action) {
            self.set_status(StatusKind::Info, e);
        }
    }

    fn controllers_mut(&mut self, layer: LayerId) -> Option<&mut Vec<Controller>> {
        match &mut self.doc.find_mut(layer)?.kind {
            LayerKind::Bone { skeleton, .. } => Some(&mut skeleton.controllers),
            _ => None,
        }
    }

    fn controller_guard(&mut self, layer: LayerId) -> Result<(), String> {
        if self.editing_blocked() {
            return Err("Pause the animation to change controllers.".to_owned());
        }
        if self.editing_action().is_some() {
            return Err("Finish editing the action first.".to_owned());
        }
        if self.skeleton(layer).is_none() {
            return Err("That is not a bone layer.".to_owned());
        }
        Ok(())
    }

    /// Add a slider or a pad called `name` (or "Slider N" / "Pad N") to a bone
    /// layer. One undo step. Returns its index.
    pub fn add_controller(
        &mut self,
        layer: LayerId,
        name: &str,
        kind: ControllerKind,
    ) -> Result<usize, String> {
        self.controller_guard(layer)?;
        let count = self.controllers(layer).len();
        if count >= MAX_CONTROLLERS {
            return Err(format!("A rig has at most {MAX_CONTROLLERS} controllers."));
        }
        let name = name.trim();
        if name.chars().count() > 200 {
            return Err("A controller's name has at most 200 characters.".to_owned());
        }
        let name = if name.is_empty() {
            format!(
                "{} {}",
                if kind == ControllerKind::Pad {
                    "Pad"
                } else {
                    "Slider"
                },
                count + 1
            )
        } else {
            name.to_owned()
        };
        self.checkpoint("Add controller");
        let list = self.controllers_mut(layer).ok_or("That is not a bone layer.")?;
        list.push(Controller::new(name, kind));
        Ok(list.len() - 1)
    }

    /// Remove a controller. One undo step.
    pub fn remove_controller(&mut self, layer: LayerId, index: usize) -> Result<(), String> {
        self.controller_guard(layer)?;
        if index >= self.controllers(layer).len() {
            return Err("There is no such controller.".to_owned());
        }
        self.checkpoint("Remove controller");
        if let Some(list) = self.controllers_mut(layer) {
            list.remove(index);
        }
        Ok(())
    }

    /// Make controller `controller` play action `action` from its value `from`
    /// (the action on its first frame) to `to` (its last), reading `axis`. A
    /// link the controller already has to that action on that axis is replaced.
    /// A Smart Bone action cannot be linked: it plays by its own bone. One undo
    /// step.
    pub fn link_controller(
        &mut self,
        layer: LayerId,
        controller: usize,
        action: usize,
        axis: ControllerAxis,
        from: f32,
        to: f32,
    ) -> Result<(), String> {
        self.controller_guard(layer)?;
        let Some(c) = self.controllers(layer).get(controller) else {
            return Err("There is no such controller.".to_owned());
        };
        let Some(act) = self.rig_actions(layer).get(action) else {
            return Err("There is no such action.".to_owned());
        };
        if act.driver.is_some() {
            return Err("A Smart Bone action plays by its own bone and cannot be linked.".to_owned());
        }
        if !(from.is_finite() && to.is_finite()) || from == to {
            return Err("The range needs two different numbers.".to_owned());
        }
        if c.kind == ControllerKind::Slider && axis == ControllerAxis::Y {
            return Err("A slider has one value: link its x.".to_owned());
        }
        let replaces = c.links.iter().any(|l| l.action == action && l.axis == axis);
        if !replaces && c.links.len() >= MAX_LINKS {
            return Err(format!("A controller drives at most {MAX_LINKS} actions."));
        }
        self.checkpoint("Link controller");
        if let Some(c) = self.controllers_mut(layer).and_then(|l| l.get_mut(controller)) {
            c.links.retain(|l| !(l.action == action && l.axis == axis));
            c.links.push(ControllerLink {
                action,
                axis,
                from,
                to,
            });
        }
        Ok(())
    }

    /// Take away a controller's hold on an action. One undo step; nothing
    /// changes (and no step is made) when there was none.
    pub fn unlink_controller(
        &mut self,
        layer: LayerId,
        controller: usize,
        action: usize,
    ) -> Result<bool, String> {
        self.controller_guard(layer)?;
        let Some(c) = self.controllers(layer).get(controller) else {
            return Err("There is no such controller.".to_owned());
        };
        if !c.links.iter().any(|l| l.action == action) {
            return Ok(false);
        }
        self.checkpoint("Unlink controller");
        if let Some(c) = self.controllers_mut(layer).and_then(|l| l.get_mut(controller)) {
            c.links.retain(|l| l.action != action);
        }
        Ok(true)
    }

    /// Rename a controller. One undo step; nothing changes (and no step is
    /// made) when the name is the same. A name is at most 200 characters.
    pub fn rename_controller(
        &mut self,
        layer: LayerId,
        controller: usize,
        name: &str,
    ) -> Result<bool, String> {
        self.rename_controller_with(layer, controller, name, true)
    }

    /// [`rename_controller`](Self::rename_controller) for a text box that has
    /// begun its edit: no undo step of its own, and the text is kept as typed.
    pub fn change_controller_name(
        &mut self,
        layer: LayerId,
        controller: usize,
        name: &str,
    ) -> Result<bool, String> {
        self.rename_controller_with(layer, controller, name, false)
    }

    fn rename_controller_with(
        &mut self,
        layer: LayerId,
        controller: usize,
        name: &str,
        step: bool,
    ) -> Result<bool, String> {
        self.controller_guard(layer)?;
        if name.chars().count() > 200 {
            return Err("A controller's name has at most 200 characters.".to_owned());
        }
        let name = if step { name.trim() } else { name };
        let Some(c) = self.controllers(layer).get(controller) else {
            return Err("There is no such controller.".to_owned());
        };
        if c.name == name {
            return Ok(false);
        }
        if step {
            self.checkpoint("Rename controller");
        }
        if let Some(c) = self.controllers_mut(layer).and_then(|l| l.get_mut(controller)) {
            name.clone_into(&mut c.name);
        }
        Ok(true)
    }

    /// Change the range of link `link` (its position in the controller's
    /// links) for a widget that has begun its edit: no undo step of its own.
    /// The range needs two different finite numbers.
    pub fn change_link_range(
        &mut self,
        layer: LayerId,
        controller: usize,
        link: usize,
        from: f32,
        to: f32,
    ) -> Result<bool, String> {
        self.controller_guard(layer)?;
        if !(from.is_finite() && to.is_finite()) || from == to {
            return Err("The range needs two different numbers.".to_owned());
        }
        let Some(l) = self
            .controllers(layer)
            .get(controller)
            .and_then(|c| c.links.get(link))
        else {
            return Err("There is no such link.".to_owned());
        };
        if l.from == from && l.to == to {
            return Ok(false);
        }
        if let Some(l) = self
            .controllers_mut(layer)
            .and_then(|list| list.get_mut(controller))
            .and_then(|c| c.links.get_mut(link))
        {
            l.from = from;
            l.to = to;
        }
        Ok(true)
    }

    /// Set one value of a controller (0 to 1) on the playhead frame by the
    /// keying rule: on the rest pose the value itself, on a later frame a key.
    /// One undo step. Returns whether anything changed.
    pub fn set_controller_value(
        &mut self,
        layer: LayerId,
        controller: usize,
        axis: ControllerAxis,
        value: f32,
    ) -> Result<bool, String> {
        self.controller_value_with(layer, controller, axis, value, true)
    }

    /// [`set_controller_value`](Self::set_controller_value) for a widget that
    /// has begun its edit: no undo step of its own.
    pub fn change_controller_value(
        &mut self,
        layer: LayerId,
        controller: usize,
        axis: ControllerAxis,
        value: f32,
    ) -> Result<bool, String> {
        self.controller_value_with(layer, controller, axis, value, false)
    }

    fn controller_value_with(
        &mut self,
        layer: LayerId,
        controller: usize,
        axis: ControllerAxis,
        value: f32,
        step: bool,
    ) -> Result<bool, String> {
        self.controller_guard(layer)?;
        if !value.is_finite() {
            return Err("A controller value must be a number.".to_owned());
        }
        let keying = self.keying();
        let frame = self.current_frame();
        let Some(current) = self.controllers(layer).get(controller) else {
            return Err("There is no such controller.".to_owned());
        };
        let mut edited = current.clone();
        let channel = match axis {
            ControllerAxis::X => &mut edited.x,
            ControllerAxis::Y => &mut edited.y,
        };
        channel.write(frame, value.clamp(0.0, 1.0), keying);
        if edited == *current {
            return Ok(false);
        }
        if step {
            self.checkpoint("Set controller");
        }
        if let Some(c) = self.controllers_mut(layer).and_then(|l| l.get_mut(controller)) {
            *c = edited;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;
    use fanimate_core::actions::RigAction;
    use fanimate_core::{Document, Skeleton, Vec2};

    /// A bone layer with a jaw bone and two regular actions ("open", "wide") and a Smart Bone action.
    fn rig() -> (Editor, LayerId) {
        let mut doc = Document::new("t", 800.0, 600.0, 24.0);
        let mut skeleton = Skeleton::default();
        let jaw = skeleton.add("jaw", None, Vec2::ZERO, 30.0, 0.0);
        for name in ["open", "wide"] {
            let mut a = RigAction::new(name, None, 10);
            a.bones.push(fanimate_core::actions::BoneKeys {
                bone: jaw,
                angle: fanimate_core::Channel::new(0.0)
                    .with_key(0, 0.0, fanimate_core::Interpolation::Linear)
                    .with_key(10, 0.5, fanimate_core::Interpolation::Linear),
            });
            skeleton.actions.push(a);
        }
        skeleton.actions.push(RigAction::new("turn", Some(jaw), 10));
        let layer = doc.new_layer(
            "Rig",
            LayerKind::Bone {
                skeleton,
                children: vec![],
            },
        );
        let id = layer.id;
        doc.layers.push(layer);
        (Editor::new(doc), id)
    }

    #[test]
    fn controllers_are_added_named_linked_and_removed_each_in_one_undo_step() {
        let (mut ed, layer) = rig();
        let slider = ed
            .add_controller(layer, "  Smile ", ControllerKind::Slider)
            .unwrap();
        let pad = ed.add_controller(layer, "", ControllerKind::Pad).unwrap();
        assert_eq!(ed.controllers(layer)[slider].name, "Smile");
        assert_eq!(
            ed.controllers(layer)[pad].name,
            "Pad 2",
            "an empty name is numbered"
        );
        ed.link_controller(layer, slider, 0, ControllerAxis::X, 0.0, 1.0)
            .unwrap();
        ed.link_controller(layer, pad, 1, ControllerAxis::Y, 0.25, 0.75)
            .unwrap();
        // linking the same action and axis again replaces the link
        ed.link_controller(layer, slider, 0, ControllerAxis::X, 0.5, 1.0)
            .unwrap();
        assert_eq!(ed.controllers(layer)[slider].links.len(), 1);
        assert_eq!(ed.controllers(layer)[slider].links[0].from, 0.5);
        assert_eq!(ed.unlink_controller(layer, slider, 0), Ok(true));
        assert_eq!(
            ed.unlink_controller(layer, slider, 0),
            Ok(false),
            "nothing to take away"
        );
        ed.remove_controller(layer, slider).unwrap();
        assert_eq!(ed.controllers(layer).len(), 1);
        ed.run(Action::Undo);
        assert_eq!(ed.controllers(layer).len(), 2, "removing was one step");
        ed.run(Action::Undo);
        assert_eq!(ed.controllers(layer)[slider].links.len(), 1, "so was unlinking");
    }

    #[test]
    fn what_cannot_be_linked_is_refused_and_changes_nothing() {
        let (mut ed, layer) = rig();
        let slider = ed.add_controller(layer, "S", ControllerKind::Slider).unwrap();
        let revision = ed.revision();
        assert!(
            ed.link_controller(layer, slider, 2, ControllerAxis::X, 0.0, 1.0)
                .unwrap_err()
                .contains("Smart Bone")
        );
        assert!(
            ed.link_controller(layer, slider, 9, ControllerAxis::X, 0.0, 1.0)
                .is_err()
        );
        assert!(
            ed.link_controller(layer, 9, 0, ControllerAxis::X, 0.0, 1.0)
                .is_err()
        );
        assert!(
            ed.link_controller(layer, slider, 0, ControllerAxis::Y, 0.0, 1.0)
                .unwrap_err()
                .contains("slider")
        );
        for (from, to) in [(0.5, 0.5), (f32::NAN, 1.0), (0.0, f32::INFINITY)] {
            assert!(
                ed.link_controller(layer, slider, 0, ControllerAxis::X, from, to)
                    .is_err()
            );
        }
        assert!(ed.remove_controller(layer, 5).is_err());
        assert!(
            ed.add_controller(LayerId(999), "x", ControllerKind::Slider)
                .is_err()
        );
        assert!(
            ed.add_controller(layer, &"n".repeat(300), ControllerKind::Slider)
                .is_err()
        );
        assert_eq!(ed.revision(), revision);
        ed.playing = true;
        assert!(ed.add_controller(layer, "x", ControllerKind::Slider).is_err());
    }

    #[test]
    fn a_value_follows_the_keying_rule_is_held_in_range_and_a_drag_is_one_step() {
        let (mut ed, layer) = rig();
        let slider = ed.add_controller(layer, "S", ControllerKind::Slider).unwrap();
        ed.go_to(0);
        assert_eq!(
            ed.set_controller_value(layer, slider, ControllerAxis::X, 0.3),
            Ok(true)
        );
        assert_eq!(ed.controllers(layer)[slider].x.base, 0.3);
        assert!(ed.controllers(layer)[slider].x.keys.is_empty());
        ed.go_to(12);
        ed.set_controller_value(layer, slider, ControllerAxis::X, 9.0)
            .unwrap();
        let x = &ed.controllers(layer)[slider].x;
        assert_eq!(
            (x.sample(0.0), x.sample(12.0)),
            (0.3, 1.0),
            "held at 1, and the rest pose seeded"
        );
        assert_eq!(
            ed.set_controller_value(layer, slider, ControllerAxis::X, 1.0),
            Ok(false),
            "no change, no step"
        );
        assert!(
            ed.set_controller_value(layer, slider, ControllerAxis::X, f32::NAN)
                .is_err()
        );
        // a drag: one checkpoint, then live changes
        let before = ed.revision();
        ed.checkpoint("drag");
        for n in 1..=20 {
            ed.change_controller_value(layer, slider, ControllerAxis::Y, n as f32 / 40.0)
                .unwrap();
        }
        assert_eq!(ed.revision(), before + 1);
    }

    #[test]
    fn deleting_an_action_renumbers_the_links_to_the_ones_after_it_and_drops_the_links_to_it() {
        let (mut ed, layer) = rig();
        ed.selection = vec![layer];
        let a = ed.add_controller(layer, "A", ControllerKind::Slider).unwrap();
        ed.link_controller(layer, a, 0, ControllerAxis::X, 0.0, 1.0)
            .unwrap();
        ed.link_controller(layer, a, 1, ControllerAxis::X, 0.0, 0.5)
            .unwrap();
        ed.run(Action::DeleteAction(0));
        let links = &ed.controllers(layer)[a].links;
        assert_eq!(links.len(), 1, "the link to the deleted action went");
        assert_eq!(links[0].action, 0, "and the one to \"wide\" moved up");
        assert_eq!(ed.rig_actions(layer)[links[0].action].name, "wide");
        ed.run(Action::Undo);
        assert_eq!(ed.controllers(layer)[a].links.len(), 2, "undo brings both back");
    }

    #[test]
    fn a_controller_is_renamed_and_a_links_range_changed_live() {
        let (mut ed, layer) = rig();
        let c = ed.add_controller(layer, "A", ControllerKind::Slider).unwrap();
        ed.link_controller(layer, c, 0, ControllerAxis::X, 0.0, 1.0)
            .unwrap();
        let before = ed.revision();
        assert_eq!(ed.rename_controller(layer, c, "  Smile "), Ok(true));
        assert_eq!(ed.controllers(layer)[c].name, "Smile");
        assert_eq!(
            ed.rename_controller(layer, c, "Smile"),
            Ok(false),
            "same name, no step"
        );
        assert_eq!(ed.revision(), before + 1);
        assert!(ed.rename_controller(layer, c, &"n".repeat(201)).is_err());
        assert!(ed.rename_controller(layer, 9, "x").is_err());
        // typing keeps what was typed, and a drag of a range is one step
        ed.checkpoint("type");
        let at = ed.revision();
        ed.change_controller_name(layer, c, "Smile ").unwrap();
        assert_eq!(ed.controllers(layer)[c].name, "Smile ");
        for n in 1..=10 {
            ed.change_link_range(layer, c, 0, 0.0, 1.0 - n as f32 / 20.0)
                .unwrap();
        }
        assert_eq!(ed.revision(), at, "no steps of their own");
        assert_eq!(ed.controllers(layer)[c].links[0].to, 0.5);
        assert!(ed.change_link_range(layer, c, 0, 0.3, 0.3).is_err());
        assert!(ed.change_link_range(layer, c, 0, f32::NAN, 1.0).is_err());
        assert!(ed.change_link_range(layer, c, 4, 0.0, 1.0).is_err());
        assert_eq!(ed.change_link_range(layer, c, 0, 0.0, 0.5), Ok(false));
        ed.playing = true;
        assert!(ed.rename_controller(layer, c, "x").is_err());
    }

    #[test]
    fn the_panel_commands_make_link_unlink_and_delete_controllers() {
        let (mut ed, layer) = rig();
        ed.selection = vec![layer];
        assert!(ed.can_run(Action::NewController(ControllerKind::Slider)));
        ed.run(Action::NewController(ControllerKind::Slider));
        ed.run(Action::NewController(ControllerKind::Pad));
        let names: Vec<&str> = ed.controllers(layer).iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Slider 1", "Pad 2"]);
        // a slider has no y, a Smart Bone action ("turn", 2) cannot be played, a missing one neither
        assert!(!ed.can_run(Action::LinkController(0, 0, ControllerAxis::Y)));
        assert!(ed.can_run(Action::LinkController(1, 0, ControllerAxis::Y)));
        assert!(!ed.can_run(Action::LinkController(0, 2, ControllerAxis::X)));
        assert!(!ed.can_run(Action::LinkController(0, 9, ControllerAxis::X)));
        assert!(!ed.can_run(Action::UnlinkController(0, 0)));
        ed.run(Action::LinkController(0, 0, ControllerAxis::X));
        let link = ed.controllers(layer)[0].links[0];
        assert_eq!(
            (link.action, link.axis, link.from, link.to),
            (0, ControllerAxis::X, 0.0, 1.0)
        );
        assert!(ed.can_run(Action::UnlinkController(0, 0)));
        ed.run(Action::UnlinkController(0, 0));
        assert!(ed.controllers(layer)[0].links.is_empty());
        ed.run(Action::DeleteController(0));
        assert_eq!(ed.controllers(layer).len(), 1);
        assert!(!ed.can_run(Action::DeleteController(1)));
        // nothing here while an action is open
        ed.run(Action::EditAction(0));
        assert!(!ed.can_run(Action::NewController(ControllerKind::Slider)));
        assert!(!ed.can_run(Action::DeleteController(0)));
    }
}
