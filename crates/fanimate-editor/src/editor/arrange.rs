//! Housekeeping commands: guides, a layer's shapes (rename, hide, duplicate,
//! delete), locking layers, and changing a group into a bone layer or a
//! switch layer and back.

use fanimate_core::bones::Skeleton;
use fanimate_core::{Guide, Layer, LayerId, LayerKind, Vec2};

use super::{Editor, StatusKind};

/// The kinds of layer a group-like layer can become.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConvertTo {
    Group,
    Bone,
    Switch,
}

impl ConvertTo {
    pub const ALL: [ConvertTo; 3] = [Self::Group, Self::Bone, Self::Switch];

    pub fn label(self) -> &'static str {
        match self {
            Self::Group => "Group",
            Self::Bone => "Bone Layer",
            Self::Switch => "Switch Layer",
        }
    }

    fn of(kind: &LayerKind) -> Option<Self> {
        match kind {
            LayerKind::Group { .. } => Some(Self::Group),
            LayerKind::Bone { .. } => Some(Self::Bone),
            LayerKind::Switch { .. } => Some(Self::Switch),
            _ => None,
        }
    }
}

/// Where Align puts the selected layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlignTo {
    Left,
    Centre,
    Right,
    Top,
    Middle,
    Bottom,
}

impl AlignTo {
    pub const ALL: [AlignTo; 6] = [
        Self::Left,
        Self::Centre,
        Self::Right,
        Self::Top,
        Self::Middle,
        Self::Bottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Align Left",
            Self::Centre => "Align Centre",
            Self::Right => "Align Right",
            Self::Top => "Align Top",
            Self::Middle => "Align Middle",
            Self::Bottom => "Align Bottom",
        }
    }
}

/// What changing a layer's kind throws away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loss {
    pub bones: usize,
    pub actions: usize,
    /// Keyframes on bones and in actions, and a switch layer's choices.
    pub keys: usize,
    /// Shapes and pictures that followed a bone.
    pub bindings: usize,
}

impl Loss {
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }

    /// "12 bones, 3 actions and 40 keyframes", in the words an animator uses.
    pub fn describe(&self) -> String {
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut parts = Vec::new();
        if self.bones > 0 {
            parts.push(plural(self.bones, "bone", "bones"));
        }
        if self.actions > 0 {
            parts.push(plural(self.actions, "action", "actions"));
        }
        if self.keys > 0 {
            parts.push(plural(self.keys, "keyframe", "keyframes"));
        }
        if self.bindings > 0 {
            parts.push(format!(
                "the binding of {}",
                plural(self.bindings, "shape or picture", "shapes and pictures")
            ));
        }
        match parts.as_slice() {
            [] => "nothing".into(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        }
    }
}

/// Count the bound shapes and pictures among `children`, not looking inside
/// a bone layer (its shapes follow its own bones).
fn bound(children: &[Layer]) -> usize {
    children
        .iter()
        .map(|l| match &l.kind {
            LayerKind::Vector { shapes } => shapes
                .iter()
                .filter(|s| s.bone.is_some() || s.skin.is_some())
                .count(),
            LayerKind::Image(img) => usize::from(img.bone.is_some()),
            LayerKind::Group { children } | LayerKind::Switch { children, .. } => bound(children),
            _ => 0,
        })
        .sum()
}

/// Let go of every shape and picture among `children` that follows a bone.
fn unbind(children: &mut [Layer]) {
    for l in children {
        match &mut l.kind {
            LayerKind::Vector { shapes } => {
                for s in shapes {
                    s.bone = None;
                    s.skin = None;
                }
            }
            LayerKind::Image(img) => img.bone = None,
            LayerKind::Group { children } | LayerKind::Switch { children, .. } => unbind(children),
            _ => {}
        }
    }
}

impl Editor {
    // ------------------------------------------------------------------
    // Guides

    /// Place a guide. One undo step.
    pub fn add_guide(&mut self, vertical: bool, position: f32) {
        if !position.is_finite() {
            return;
        }
        self.checkpoint("Add guide");
        self.doc.guides.push(Guide {
            vertical,
            position,
            locked: false,
        });
    }

