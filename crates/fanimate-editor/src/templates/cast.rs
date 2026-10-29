//! The cartoon characters of the templates: layered SVG art on a bone rig
//! (body, head, two arms with forearms), a blinking pair of eyes and a
//! switch layer of mouth shapes, with the acting that goes with talking:
//! breathing, nodding, gesturing while they speak and listening while others
//! do.

use std::f32::consts::{FRAC_PI_2, PI};

use fanimate_core::{Document, Frame, Interpolation, Key, Layer, LayerKind, Skeleton, Vec2};

use super::art::{self, MouthStyle, vector_mouths};
use crate::editor::labelled;
use crate::editor::sound::mouth_keys;
use crate::editor::{MouthCue, svg};

macro_rules! looks {
    ($($name:literal),* $(,)?) => {
        [$(($name, include_str!(concat!("../../assets/templates/characters/", $name, ".svg")))),*]
    };
}

/// Every character's artwork by name.
pub(super) const LOOK_NAMES: [(&str, &str); 8] = looks![
    "gaffer",
    "striker",
    "manager",
    "reporter",
    "keeper",
    "pundit",
    "presenter",
    "buddy"
];

/// The mouth a character talks with.
#[derive(Clone, Copy)]
pub enum Mouth {
    /// The photo-real mouth set.
    Photo,
    /// One of the five vector styles (an index into [`art::STYLES`]).
    Style(usize),
}

/// Where a character stands and how big it is.
#[derive(Clone, Copy)]
pub struct Stand {
    /// Where the feet are, in the document.
    pub feet: Vec2,
    pub scale: f32,
}

/// Where the mouth sits on every character, in its artwork (800 x 1000).
const MOUTH_AT: Vec2 = Vec2::new(400.0, 418.0);
/// Where the feet are in the artwork.
const FEET_AT: Vec2 = Vec2::new(400.0, 960.0);

/// The bones of a rig.
#[derive(Clone, Copy)]
struct Bones {
    body: usize,
    head: usize,
    arm_a: usize,
    fore_a: usize,
    arm_b: usize,
    fore_b: usize,
}

/// A character in a scene: its rig layer and the layers the acting keys.
pub struct Actor {
    pub rig: Layer,
    /// The mouth's switch layer.
    pub mouth: fanimate_core::LayerId,
    bones: Bones,
}

/// A bone's origin in its parent's space, from the world position of both.
fn local(parent_at: Vec2, parent_angle: f32, child_at: Vec2) -> Vec2 {
    let d = child_at - parent_at;
    let (s, c) = (-parent_angle).sin_cos();
    Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c)
}

