//! Built-in sample documents: a hello-world rig and the benchmark scene.

use std::f32::consts::TAU;

use glam::Vec2;

use crate::Color;
use crate::anim::{Frame, Interpolation};
use crate::bones::Skeleton;
use crate::path::{Path, Shape, Style};
use crate::scene::{Document, LayerKind};

/// Brand colours used by the samples (from `brand/tokens/tokens.json`).
pub mod palette {
    use crate::Color;

    pub const NIGHT_BG: Color = rgb(0x15, 0x11, 0x1F);
    pub const VIOLET_300: Color = rgb(0xBC, 0x9B, 0xFF);
    pub const VIOLET_400: Color = rgb(0x9F, 0x73, 0xFF);
    pub const VIOLET_500: Color = rgb(0x84, 0x50, 0xF5);
    pub const VIOLET_600: Color = rgb(0x6D, 0x35, 0xDE);
    pub const VIOLET_950: Color = rgb(0x1C, 0x0B, 0x3D);
    pub const KEYFRAME: Color = rgb(0xFF, 0xB8, 0x4D);
    pub const BONE: Color = rgb(0x5E, 0xEA, 0xD4);

    const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }
}

/// The Bone-F mark as a tiny rig: the middle arm is a bone that waves.
pub fn hello() -> Document {
    let mut doc = Document::new("Hello Fanimate", 1280.0, 720.0, 24.0);
    // two seconds, frames 1 to 48
    doc.end = 48;
    doc.background = palette::VIOLET_950;

    let (cx, cy) = (640.0, 360.0);
    let w = 56.0; // stroke width of the F
    let stem_x = cx - 70.0;
    let joint = Vec2::new(stem_x, cy - 10.0);

    let mut skel = Skeleton::default();
    let arm = skel.add("arm", None, joint, 110.0, 0.0);
    for (frame, angle) in [(1, 0.0), (13, -0.35), (25, 0.0), (37, 0.25), (48, 0.0)] {
        skel.key_angle(arm, frame, angle, Interpolation::Smooth);
    }

    let white = Style::fill(Color::WHITE);
    let shapes = vec![
        Shape::new(
            "stem",
            Path::rect(
                Vec2::new(stem_x - w / 2.0, cy - 220.0),
                Vec2::new(w, 440.0),
                w / 2.0,
            ),
            white,
        ),
        Shape::new(
            "top arm",
            Path::rect(
                Vec2::new(stem_x - w / 2.0, cy - 220.0),
                Vec2::new(210.0, w),
                w / 2.0,
            ),
            white,
        ),
        Shape::new(
            "middle arm",
            Path::rect(
                Vec2::new(stem_x - w / 2.0, joint.y - w / 2.0),
                Vec2::new(170.0, w),
                w / 2.0,
            ),
            white,
        )
        .bound_to(arm),
        Shape::new("joint", Path::circle(joint, w * 0.78), white).bound_to(arm),
        // the pivot dot is see-through in the brand mark, so it matches the background
        Shape::new(
            "pivot",
            Path::circle(joint, w * 0.3),
            Style::fill(palette::VIOLET_950),
        )
        .bound_to(arm),
    ];

    let art = doc.new_layer("F", LayerKind::vector(shapes));
    let rig = doc.new_layer(
        "Bone-F rig",
        LayerKind::Bone {
            skeleton: skel,
            children: vec![art],
        },
    );
    doc.layers.push(rig);
    doc
}