    /// Start dragging guides: the one undo step for the drag.
    pub fn begin_guide_drag(&mut self) {
        self.checkpoint("Move guide");
    }

    /// Move guide `index`. Call [`begin_guide_drag`](Self::begin_guide_drag)
    /// once first. A locked guide stays where it is.
    pub fn move_guide(&mut self, index: usize, position: f32) {
        if let Some(g) = self.doc.guides.get_mut(index)
            && !g.locked
            && position.is_finite()
        {
            g.position = position;
            self.dirty = true;
        }
    }

    pub fn delete_guide(&mut self, index: usize) {
        if index < self.doc.guides.len() {
            self.checkpoint("Delete guide");
            self.doc.guides.remove(index);
        }
    }

    pub fn toggle_guide_lock(&mut self, index: usize) {
        if index < self.doc.guides.len() {
            self.checkpoint("Lock guide");
            let g = &mut self.doc.guides[index];
            g.locked = !g.locked;
        }
    }

    // ------------------------------------------------------------------
    // Aligning

    /// Each layer's box in document space, with its children's: the layer's
    /// own box (see `layer_bounds`) carried through the layer's transform.
    /// `None` for a layer that draws nothing.
    pub fn layer_doc_bounds(&self, ids: &[LayerId]) -> Vec<Option<(Vec2, Vec2)>> {
        self.layer_bounds(ids)
            .into_iter()
            .zip(ids)
            .map(|(bounds, &id)| {
                let (lo, hi) = bounds?;
                let world = self.world(id);
                let corners =
                    [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)].map(|c| world.transform_point2(c));
                Some(
                    corners
                        .iter()
                        .fold((corners[0], corners[0]), |(a, b), c| (a.min(*c), b.max(*c))),
                )
            })
            .collect()
    }

    /// Line the selected layers up: one layer goes to the canvas's edge or
    /// centre, several to the edge or centre of the box they cover together.
    /// One undo step.
    pub(super) fn align_selected(&mut self, to: AlignTo) {
        let roots: Vec<LayerId> = self
            .selected_roots()
            .into_iter()
            .filter(|id| !self.is_locked(*id))
            .collect();
        let boxes: Vec<(LayerId, (Vec2, Vec2))> = roots
            .iter()
            .copied()
            .zip(self.layer_doc_bounds(&roots))
            .filter_map(|(id, bounds)| Some((id, bounds?)))
            .collect();
        if boxes.is_empty() {
            self.set_status(StatusKind::Info, "There is nothing to align");
            return;
        }
        let (tmin, tmax) = if boxes.len() == 1 {
            (Vec2::ZERO, self.doc.size())
        } else {
            boxes.iter().fold(
                (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
                |(lo, hi), (_, (a, b))| (lo.min(*a), hi.max(*b)),
            )
        };
        let mut moves = Vec::new();
        for (id, (lo, hi)) in &boxes {
            let delta = match to {
                AlignTo::Left => Vec2::new(tmin.x - lo.x, 0.0),
                AlignTo::Centre => Vec2::new((tmin.x + tmax.x) / 2.0 - (lo.x + hi.x) / 2.0, 0.0),
                AlignTo::Right => Vec2::new(tmax.x - hi.x, 0.0),
                AlignTo::Top => Vec2::new(0.0, tmin.y - lo.y),
                AlignTo::Middle => Vec2::new(0.0, (tmin.y + tmax.y) / 2.0 - (lo.y + hi.y) / 2.0),
                AlignTo::Bottom => Vec2::new(0.0, tmax.y - hi.y),
            };
            if delta.length() > 1e-3 {
                moves.push((*id, delta));
            }
        }
        if moves.is_empty() {
            return;
        }
        self.checkpoint(to.label());
        for (id, delta) in moves {
            // the layer's position is in its parent's space
            let inverse = self.parent_world(id).matrix2.inverse();
            let local = inverse * delta;
            if let Some(mut t) = self.layer_transform(id)
                && local.is_finite()
            {
                t.position += local;
                self.set_layer_transform(id, t);
            }
        }
    }

    // ------------------------------------------------------------------
    // Locking

    /// Whether the layer, or a group or bone layer it is in, is locked.
    pub fn is_locked(&self, id: LayerId) -> bool {
        let mut current = Some(id);
        while let Some(c) = current {
            if self.doc.find(c).is_some_and(|l| l.locked) {
                return true;
            }
            current = self.doc.locate(c).and_then(|(p, _)| p);
        }
        false
    }

    /// Lock or unlock one layer (the padlock on its row in the Layers
    /// panel). One undo step.
    pub fn set_locked(&mut self, id: LayerId, locked: bool) {
        if self.doc.find(id).is_none_or(|l| l.locked == locked) {
            return;
        }
        self.checkpoint(if locked { "Lock layer" } else { "Unlock layer" });
        if let Some(l) = self.doc.find_mut(id) {
            l.locked = locked;
        }
        if locked {
            self.points.retain(|p| p.layer != id);
        }
    }

    /// Lock the selected layers, or unlock them when they are all locked.
    pub(super) fn toggle_lock(&mut self) {
        let roots = self.selected_roots();
        if roots.is_empty() {
            return;
        }
        let lock = roots
            .iter()
            .any(|id| self.doc.find(*id).is_some_and(|l| !l.locked));
        self.checkpoint(if lock { "Lock layer" } else { "Unlock layer" });
        for id in roots {
            if let Some(l) = self.doc.find_mut(id) {
                l.locked = lock;
            }
        }
        if lock {
            // what the tools were editing is no longer theirs to edit
            self.points.clear();
        }
    }

    // ------------------------------------------------------------------
    // Shapes

    /// The shapes whose every point is chosen, on the point layer.
    pub fn selected_shapes(&self) -> Vec<(LayerId, usize)> {
        let Some(layer) = self.point_layer() else {
            return Vec::new();
        };
        if self.points.is_empty() {
            return Vec::new();
        }
        // each shape's points, and how many of them are chosen
        let mut counts: std::collections::BTreeMap<usize, (usize, usize)> = Default::default();
        for (r, _) in self.point_positions() {
            if r.layer == layer {
                let c = counts.entry(r.shape).or_default();
                c.0 += 1;
                c.1 += usize::from(self.points.contains(&r));
            }
        }
        counts
            .into_iter()
            .filter(|(_, (all, chosen))| *all > 0 && all == chosen)
            .map(|(i, _)| (layer, i))
            .collect()
    }

    pub fn rename_shape(&mut self, layer: LayerId, index: usize, name: &str) {
        let name = name.trim();
        if name.is_empty() || self.shape(layer, index).is_none_or(|s| s.name == name) {
            return;
        }
        self.checkpoint("Rename shape");
        if let Some(s) = self.shape_mut(layer, index) {
            s.name = name.to_owned();
        }
    }

    pub(super) fn set_shape_visible(&mut self, layer: LayerId, index: usize, visible: bool) {
        if self.shape(layer, index).is_none_or(|s| s.visible == visible) {
            return;
        }
        self.checkpoint(if visible { "Show shape" } else { "Hide shape" });
        if let Some(s) = self.shape_mut(layer, index) {
            s.visible = visible;
        }
        // a hidden shape has no points to edit
        self.points.retain(|p| !(p.layer == layer && p.shape == index));
    }

    /// Copy the selected shapes, each in front of itself.
    pub(super) fn duplicate_shapes(&mut self) {
        let chosen = self.selected_shapes();
        if chosen.is_empty() {
            return;
        }
        self.checkpoint("Duplicate shape");
        let layer = chosen[0].0;
        self.points.clear();
        if let Some(shapes) = self.layer_shapes_mut(layer) {
            // later shapes first, so earlier indices stay valid
            for (_, i) in chosen.iter().rev() {
                if let Some(mut copy) = shapes.get(*i).cloned() {
                    copy.name = format!("{} copy", copy.name);
                    shapes.insert(i + 1, copy);
                }
            }
        }
    }

    /// Delete the selected shapes.
    pub(super) fn delete_shapes(&mut self) {
        let chosen = self.selected_shapes();
        if chosen.is_empty() {
            return;
        }
        self.checkpoint(if chosen.len() == 1 {
            "Delete shape"
        } else {
            "Delete shapes"
        });
        let layer = chosen[0].0;
        self.points.clear();
        if let Some(shapes) = self.layer_shapes_mut(layer) {
            for (_, i) in chosen.iter().rev() {
                if *i < shapes.len() {
                    shapes.remove(*i);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Changing a layer's kind

    /// What converting layer `id` to `to` would throw away; `None` when it
    /// cannot be converted (not a group, bone or switch layer, or already one).
    pub fn conversion_loss(&self, id: LayerId, to: ConvertTo) -> Option<Loss> {
        let layer = self.doc.find(id)?;
        let from = ConvertTo::of(&layer.kind)?;
        if from == to {
            return None;
        }
        let mut loss = Loss::default();
        match &layer.kind {
            LayerKind::Bone { skeleton, children } => {
                loss.bones = skeleton.bones.len();
                loss.actions = skeleton.actions.len();
                loss.keys = skeleton.bones.iter().map(|b| b.angle.keys.len()).sum::<usize>()
                    + skeleton
                        .actions
                        .iter()
                        .map(|a| {
                            a.bones.iter().map(|b| b.angle.keys.len()).sum::<usize>()
                                + a.shapes.iter().map(|s| s.path_keys.len()).sum::<usize>()
                        })
                        .sum::<usize>();
                loss.bindings = bound(children);
            }
            LayerKind::Switch { switch, .. } => loss.keys = switch.len(),
            _ => {}
        }
        Some(loss)
    }

    /// Change the layer's kind, keeping its children, name, transform and
    /// keys. What the old kind held that the new one has no place for (a
    /// skeleton, the choices of a switch) is dropped, and shapes that followed
    /// a bone let go. One undo step.
    pub(super) fn convert_layer(&mut self, to: ConvertTo) {
        let Some(id) = self.primary() else { return };
        if self.conversion_loss(id, to).is_none() {
            return;
        }
        self.checkpoint("Change layer type");
        let Some(layer) = self.doc.find_mut(id) else {
            return;
        };
        let kind = std::mem::replace(&mut layer.kind, LayerKind::Group { children: Vec::new() });
        let (mut children, from) = match kind {
            LayerKind::Group { children } => (children, ConvertTo::Group),
            LayerKind::Bone { children, .. } => (children, ConvertTo::Bone),
            LayerKind::Switch { children, .. } => (children, ConvertTo::Switch),
            other => {
                layer.kind = other;
                return;
            }
        };
        if from == ConvertTo::Bone {
            unbind(&mut children);
        }
        layer.kind = match to {
            ConvertTo::Group => LayerKind::Group { children },
            ConvertTo::Bone => LayerKind::Bone {
                skeleton: Skeleton::default(),
                children,
            },
            ConvertTo::Switch => LayerKind::Switch {
                children,
                switch: Vec::new(),
            },
        };
        let name = layer.name.clone();
        let note = if to == ConvertTo::Switch {
            " A switch layer shows one child at a time."
        } else {
            ""
        };
        self.set_status(
            StatusKind::Info,
            format!("\"{name}\" is now a {}.{note}", to.label().to_lowercase()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, SelectMode, Tool};
    use fanimate_core::{Color, Document, Interpolation, Shape, Style, Vec2};

    fn square(name: &str, at: f32) -> Shape {
        Shape::rectangle(
            name,
            Vec2::splat(at),
            Vec2::splat(20.0),
            0.0,
            Style::fill(Color::BLACK),
        )
    }

    /// An editor with one vector layer holding three shapes.
    fn with_shapes() -> (Editor, LayerId) {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        if let LayerKind::Vector { shapes } = &mut ed.doc.find_mut(id).unwrap().kind {
            *shapes = vec![square("a", 0.0), square("b", 100.0), square("c", 200.0)];
        }
        (ed, id)
    }

    fn names(ed: &Editor, id: LayerId) -> Vec<String> {
        ed.layer_shapes(id)
            .unwrap()
            .iter()
            .map(|s| s.name.clone())
            .collect()
    }

    #[test]
    fn guides_are_placed_moved_locked_and_removed_with_undo() {
        let mut ed = Editor::untitled();
        ed.add_guide(true, 320.0);
        ed.add_guide(false, f32::NAN);
        assert_eq!(ed.doc.guides.len(), 1, "a guide with no position is not placed");
        ed.begin_guide_drag();
        ed.move_guide(0, 340.0);
        ed.move_guide(0, 360.0);
        assert_eq!(ed.doc.guides[0].position, 360.0);
        ed.run(Action::Undo);
        assert_eq!(ed.doc.guides[0].position, 320.0, "a drag is one undo step");
        ed.toggle_guide_lock(0);
        ed.move_guide(0, 500.0);
        assert_eq!(ed.doc.guides[0].position, 320.0, "a locked guide does not move");
        ed.delete_guide(0);
        assert!(ed.doc.guides.is_empty());
        ed.run(Action::Undo);
        assert_eq!(ed.doc.guides.len(), 1);
    }

    #[test]
    fn a_locked_layer_cannot_be_drawn_on_or_hit_and_unlocks_again() {
        let (mut ed, id) = with_shapes();
        ed.select(id, SelectMode::Replace);
        assert!(!ed.is_locked(id));
        ed.run(Action::ToggleLock);
        assert!(ed.is_locked(id));
        assert!(
            ed.shape_at(Vec2::new(10.0, 10.0), 2.0).is_none(),
            "the canvas cannot pick it"
        );
        assert!(!ed.begin_stroke(Vec2::ZERO, 1.0, 1.0), "nor draw on it");
        assert!(matches!(ed.status, Some((StatusKind::Warning, _))));
        ed.run(Action::ToggleLock);
        assert!(!ed.is_locked(id));
        assert!(ed.shape_at(Vec2::new(10.0, 10.0), 2.0).is_some());
    }

    #[test]
    fn a_locked_group_locks_what_is_inside_it() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let inner = doc.new_layer("in", LayerKind::vector(vec![square("s", 0.0)]));
        let inner_id = inner.id;
        let mut group = doc.new_layer(
            "g",
            LayerKind::Group {
                children: vec![inner],
            },
        );
        group.locked = true;
        doc.layers.push(group);
        let ed = Editor::new(doc);
        assert!(ed.is_locked(inner_id));
        assert!(ed.shape_at(Vec2::new(5.0, 5.0), 2.0).is_none());
    }

    #[test]
    fn a_locked_layer_does_not_move() {
        let (mut ed, id) = with_shapes();
        ed.select(id, SelectMode::Replace);
        ed.run(Action::ToggleLock);
        let mut t = ed.layer_transform(id).unwrap();
        t.position = Vec2::new(50.0, 0.0);
        ed.set_layer_transform(id, t);
        assert_eq!(ed.layer_transform(id).unwrap().position, Vec2::ZERO);
    }

    #[test]
    fn shapes_can_be_renamed_hidden_duplicated_and_deleted() {
        let (mut ed, id) = with_shapes();
        ed.rename_shape(id, 1, "  body ");
        assert_eq!(names(&ed, id), ["a", "body", "c"]);
        ed.rename_shape(id, 1, "   ");
        assert_eq!(names(&ed, id)[1], "body", "a blank name changes nothing");
        // a hidden shape is not drawn, hit or editable
        ed.run(Action::SetShapeVisible(id, 2, false));
        assert!(ed.shape_at(Vec2::new(205.0, 205.0), 2.0).is_none());
        assert!(!ed.layer_shapes(id).unwrap()[2].visible);
        ed.run(Action::SetShapeVisible(id, 2, true));
        assert!(ed.shape_at(Vec2::new(205.0, 205.0), 2.0).is_some());
        // duplicate and delete work on the selected shapes
        ed.tool = Tool::SelectPoints;
        ed.select_shape(id, 0, SelectMode::Replace);
        ed.run(Action::DuplicateShapes);
        assert_eq!(names(&ed, id), ["a", "a copy", "body", "c"]);
        ed.select_shape(id, 2, SelectMode::Replace);
        ed.select_shape(id, 3, SelectMode::Toggle);
        assert_eq!(ed.selected_shapes().len(), 2);
        ed.run(Action::DeleteShapes);
        assert_eq!(names(&ed, id), ["a", "a copy"]);
        ed.run(Action::Undo);
        assert_eq!(names(&ed, id).len(), 4);
        assert!(
            !ed.can_run(Action::DeleteShapes),
            "nothing is selected after undo"
        );
    }

    #[test]
    fn a_group_becomes_a_bone_layer_and_back_without_losing_children() {
        let mut ed = Editor::untitled();
        ed.run(Action::NewGroupLayer);
        let g = ed.primary().unwrap();
        ed.run(Action::NewVectorLayer);
        let child = ed.primary().unwrap();
        // the new layer is inside the group when the group is selected
        ed.select(g, SelectMode::Replace);
        assert_eq!(ed.conversion_loss(g, ConvertTo::Bone), Some(Loss::default()));
        ed.run(Action::ConvertLayerType(ConvertTo::Bone));
        assert!(matches!(ed.doc.find(g).unwrap().kind, LayerKind::Bone { .. }));
        assert!(ed.doc.find(child).is_some(), "the children stay");
        // nothing lost, so no question; and it can be undone in one step
        ed.run(Action::Undo);
        assert!(matches!(ed.doc.find(g).unwrap().kind, LayerKind::Group { .. }));
        // converting to what it already is does nothing
        assert!(!ed.can_run(Action::ConvertLayerType(ConvertTo::Group)));
    }

    #[test]
    fn converting_a_rig_says_what_it_loses_and_lets_the_shapes_go() {
        let mut ed = Editor::new(fanimate_core::samples::hello());
        let rig = ed
            .doc
            .layers
            .iter()
            .find(|l| matches!(l.kind, LayerKind::Bone { .. }))
            .unwrap()
            .id;
        ed.select(rig, SelectMode::Replace);
        let loss = ed
            .conversion_loss(rig, ConvertTo::Group)
            .expect("a rig can become a group");
        assert_eq!(loss.bones, 1);
        assert!(loss.keys >= 5, "the arm's keys: {loss:?}");
        assert!(loss.bindings >= 3, "the arm shapes follow the bone: {loss:?}");
        assert_eq!(loss.describe().split(' ').next(), Some("1"));
        // running it asks first: the app is told to confirm
        let outcome = ed.run(Action::ConvertLayerType(ConvertTo::Group));
        assert_eq!(
            outcome,
            crate::Outcome::App(Action::ConvertLayerType(ConvertTo::Group))
        );
        assert!(
            matches!(ed.doc.find(rig).unwrap().kind, LayerKind::Bone { .. }),
            "not yet"
        );
        // once it is confirmed, the skeleton is gone and the shapes follow nothing
        ed.run(Action::ConfirmConvertLayerType(ConvertTo::Group));
        let converted = ed.doc.find(rig).unwrap();
        assert!(matches!(converted.kind, LayerKind::Group { .. }));
        let LayerKind::Group { children } = &converted.kind else {
            unreachable!()
        };
        assert!(
            children.iter().all(|c| matches!(&c.kind, LayerKind::Vector { shapes } if shapes.iter().all(|s| s.bone.is_none() && s.skin.is_none())))
        );
        assert!(fanimate_fnm::validate::validate(&ed.doc).is_clean());
        ed.run(Action::Undo);
        assert!(matches!(ed.doc.find(rig).unwrap().kind, LayerKind::Bone { .. }));
    }

    #[test]
    fn a_switch_layer_loses_its_choices_when_it_becomes_a_group() {
        let mut ed = Editor::untitled();
        ed.run(Action::NewSwitchLayer);
        let sw = ed.primary().unwrap();
        if let Some(LayerKind::Switch { switch, .. }) = ed.doc.find_mut(sw).map(|l| &mut l.kind) {
            switch.push(fanimate_core::Key {
                frame: 3,
                value: "open".into(),
                interp: Interpolation::Step,
            });
        }
        assert_eq!(ed.conversion_loss(sw, ConvertTo::Group).unwrap().keys, 1);
        assert_eq!(
            ed.conversion_loss(sw, ConvertTo::Group).unwrap().describe(),
            "1 keyframe"
        );
        ed.run(Action::ConfirmConvertLayerType(ConvertTo::Group));
        assert!(matches!(ed.doc.find(sw).unwrap().kind, LayerKind::Group { .. }));
    }

    #[test]
    fn layers_that_are_not_group_like_cannot_be_converted() {
        let (ed, id) = with_shapes();
        assert!(ed.conversion_loss(id, ConvertTo::Group).is_none());
        assert!(!ed.can_run(Action::ConvertLayerType(ConvertTo::Group)));
    }

    fn bounds(ed: &Editor, id: LayerId) -> (Vec2, Vec2) {
        ed.layer_doc_bounds(&[id])[0].expect("the layer draws something")
    }

    /// A layer holding one 20 x 20 square at `at`, selected.
    fn square_layer(ed: &mut Editor, at: f32) -> LayerId {
        ed.run(Action::NewVectorLayer);
        let id = ed.primary().unwrap();
        if let LayerKind::Vector { shapes } = &mut ed.doc.find_mut(id).unwrap().kind {
            shapes.push(square("s", at));
        }
        id
    }

    #[test]
    fn one_layer_aligns_to_the_canvas_and_several_to_each_other() {
        let mut ed = Editor::untitled();
        let size = ed.doc.size();
        let a = square_layer(&mut ed, 100.0);
        ed.run(Action::Align(AlignTo::Left));
        assert!(bounds(&ed, a).0.x.abs() < 1e-3);
        ed.run(Action::Align(AlignTo::Right));
        assert!((bounds(&ed, a).1.x - size.x).abs() < 1e-3);
        ed.run(Action::Align(AlignTo::Centre));
        let (lo, hi) = bounds(&ed, a);
        assert!(((lo.x + hi.x) / 2.0 - size.x / 2.0).abs() < 1e-3);
        ed.run(Action::Align(AlignTo::Bottom));
        assert!((bounds(&ed, a).1.y - size.y).abs() < 1e-3);
        // two layers line up to the box they cover together
        let mut ed = Editor::untitled();
        let a = square_layer(&mut ed, 100.0);
        let b = square_layer(&mut ed, 300.0);
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        ed.run(Action::Align(AlignTo::Right));
        assert!((bounds(&ed, a).1.x - 320.0).abs() < 1e-3);
        assert!((bounds(&ed, b).1.x - 320.0).abs() < 1e-3);
        // one undo step for the whole thing
        ed.run(Action::Undo);
        assert!((bounds(&ed, a).1.x - 120.0).abs() < 1e-3);
    }

    #[test]
    fn aligning_follows_the_layers_own_transform_and_skips_locked_layers() {
        let mut ed = Editor::untitled();
        let a = square_layer(&mut ed, 100.0);
        // a layer scaled up still lands where asked
        let mut t = ed.layer_transform(a).unwrap();
        t.scale = Vec2::splat(2.0);
        ed.set_layer_transform(a, t);
        ed.run(Action::Align(AlignTo::Left));
        assert!(bounds(&ed, a).0.x.abs() < 1e-3);
        ed.run(Action::ToggleLock);
        let before = bounds(&ed, a);
        ed.run(Action::Align(AlignTo::Right));
        assert_eq!(bounds(&ed, a), before, "a locked layer stays put");
    }

    #[test]
    fn a_project_can_be_saved_as_a_folder_and_opens_again() {
        let dir = std::env::temp_dir().join(format!("fanimate-save-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("walk.fnm");
        let mut ed = Editor::new(fanimate_core::samples::hello());
        ed.dirty = true;
        ed.save_folder_to(&path).expect("it saves");
        assert!(path.is_dir(), "a folder, not a file");
        assert!(path.join("manifest.json").is_file());
        assert_eq!(ed.path.as_deref(), Some(path.as_path()));
        assert!(!ed.dirty);
        let (loaded, report) = fanimate_fnm::load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded, ed.doc);
        // saving again (Ctrl+S) keeps it a folder
        ed.save_to(&path).unwrap();
        assert!(path.is_dir());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_a_conversion_loses_reads_naturally() {
        assert_eq!(Loss::default().describe(), "nothing");
        let loss = Loss {
            bones: 12,
            actions: 3,
            keys: 40,
            bindings: 0,
        };
        assert_eq!(loss.describe(), "12 bones, 3 actions and 40 keyframes");
        assert_eq!(
            Loss {
                bindings: 1,
                ..Loss::default()
            }
            .describe(),
            "the binding of 1 shape or picture"
        );
    }
}