/// Build the rig of the character `look` standing at `stand`, talking with `mouth`.
/// An unknown name gives an empty rig; the tests check every name used.
pub fn actor(doc: &mut Document, look: &str, mouth: Mouth, stand: Stand) -> Actor {
    let art_text = LOOK_NAMES.iter().find(|(n, _)| *n == look).map_or("", |(_, s)| s);
    let (parts, _) = svg::bundled(look, art_text);

    // bones: the body stands on the hips and points up, the head sits on the neck
    let hips = Vec2::new(400.0, 740.0);
    let neck = Vec2::new(400.0, 488.0);
    let mut skel = Skeleton::default();
    let body = skel.add("body", None, hips, 252.0, -FRAC_PI_2);
    let head = skel.add("head", Some(body), local(hips, -FRAC_PI_2, neck), 180.0, 0.0);
    // arms hang down: pointing down is PI from the body's own direction
    let shoulder_a = Vec2::new(262.0, 520.0);
    let shoulder_b = Vec2::new(538.0, 520.0);
    let arm_a = skel.add(
        "arm A",
        Some(body),
        local(hips, -FRAC_PI_2, shoulder_a),
        150.0,
        PI,
    );
    let fore_a = skel.add("forearm A", Some(arm_a), Vec2::new(150.0, 0.0), 165.0, 0.0);
    let arm_b = skel.add(
        "arm B",
        Some(body),
        local(hips, -FRAC_PI_2, shoulder_b),
        150.0,
        PI,
    );
    let fore_b = skel.add("forearm B", Some(arm_b), Vec2::new(150.0, 0.0), 165.0, 0.0);
    let bones = Bones {
        body,
        head,
        arm_a,
        fore_a,
        arm_b,
        fore_b,
    };

    let bone_of = |name: &str| match name {
        "body" => Some(body),
        "upper arm A" => Some(arm_a),
        "forearm A" => Some(fore_a),
        "upper arm B" => Some(arm_b),
        "forearm B" => Some(fore_b),
        "head back" | "face" | "head front" | "eyes open" | "eyes closed" => Some(head),
        _ => None,
    };

    let mut layers: Vec<Layer> = Vec::new();
    let mut eyes: Vec<Layer> = Vec::new();
    let mut mouth_id = None;
    for (name, mut shapes) in parts {
        if let Some(b) = bone_of(&name) {
            for shape in &mut shapes {
                shape.bone = Some(b);
            }
        }
        match name.as_str() {
            "eyes open" | "eyes closed" => {
                let child_name = if name == "eyes open" { "open" } else { "closed" };
                eyes.push(labelled(doc.new_layer(child_name, LayerKind::vector(shapes))));
                if eyes.len() == 2 {
                    // children are open then closed, so the switch rests on open eyes
                    layers.push(art::resting_switch(doc, "Eyes", std::mem::take(&mut eyes)));
                }
            }
            "head front" => {
                // the mouth goes between the eyes and the hair, moustache and glasses
                let switch = match mouth {
                    Mouth::Photo => art::photo_mouths(doc, MOUTH_AT, 165.0, Some(head)),
                    Mouth::Style(i) => {
                        let style: &MouthStyle = &art::STYLES[i.min(art::STYLES.len() - 1)];
                        vector_mouths(doc, style, MOUTH_AT, 1.25, Some(head))
                    }
                };
                mouth_id = Some(switch.id);
                layers.push(switch);
                layers.push(labelled(doc.new_layer(name, LayerKind::vector(shapes))));
            }
            _ => layers.push(labelled(doc.new_layer(name, LayerKind::vector(shapes)))),
        }
    }

    let mut rig = labelled(doc.new_layer(
        look,
        LayerKind::Bone {
            skeleton: skel,
            children: layers,
        },
    ));
    rig.transform.position.base = stand.feet - FEET_AT * stand.scale;
    rig.transform.scale.base = Vec2::splat(stand.scale);
    Actor {
        rig,
        mouth: mouth_id.unwrap_or(fanimate_core::LayerId(0)),
        bones,
    }
}

impl Actor {
    /// Key the mouth from cues in seconds from the start of a sound that starts on `start`.
    pub fn lip_sync(&mut self, cues: &[MouthCue], start: Frame, fps: f32) {
        let LayerKind::Bone { children, .. } = &mut self.rig.kind else {
            return;
        };
        for layer in children.iter_mut().filter(|l| l.id == self.mouth) {
            let LayerKind::Switch { children, switch } = &mut layer.kind else {
                continue;
            };
            let (mut keys, _) = mouth_keys(children, cues, start, fps);
            // keep the resting key of the start while the first cue comes later
            if keys.first().is_none_or(|k| k.frame > 1) {
                keys.insert(
                    0,
                    Key {
                        frame: 1,
                        value: "X".to_owned(),
                        interp: Interpolation::Step,
                    },
                );
            }
            *switch = keys;
        }
    }

    /// Show the mouth shape `shape` from frame `from` and go back to rest on frame `to`:
    /// a smile at a punchline, a gasp at someone else's.
    pub fn react(&mut self, shape: &str, from: Frame, to: Frame) {
        let LayerKind::Bone { children, .. } = &mut self.rig.kind else {
            return;
        };
        for layer in children.iter_mut().filter(|l| l.id == self.mouth) {
            let LayerKind::Switch { switch, .. } = &mut layer.kind else {
                continue;
            };
            // what the mouth was doing when the reaction ends is forgotten: it rests
            switch.retain(|k| k.frame < from || k.frame > to);
            for (frame, value) in [(from, shape), (to, "X")] {
                fanimate_core::anim::set_key_in(switch, frame, value.to_owned(), Interpolation::Step);
            }
        }
    }

    /// Blink now and then: the eyes close for three frames, about every four seconds,
    /// at moments that differ from character to character.
    pub fn blink(&mut self, seed: u32, end: Frame, fps: f32) {
        let LayerKind::Bone { children, .. } = &mut self.rig.kind else {
            return;
        };
        for layer in children.iter_mut().filter(|l| l.name == "Eyes") {
            let LayerKind::Switch { switch, .. } = &mut layer.kind else {
                continue;
            };
            let mut rng = Lcg(seed);
            let mut f = 1 + (fps * (1.0 + rng.next() * 2.0)) as Frame;
            while f < end {
                for (frame, value) in [(f, "closed"), (f + 3, "open")] {
                    fanimate_core::anim::set_key_in(switch, frame, value.to_owned(), Interpolation::Step);
                }
                f += (fps * (2.5 + rng.next() * 3.0)) as Frame;
            }
        }
    }