/// The Gate 1 benchmark scene: a creature with `bones` bones split into
/// eight limbs, every bone carrying a segment and a joint shape, every bone
/// keyed through the whole range, and up to ten Smart Bones, spread over
/// the bones, each reshaping the body as its bone turns. The Phase 1 budget
/// is 60 fps with 40 bones and 10 Smart Bones.
pub fn benchmark(bones: usize) -> Document {
    let mut doc = Document::new(format!("Benchmark ({bones} bones)"), 1920.0, 1080.0, 24.0);
    // five seconds, frames 1 to 120
    doc.end = 120;
    doc.background = palette::NIGHT_BG;

    let center = Vec2::new(960.0, 540.0);
    let limbs = 8.min(bones.max(1));
    let mut skel = Skeleton::default();
    let mut limb_shapes = Vec::new();

    let mut made = 0;
    for limb in 0..limbs {
        // spread the bones as evenly as possible across limbs
        let count = bones / limbs + usize::from(limb < bones % limbs);
        let dir = limb as f32 / limbs as f32 * TAU;
        let mut parent = None;
        let mut length = 70.0;
        for seg in 0..count {
            let origin = match parent {
                None => center + Vec2::from_angle(dir) * 90.0,
                Some(_) => Vec2::new(length, 0.0),
            };
            let rest = if parent.is_none() { dir } else { 0.0 };
            length = 70.0 * 0.88_f32.powi(seg as i32);
            let bone = skel.add(format!("limb{limb}.{seg}"), parent, origin, length, rest);

            // wave: each segment lags the one before it
            let phase = limb as f32 * 0.7 + seg as f32 * 0.6;
            let amp = 0.35 + 0.05 * seg as f32;
            let mut frame: Frame = doc.start;
            while frame <= doc.end {
                let t = (frame - doc.start) as f32 / doc.frame_count() as f32 * TAU * 2.0;
                skel.key_angle(bone, frame, rest + amp * (t + phase).sin(), Interpolation::Smooth);
                frame += 6;
            }
            parent = Some(bone);
            made += 1;
        }
    }
    debug_assert_eq!(made, bones);

    // Shapes are drawn in rest pose; compute it to place them.
    let mut rest = Vec::new();
    skel.rest_pose(&mut rest);
    for (i, bone) in skel.bones.iter().enumerate() {
        let depth = std::iter::successors(bone.parent, |&p| skel.bones[p].parent).count();
        let t = (depth as f32 / 6.0).min(1.0);
        let color = Color::lerp(palette::VIOLET_300, palette::VIOLET_600, t);
        let width = 34.0 * 0.85_f32.powi(depth as i32);
        let to_world = |p: Vec2| rest[i].transform_point2(p);

        // segment as a polygon in rest pose world space
        let seg = Path::rect(
            Vec2::new(0.0, -width / 2.0),
            Vec2::new(bone.length, width),
            width / 2.0,
        );
        let seg = Path {
            cmds: seg.cmds.iter().map(|c| map_cmd(*c, to_world)).collect(),
        };
        limb_shapes.push(Shape::new(format!("seg {i}"), seg, Style::fill(color)).bound_to(i));
        limb_shapes.push(
            Shape::new(
                format!("joint {i}"),
                Path::circle(to_world(Vec2::ZERO), width * 0.42),
                Style::fill(palette::KEYFRAME),
            )
            .bound_to(i),
        );
    }

    let body = Shape::new(
        "body",
        Path::circle(center, 110.0),
        Style::fill(palette::VIOLET_500).with_stroke(palette::BONE, 6.0),
    );
    let limbs_layer = doc.new_layer("limbs", LayerKind::vector(limb_shapes));
    let body_layer = doc.new_layer("body", LayerKind::vector(vec![body.clone()]));

    // Smart Bones: each turns from its rest angle to 0.6 more over 24
    // frames (frames 1 to 25 of the action), pushing one of the body's
    // points out
    let smart = 10.min(bones);
    for k in 0..smart {
        let driver = k * bones / smart;
        let rest = skel.bones[driver].angle.base;
        let mut act = crate::actions::RigAction::new(format!("smart {k}"), Some(driver), 25);
        act.bones.push(crate::actions::BoneKeys {
            bone: driver,
            angle: crate::Channel::new(rest)
                .with_key(1, rest, Interpolation::Linear)
                .with_key(25, rest + 0.6, Interpolation::Linear),
        });
        let mut pushed = body.path.clone();
        let point = k % 4;
        let out = pushed.on_curve_points()[point] - center;
        pushed.move_points(&[point].into_iter().collect(), out.normalize_or_zero() * 15.0);
        act.shapes.push(crate::actions::ShapeKeys {
            layer: body_layer.id,
            shape: 0,
            path_keys: vec![
                crate::Key {
                    frame: 1,
                    value: body.path.clone(),
                    interp: Interpolation::Linear,
                },
                crate::Key {
                    frame: 25,
                    value: pushed,
                    interp: Interpolation::Linear,
                },
            ],
        });
        skel.actions.push(act);
    }
    let rig = doc.new_layer(
        "creature",
        LayerKind::Bone {
            skeleton: skel,
            children: vec![limbs_layer, body_layer],
        },
    );
    doc.layers.push(rig);
    doc
}

