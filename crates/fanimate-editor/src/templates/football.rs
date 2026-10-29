//! Four full scenes of a football parody: a dressing room, a press
//! conference, a penalty shootout and a studio of pundits. Each has a set
//! drawn in SVG, cartoon characters on bone rigs, a line of dialogue spoken by
//! the computer's voices and mouth shapes keyed to it frame by frame (the
//! photo-real set), so they play the moment they open and show what a finished
//! scene is made of. Every character is made up.

use fanimate_core::{Document, Frame, Interpolation, Layer, Vec2};

use super::art::set_layers;
use super::cast::{Mouth, Stand, actor};
use super::speech::Speech;

macro_rules! scene_files {
    ($name:literal) => {
        (
            include_str!(concat!("../../assets/templates/scenes/", $name, "-back.svg")),
            include_str!(concat!("../../assets/templates/scenes/", $name, "-front.svg")),
            Speech::new(
                include_str!(concat!("../../assets/templates/speech/", $name, ".json")),
                include_bytes!(concat!("../../assets/templates/speech/", $name, ".wav")),
            ),
        )
    };
}

/// A character of a scene: who it is (the name its lines are filed under in the
/// speech) and where it stands.
struct Player {
    who: &'static str,
    stand: Stand,
}

/// The frame the sound and the scene start on.
const START: Frame = 1;
/// Seconds of quiet after the last word.
const TAIL: f32 = 1.5;

fn stand(x: f32, y: f32, scale: f32) -> Stand {
    Stand {
        feet: Vec2::new(x, y),
        scale,
    }
}

/// Put a scene together: the set, the sound, the cast back to front, then
/// what stands in front of them.
fn scene(
    name: &str,
    (back, front, speech): (&str, &str, Speech),
    players: &[Player],
    seed: u32,
    finish: impl FnOnce(&mut Document, &mut [Layer], Frame),
) -> Document {
    let mut doc = Document::new(name, 1920.0, 1080.0, 24.0);
    doc.start = START;
    doc.end = speech.end_frame(START, doc.fps, TAIL);
    let end = doc.end;
    doc.background = fanimate_core::Color::rgb(0.1, 0.1, 0.12);

    let mut layers = set_layers(&mut doc, &format!("{name} set"), back);
    let sound = speech.audio_layer(&mut doc, "Dialogue", START);
    layers.push(sound);
    for (i, player) in players.iter().enumerate() {
        let mut a = actor(&mut doc, player.who, Mouth::Photo, player.stand);
        let who = player.who;
        a.lip_sync(&speech.cues_for(who), START, doc.fps);
        // the punchline lands: the one who delivers it smiles, the others gasp
        if let Some(last) = speech.script.lines.last() {
            let frame = |t: f32| START + (t * doc.fps).round() as Frame;
            if last.who == who {
                a.react("Smile", frame(last.end) + 3, end);
            } else {
                a.react("Surprised", frame(last.start) + 10, frame(last.start) + 40);
            }
        }
        a.blink(seed + 11 * i as u32, end, doc.fps);
        a.act(
            seed + 31 * i as u32,
            end,
            &speech.spans_for(who, START, doc.fps),
            &speech.spans_not(who, START, doc.fps),
        );
        layers.push(a.rig);
    }
    let mut front_layers = set_layers(&mut doc, &format!("{name} front"), front);
    finish(&mut doc, &mut front_layers, end);
    layers.extend(front_layers);
    doc.layers = layers;

    // a slow push in, so even a still moment is alive
    doc.camera.zoom.set_key(START, 1.0, Interpolation::Smooth);
    doc.camera.zoom.set_key(end, 1.05, Interpolation::Smooth);
    doc
}

/// The gaffer shouts at his squad at half time.
pub fn dressing_room() -> Document {
    scene(
        "Dressing room",
        scene_files!("dressing-room"),
        &[
            Player {
                who: "gaffer",
                stand: stand(600.0, 1010.0, 1.0),
            },
            Player {
                who: "striker",
                stand: stand(1330.0, 1030.0, 0.92),
            },
        ],
        101,
        |_, _, _| {},
    )
}

/// A manager answers a reporter after a heavy defeat; the cameras flash.
pub fn press_conference() -> Document {
    scene(
        "Press conference",
        scene_files!("press-conference"),
        &[
            Player {
                who: "manager",
                stand: stand(700.0, 1060.0, 1.0),
            },
            Player {
                who: "reporter",
                stand: stand(1560.0, 1050.0, 0.95),
            },
        ],
        202,
        |_, front: &mut [Layer], end| {
            // white flashes a few times, mostly while the reporter asks
            for layer in front.iter_mut().filter(|l| l.name == "Flash") {
                layer.opacity.base = 0.0;
                let mut f = 20;
                while f + 6 < end {
                    for (frame, value) in [(f, 0.0), (f + 1, 0.7), (f + 4, 0.0)] {
                        layer.opacity.set_key(frame, value, Interpolation::Linear);
                    }
                    f += 71;
                }
            }
        },
    )
}

/// A striker and a keeper trade insults before a penalty.
pub fn penalty_shootout() -> Document {
    scene(
        "Penalty shootout",
        scene_files!("penalty-shootout"),
        &[
            Player {
                who: "keeper",
                stand: stand(960.0, 670.0, 0.5),
            },
            Player {
                who: "striker",
                stand: stand(360.0, 1050.0, 0.85),
            },
        ],
        303,
        |_, _, _| {},
    )
}

/// Two pundits argue over a tackle in the studio.
pub fn pundits() -> Document {
    scene(
        "Studio pundits",
        scene_files!("pundits"),
        &[
            Player {
                who: "presenter",
                stand: stand(630.0, 1070.0, 1.0),
            },
            Player {
                who: "pundit",
                stand: stand(1290.0, 1070.0, 1.0),
            },
        ],
        404,
        |_, _, _| {},
    )
}
