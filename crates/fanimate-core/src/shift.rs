//! Moving a whole document along the timeline.
//!
//! Version 6 counts frames from 1, with frame 0 kept for the rest pose, so a
//! file saved before it has every frame number put up by one when it loads.
//! The same move is what shifting a clip would need, so it takes any amount.

use crate::actions::RigAction;
use crate::anim::{Channel, Frame, Key};
use crate::frames::Cel;
use crate::path::Shape;
use crate::scene::{Document, Layer, LayerKind};

/// Something that holds frame numbers and can move them all.
trait Shift {
    fn shift(&mut self, by: Frame);
}

impl<T> Shift for Vec<Key<T>> {
    fn shift(&mut self, by: Frame) {
        for key in self.iter_mut() {
            key.frame = key.frame.saturating_add(by);
        }
        // saturating can land several keys on the last frame there is, and
        // a list holds at most one key per frame
        self.dedup_by_key(|k| k.frame);
    }
}

impl Shift for Vec<Cel> {
    fn shift(&mut self, by: Frame) {
        for cel in self.iter_mut() {
            cel.frame = cel.frame.saturating_add(by);
        }
        self.dedup_by_key(|c| c.frame);
    }
}

impl<T> Shift for Channel<T> {
    fn shift(&mut self, by: Frame) {
        self.keys.shift(by);
    }
}

fn shape(s: &mut Shape, by: Frame) {
    s.path_keys.shift(by);
    s.fill_keys.shift(by);
    s.stroke_keys.shift(by);
    s.trim_keys.shift(by);
}

fn action(a: &mut RigAction, by: Frame) {
    a.length = a.length.saturating_add(by);
    for bone in &mut a.bones {
        bone.angle.shift(by);
    }
    for keys in &mut a.shapes {
        keys.path_keys.shift(by);
    }
}

fn layers(list: &mut [Layer], by: Frame) {
    for l in list {
        l.opacity.shift(by);
        l.order.shift(by);
        l.transform.position.shift(by);
        l.transform.rotation.shift(by);
        l.transform.scale.shift(by);
        match &mut l.kind {
            LayerKind::Vector { shapes } => shapes.iter_mut().for_each(|s| shape(s, by)),
            LayerKind::Frames { cels } => {
                for cel in cels.iter_mut() {
                    cel.shapes_mut().for_each(|s| shape(s, by));
                }
                cels.shift(by);
            }
            LayerKind::Bone { skeleton, children } => {
                for bone in &mut skeleton.bones {
                    bone.angle.shift(by);
                }
                for act in &mut skeleton.actions {
                    action(act, by);
                }
                for c in &mut skeleton.controllers {
                    c.x.shift(by);
                    c.y.shift(by);
                }
                layers(children, by);
            }
            LayerKind::Group { children } => layers(children, by),
            LayerKind::Switch { children, switch } => {
                switch.shift(by);
                layers(children, by);
            }
            LayerKind::Audio(audio) => audio.start = audio.start.saturating_add(by),
            LayerKind::Image(_) => {}
            LayerKind::Text(text) => {
                text.path_offset.shift(by);
                for a in &mut text.animators {
                    a.selector.start.shift(by);
                    a.selector.end.shift(by);
                    a.selector.offset.shift(by);
                    a.position.shift(by);
                    a.rotation.shift(by);
                    a.scale.shift(by);
                    a.opacity.shift(by);
                    if let Some(fill) = &mut a.fill {
                        fill.shift(by);
                    }
                }
            }
        }
    }
}

