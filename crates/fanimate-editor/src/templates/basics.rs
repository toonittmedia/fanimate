//! Four basic templates built from the same artwork: a bouncing ball, a rigged
//! character at rest, a talking head with spoken dialogue, and the five mouth
//! styles lip syncing side by side.

use fanimate_core::{Color, Document, Frame, Interpolation, LayerKind, Path, Shape, Style, Vec2};

use super::art::backdrop;
use super::cast::{Mouth, Stand, actor};
use super::speech::Speech;
use crate::editor::labelled;

const START: Frame = 1;

fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0)
}

/// A ball that falls and bounces lower each time, squashing as it lands and
/// stretching as it moves fast, with a shadow that shrinks as it rises: the
/// classic first exercise, keyed on the ball's position and scale.
pub fn bouncing_ball() -> Document {
    let mut doc = Document::new("Bouncing ball", 1920.0, 1080.0, 24.0);
    doc.start = START;
    doc.end = 64;
    doc.background = rgb8(0xDB, 0xE9, 0xF2);

    let floor_y = 880.0;
    let x = 960.0;
    let radius = 90.0;
    let backdrop_layer = backdrop(
        &mut doc,
        "Wall",
        Vec2::ZERO,
        Vec2::new(1920.0, floor_y),
        rgb8(0xDB, 0xE9, 0xF2),
    );
    let floor = backdrop(
        &mut doc,
        "Floor",
        Vec2::new(0.0, floor_y),
        Vec2::new(1920.0, 200.0),
        rgb8(0xB9, 0x8E, 0x6B),
    );

    let mut shadow = labelled(doc.new_layer(
        "Shadow",
        LayerKind::vector(vec![Shape::new(
            "shadow",
            Path::ellipse(Vec2::ZERO, Vec2::new(radius * 1.1, radius * 0.22)),
            Style::fill(Color::rgba(0.2, 0.12, 0.08, 0.35)),
        )]),
    ));
    shadow.transform.position.base = Vec2::new(x, floor_y + 16.0);

    // the ball is drawn with its lowest point on the layer's origin, so squashing
    // it squashes it into the floor
    let mut ball = labelled(doc.new_layer(
        "Ball",
        LayerKind::vector(vec![
            Shape::new(
                "ball",
                Path::circle(Vec2::new(0.0, -radius), radius),
                Style::fill(rgb8(0xE6, 0x4B, 0x4B)),
            ),
            Shape::new(
                "stripe",
                Path::rect(
                    Vec2::new(-radius * 0.9, -radius - 14.0),
                    Vec2::new(radius * 1.8, 28.0),
                    14.0,
                ),
                Style::fill(rgb8(0xFF, 0xFF, 0xFF)),
            ),
            Shape::new(
                "shine",
                Path::circle(Vec2::new(-radius * 0.38, -radius * 1.5), radius * 0.2),
                Style::fill(Color::rgba(1.0, 1.0, 1.0, 0.55)),
            ),
        ]),
    ));

    // (apex frame, contact frame, height): each bounce is lower and quicker
    let bounces: [(Frame, Frame, f32); 4] =
        [(1, 13, 640.0), (22, 31, 370.0), (37, 43, 180.0), (47, 51, 65.0)];
    let normal = Vec2::ONE;
    ball.transform.scale.set_key(START, normal, Interpolation::Linear);
    let mut shadow_scale = Vec::new();
    for (apex, contact, height) in bounces {
        let fall = (contact - apex) as f32;
        // falling: the distance goes with the square of the time
        for f in apex..=contact {
            let t = (f - apex) as f32 / fall;
            let up = height * (1.0 - t * t);
            ball.transform
                .position
                .set_key(f, Vec2::new(x, floor_y - up), Interpolation::Linear);
            shadow_scale.push((f, 1.0 - 0.55 * (up / 640.0)));
        }
    }
    // rising after each contact mirrors the next fall, to the next apex
    for pair in bounces.windows(2) {
        let (_, contact, _) = pair[0];
        let (apex, _, height) = pair[1];
        let rise = (apex - contact) as f32;
        for f in contact..=apex {
            let t = (apex - f) as f32 / rise;
            let up = height * (1.0 - t * t);
            ball.transform
                .position
                .set_key(f, Vec2::new(x, floor_y - up), Interpolation::Linear);
            shadow_scale.push((f, 1.0 - 0.55 * (up / 640.0)));
        }
    }
    // the last contact, then a little roll to rest
    ball.transform
        .position
        .set_key(doc.end, Vec2::new(x, floor_y), Interpolation::Linear);
    shadow_scale.push((doc.end, 1.0));
    for (frame, s) in shadow_scale {
        shadow
            .transform
            .scale
            .set_key(frame, Vec2::splat(s), Interpolation::Linear);
    }
    // squash on every contact, stretch just before and after
    for (_, contact, height) in bounces {
        let hard = (height / 640.0).sqrt();
        let (squash, stretch) = (0.34 * hard + 0.08, 0.2 * hard + 0.04);
        for (frame, scale) in [
            (contact - 2, Vec2::new(1.0 - stretch * 0.5, 1.0 + stretch)),
            (contact - 1, Vec2::new(1.0 - stretch * 0.5, 1.0 + stretch)),
            (contact, Vec2::new(1.0 + squash, 1.0 - squash)),
            (contact + 1, Vec2::new(1.0 - stretch * 0.5, 1.0 + stretch)),
            (contact + 3, normal),
        ] {
            ball.transform.scale.set_key(frame, scale, Interpolation::Linear);
        }
    }
    ball.transform
        .scale
        .set_key(doc.end, normal, Interpolation::Linear);

    doc.layers.extend([backdrop_layer, floor, shadow, ball]);
    doc
}

