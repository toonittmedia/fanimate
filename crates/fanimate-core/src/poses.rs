//! Poses: a whole rig's bone angles at one moment, kept as a one-frame
//! action so they live in the actions list, are inserted at the playhead like
//! any action, and need nothing new in the file format.
//!
//! **Mirroring** turns a pose left to right by the bones' names: a bone named
//! `arm L` swaps with `arm R`, a bone with no side in its name (a spine, a
//! head) stays where it is, and every bone's turn away from its rest angle is
//! reversed, as a reflection in a mirror reverses it. It works from names
//! because that is how animators already name rigs; a bone with a side but no
//! partner is left out and counted, never guessed at.

use crate::actions::{BoneKeys, RigAction};
use crate::anim::{Channel, Interpolation};
use crate::bones::Skeleton;

impl RigAction {
    /// Whether the action is a pose: not driven by a bone, and every key on
    /// its first frame, so it sets angles rather than animating them.
    pub fn is_pose(&self) -> bool {
        self.driver.is_none()
            && self.shapes.is_empty()
            && self
                .bones
                .iter()
                .all(|b| b.angle.keys.iter().all(|k| k.frame == 0))
            && self.bones.iter().any(|b| !b.angle.keys.is_empty())
    }
}

/// The pose of the whole rig on `frame`: every bone's angle there.
pub fn snapshot(skeleton: &Skeleton, name: impl Into<String>, frame: f32) -> RigAction {
    let mut pose = RigAction::new(name, None, 1);
    for (i, bone) in skeleton.bones.iter().enumerate() {
        let angle = bone.angle.sample(frame);
        if angle.is_finite() {
            pose.bones.push(BoneKeys {
                bone: i,
                angle: Channel::new(bone.angle.base).with_key(0, angle, Interpolation::Linear),
            });
        }
    }
    pose
}

/// What mirroring a pose did.
#[derive(Clone, Debug, PartialEq)]
pub struct Mirrored {
    pub pose: RigAction,
    /// Bones swapped with their opposite side.
    pub swapped: usize,
    /// Bones with no side in their name, turned the other way where they are.
    pub centre: usize,
    /// Bones with a side but no partner bone, left out.
    pub unmatched: usize,
}

/// The pose as seen in a mirror (see the module docs).
pub fn mirror_pose(skeleton: &Skeleton, pose: &RigAction) -> Mirrored {
    let mut out = RigAction::new(
        mirror_name(&pose.name).unwrap_or_else(|| format!("{} (mirrored)", pose.name)),
        None,
        pose.length,
    );
    let (mut swapped, mut centre, mut unmatched) = (0, 0, 0);
    for entry in &pose.bones {
        let Some(bone) = skeleton.bones.get(entry.bone) else {
            continue;
        };
        let Some(value) = entry.angle.keys.first().map(|k| k.value) else {
            continue;
        };
        let target = match mirror_name(&bone.name) {
            None => {
                centre += 1;
                entry.bone
            }
            Some(partner) => match skeleton
                .bones
                .iter()
                .position(|b| b.name.eq_ignore_ascii_case(&partner))
            {
                Some(j) if j != entry.bone => {
                    swapped += 1;
                    j
                }
                _ => {
                    unmatched += 1;
                    continue;
                }
            },
        };
        let rest = skeleton.bones[target].angle.base;
        let turned = rest - (value - bone.angle.base);
        if turned.is_finite() {
            out.bones.push(BoneKeys {
                bone: target,
                angle: Channel::new(rest).with_key(0, turned, Interpolation::Linear),
            });
        }
    }
    out.bones.sort_by_key(|b| b.bone);
    Mirrored {
        pose: out,
        swapped,
        centre,
        unmatched,
    }
}

/// The name of the opposite side: `arm L` is `arm R`, `Left Leg` is `Right Leg`,
/// `hand_right` is `hand_left`, `LeftArm` is `RightArm`. Case is kept. `None`
/// when the name has no side in it, or has two (it would not be clear which).
pub fn mirror_name(name: &str) -> Option<String> {
    // words and the separators between them
    let mut parts: Vec<(String, bool)> = Vec::new();
    for c in name.chars() {
        let separator = matches!(c, ' ' | '_' | '-' | '.');
        match parts.last_mut() {
            Some((text, sep)) if *sep == separator && !separator => text.push(c),
            _ => parts.push((c.to_string(), separator)),
        }
    }
    let mut result: Vec<String> = parts.iter().map(|(t, _)| t.clone()).collect();
    let mut sides = 0;
    for (i, (text, separator)) in parts.iter().enumerate() {
        if *separator {
            continue;
        }
        if let Some(swapped) = swap_word(text) {
            result[i] = swapped;
            sides += 1;
        }
    }
    (sides == 1).then(|| result.concat())
}