/// The Gate 1 project-open scene: the benchmark character animated for
/// `seconds` (an episode is 300), its bones keyed every 6 frames all the
/// way through, as a long shot animated by hand would be.
pub fn episode(bones: usize, seconds: u32) -> Document {
    let mut doc = benchmark(bones);
    let period = doc.frame_count() as Frame;
    let frames = (seconds as f32 * doc.fps).round().max(1.0) as Frame;
    doc.name = format!("Episode ({bones} bones, {seconds} s)");
    doc.end = doc.start + frames - 1;
    let end = doc.end;
    for layer in &mut doc.layers {
        if let LayerKind::Bone { skeleton, .. } = &mut layer.kind {
            for bone in &mut skeleton.bones {
                let cycle = bone.angle.keys.clone();
                for repeat in 1..=frames / period {
                    for key in &cycle {
                        let frame = key.frame + repeat * period;
                        if frame <= end {
                            bone.angle.set_key(frame, key.value, key.interp);
                        }
                    }
                }
            }
        }
    }
    doc
}

// Templates: starting points that open as new, unsaved files. Frames count
// from 1, so each runs from frame 1.

const fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// A layer that takes the next label colour, as one made in the app does.
fn labelled(mut layer: crate::scene::Layer) -> crate::scene::Layer {
    layer.label = Some(crate::Label::for_id(layer.id));
    layer
}

/// An empty 1080p file at 24 fps with one vector layer, five seconds long.
pub fn blank() -> Document {
    let mut doc = Document::new("Blank 1080p 24 fps", 1920.0, 1080.0, 24.0);
    doc.start = 1;
    doc.end = 120;
    let layer = labelled(doc.new_layer("Layer 1", LayerKind::vector(vec![])));
    doc.layers.push(layer);
    doc
}

/// A bone of a stick figure drawn as a rounded bar in the rest pose, bound
/// to the bone.
fn limb(name: &str, rest: &[glam::Affine2], bone: usize, length: f32, width: f32, color: Color) -> Shape {
    let bar = Path::rect(
        Vec2::new(0.0, -width / 2.0),
        Vec2::new(length, width),
        width / 2.0,
    );
    let to_world = |p: Vec2| rest[bone].transform_point2(p);
    let path = Path {
        cmds: bar.cmds.iter().map(|c| map_cmd(*c, to_world)).collect(),
    };
    Shape::new(name, path, Style::fill(color)).bound_to(bone)
}

