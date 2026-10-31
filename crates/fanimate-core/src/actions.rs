//! Actions and Smart Bones.
//!
//! An action is a short animation kept on a bone layer, outside the main
//! timeline: bone angles and point keys of the shapes inside the layer. A
//! regular action is a reusable clip (a blink, a walk step) inserted into
//! the timeline where needed. A Smart Bone action has a driver bone: the
//! action's own keys for that bone map its angle to a frame of the action,
//! and wherever the driver is turned in the timeline, the rest of the
//! action plays by that much, added on top. Turning a head bone can morph
//! the face into a three-quarter view; two Smart Bones can shape one
//! drawing, their changes adding up.
//!
//! An action is edited in place: entering it swaps its keys into the bone
//! layer (keeping the timeline's keys aside), so every tool edits it as it
//! would the timeline; leaving swaps them back. The swap lives in the
//! document, so undo, saving and crash recovery always see a consistent
//! state, and [`Document::mainline`] gives the document with any action
//! put away, which is what gets saved.

use std::borrow::Cow;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::anim::{Channel, Frame, Key};
use crate::bones::Skeleton;
use crate::path::{Path, PathCmd};
use crate::scene::{Document, Layer, LayerId, LayerKind};

/// The most master controllers a rig holds.
pub const MAX_CONTROLLERS: usize = 256;
/// The most actions one controller drives.
pub const MAX_LINKS: usize = 64;

/// Which value of a controller a link reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerAxis {
    #[default]
    X,
    Y,
}

/// The shape of a controller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerKind {
    /// One value from 0 to 1.
    #[default]
    Slider,
    /// Two values from 0 to 1: a point on a square pad.
    Pad,
}

/// A controller's hold on an action: the action plays to the frame the
/// controller's value maps to, from its first frame at `from` to its last at `to`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControllerLink {
    /// The action's index in the rig's list.
    pub action: usize,
    #[serde(default)]
    pub axis: ControllerAxis,
    /// The value at which the action is on its first frame.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub from: f32,
    /// The value at which the action is on its last frame.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub to: f32,
}

fn zero() -> f32 {
    0.0
}
fn one() -> f32 {
    1.0
}
fn is_zero(v: &f32) -> bool {
    *v == 0.0
}
fn is_one(v: &f32) -> bool {
    *v == 1.0
}
fn half() -> Channel<f32> {
    Channel::new(0.0)
}
fn is_plain_zero(c: &Channel<f32>) -> bool {
    c.keys.is_empty() && c.base == 0.0 && c.loop_mode.is_off()
}

/// A master controller: a slider or a pad whose values (which have keys, like
/// any animated value) play other actions, so one control can pose a whole
/// face or body: a smile slider that works the mouth, brows and cheeks, a
/// pad that looks the head in any direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Controller {
    pub name: String,
    #[serde(default)]
    pub kind: ControllerKind,
    /// The slider's value, or the pad's horizontal one, from 0 to 1.
    #[serde(default = "half", skip_serializing_if = "is_plain_zero")]
    pub x: Channel<f32>,
    /// The pad's vertical value, from 0 to 1 (unused by a slider).
    #[serde(default = "half", skip_serializing_if = "is_plain_zero")]
    pub y: Channel<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<ControllerLink>,
}

impl Controller {
    pub fn new(name: impl Into<String>, kind: ControllerKind) -> Self {
        Self {
            name: name.into(),
            kind,
            x: Channel::new(0.0),
            y: Channel::new(0.0),
            links: Vec::new(),
        }
    }

    /// The value of one axis on `frame`, held between 0 and 1 (a value that is
    /// not a number is 0).
    pub fn value(&self, axis: ControllerAxis, frame: f32) -> f32 {
        let v = match axis {
            ControllerAxis::X => self.x.sample(frame),
            ControllerAxis::Y => self.y.sample(frame),
        };
        if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
    }

    /// The frame of `action` that `link` plays on timeline frame `frame`: the
    /// value mapped from `from..to` onto the action's frames, held at both
    /// ends. `None` for a Smart Bone action (it plays by its own bone) and
    /// for a link whose range is empty or not a number.
    pub fn action_frame(&self, link: &ControllerLink, action: &RigAction, frame: f32) -> Option<f32> {
        if action.driver.is_some() {
            return None;
        }
        let span = link.to - link.from;
        if span == 0.0 || !span.is_finite() || !link.from.is_finite() {
            return None;
        }
        let t = ((self.value(link.axis, frame) - link.from) / span).clamp(0.0, 1.0);
        Some(t * action.length as f32)
    }
}