/// A cartoon character on six bones, at rest: breathing, blinking, nodding now
/// and then, with a switch layer of mouth shapes ready for Lip Sync.
pub fn character_rig() -> Document {
    let mut doc = Document::new("Character rig", 1920.0, 1080.0, 24.0);
    doc.start = START;
    doc.end = 145;
    doc.background = rgb8(0xF2, 0xE6, 0xD2);
    let wall = backdrop(
        &mut doc,
        "Wall",
        Vec2::ZERO,
        Vec2::new(1920.0, 880.0),
        rgb8(0xF2, 0xE6, 0xD2),
    );
    let floor = backdrop(
        &mut doc,
        "Floor",
        Vec2::new(0.0, 880.0),
        Vec2::new(1920.0, 200.0),
        rgb8(0xC9, 0xA9, 0x82),
    );
    let mut a = actor(
        &mut doc,
        "buddy",
        Mouth::Style(0),
        Stand {
            feet: Vec2::new(960.0, 1040.0),
            scale: 1.0,
        },
    );
    a.blink(5, doc.end, doc.fps);
    // slow nods every few seconds, as if listening
    a.act(9, doc.end, &[], &[(20, 40), (80, 100)]);
    doc.layers.extend([wall, floor, a.rig]);
    doc
}

/// One character, a spoken line and the Classic mouth style: a talking head.
pub fn talking_head() -> Document {
    let speech = Speech::new(
        include_str!("../../assets/templates/speech/talking-head.json"),
        include_bytes!("../../assets/templates/speech/talking-head.wav"),
    );
    let mut doc = Document::new("Talking head", 1920.0, 1080.0, 24.0);
    doc.start = START;
    doc.end = speech.end_frame(START, doc.fps, 1.0);
    doc.background = rgb8(0x3B, 0x6E, 0x8F);
    let wall = backdrop(
        &mut doc,
        "Wall",
        Vec2::ZERO,
        Vec2::new(1920.0, 1080.0),
        rgb8(0x3B, 0x6E, 0x8F),
    );
    let sound = speech.audio_layer(&mut doc, "Dialogue", START);
    let mut a = actor(
        &mut doc,
        "buddy",
        Mouth::Style(0),
        Stand {
            feet: Vec2::new(960.0, 1340.0),
            scale: 1.4,
        },
    );
    a.lip_sync(&speech.cues_for("head"), START, doc.fps);
    a.blink(21, doc.end, doc.fps);
    a.act(33, doc.end, &speech.spans_for("head", START, doc.fps), &[]);
    doc.layers.extend([wall, sound, a.rig]);
    doc
}

/// Five faces, five mouth styles and one voice: the same line, lip synced
/// the same way, so the styles can be compared and the one wanted copied out.
pub fn mouth_styles() -> Document {
    let speech = Speech::new(
        include_str!("../../assets/templates/speech/mouth-styles.json"),
        include_bytes!("../../assets/templates/speech/mouth-styles.wav"),
    );
    let mut doc = Document::new("Mouth styles", 1920.0, 1080.0, 24.0);
    doc.start = START;
    doc.end = speech.end_frame(START, doc.fps, 1.0);
    doc.background = rgb8(0x24, 0x20, 0x33);

    let panel_colours = [
        rgb8(0x5B, 0x8D, 0xC9),
        rgb8(0xD9, 0x6F, 0x6F),
        rgb8(0x5F, 0xB3, 0x8B),
        rgb8(0xE0, 0xB4, 0x5A),
        rgb8(0x9A, 0x7B, 0xD1),
    ];
    let looks = ["presenter", "striker", "keeper", "pundit", "buddy"];
    let mut layers = Vec::new();
    for (i, colour) in panel_colours.iter().enumerate() {
        layers.push(backdrop(
            &mut doc,
            &format!("Panel {}", i + 1),
            Vec2::new(i as f32 * 384.0, 0.0),
            Vec2::new(384.0, 1080.0),
            *colour,
        ));
    }
    layers.push(speech.audio_layer(&mut doc, "Dialogue", START));
    for (i, look) in looks.iter().enumerate() {
        let mut a = actor(
            &mut doc,
            look,
            Mouth::Style(i),
            Stand {
                feet: Vec2::new(192.0 + i as f32 * 384.0, 1010.0),
                scale: 0.78,
            },
        );
        a.rig.name = format!("{} ({})", super::art::STYLES[i].name, look);
        a.lip_sync(&speech.cues_for("head"), START, doc.fps);
        a.blink(40 + i as u32, doc.end, doc.fps);
        a.act(
            60 + i as u32,
            doc.end,
            &speech.spans_for("head", START, doc.fps),
            &[],
        );
        layers.push(a.rig);
    }
    doc.layers.extend(layers);
    doc
}
