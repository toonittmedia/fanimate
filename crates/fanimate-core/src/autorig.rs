//! Auto-rig: a skeleton worked out from what a character's layers are
//! called and where they sit.
//!
//! This is a classical method, not a model: the layer names are read for the
//! words artists use (torso, head, upper arm, forearm, thigh, shin, tail,
//! with a left, right, L, R, A or B marking the side), the bone for each part
//! is laid along the box of its artwork, and every layer is bound to the
//! bone of its part. Where a name says nothing, or the character has no
//! torso to hang the rest from, the plan says so instead of guessing: layers
//! it could not place are left unbound and listed. The plan is a proposal in
//! ordinary bones the animator moves; nothing about it is hidden.
//!
//! The artwork is assumed to be drawn in the pose it rests in, upright and
//! facing the viewer. Arms and tails that are wider than tall are taken to
//! point sideways, away from the body.

use glam::Vec2;

use crate::bones::Skeleton;

/// The most pieces one plan reads.
pub const MAX_PIECES: usize = 500;

/// A layer of the character: its name and the box its artwork fills.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub name: String,
    pub min: Vec2,
    pub max: Vec2,
}

/// Which side of the screen a limb is on, by the artist's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    L,
    R,
}

impl Side {
    fn label(self) -> &'static str {
        match self {
            Self::L => "L",
            Self::R => "R",
        }
    }
}

/// The upper part of a limb, or the part beyond its joint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Segment {
    Upper,
    Lower,
}

/// What a layer is, by its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Body,
    Head,
    Arm(Segment),
    Leg(Segment),
    Tail,
}

/// The words of a name, lower case: split at anything that is not a letter
/// or digit and before a capital that follows a small letter (upperArmL).
fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        if c.is_uppercase() && i > 0 && chars[i - 1].is_lowercase() && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Whether some word is `stem`, its plural, or a compound ending in it
/// (forearm, upperarm, foreleg). Whole words only, so "forearm" is not an
/// "ear" and "armour" is not an "arm".
fn names(words: &[String], stems: &[&str]) -> bool {
    const PREFIXES: [&str; 6] = ["fore", "upper", "lower", "hind", "top", "back"];
    words.iter().any(|w| {
        stems.iter().any(|s| {
            *w == *s
                || *w == format!("{s}s")
                || w.strip_suffix(s).is_some_and(|front| PREFIXES.contains(&front))
        })
    })
}

/// Read a layer's name. `None` means it names no part of a body (a
/// background, a prop, a shadow).
pub fn classify(name: &str) -> Option<Role> {
    let w = words(name);
    if names(
        &w,
        &[
            "head", "face", "hair", "skull", "neck", "eye", "mouth", "nose", "brow", "ear", "hat", "eyebrow",
        ],
    ) {
        return Some(Role::Head);
    }
    if names(
        &w,
        &[
            "torso", "body", "chest", "trunk", "pelvis", "hip", "belly", "spine",
        ],
    ) {
        return Some(Role::Body);
    }
    if names(&w, &["arm", "shoulder", "elbow", "wrist", "hand", "finger"]) {
        let lower = names(&w, &["forearm", "lower", "hand", "wrist", "finger", "elbow"]);
        return Some(Role::Arm(if lower { Segment::Lower } else { Segment::Upper }));
    }
    if names(
        &w,
        &[
            "leg", "thigh", "knee", "shin", "calf", "foot", "feet", "ankle", "shoe", "toe",
        ],
    ) {
        let lower = names(
            &w,
            &["shin", "calf", "foot", "feet", "ankle", "shoe", "toe", "lower"],
        );
        return Some(Role::Leg(if lower { Segment::Lower } else { Segment::Upper }));
    }
    if names(&w, &["tail"]) {
        return Some(Role::Tail);
    }
    None
}

/// The side a name says, if it says one: a whole word `left`, `l`, `a`
/// (or `right`, `r`, `b`), or a word that starts or ends with left or right.
pub fn side_of(name: &str) -> Option<Side> {
    for w in words(name) {
        match w.as_str() {
            "left" | "l" | "a" => return Some(Side::L),
            "right" | "r" | "b" => return Some(Side::R),
            _ => {
                if w.starts_with("left") || w.ends_with("left") {
                    return Some(Side::L);
                }
                if w.starts_with("right") || w.ends_with("right") {
                    return Some(Side::R);
                }
            }
        }
    }
    None
}

/// One planned bone, in the layers' own space (angles in world terms).
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedBone {
    pub name: String,
    /// Index of the parent in [`RigPlan::bones`].
    pub parent: Option<usize>,
    pub origin: Vec2,
    /// The direction the bone points, in radians (y points down).
    pub angle: f32,
    pub length: f32,
}