/// A stick figure on twelve bones with a two-step walk, four cycles long.
/// The head, arms and legs are bars bound to their bones; the left side is
/// in front.
pub fn walk_cycle() -> Document {
    use std::f32::consts::{FRAC_PI_2, PI};

    let mut doc = Document::new("Walk cycle rig", 1920.0, 1080.0, 24.0);
    doc.start = 1;
    // four cycles of 24 frames, and the frame that closes the last one
    doc.end = 97;
    doc.background = palette::NIGHT_BG;

    // a child's origin is in its parent's space, x along the parent
    let mut skel = Skeleton::default();
    let hips = Vec2::new(960.0, 640.0);
    let spine = skel.add("spine", None, hips, 170.0, -FRAC_PI_2);
    let head = skel.add("head", Some(spine), Vec2::new(170.0, 0.0), 70.0, 0.0);
    let arm_l = skel.add("arm L", Some(spine), Vec2::new(150.0, 0.0), 100.0, PI);
    let fore_l = skel.add("forearm L", Some(arm_l), Vec2::new(100.0, 0.0), 90.0, 0.0);
    let arm_r = skel.add("arm R", Some(spine), Vec2::new(150.0, 0.0), 100.0, PI);
    let fore_r = skel.add("forearm R", Some(arm_r), Vec2::new(100.0, 0.0), 90.0, 0.0);
    let thigh_l = skel.add("thigh L", Some(spine), Vec2::ZERO, 140.0, PI);
    let shin_l = skel.add("shin L", Some(thigh_l), Vec2::new(140.0, 0.0), 140.0, 0.0);
    let foot_l = skel.add("foot L", Some(shin_l), Vec2::new(140.0, 0.0), 55.0, -FRAC_PI_2);
    let thigh_r = skel.add("thigh R", Some(spine), Vec2::ZERO, 140.0, PI);
    let shin_r = skel.add("shin R", Some(thigh_r), Vec2::new(140.0, 0.0), 140.0, 0.0);
    let foot_r = skel.add("foot R", Some(shin_r), Vec2::new(140.0, 0.0), 55.0, -FRAC_PI_2);

    // How far each bone turns from its rest angle at the quarter points of
    // a 24 frame cycle. A positive turn is clockwise: a thigh swings back,
    // a shin folds back.
    let swing = |amount: f32, quarters: [f32; 4]| quarters.map(|q| q * amount);
    let thigh_a = swing(0.5, [0.0, 1.0, 0.0, -1.0]);
    let thigh_b = swing(0.5, [0.0, -1.0, 0.0, 1.0]);
    let arm_a = swing(0.4, [0.0, 1.0, 0.0, -1.0]);
    let arm_b = swing(0.4, [0.0, -1.0, 0.0, 1.0]);
    let knee_a = [0.15, 0.1, 0.7, 0.15];
    let knee_b = [0.7, 0.15, 0.1, 0.15];
    for step in 0..=16_usize {
        let frame = 1 + 6 * step as Frame;
        let quarter = step % 4;
        let mut key = |bone: usize, offsets: [f32; 4]| {
            let rest = skel.bones[bone].angle.base;
            skel.key_angle(bone, frame, rest + offsets[quarter], Interpolation::Smooth);
        };
        key(thigh_l, thigh_a);
        key(thigh_r, thigh_b);
        key(shin_l, knee_a);
        key(shin_r, knee_b);
        // the arms swing against the legs
        key(arm_l, arm_b);
        key(arm_r, arm_a);
    }

    let mut rest = Vec::new();
    skel.rest_pose(&mut rest);
    let (near, far) = (palette::VIOLET_300, palette::VIOLET_600);
    let bar = |name: &str, bone: usize, width: f32, color: Color| {
        limb(name, &rest, bone, skel.bones[bone].length, width, color)
    };
    // far side first, then the body, then the near side and the head
    let shapes = vec![
        bar("arm R", arm_r, 24.0, far),
        bar("forearm R", fore_r, 20.0, far),
        bar("thigh R", thigh_r, 34.0, far),
        bar("shin R", shin_r, 28.0, far),
        bar("foot R", foot_r, 22.0, far),
        bar("body", spine, 52.0, palette::VIOLET_500),
        bar("thigh L", thigh_l, 34.0, near),
        bar("shin L", shin_l, 28.0, near),
        bar("foot L", foot_l, 22.0, near),
        bar("arm L", arm_l, 24.0, near),
        bar("forearm L", fore_l, 20.0, near),
        bar("head", head, 84.0, palette::VIOLET_400),
    ];

    let ground = doc.new_layer(
        "Ground",
        LayerKind::vector(vec![Shape::new(
            "ground",
            Path::rect(Vec2::new(0.0, 880.0), Vec2::new(1920.0, 200.0), 0.0),
            Style::fill(palette::VIOLET_950),
        )]),
    );
    let figure = doc.new_layer("Figure", LayerKind::vector(shapes));
    let rig = doc.new_layer(
        "Walk rig",
        LayerKind::Bone {
            skeleton: skel,
            children: vec![figure],
        },
    );
    doc.layers.push(labelled(ground));
    doc.layers.push(labelled(rig));
    doc
}