/// One action on a bone layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigAction {
    pub name: String,
    /// For a Smart Bone action, the bone whose angle plays it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<usize>,
    /// The action's last frame; it runs from frame 0.
    pub length: Frame,
    /// Bone angles the action animates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bones: Vec<BoneKeys>,
    /// Point keys of shapes inside the bone layer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<ShapeKeys>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoneKeys {
    pub bone: usize,
    pub angle: Channel<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeKeys {
    /// The vector layer, inside the bone layer.
    pub layer: LayerId,
    /// The shape's index in that layer.
    pub shape: usize,
    pub path_keys: Vec<Key<Path>>,
}

impl RigAction {
    pub fn new(name: impl Into<String>, driver: Option<usize>, length: Frame) -> Self {
        Self {
            name: name.into(),
            driver,
            length: length.max(1),
            bones: Vec::new(),
            shapes: Vec::new(),
        }
    }

    fn bone(&self, bone: usize) -> Option<&Channel<f32>> {
        self.bones.iter().find(|b| b.bone == bone).map(|b| &b.angle)
    }

    /// For a Smart Bone action, the frame of the action that the driver at
    /// `angle` plays: found on the action's own keys for the driver, held
    /// at the first or last key outside them. `None` when the action has no
    /// driver or the driver has fewer than two keys in it.
    pub fn frame_for(&self, angle: f32) -> Option<f32> {
        let ch = self.bone(self.driver?)?;
        if ch.keys.len() < 2 || !angle.is_finite() {
            return None;
        }
        let (first, last) = (&ch.keys[0], &ch.keys[ch.keys.len() - 1]);
        if let Some(k) = ch.keys.iter().find(|k| k.value == angle) {
            return Some(k.frame as f32);
        }
        // where the angle falls between two keys; each segment moves one
        // way, so a bisection on it finds the frame
        for w in ch.keys.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let (lo, hi) = (a.value.min(b.value), a.value.max(b.value));
            if angle < lo || angle > hi || a.value == b.value {
                continue;
            }
            let rising = b.value > a.value;
            let (mut f0, mut f1) = (a.frame as f32, b.frame as f32);
            for _ in 0..24 {
                let mid = (f0 + f1) / 2.0;
                if (ch.sample(mid) < angle) == rising {
                    f0 = mid;
                } else {
                    f1 = mid;
                }
            }
            return Some((f0 + f1) / 2.0);
        }
        // beyond the keys: nearer end
        let to_first = (angle - first.value).abs();
        let to_last = (angle - last.value).abs();
        Some(if to_first <= to_last {
            first.frame as f32
        } else {
            last.frame as f32
        })
    }

    /// How much the action turns `bone` at action frame `f`, from its
    /// frame 0.
    pub fn bone_offset(&self, bone: usize, f: f32) -> f32 {
        self.bone(bone).map_or(0.0, |ch| ch.sample(f) - ch.sample(0.0))
    }

    /// The point keys the action holds for a shape.
    pub fn shape_keys(&self, layer: LayerId, shape: usize) -> Option<&[Key<Path>]> {
        self.shapes
            .iter()
            .find(|s| s.layer == layer && s.shape == shape)
            .map(|s| &s.path_keys[..])
    }

    /// Renumber bone references after a bone is removed (see
    /// [`Skeleton::remove`]). Returns false when the action's driver went.
    pub fn renumber(&mut self, map: &[Option<usize>]) -> bool {
        let get = |b: usize| map.get(b).copied().flatten();
        if let Some(d) = self.driver {
            match get(d) {
                Some(n) => self.driver = Some(n),
                None => return false,
            }
        }
        self.bones.retain_mut(|b| match get(b.bone) {
            Some(n) => {
                b.bone = n;
                true
            }
            None => false,
        });
        true
    }
}