/// A whole word that is a side, or a side stuck to a camel-case word.
fn swap_word(word: &str) -> Option<String> {
    let lower = word.to_ascii_lowercase();
    let pair = |from: &str, to: &str, case_of: &str| -> String {
        let _ = from;
        if case_of.chars().all(|c| c.is_ascii_uppercase()) && case_of.len() > 1 {
            to.to_ascii_uppercase()
        } else if case_of.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let mut t = to.to_owned();
            if let Some(first) = t.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            t
        } else {
            to.to_owned()
        }
    };
    match lower.as_str() {
        "left" => return Some(pair("left", "right", word)),
        "right" => return Some(pair("right", "left", word)),
        "l" => return Some(if word == "L" { "R".into() } else { "r".into() }),
        "r" => return Some(if word == "R" { "L".into() } else { "l".into() }),
        _ => {}
    }
    // LeftArm, RightFoot: the side first, the next word starting a capital
    for (side, other) in [("Left", "Right"), ("Right", "Left")] {
        if let Some(rest) = word.strip_prefix(side)
            && rest.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        {
            return Some(format!("{other}{rest}"));
        }
        if let Some(rest) = word.strip_suffix(side)
            && rest.chars().last().is_some_and(|c| c.is_ascii_lowercase())
        {
            return Some(format!("{rest}{other}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    fn rig(names: &[&str]) -> Skeleton {
        let mut s = Skeleton::default();
        for (i, name) in names.iter().enumerate() {
            s.add(*name, i.checked_sub(1), Vec2::ZERO, 10.0, 0.0);
        }
        s
    }

    #[test]
    fn a_side_in_a_name_is_swapped_and_its_case_kept() {
        for (from, to) in [
            ("arm L", "arm R"),
            ("arm R", "arm L"),
            ("L arm", "R arm"),
            ("arm_l", "arm_r"),
            ("Left Leg", "Right Leg"),
            ("right hand", "left hand"),
            ("hand.LEFT", "hand.RIGHT"),
            ("LeftArm", "RightArm"),
            ("armRight", "armLeft"),
            ("upper-arm-L", "upper-arm-R"),
        ] {
            assert_eq!(mirror_name(from).as_deref(), Some(to), "{from}");
        }
    }

    #[test]
    fn a_name_with_no_side_or_two_is_not_mirrored_and_other_words_are_not_taken_for_sides() {
        for name in [
            "spine",
            "head",
            "",
            "Rightsize",
            "Leftover",
            "lobster",
            "Rail",
            "left arm right",
            "L R",
        ] {
            assert_eq!(mirror_name(name), None, "{name:?}");
        }
        // mirroring twice is the way back
        for name in ["arm L", "Left Leg", "LeftArm", "hand_right"] {
            let there = mirror_name(name).unwrap();
            assert_eq!(mirror_name(&there).as_deref(), Some(name));
        }
    }

    #[test]
    fn a_pose_holds_every_bones_angle_on_the_frame_and_is_recognised_as_a_pose() {
        let mut s = rig(&["spine", "arm L", "arm R"]);
        s.bones[1].angle.set_key(1, 0.0, Interpolation::Linear);
        s.bones[1].angle.set_key(11, 1.0, Interpolation::Linear);
        let pose = snapshot(&s, "Wave", 6.0);
        assert_eq!(pose.bones.len(), 3);
        let angle = |bone: usize| pose.bones.iter().find(|b| b.bone == bone).unwrap().angle.keys[0].value;
        assert!((angle(1) - 0.5).abs() < 1e-5, "halfway between the keys");
        assert!(pose.is_pose());
        // an action that animates is not a pose, nor is a Smart Bone action or an empty one
        let mut moving = pose.clone();
        moving.bones[0].angle.set_key(5, 1.0, Interpolation::Linear);
        assert!(!moving.is_pose());
        let mut smart = pose.clone();
        smart.driver = Some(0);
        assert!(!smart.is_pose());
        assert!(!RigAction::new("empty", None, 1).is_pose());
    }

    #[test]
    fn mirroring_swaps_the_sides_and_reverses_each_turn_from_rest() {
        let mut s = rig(&["spine", "arm L", "arm R", "tail"]);
        s.bones[1].angle.base = 0.2;
        s.bones[2].angle.base = -0.2;
        let mut pose = RigAction::new("Reach L", None, 1);
        let mut key = |bone: usize, value: f32| {
            pose.bones.push(BoneKeys {
                bone,
                angle: Channel::new(s.bones[bone].angle.base).with_key(0, value, Interpolation::Linear),
            });
        };
        key(0, 0.3); // the spine leans
        key(1, 1.2); // the left arm, a turn of 1.0 from its rest
        key(3, -0.4);
        let m = mirror_pose(&s, &pose);
        assert_eq!((m.swapped, m.centre, m.unmatched), (1, 2, 0));
        assert_eq!(m.pose.name, "Reach R");
        let value = |bone: usize| m.pose.bones.iter().find(|b| b.bone == bone).unwrap().angle.keys[0].value;
        assert!((value(0) + 0.3).abs() < 1e-5, "the spine leans the other way");
        assert!(
            (value(2) - (-0.2 - 1.0)).abs() < 1e-5,
            "the right arm turns the other way from its rest: {}",
            value(2)
        );
        assert!((value(3) - 0.4).abs() < 1e-5);
        assert!(
            m.pose.bones.iter().all(|b| b.bone != 1),
            "the left arm is not in the mirrored pose"
        );
        // and mirroring that gives the original back
        let back = mirror_pose(&s, &m.pose);
        assert_eq!(back.pose.name, "Reach L");
        for entry in &pose.bones {
            let v = back
                .pose
                .bones
                .iter()
                .find(|b| b.bone == entry.bone)
                .unwrap()
                .angle
                .keys[0]
                .value;
            assert!((v - entry.angle.keys[0].value).abs() < 1e-5);
        }
    }

    #[test]
    fn a_bone_with_a_side_and_no_partner_is_left_out_and_counted_not_guessed() {
        let s = rig(&["spine", "wing L"]);
        let pose = snapshot(&s, "Flap", 1.0);
        let m = mirror_pose(&s, &pose);
        assert_eq!((m.swapped, m.centre, m.unmatched), (0, 1, 1));
        assert_eq!(m.pose.bones.len(), 1);
        assert_eq!(m.pose.name, "Flap (mirrored)");
        // a pose that names bones that are gone is read as far as it can be
        let mut stale = snapshot(&s, "Old", 1.0);
        stale.bones.push(BoneKeys {
            bone: 99,
            angle: Channel::new(0.0).with_key(0, 1.0, Interpolation::Linear),
        });
        assert_eq!(mirror_pose(&s, &stale).pose.bones.len(), 1);
    }
}