impl Document {
    /// Move everything on the timeline `delta` frames later (earlier when
    /// negative): the playing range, every key of every layer and of the
    /// camera, the drawings of frame-by-frame layers, the start of sounds,
    /// and the keys and length of every action, including the keys of an
    /// action being edited and the timeline's put aside while it is. Guides
    /// are positions, not frames, and stay. Arithmetic saturates, so a
    /// damaged file cannot make this panic.
    pub fn shift_frames(&mut self, delta: Frame) {
        if delta == 0 {
            return;
        }
        self.start = self.start.saturating_add(delta);
        self.end = self.end.saturating_add(delta);
        self.camera.position.shift(delta);
        self.camera.zoom.shift(delta);
        self.camera.roll.shift(delta);
        if let Some(edit) = &mut self.action_edit {
            for channel in &mut edit.bones {
                channel.shift(delta);
            }
            for keys in &mut edit.shapes {
                keys.path_keys.shift(delta);
            }
        }
        layers(&mut self.layers, delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{BoneKeys, ShapeKeys};
    use crate::anim::{Interpolation, set_key_in};
    use crate::scene::LayerId;
    use crate::{Color, Style, Vec2};
    use serde_json::Value;

    /// Every `frame` a document would write: keys, drawings, all of it.
    fn frames_of(doc: &Document) -> Vec<i64> {
        fn walk(v: &Value, out: &mut Vec<i64>) {
            match v {
                Value::Object(map) => {
                    if let Some(f) = map.get("frame").and_then(Value::as_i64) {
                        out.push(f);
                    }
                    map.values().for_each(|v| walk(v, out));
                }
                Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(&serde_json::to_value(doc).unwrap(), &mut out);
        out
    }

    /// Key every kind of thing a document can animate, all on frame 10 (the
    /// drawings of the frame-by-frame layer on 10 and 20), the playing range
    /// being 10 to 20. Returns the document and its bone layer.
    fn everything() -> (Document, LayerId) {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        doc.start = 10;
        doc.end = 20;
        let style = Style::fill(Color::BLACK).with_stroke(Color::BLACK, 1.0);
        let mut keyed = Shape::rectangle("r", Vec2::ZERO, Vec2::splat(10.0), 0.0, style);
        keyed.key_path(10, keyed.path.clone(), Interpolation::Linear);
        set_key_in(&mut keyed.fill_keys, 10, Color::WHITE, Interpolation::Linear);
        set_key_in(&mut keyed.stroke_keys, 10, Color::WHITE, Interpolation::Linear);
        set_key_in(
            &mut keyed.trim_keys,
            10,
            Vec2::new(0.0, 0.5),
            Interpolation::Linear,
        );

        let art = doc.new_layer("art", LayerKind::vector(vec![keyed.clone()]));
        let art_id = art.id;

        let mut skeleton = crate::Skeleton::default();
        let bone = skeleton.add("b", None, Vec2::ZERO, 10.0, 0.0);
        skeleton.key_angle(bone, 10, 0.5, Interpolation::Linear);
        let mut act = RigAction::new("wave", Some(bone), 20);
        act.bones.push(BoneKeys {
            bone,
            angle: Channel::new(0.0).with_key(10, 1.0, Interpolation::Linear),
        });
        act.shapes.push(ShapeKeys {
            layer: art_id,
            shape: 0,
            path_keys: vec![Key {
                frame: 10,
                value: keyed.path.clone(),
                interp: Interpolation::Linear,
            }],
        });
        skeleton.actions.push(act);
        let rig = doc.new_layer(
            "rig",
            LayerKind::Bone {
                skeleton,
                children: vec![art],
            },
        );
        let rig_id = rig.id;

        let mut first = Cel::new(10);
        first.line.push(keyed);
        let frames = doc.new_layer(
            "anim",
            LayerKind::Frames {
                cels: vec![first, Cel::new(20)],
            },
        );
        let mouth = doc.new_layer("A", LayerKind::vector(vec![]));
        let switch = doc.new_layer(
            "mouth",
            LayerKind::Switch {
                children: vec![mouth],
                switch: vec![Key {
                    frame: 10,
                    value: "A".to_owned(),
                    interp: Interpolation::Step,
                }],
            },
        );
        let group = doc.new_layer(
            "g",
            LayerKind::Group {
                children: vec![frames, switch],
            },
        );
        let mut audio = crate::Audio::new("media/x.wav");
        audio.start = 10;
        let sound = doc.new_layer("sound", LayerKind::Audio(audio));
        doc.layers = vec![rig, group, sound];

        // every layer's own channels
        doc.walk_mut(|l| {
            l.opacity.set_key(10, 0.5, Interpolation::Linear);
            l.order.set_key(10, 1.0, Interpolation::Step);
            l.transform.position.set_key(10, Vec2::ONE, Interpolation::Linear);
            l.transform.rotation.set_key(10, 0.1, Interpolation::Linear);
            l.transform.scale.set_key(10, Vec2::ONE, Interpolation::Linear);
        });
        doc.camera.position.set_key(10, Vec2::ONE, Interpolation::Linear);
        doc.camera.zoom.set_key(10, 2.0, Interpolation::Linear);
        doc.camera.roll.set_key(10, 0.3, Interpolation::Linear);
        doc.guides.push(crate::scene::Guide {
            vertical: true,
            position: 10.0,
            locked: false,
        });
        (doc, rig_id)
    }

    fn only(frames: &[i64], a: i64, b: i64) -> bool {
        frames.iter().all(|f| *f == a || *f == b)
    }

    #[test]
    fn shifting_moves_every_kind_of_key_and_nothing_else() {
        let (original, rig) = everything();
        let before = frames_of(&original);
        // 7 layers with 5 channels, 4 shape key lists in two places, 2 drawings,
        // a bone, 2 action keys, a switch key and 3 camera keys
        assert!(before.len() >= 50, "{} keys", before.len());
        assert!(only(&before, 10, 20));

        let mut doc = original.clone();
        doc.shift_frames(1);
        let after = frames_of(&doc);
        assert_eq!(after.len(), before.len());
        assert!(only(&after, 11, 21), "a key was missed: {after:?}");
        assert_eq!((doc.start, doc.end), (11, 21));
        let LayerKind::Audio(sound) = &doc.layers[2].kind else {
            unreachable!()
        };
        assert_eq!(sound.start, 11, "sounds start a frame later too");
        let LayerKind::Bone { skeleton, .. } = &doc.find(rig).unwrap().kind else {
            unreachable!()
        };
        assert_eq!(skeleton.actions[0].length, 21);
        assert_eq!(doc.guides, original.guides, "guides are positions, not frames");
        assert_eq!(doc.frame_count(), original.frame_count());

        // moving back gives the document it was
        doc.shift_frames(-1);
        assert_eq!(doc, original);
    }

    #[test]
    fn the_keys_of_an_action_being_edited_and_the_ones_put_aside_both_move() {
        let (mut doc, rig) = everything();
        assert!(doc.enter_action(rig, 0));
        assert!(doc.action_edit.is_some());
        let before = frames_of(&doc);
        assert!(only(&before, 10, 20));
        let mut shifted = doc.clone();
        shifted.shift_frames(1);
        assert!(only(&frames_of(&shifted), 11, 21));
        assert_eq!(frames_of(&shifted).len(), before.len());
        // leaving the action afterwards puts everything where a shifted
        // document has it
        let mut left = shifted.clone();
        left.leave_action();
        let mut expected = everything().0;
        expected.shift_frames(1);
        assert_eq!(left, expected);
    }

    #[test]
    fn an_empty_shift_and_an_overflowing_one_are_harmless() {
        let (mut doc, _) = everything();
        let same = doc.clone();
        doc.shift_frames(0);
        assert_eq!(doc, same);

        // two keys that would pass the last frame there is end up as one
        let mut ch = Channel::new(0.0_f32)
            .with_key(Frame::MAX - 1, 1.0, Interpolation::Linear)
            .with_key(Frame::MAX, 2.0, Interpolation::Linear);
        ch.shift(5);
        assert_eq!(ch.keys.len(), 1);
        assert_eq!(ch.keys[0].frame, Frame::MAX);
        let mut huge = Document::new("t", 10.0, 10.0, 24.0);
        huge.end = Frame::MAX;
        huge.shift_frames(Frame::MAX);
        assert_eq!((huge.start, huge.end), (Frame::MAX, Frame::MAX));
        assert_eq!(huge.frame_count(), 1);
    }

    #[test]
    fn a_new_document_runs_from_frame_one() {
        let doc = Document::new("t", 10.0, 10.0, 24.0);
        assert_eq!((doc.start, doc.end), (1, 96));
        assert_eq!(doc.frame_count(), 96);
        assert_eq!(Document::new("t", 10.0, 10.0, 12.0).end, 48);
        // a nonsense frame rate still gives a document that plays
        assert_eq!(Document::new("t", 10.0, 10.0, f32::NAN).frame_count(), 1);
    }
}