    /// Breathing, nodding and gesturing: `speaking` are the spans of this character's lines
    /// and `listening` those of the others, in frames.
    pub fn act(&mut self, seed: u32, end: Frame, speaking: &[(Frame, Frame)], listening: &[(Frame, Frame)]) {
        let b = self.bones;
        let LayerKind::Bone { skeleton, .. } = &mut self.rig.kind else {
            return;
        };
        let rest = |skeleton: &Skeleton, bone: usize| skeleton.bones[bone].angle.base;
        let mut rng = Lcg(seed);

        // breathing: the body sways a hair about once a second and a half
        let (body_rest, head_rest) = (rest(skeleton, b.body), rest(skeleton, b.head));
        let mut f = 1;
        let mut sign = 1.0;
        while f < end {
            skeleton.key_angle(b.body, f, body_rest + 0.012 * sign, Interpolation::Smooth);
            sign = -sign;
            f += 18;
        }
        skeleton.key_angle(b.body, end, body_rest, Interpolation::Smooth);

        // the head: still at the start, nodding on the beat of speech, a slow nod at
        // the start of what others say
        let mut head: Vec<(Frame, f32)> = vec![(1, 0.0)];
        for &(from, to) in speaking {
            let mut t = from;
            let mut flip = if rng.next() > 0.5 { 1.0 } else { -1.0 };
            while t < to {
                head.push((t, 0.05 * flip * (0.6 + rng.next() * 0.8)));
                flip = -flip;
                t += 8 + (rng.next() * 6.0) as Frame;
            }
            head.push((to, 0.0));
        }
        for &(from, _) in listening {
            head.push((from + 6, 0.07));
            head.push((from + 16, -0.02));
            head.push((from + 26, 0.0));
        }
        head.push((end, 0.0));
        head.sort_by_key(|(f, _)| *f);
        head.dedup_by_key(|(f, _)| *f);
        for (frame, offset) in head {
            skeleton.key_angle(b.head, frame, head_rest + offset, Interpolation::Smooth);
        }

        // gestures: while speaking an arm lifts and falls every second or so, the arms taking turns
        let arm_rest = [rest(skeleton, b.arm_a), rest(skeleton, b.arm_b)];
        let fore_rest = [rest(skeleton, b.fore_a), rest(skeleton, b.fore_b)];
        let mut arms: [Vec<(Frame, f32, f32)>; 2] = [vec![(1, 0.0, 0.0)], vec![(1, 0.0, 0.0)]];
        let mut turn = usize::from(rng.next() > 0.5);
        let mut free = [1, 1];
        for &(from, to) in speaking {
            let mut t = from + 10;
            while t + 30 < to {
                let side = turn % 2;
                turn += 1;
                if t > free[side] + 2 {
                    // arm A is on the viewer's left: out is clockwise, so positive; arm B the other way
                    let sign = if side == 0 { 1.0 } else { -1.0 };
                    let lift = (0.45 + rng.next() * 0.55) * sign;
                    let bend = -(1.0 + rng.next() * 0.7) * sign;
                    arms[side].push((t, 0.0, 0.0));
                    arms[side].push((t + 8, lift, bend));
                    arms[side].push((t + 16, lift * 0.85, bend * 1.1));
                    arms[side].push((t + 26, 0.0, 0.0));
                    free[side] = t + 26;
                }
                t += 24 + (rng.next() * 8.0) as Frame;
            }
        }
        for (side, keys) in arms.iter_mut().enumerate() {
            keys.push((end, 0.0, 0.0));
            let (arm, fore) = if side == 0 {
                (b.arm_a, b.fore_a)
            } else {
                (b.arm_b, b.fore_b)
            };
            for &(frame, lift, bend) in keys.iter() {
                skeleton.key_angle(arm, frame, arm_rest[side] + lift, Interpolation::Smooth);
                skeleton.key_angle(fore, frame, fore_rest[side] + bend, Interpolation::Smooth);
            }
        }
    }
}

/// A small deterministic random number generator, so a template is the same every time.
pub struct Lcg(pub u32);

impl Lcg {
    /// A number from 0 up to 1.
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}