/// Sample point keys like a shape's own (see `Shape::path_at`).
pub fn sample_keys(keys: &[Key<Path>], f: f32) -> Option<Cow<'_, Path>> {
    let (first, last) = (keys.first()?, keys.last()?);
    if f <= first.frame as f32 {
        return Some(Cow::Borrowed(&first.value));
    }
    if f >= last.frame as f32 {
        return Some(Cow::Borrowed(&last.value));
    }
    let next = keys.partition_point(|k| k.frame as f32 <= f);
    let (a, b) = (&keys[next - 1], &keys[next]);
    let t = a.interp.apply((f - a.frame as f32) / (b.frame - a.frame) as f32);
    Some(match Path::lerp(&a.value, &b.value, t) {
        Some(p) => Cow::Owned(p),
        None => Cow::Borrowed(&a.value),
    })
}

/// Add `weight` times (`to` - `from`) to `path`, point by point. Paths
/// made of other commands are left alone.
pub fn add_offset(path: &mut Path, from: &Path, to: &Path) {
    if !path.same_structure(from) || !path.same_structure(to) {
        return;
    }
    let d = |a: Vec2, b: Vec2| b - a;
    for ((p, a), b) in path.cmds.iter_mut().zip(&from.cmds).zip(&to.cmds) {
        match (p, *a, *b) {
            (PathCmd::MoveTo(p), PathCmd::MoveTo(a), PathCmd::MoveTo(b))
            | (PathCmd::LineTo(p), PathCmd::LineTo(a), PathCmd::LineTo(b)) => *p += d(a, b),
            (PathCmd::CubicTo(p1, p2, p), PathCmd::CubicTo(a1, a2, a), PathCmd::CubicTo(b1, b2, b)) => {
                *p1 += d(a1, b1);
                *p2 += d(a2, b2);
                *p += d(a, b);
            }
            _ => {}
        }
    }
}

/// An action being edited: which one, and the timeline's own keys, kept
/// aside until it is left.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionEdit {
    /// The bone layer.
    pub layer: LayerId,
    pub action: usize,
    /// The timeline's bone angle channels, one per bone.
    pub bones: Vec<Channel<f32>>,
    /// The timeline's point keys of every shape inside the bone layer.
    pub shapes: Vec<ShapeKeys>,
}

impl Document {
    /// The document with any action being edited put away: what is saved.
    pub fn mainline(&self) -> Cow<'_, Document> {
        if self.action_edit.is_none() {
            return Cow::Borrowed(self);
        }
        let mut doc = self.clone();
        doc.leave_action();
        Cow::Owned(doc)
    }

    /// Start editing action `action` of bone layer `layer`. Leaves any
    /// action being edited first. Returns false if there is no such action.
    pub fn enter_action(&mut self, layer: LayerId, action: usize) -> bool {
        self.leave_action();
        let Some(l) = self.find_mut(layer) else {
            return false;
        };
        let LayerKind::Bone { skeleton, children } = &mut l.kind else {
            return false;
        };
        let Some(act) = skeleton.actions.get(action).cloned() else {
            return false;
        };
        let bones = skeleton
            .bones
            .iter_mut()
            .enumerate()
            .map(|(i, b)| {
                let replacement = act.bone(i).cloned().unwrap_or_else(|| Channel::new(b.angle.base));
                std::mem::replace(&mut b.angle, replacement)
            })
            .collect();
        let mut shapes = Vec::new();
        for_each_shape(children, &mut |layer, i, shape| {
            let keys = act.shape_keys(layer, i).map(<[_]>::to_vec).unwrap_or_default();
            shapes.push(ShapeKeys {
                layer,
                shape: i,
                path_keys: std::mem::replace(&mut shape.path_keys, keys),
            });
        });
        self.action_edit = Some(ActionEdit {
            layer,
            action,
            bones,
            shapes,
        });
        true
    }

    /// Stop editing the action: its keys go back into it and the
    /// timeline's come back.
    pub fn leave_action(&mut self) {
        let Some(edit) = self.action_edit.take() else {
            return;
        };
        let Some(l) = self.find_mut(edit.layer) else {
            return;
        };
        let LayerKind::Bone { skeleton, children } = &mut l.kind else {
            return;
        };
        let Skeleton { bones, actions, .. } = skeleton;
        let mut bone_keys = Vec::new();
        for (i, b) in bones.iter_mut().enumerate() {
            let main = edit
                .bones
                .get(i)
                .cloned()
                .unwrap_or_else(|| Channel::new(b.angle.base));
            let mut action_channel = std::mem::replace(&mut b.angle, main);
            if action_channel.is_animated() {
                // the rest pose is the timeline's
                action_channel.base = b.angle.base;
                bone_keys.push(BoneKeys {
                    bone: i,
                    angle: action_channel,
                });
            }
        }
        let mut shape_keys = Vec::new();
        for_each_shape(children, &mut |layer, i, shape| {
            let main = edit
                .shapes
                .iter()
                .find(|s| s.layer == layer && s.shape == i)
                .map(|s| s.path_keys.clone())
                .unwrap_or_default();
            let keys = std::mem::replace(&mut shape.path_keys, main);
            if !keys.is_empty() {
                shape_keys.push(ShapeKeys {
                    layer,
                    shape: i,
                    path_keys: keys,
                });
            }
        });
        if let Some(act) = actions.get_mut(edit.action) {
            act.bones = bone_keys;
            act.shapes = shape_keys;
        }
    }
}