/// A face and a switch layer of mouth shapes named the way Lip Sync looks
/// for them (Rhubarb's A to H and X). With no sound yet, the mouth rests;
/// import a sound, select the switch layer and run Lip Sync.
pub fn lip_sync() -> Document {
    let mut doc = Document::new("Lip sync", 1920.0, 1080.0, 24.0);
    doc.start = 1;
    doc.end = 120;
    doc.background = rgb8(0x2B, 0x2A, 0x3A);

    let skin = Style::fill(rgb8(0xF1, 0xC8, 0x9E));
    let ink = Color::rgb(0.12, 0.08, 0.1);
    let face = doc.new_layer(
        "Face",
        LayerKind::vector(vec![
            Shape::new("head", Path::circle(Vec2::new(960.0, 520.0), 330.0), skin),
            Shape::new(
                "eye L",
                Path::circle(Vec2::new(850.0, 430.0), 30.0),
                Style::fill(ink),
            ),
            Shape::new(
                "eye R",
                Path::circle(Vec2::new(1070.0, 430.0), 30.0),
                Style::fill(ink),
            ),
        ]),
    );

    // name, width, height: each centred on the mouth's place
    const MOUTHS: [(&str, f32, f32); 9] = [
        ("A", 100.0, 10.0),
        ("B", 110.0, 26.0),
        ("C", 100.0, 56.0),
        ("D", 110.0, 96.0),
        ("E", 60.0, 66.0),
        ("F", 44.0, 44.0),
        ("G", 110.0, 22.0),
        ("H", 90.0, 48.0),
        ("X", 90.0, 8.0),
    ];
    let lip = Style::fill(rgb8(0x8E, 0x2F, 0x3C));
    let centre = Vec2::new(960.0, 650.0);
    let mut children = Vec::new();
    for (name, w, h) in MOUTHS {
        let shape = Shape::new(
            format!("mouth {name}"),
            Path::rect(centre - Vec2::new(w, h) / 2.0, Vec2::new(w, h), h.min(w) / 2.0),
            lip,
        );
        children.push(doc.new_layer(name, LayerKind::vector(vec![shape])));
    }
    let mouth = doc.new_layer(
        "Mouth",
        LayerKind::Switch {
            children,
            switch: vec![crate::Key {
                frame: 1,
                value: "X".to_owned(),
                interp: Interpolation::Step,
            }],
        },
    );
    doc.layers.push(labelled(face));
    doc.layers.push(labelled(mouth));
    doc
}

/// A frame-by-frame layer with twelve empty drawings, one every two frames
/// (on twos), which is one second at 24 fps.
pub fn frame_by_frame() -> Document {
    let mut doc = Document::new("Frame by frame", 1920.0, 1080.0, 24.0);
    doc.start = 1;
    doc.end = 24;
    let cels = (0..12).map(|i| crate::Cel::new(1 + 2 * i)).collect();
    let layer = labelled(doc.new_layer("Drawings", LayerKind::Frames { cels }));
    doc.layers.push(layer);
    doc
}

fn map_cmd(c: crate::PathCmd, f: impl Fn(Vec2) -> Vec2) -> crate::PathCmd {
    use crate::PathCmd::*;
    match c {
        MoveTo(p) => MoveTo(f(p)),
        LineTo(p) => LineTo(f(p)),
        CubicTo(a, b, p) => CubicTo(f(a), f(b), f(p)),
        Close => Close,
    }
}

