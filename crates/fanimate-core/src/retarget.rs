//! Retargeting: copy an action from one rig to another that has different
//! proportions, a different rest pose, or both. Bones are paired by name (the
//! way animators already name them: "arm L", "Arm_L" and "arm.l" are one
//! bone), and what is carried over is each bone's turn away from its own rest
//! angle, so a walk made on a tall rig plays on a short one without the short
//! rig's arms jumping to the tall rig's rest angles. Angles do not depend on
//! how long a bone is, so proportions need no scaling.
//!
//! What cannot be carried is counted, never guessed at: a bone the other rig
//! has no match for, and the point keys of shapes (they belong to the first
//! rig's artwork). A Smart Bone action needs its driver bone matched, or it
//! cannot play.

use crate::actions::{BoneKeys, RigAction};
use crate::anim::Channel;
use crate::bones::Skeleton;

/// What retargeting made and what it left behind.
#[derive(Clone, Debug, PartialEq)]
pub struct Retargeted {
    pub action: RigAction,
    /// Bones paired with a bone of the other rig.
    pub matched: usize,
    /// Names of the action's bones that the other rig has no match for.
    pub unmatched: Vec<String>,
    /// Shapes whose point keys were left out.
    pub shapes_dropped: usize,
}

/// A bone's name without case, spaces, dots, dashes or underscores, so the
/// usual spellings of one name compare equal.
fn key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The index in `to` of the bone named like bone `from_bone` of `from`: the
/// first one when several are named alike, none for a name with nothing in it.
fn partner(from: &Skeleton, from_bone: usize, to: &Skeleton) -> Option<usize> {
    let wanted = key(&from.bones.get(from_bone)?.name);
    if wanted.is_empty() {
        return None;
    }
    to.bones.iter().position(|b| key(&b.name) == wanted)
}