/// Every shape in vector layers inside `layers`, not inside nested bone
/// layers (their shapes belong to that rig).
fn for_each_shape(layers: &mut [Layer], f: &mut impl FnMut(LayerId, usize, &mut crate::Shape)) {
    for l in layers {
        let id = l.id;
        match &mut l.kind {
            LayerKind::Vector { shapes } => {
                for (i, s) in shapes.iter_mut().enumerate() {
                    f(id, i, s);
                }
            }
            LayerKind::Group { children } | LayerKind::Switch { children, .. } => for_each_shape(children, f),
            LayerKind::Bone { .. }
            | LayerKind::Image(_)
            | LayerKind::Frames { .. }
            | LayerKind::Audio(_)
            | LayerKind::Text(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::Interpolation;
    use crate::path::{Shape, Style};
    use crate::{Color, evaluate};
    use std::f32::consts::FRAC_PI_2;

    /// A head bone driving a smart action that moves a mouth point down,
    /// and turns a second bone.
    fn rigged() -> (Document, LayerId, LayerId) {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut skel = Skeleton::default();
        let head = skel.add("head", None, Vec2::ZERO, 10.0, 0.0);
        let jaw = skel.add("jaw", None, Vec2::new(0.0, 20.0), 10.0, 0.0);
        let mouth = Shape::new(
            "mouth",
            Path::new()
                .move_to(Vec2::new(0.0, 50.0))
                .line_to(Vec2::new(10.0, 50.0)),
            Style::default().with_stroke(Color::BLACK, 1.0),
        );
        let art = doc.new_layer("face", LayerKind::vector(vec![mouth.clone()]));
        let art_id = art.id;
        let mut act = RigAction::new("turn", Some(head), 10);
        act.bones.push(BoneKeys {
            bone: head,
            angle: Channel::new(0.0)
                .with_key(0, 0.0, Interpolation::Linear)
                .with_key(10, FRAC_PI_2, Interpolation::Linear),
        });
        act.bones.push(BoneKeys {
            bone: jaw,
            angle: Channel::new(0.0)
                .with_key(0, 0.0, Interpolation::Linear)
                .with_key(10, 1.0, Interpolation::Linear),
        });
        let mut open = mouth.path.clone();
        open.move_points(&[1].into_iter().collect(), Vec2::new(0.0, 20.0));
        act.shapes.push(ShapeKeys {
            layer: art_id,
            shape: 0,
            path_keys: vec![
                Key {
                    frame: 0,
                    value: mouth.path.clone(),
                    interp: Interpolation::Linear,
                },
                Key {
                    frame: 10,
                    value: open,
                    interp: Interpolation::Linear,
                },
            ],
        });
        skel.actions.push(act);
        let rig = doc.new_layer(
            "rig",
            LayerKind::Bone {
                skeleton: skel,
                children: vec![art],
            },
        );
        let rig_id = rig.id;
        doc.layers.push(rig);
        (doc, rig_id, art_id)
    }

    fn skel(doc: &mut Document, id: LayerId) -> &mut Skeleton {
        match &mut doc.find_mut(id).unwrap().kind {
            LayerKind::Bone { skeleton, .. } => skeleton,
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_driver_angle_finds_the_action_frame() {
        let (mut doc, rig, _) = rigged();
        let act = skel(&mut doc, rig).actions[0].clone();
        assert!((act.frame_for(FRAC_PI_2 / 2.0).unwrap() - 5.0).abs() < 1e-3);
        assert_eq!(act.frame_for(-1.0), Some(0.0));
        assert_eq!(act.frame_for(9.0), Some(10.0));
        assert!((act.bone_offset(1, 5.0) - 0.5).abs() < 1e-5);
        assert!(RigAction::new("x", None, 5).frame_for(0.3).is_none());
    }

    #[test]
    fn turning_the_driver_plays_the_action() {
        let (mut doc, rig, _) = rigged();
        // nothing turned: nothing changes
        let at = |doc: &Document| evaluate(doc, 0.0)[0].shape.path.on_curve_points()[1];
        assert_eq!(at(&doc), Vec2::new(10.0, 50.0));
        skel(&mut doc, rig).bones[0].angle = Channel::new(FRAC_PI_2 / 2.0);
        assert!(
            at(&doc).abs_diff_eq(Vec2::new(10.0, 60.0), 1e-3),
            "{:?}",
            at(&doc)
        );
    }

    #[test]
    fn editing_an_action_swaps_keys_and_mainline_puts_them_back() {
        let (mut doc, rig, art) = rigged();
        let before = doc.clone();
        assert!(doc.enter_action(rig, 0));
        // the action's keys are now the layer's
        assert_eq!(skel(&mut doc, rig).bones[0].angle.keys.len(), 2);
        let LayerKind::Vector { shapes } = &doc.find(art).unwrap().kind else {
            unreachable!()
        };
        assert_eq!(shapes[0].path_keys.len(), 2);
        // while editing, Smart Bones are not applied on top
        assert_eq!(
            evaluate(&doc, 0.0)[0].shape.path.on_curve_points()[1],
            Vec2::new(10.0, 50.0)
        );
        assert_eq!(*doc.mainline(), before);
        // a change made in the action is kept in it
        skel(&mut doc, rig).bones[1]
            .angle
            .set_key(10, 2.0, Interpolation::Linear);
        doc.leave_action();
        let act = &skel(&mut doc, rig).actions[0];
        assert!((act.bone_offset(1, 10.0) - 2.0).abs() < 1e-5);
        assert!(skel(&mut doc, rig).bones.iter().all(|b| b.angle.keys.is_empty()));
    }

    #[test]
    fn renumbering_drops_actions_whose_driver_went() {
        let mut act = RigAction::new("a", Some(1), 5);
        act.bones.push(BoneKeys {
            bone: 0,
            angle: Channel::new(0.0),
        });
        assert!(act.renumber(&[None, Some(0)]));
        assert_eq!(act.driver, Some(0));
        assert!(act.bones.is_empty());
        assert!(!act.renumber(&[None]));
    }

    /// The rig with a regular action "open" (index 1) that pulls the mouth point down by 20 over 10 frames.
    fn with_open() -> (Document, LayerId) {
        let (mut doc, rig, _) = rigged();
        let mut open = RigAction::new("open", None, 10);
        open.shapes = skel(&mut doc, rig).actions[0].shapes.clone();
        skel(&mut doc, rig).actions.push(open);
        (doc, rig)
    }

    fn mouth_y(doc: &Document, frame: f32) -> f32 {
        evaluate(doc, frame)[0].shape.path.on_curve_points()[1].y
    }

    fn slider(doc: &mut Document, rig: LayerId, x: Channel<f32>, link: ControllerLink) {
        let mut c = Controller::new("Smile", ControllerKind::Slider);
        c.x = x;
        c.links.push(link);
        skel(doc, rig).controllers.push(c);
    }

    const LINK: ControllerLink = ControllerLink {
        action: 1,
        axis: ControllerAxis::X,
        from: 0.0,
        to: 1.0,
    };

    #[test]
    fn a_slider_plays_a_regular_action_by_its_value_held_at_both_ends() {
        let (mut doc, rig) = with_open();
        assert_eq!(mouth_y(&doc, 0.0), 50.0, "no controller, no change");
        for (value, expect) in [
            (0.0, 50.0),
            (0.5, 60.0),
            (1.0, 70.0),
            (3.0, 70.0),
            (-2.0, 50.0),
            (f32::NAN, 50.0),
        ] {
            skel(&mut doc, rig).controllers.clear();
            slider(&mut doc, rig, Channel::new(value), LINK);
            assert!(
                (mouth_y(&doc, 0.0) - expect).abs() < 1e-3,
                "value {value}: {}",
                mouth_y(&doc, 0.0)
            );
        }
    }

    #[test]
    fn a_range_maps_the_value_onto_the_action_and_a_pad_axis_is_its_own_value() {
        let (mut doc, rig) = with_open();
        let mut pad = Controller::new("Look", ControllerKind::Pad);
        pad.x = Channel::new(0.9);
        pad.y = Channel::new(0.75);
        // the action runs while y goes from 0.5 to 1; x is not linked
        pad.links.push(ControllerLink {
            axis: ControllerAxis::Y,
            from: 0.5,
            to: 1.0,
            ..LINK
        });
        skel(&mut doc, rig).controllers.push(pad);
        assert!(
            (mouth_y(&doc, 0.0) - 60.0).abs() < 1e-3,
            "y = 0.75 is halfway through 0.5..1"
        );
        // a range written backwards plays the action backwards
        skel(&mut doc, rig).controllers[0].links[0] = ControllerLink {
            axis: ControllerAxis::Y,
            from: 1.0,
            to: 0.5,
            ..LINK
        };
        assert!((mouth_y(&doc, 0.0) - 60.0).abs() < 1e-3);
        skel(&mut doc, rig).controllers[0].y = Channel::new(1.0);
        assert!((mouth_y(&doc, 0.0) - 50.0).abs() < 1e-3);
    }

    #[test]
    fn a_controllers_values_have_keys_and_move_the_action_over_time() {
        let (mut doc, rig) = with_open();
        let x = Channel::new(0.0)
            .with_key(1, 0.0, Interpolation::Linear)
            .with_key(11, 1.0, Interpolation::Linear);
        slider(&mut doc, rig, x, LINK);
        assert!((mouth_y(&doc, 1.0) - 50.0).abs() < 1e-3);
        assert!((mouth_y(&doc, 6.0) - 60.0).abs() < 1e-3);
        assert!((mouth_y(&doc, 11.0) - 70.0).abs() < 1e-3);
    }

    #[test]
    fn links_that_cannot_play_do_nothing_and_a_controller_is_not_applied_while_its_rig_is_edited() {
        let (mut doc, rig) = with_open();
        let smart = ControllerLink { action: 0, ..LINK }; // action 0 is a Smart Bone action
        let beyond = ControllerLink { action: 9, ..LINK };
        let empty = ControllerLink { to: 0.0, ..LINK };
        let nan = ControllerLink { to: f32::NAN, ..LINK };
        for link in [smart, beyond, empty, nan] {
            skel(&mut doc, rig).controllers.clear();
            slider(&mut doc, rig, Channel::new(1.0), link);
            assert_eq!(mouth_y(&doc, 0.0), 50.0, "{link:?}");
        }
        skel(&mut doc, rig).controllers.clear();
        slider(&mut doc, rig, Channel::new(1.0), LINK);
        assert!((mouth_y(&doc, 0.0) - 70.0).abs() < 1e-3);
        assert!(doc.enter_action(rig, 1));
        assert_eq!(
            mouth_y(&doc, 0.0),
            50.0,
            "the action being edited is shown as it is"
        );
    }

    #[test]
    fn controller_data_survives_saving_and_a_rig_without_them_writes_nothing() {
        let (mut doc, rig) = with_open();
        let x = Channel::new(0.25)
            .with_key(1, 0.0, Interpolation::Linear)
            .with_key(9, 1.0, Interpolation::Smooth);
        slider(
            &mut doc,
            rig,
            x,
            ControllerLink {
                from: 0.1,
                to: 0.9,
                ..LINK
            },
        );
        let json = serde_json::to_string(&skel(&mut doc, rig).clone()).unwrap();
        let back: Skeleton = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *skel(&mut doc, rig));
        let plain = serde_json::to_string(&Skeleton::default()).unwrap();
        assert!(!plain.contains("controllers"), "{plain}");
        // a link with the default range writes no range
        let c = Controller {
            links: vec![LINK],
            ..Controller::new("C", ControllerKind::Slider)
        };
        let text = serde_json::to_string(&c).unwrap();
        assert!(!text.contains("from") && !text.contains("\"to\""), "{text}");
    }
}