/// What [`plan`] proposes.
#[derive(Clone, Debug, PartialEq)]
pub struct RigPlan {
    /// Parents come before their children.
    pub bones: Vec<PlannedBone>,
    /// For each piece given, the bone it follows, or `None` if it is not bound.
    pub binding: Vec<Option<usize>>,
}

impl RigPlan {
    /// The names of the pieces left unbound, for the status line.
    pub fn unbound<'a>(&self, pieces: &'a [Piece]) -> Vec<&'a str> {
        self.binding
            .iter()
            .zip(pieces)
            .filter(|(b, _)| b.is_none())
            .map(|(_, p)| p.name.as_str())
            .collect()
    }

    /// The skeleton: bones with origins and angles relative to their
    /// parents, in the rest pose the artwork is drawn in.
    pub fn skeleton(&self) -> Skeleton {
        let mut skeleton = Skeleton::default();
        for bone in &self.bones {
            let (origin, angle) = match bone.parent {
                Some(p) => {
                    let parent = &self.bones[p];
                    let d = bone.origin - parent.origin;
                    let (s, c) = (-parent.angle).sin_cos();
                    (
                        Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c),
                        bone.angle - parent.angle,
                    )
                }
                None => (bone.origin, bone.angle),
            };
            skeleton.add(bone.name.clone(), bone.parent, origin, bone.length, angle);
        }
        skeleton
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec2,
    max: Vec2,
}

impl Bounds {
    fn union(a: Option<Bounds>, p: &Piece) -> Option<Bounds> {
        Some(match a {
            Some(b) => Bounds {
                min: b.min.min(p.min),
                max: b.max.max(p.max),
            },
            None => Bounds {
                min: p.min,
                max: p.max,
            },
        })
    }

    fn size(self) -> Vec2 {
        self.max - self.min
    }

    fn centre(self) -> Vec2 {
        (self.min + self.max) / 2.0
    }
}

/// Where a limb or tail starts and which way it points: sideways (away from
/// the body) when it is clearly wider than tall, else down.
fn along(b: Bounds, body_x: f32) -> (Vec2, f32, f32) {
    let size = b.size();
    if size.x > size.y * 1.3 {
        let right = b.centre().x >= body_x;
        let origin = Vec2::new(if right { b.min.x } else { b.max.x }, b.centre().y);
        (origin, if right { 0.0 } else { std::f32::consts::PI }, size.x)
    } else {
        (
            Vec2::new(b.centre().x, b.min.y),
            std::f32::consts::FRAC_PI_2,
            size.y,
        )
    }
}