/// A text benchmark: a title of `characters` characters wrapped into a column
/// and animated by two animators (a fade in by character and a wave), so every
/// frame moves, scales and fades every glyph. The budget for the text
/// feature is 60 fps with 1,000 animating glyphs.
pub fn text_benchmark(characters: usize) -> Document {
    const WORDS: [&str; 8] = [
        "Animation",
        "moves",
        "words",
        "one",
        "character",
        "at",
        "a",
        "time",
    ];
    let mut doc = Document::new("Text benchmark", 1920.0, 1080.0, 24.0);
    doc.end = 120;
    doc.background = Color::rgba(0.11, 0.04, 0.24, 1.0);
    let mut content = String::new();
    let mut word = 0;
    while content.chars().count() < characters {
        if !content.is_empty() {
            content.push(' ');
        }
        content.push_str(WORDS[word % WORDS.len()]);
        word += 1;
    }
    let content: String = content.chars().take(characters).collect();
    let mut text = crate::text::Text::new(content);
    {
        let look = text.look_mut();
        look.size = 36.0;
        look.width = Some(1760.0);
        look.fill = Color::WHITE;
    }
    text.animators
        .push(crate::text::TextPreset::FadeIn.animator(1, 60));
    text.animators.push(crate::text::TextPreset::Wave.animator(1, 48));
    let mut layer = doc.new_layer("Title", crate::LayerKind::Text(text));
    layer.transform.position.base = Vec2::new(80.0, 60.0);
    doc.layers.push(layer);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluate;

    #[test]
    fn an_episode_is_keyed_all_the_way_through() {
        let doc = episode(4, 300);
        assert_eq!(doc.frame_count(), 7200);
        let mut keys = 0;
        let mut last = 0;
        doc.walk(|l| {
            if let LayerKind::Bone { skeleton, .. } = &l.kind {
                for b in &skeleton.bones {
                    keys += b.angle.keys.len();
                    last = last.max(b.angle.keys.last().map_or(0, |k| k.frame));
                }
            }
        });
        assert_eq!(keys, 4 * 1200);
        assert_eq!(
            last, 7195,
            "the last key of the last whole cycle, in frames 1 to 7200"
        );
    }

    #[test]
    fn benchmark_has_requested_bones_and_shapes() {
        for n in [1, 7, 40, 100] {
            let doc = benchmark(n);
            let LayerKind::Bone { skeleton, .. } = &doc.layers[0].kind else {
                panic!("expected a bone layer");
            };
            assert_eq!(skeleton.bones.len(), n);
            assert_eq!(evaluate(&doc, 10.0).len(), n * 2 + 1);
            assert_eq!(skeleton.actions.len(), 10.min(n));
        }
        // the Smart Bones reshape the body while their bones turn
        let doc = benchmark(40);
        let rest = Path::circle(Vec2::new(960.0, 540.0), 110.0);
        let changed = (1..=120).any(|f| {
            let items = evaluate(&doc, f as f32);
            items.last().is_some_and(|i| i.shape.path != rest)
        });
        assert!(changed);
    }

    #[test]
    fn templates_start_on_frame_one_and_draw() {
        for (doc, shapes) in [
            (blank(), 0),
            (walk_cycle(), 13),
            (lip_sync(), 4),
            (frame_by_frame(), 0),
        ] {
            assert_eq!(doc.start, 1, "{}", doc.name);
            assert!(doc.end >= doc.start, "{}", doc.name);
            assert!(!doc.layers.is_empty(), "{}", doc.name);
            for frame in [doc.start, (doc.start + doc.end) / 2, doc.end] {
                assert_eq!(
                    evaluate(&doc, frame as f32).len(),
                    shapes,
                    "{} at {frame}",
                    doc.name
                );
            }
        }
    }

    #[test]
    fn the_walk_moves_the_legs_and_comes_back_to_where_it_began() {
        let doc = walk_cycle();
        let at = |f: f32| -> Vec<Vec2> {
            // a shape bound to one bone is moved by its draw transform, not
            // by rewriting its path
            evaluate(&doc, f)
                .iter()
                .flat_map(|i| {
                    let t = i.transform;
                    i.shape
                        .path
                        .on_curve_points()
                        .into_iter()
                        .map(move |p| t.transform_point2(p))
                })
                .collect()
        };
        // the biggest distance any point is apart between two frames
        let apart = |a: f32, b: f32| {
            at(a)
                .iter()
                .zip(at(b))
                .map(|(p, q)| p.distance(q))
                .fold(0.0_f32, f32::max)
        };
        assert!(apart(1.0, 7.0) > 20.0, "the figure moves");
        assert!(apart(1.0, 25.0) < 0.01, "a cycle is 24 frames");
        assert!(apart(1.0, 97.0) < 0.01);
    }

    #[test]
    fn the_lip_sync_template_has_every_mouth_shape() {
        let doc = lip_sync();
        let LayerKind::Switch { children, switch } = &doc.layers[1].kind else {
            panic!("the second layer is the switch layer");
        };
        let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["A", "B", "C", "D", "E", "F", "G", "H", "X"]);
        // it rests on X until a sound is lip synced
        assert_eq!(switch.len(), 1);
        assert_eq!(switch_choice_name(&doc), "X");
    }

    fn switch_choice_name(doc: &Document) -> String {
        let LayerKind::Switch { children, switch } = &doc.layers[1].kind else {
            panic!("not a switch layer");
        };
        let i = crate::scene::switch_choice(children, switch, 1.0).expect("a child shows");
        children[i].name.clone()
    }

    #[test]
    fn the_frame_by_frame_template_has_twelve_empty_drawings() {
        let doc = frame_by_frame();
        let LayerKind::Frames { cels } = &doc.layers[0].kind else {
            panic!("expected a frame-by-frame layer");
        };
        assert_eq!(cels.len(), 12);
        assert!(cels.iter().all(|c| c.is_empty()));
        assert_eq!(cels[0].frame, 1);
        assert!(cels.windows(2).all(|w| w[0].frame < w[1].frame));
    }

    #[test]
    fn hello_evaluates() {
        assert_eq!(evaluate(&hello(), 6.0).len(), 5);
    }
}