/// Copy `action` of rig `from` onto rig `to`. `Err` when it is a Smart Bone
/// action whose driver has no match, or when no bone at all could be paired.
pub fn retarget(from: &Skeleton, action: &RigAction, to: &Skeleton) -> Result<Retargeted, String> {
    let driver = match action.driver {
        Some(d) => Some(partner(from, d, to).ok_or_else(|| {
            let name = from.bones.get(d).map_or("?", |b| b.name.as_str());
            format!("The driver bone \"{name}\" has no match in the other rig.")
        })?),
        None => None,
    };
    let mut out = RigAction::new(action.name.clone(), driver, action.length);
    let mut unmatched = Vec::new();
    for entry in &action.bones {
        let (Some(src), Some(dst)) = (from.bones.get(entry.bone), partner(from, entry.bone, to)) else {
            let name = from.bones.get(entry.bone).map_or("?", |b| b.name.as_str());
            unmatched.push(name.to_owned());
            continue;
        };
        // the turn away from the rest angle, carried onto the other rig's
        let (rest_from, rest_to) = (src.angle.base, to.bones[dst].angle.base);
        let mut channel = Channel::new(entry.angle.base - rest_from + rest_to);
        channel.loop_mode = entry.angle.loop_mode;
        for k in &entry.angle.keys {
            channel.set_key(k.frame, k.value - rest_from + rest_to, k.interp);
        }
        // two bones of one rig named alike would both land on one: the first wins
        if out.bones.iter().any(|b| b.bone == dst) {
            continue;
        }
        out.bones.push(BoneKeys {
            bone: dst,
            angle: channel,
        });
    }
    if out.bones.is_empty() {
        return Err("None of the bones of that action have a match in the other rig.".to_owned());
    }
    Ok(Retargeted {
        matched: out.bones.len(),
        action: out,
        unmatched,
        shapes_dropped: action.shapes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::Interpolation;
    use glam::Vec2;

    fn rig(names: &[&str], rests: &[f32]) -> Skeleton {
        let mut s = Skeleton::default();
        for (i, n) in names.iter().enumerate() {
            let parent = if i == 0 { None } else { Some(0) };
            s.add(*n, parent, Vec2::ZERO, 10.0 + i as f32, rests[i]);
        }
        s
    }

    fn walk(bone: usize) -> RigAction {
        let mut a = RigAction::new("walk", None, 24);
        a.bones.push(BoneKeys {
            bone,
            angle: Channel::new(0.5)
                .with_key(0, 0.5, Interpolation::Linear)
                .with_key(12, 1.0, Interpolation::Smooth)
                .with_key(24, 0.5, Interpolation::Linear),
        });
        a
    }

    #[test]
    fn spellings_of_one_name_are_one_name() {
        assert_eq!(key("Arm_L"), key("arm l"));
        assert_eq!(key("arm.L"), key(" ARM-l "));
        assert_ne!(key("arm L"), key("arm R"));
        assert_eq!(key("___"), "");
    }

    #[test]
    fn the_turn_from_rest_is_carried_and_the_other_rigs_rest_is_kept() {
        // the source arm rests at 0.5 and swings to 1.0: a turn of 0.5 and back
        let from = rig(&["body", "arm L"], &[0.0, 0.5]);
        // the other rig's arm rests at -0.2, and is named another way
        let to = rig(&["Body", "Arm_L", "tail"], &[0.0, -0.2, 0.0]);
        let r = retarget(&from, &walk(1), &to).unwrap();
        assert_eq!((r.matched, r.unmatched.len(), r.shapes_dropped), (1, 0, 0));
        assert_eq!(r.action.bones[0].bone, 1);
        let keys = &r.action.bones[0].angle.keys;
        let values: Vec<f32> = keys.iter().map(|k| k.value).collect();
        assert!(
            (values[0] - -0.2).abs() < 1e-6,
            "at rest, the new rig is at its rest"
        );
        assert!((values[1] - 0.3).abs() < 1e-6, "0.5 of turn from -0.2");
        assert_eq!(keys[1].interp, Interpolation::Smooth, "easing is kept");
        assert_eq!(r.action.length, 24);
        assert!(
            (r.action.bone_offset(1, 12.0) - 0.5).abs() < 1e-6,
            "the swing is the same size"
        );
    }

    #[test]
    fn bones_without_a_match_are_counted_and_the_rest_still_goes() {
        let from = rig(&["body", "arm L", "wing"], &[0.0, 0.0, 0.0]);
        let to = rig(&["body", "arm L"], &[0.0, 0.0]);
        let mut a = walk(1);
        a.bones.push(walk(2).bones.remove(0));
        a.shapes.push(crate::actions::ShapeKeys {
            layer: crate::LayerId(1),
            shape: 0,
            path_keys: vec![],
        });
        let r = retarget(&from, &a, &to).unwrap();
        assert_eq!((r.matched, r.shapes_dropped), (1, 1));
        assert_eq!(r.unmatched, ["wing"]);
        // nothing in common is an error, not an empty action
        let stranger = rig(&["hull", "fin"], &[0.0, 0.0]);
        assert!(retarget(&from, &walk(1), &stranger).is_err());
    }

    #[test]
    fn a_smart_bone_action_needs_its_driver_matched() {
        let from = rig(&["body", "head", "jaw"], &[0.0; 3]);
        let mut smart = walk(2);
        smart.driver = Some(1);
        smart.bones.push(walk(1).bones.remove(0));
        let with_head = rig(&["body", "head", "jaw"], &[0.0; 3]);
        let r = retarget(&from, &smart, &with_head).unwrap();
        assert_eq!(r.action.driver, Some(1));
        let no_head = rig(&["body", "jaw"], &[0.0; 2]);
        assert!(retarget(&from, &smart, &no_head).unwrap_err().contains("driver"));
    }

    #[test]
    fn two_bones_named_alike_do_not_both_land_on_one() {
        let from = rig(&["body", "arm L", "arm_l"], &[0.0; 3]);
        let to = rig(&["body", "arm L"], &[0.0; 2]);
        let mut a = walk(1);
        a.bones.push(walk(2).bones.remove(0));
        let r = retarget(&from, &a, &to).unwrap();
        assert_eq!(r.action.bones.len(), 1);
    }
}