/// A plan for the pieces, or why there is none. The character needs a
/// torso (a layer named torso, body, chest and so on): the rest hangs from it.
pub fn plan(pieces: &[Piece]) -> Result<RigPlan, String> {
    if pieces.len() > MAX_PIECES {
        return Err(format!("A rig is planned for at most {MAX_PIECES} layers"));
    }
    if pieces
        .iter()
        .any(|p| !(p.min.is_finite() && p.max.is_finite()) || p.max.x < p.min.x || p.max.y < p.min.y)
    {
        return Err("A layer has no usable size".into());
    }
    let roles: Vec<Option<Role>> = pieces.iter().map(|p| classify(&p.name)).collect();
    let union_of = |want: &dyn Fn(usize) -> bool| -> Option<Bounds> {
        (0..pieces.len())
            .filter(|&i| want(i))
            .fold(None, |acc, i| Bounds::union(acc, &pieces[i]))
    };
    let body = union_of(&|i| roles[i] == Some(Role::Body))
        .ok_or("No layer is named like a torso (torso, body, chest): the rest of the rig hangs from it")?;
    let body_x = body.centre().x;

    let mut bones: Vec<PlannedBone> = Vec::new();
    let mut binding: Vec<Option<usize>> = vec![None; pieces.len()];
    let up = -std::f32::consts::FRAC_PI_2;

    let root = bones.len();
    bones.push(PlannedBone {
        name: "body".into(),
        parent: None,
        origin: Vec2::new(body.centre().x, body.max.y),
        angle: up,
        length: body.size().y.max(1.0),
    });
    for (i, role) in roles.iter().enumerate() {
        if *role == Some(Role::Body) {
            binding[i] = Some(root);
        }
    }

    if let Some(head) = union_of(&|i| roles[i] == Some(Role::Head)) {
        let neck = Vec2::new(head.centre().x, head.max.y);
        let at = bones.len();
        bones.push(PlannedBone {
            name: "head".into(),
            parent: Some(root),
            origin: neck,
            angle: up,
            length: head.size().y.max(1.0),
        });
        for (i, role) in roles.iter().enumerate() {
            if *role == Some(Role::Head) {
                binding[i] = Some(at);
            }
        }
    }

    // a limb or tail: its pieces by side, the upper part first, the lower hung from its end
    let side_of_piece = |i: usize, region: Bounds| -> Side {
        side_of(&pieces[i].name).unwrap_or(if region.centre().x < body_x {
            Side::L
        } else {
            Side::R
        })
    };
    for family in ["arm", "leg", "tail"] {
        let in_family = |i: usize| match roles[i] {
            Some(Role::Arm(_)) => family == "arm",
            Some(Role::Leg(_)) => family == "leg",
            Some(Role::Tail) => family == "tail",
            _ => false,
        };
        let segment = |i: usize| match roles[i] {
            Some(Role::Arm(s) | Role::Leg(s)) => s,
            _ => Segment::Upper,
        };
        for side in [Side::L, Side::R] {
            let mine = |i: usize| {
                in_family(i)
                    && side_of_piece(
                        i,
                        Bounds {
                            min: pieces[i].min,
                            max: pieces[i].max,
                        },
                    ) == side
            };
            let upper = union_of(&|i| mine(i) && segment(i) == Segment::Upper);
            let lower = union_of(&|i| mine(i) && segment(i) == Segment::Lower);
            // a limb with only a lower part (a foot alone) hangs that from the body
            let (first, second) = match (upper, lower) {
                (None, None) => continue,
                (Some(u), l) => (u, l),
                (None, Some(l)) => (l, None),
            };
            let only_lower = upper.is_none();
            let (origin, angle, mut length) = along(first, body_x);
            let at = bones.len();
            let first_name = match (family, only_lower) {
                ("arm", false) => format!("arm {}", side.label()),
                ("arm", true) => format!("forearm {}", side.label()),
                ("leg", false) => format!("leg {}", side.label()),
                ("leg", true) => format!("shin {}", side.label()),
                _ => format!("tail {}", side.label()),
            };
            let second_origin = second.map(|l| along(l, body_x).0);
            if let Some(o) = second_origin {
                // the upper bone reaches the joint
                length = origin.distance(o).max(1.0);
            }
            bones.push(PlannedBone {
                name: first_name,
                parent: Some(root),
                origin,
                angle,
                length: length.max(1.0),
            });
            let lower_bone = second.map(|l| {
                let (o, _, len) = along(l, body_x);
                let index = bones.len();
                bones.push(PlannedBone {
                    name: format!(
                        "{} {}",
                        if family == "arm" { "forearm" } else { "shin" },
                        side.label()
                    ),
                    parent: Some(at),
                    origin: o,
                    angle,
                    length: len.max(1.0),
                });
                index
            });
            for (i, slot) in binding.iter_mut().enumerate() {
                if mine(i) {
                    *slot = Some(match (segment(i), lower_bone) {
                        (Segment::Lower, Some(b)) => b,
                        _ => at,
                    });
                }
            }
        }
    }
    Ok(RigPlan { bones, binding })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(name: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> Piece {
        Piece {
            name: name.into(),
            min: Vec2::new(x0, y0),
            max: Vec2::new(x1, y1),
        }
    }

    #[test]
    fn names_are_read_for_the_words_artists_use() {
        assert_eq!(classify("Torso"), Some(Role::Body));
        assert_eq!(classify("head back"), Some(Role::Head));
        assert_eq!(classify("eyes open"), Some(Role::Head));
        assert_eq!(classify("upper arm A"), Some(Role::Arm(Segment::Upper)));
        assert_eq!(classify("Forearm_L"), Some(Role::Arm(Segment::Lower)));
        assert_eq!(classify("hand R"), Some(Role::Arm(Segment::Lower)));
        assert_eq!(classify("thigh"), Some(Role::Leg(Segment::Upper)));
        assert_eq!(classify("shin.r"), Some(Role::Leg(Segment::Lower)));
        assert_eq!(classify("tail"), Some(Role::Tail));
        assert_eq!(classify("Background"), None);
        assert_eq!(classify("shadow"), None);
        assert_eq!(side_of("upper arm A"), Some(Side::L));
        assert_eq!(side_of("arm B"), Some(Side::R));
        assert_eq!(side_of("leftArm"), Some(Side::L));
        assert_eq!(side_of("upperArmR"), Some(Side::R));
        assert_eq!(side_of("Right_leg"), Some(Side::R));
        assert_eq!(side_of("forearm"), None);
        // "a" inside a word is not a side
        assert_eq!(side_of("balance"), None);
    }

    fn character() -> Vec<Piece> {
        vec![
            piece("background", 0.0, 0.0, 800.0, 800.0),
            piece("torso", 350.0, 300.0, 450.0, 520.0),
            piece("head", 360.0, 180.0, 440.0, 300.0),
            piece("eyes", 370.0, 220.0, 430.0, 240.0),
            piece("upper arm L", 300.0, 310.0, 350.0, 420.0),
            piece("forearm L", 300.0, 420.0, 350.0, 520.0),
            piece("upper arm R", 450.0, 310.0, 500.0, 420.0),
            piece("forearm R", 450.0, 420.0, 500.0, 520.0),
            piece("thigh", 360.0, 520.0, 400.0, 620.0),
            piece("shin", 360.0, 620.0, 400.0, 720.0),
        ]
    }

    #[test]
    fn a_layered_character_gets_a_skeleton_with_every_part_bound() {
        let pieces = character();
        let plan = plan(&pieces).unwrap();
        let names: Vec<&str> = plan.bones.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "body",
                "head",
                "arm L",
                "forearm L",
                "arm R",
                "forearm R",
                "leg L",
                "shin L"
            ]
        );
        // parents come first
        for (i, b) in plan.bones.iter().enumerate() {
            assert!(b.parent.is_none_or(|p| p < i));
        }
        // the background is the only layer left alone
        assert_eq!(plan.unbound(&pieces), ["background"]);
        // the body points up from its bottom, the head sits on it
        let body = &plan.bones[0];
        assert_eq!(body.origin, Vec2::new(400.0, 520.0));
        assert!((body.length - 220.0).abs() < 1.0e-3);
        assert_eq!(plan.bones[1].origin, Vec2::new(400.0, 300.0));
        // the upper arm reaches the forearm's start and points down
        let arm = &plan.bones[2];
        assert!((arm.length - 110.0).abs() < 1.0e-3, "{}", arm.length);
        assert!((arm.angle - std::f32::consts::FRAC_PI_2).abs() < 1.0e-6);
        // the eyes follow the head, the shin its own bone
        assert_eq!(plan.binding[3], Some(1));
        assert_eq!(plan.binding[9], Some(7));
    }

    #[test]
    fn the_skeleton_puts_every_bone_back_at_its_planned_place() {
        let plan = plan(&character()).unwrap();
        let skeleton = plan.skeleton();
        assert_eq!(skeleton.bones.len(), plan.bones.len());
        // the forearm's joint, in world space, is where the plan said
        let mut rest = Vec::new();
        skeleton.rest_pose(&mut rest);
        for (bone, want) in rest.iter().zip(&plan.bones) {
            let at = bone.transform_point2(Vec2::ZERO);
            assert!(
                at.distance(want.origin) < 1.0e-2,
                "{}: {at:?} vs {:?}",
                want.name,
                want.origin
            );
        }
    }

    #[test]
    fn a_character_without_a_torso_is_refused_with_a_reason() {
        let e = plan(&[piece("head", 0.0, 0.0, 10.0, 10.0)]).unwrap_err();
        assert!(e.contains("torso"), "{e}");
        assert!(plan(&[piece("torso", f32::NAN, 0.0, 1.0, 1.0)]).is_err());
        assert!(plan(&[piece("torso", 5.0, 5.0, 1.0, 1.0)]).is_err());
    }

    #[test]
    fn a_limb_with_no_side_in_its_name_goes_by_where_it_is() {
        let pieces = vec![
            piece("torso", 350.0, 300.0, 450.0, 520.0),
            piece("arm", 300.0, 310.0, 350.0, 420.0),
            piece("arm", 450.0, 310.0, 500.0, 420.0),
        ];
        let plan = plan(&pieces).unwrap();
        let names: Vec<&str> = plan.bones.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["body", "arm L", "arm R"]);
    }

    #[test]
    fn an_arm_held_out_points_away_from_the_body_and_a_tail_too() {
        let pieces = vec![
            piece("torso", 350.0, 300.0, 450.0, 520.0),
            piece("arm R", 450.0, 320.0, 560.0, 350.0),
            piece("tail L", 220.0, 480.0, 350.0, 510.0),
        ];
        let plan = plan(&pieces).unwrap();
        let arm = plan.bones.iter().find(|b| b.name == "arm R").unwrap();
        assert!(arm.angle.abs() < 1.0e-6);
        assert_eq!(arm.origin.x, 450.0);
        let tail = plan.bones.iter().find(|b| b.name == "tail L").unwrap();
        assert!((tail.angle - std::f32::consts::PI).abs() < 1.0e-6);
        assert_eq!(tail.origin.x, 350.0);
    }

    #[test]
    fn a_lone_lower_part_still_gets_a_bone() {
        let pieces = vec![
            piece("torso", 0.0, 0.0, 100.0, 200.0),
            piece("shin R", 60.0, 200.0, 90.0, 300.0),
        ];
        let plan = plan(&pieces).unwrap();
        assert_eq!(plan.bones.len(), 2);
        assert_eq!(plan.bones[1].name, "shin R");
        assert_eq!(plan.binding, vec![Some(0), Some(1)]);
    }
}
