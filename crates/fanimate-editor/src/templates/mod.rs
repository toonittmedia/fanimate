//! Templates built from bundled artwork and speech: four basic ones (a
//! bouncing ball, a rigged character, a talking head, five mouth styles) and
//! four full scenes of a football parody. The art is SVG and PNG under
//! `assets/templates/`, written by the scripts in `scripts/template-art` and
//! `scripts/template-speech`; the documents are assembled here when a template
//! is opened, never stored.

mod art;
mod basics;
mod cast;
mod football;
mod speech;

pub use basics::{bouncing_ball, character_rig, mouth_styles, talking_head};
pub use football::{dressing_room, penalty_shootout, press_conference, pundits};

#[cfg(test)]
mod tests {
    use fanimate_core::{LayerKind, evaluate};

    use super::art::{LETTERS, STYLES, shapes_of};
    use super::cast::LOOK_NAMES;
    use crate::Sample;
    use crate::editor::svg;

    /// The templates made from bundled artwork.
    const BUNDLED: [Sample; 8] = [
        Sample::BouncingBall,
        Sample::CharacterRig,
        Sample::TalkingHead,
        Sample::MouthStyles,
        Sample::DressingRoom,
        Sample::PressConference,
        Sample::PenaltyShootout,
        Sample::Pundits,
    ];

    #[test]
    fn every_mouth_style_has_the_nine_shapes_and_each_draws() {
        for style in &STYLES {
            for (letter, text) in LETTERS.iter().zip(style.svgs) {
                let shapes = shapes_of(letter, text);
                assert!(!shapes.is_empty(), "{} {letter} reads as no shapes", style.name);
                assert!(
                    shapes.iter().all(|s| s.path.points().all(|p| p.is_finite())),
                    "{} {letter} has a point that is not a number",
                    style.name
                );
            }
        }
    }

    #[test]
    fn every_character_has_the_layers_the_rig_binds() {
        const PARTS: [&str; 11] = [
            "legs",
            "body",
            "upper arm B",
            "forearm B",
            "upper arm A",
            "forearm A",
            "head back",
            "face",
            "eyes open",
            "eyes closed",
            "head front",
        ];
        for (name, text) in LOOK_NAMES {
            let names: Vec<String> = svg::bundled(name, text).0.into_iter().map(|(n, _)| n).collect();
            assert_eq!(names, PARTS, "{name}");
        }
    }

    #[test]
    fn the_new_templates_are_clean_projects_that_start_on_frame_one_and_draw() {
        for sample in BUNDLED {
            let doc = sample.document();
            assert_eq!(doc.start, 1, "{}", sample.name());
            assert!(doc.end > doc.start, "{}", sample.name());
            let report = fanimate_fnm::validate(&doc);
            assert!(report.is_clean(), "{}: {report}", sample.name());
            for frame in [doc.start, (doc.start + doc.end) / 2, doc.end] {
                let items = evaluate(&doc, frame as f32);
                assert!(
                    !items.is_empty(),
                    "{} shows nothing on frame {frame}",
                    sample.name()
                );
            }
            // every picture and sound a layer names is in the document
            doc.walk(|l| match &l.kind {
                LayerKind::Image(i) => assert!(doc.media.contains_key(&i.media), "{}", sample.name()),
                LayerKind::Audio(a) => assert!(doc.media.contains_key(&a.media), "{}", sample.name()),
                _ => {}
            });
        }
    }

    #[test]
    fn the_scenes_are_spoken_and_every_mouth_moves() {
        for sample in [
            Sample::TalkingHead,
            Sample::MouthStyles,
            Sample::DressingRoom,
            Sample::PressConference,
            Sample::PenaltyShootout,
            Sample::Pundits,
        ] {
            let doc = sample.document();
            let mut sounds = 0;
            let mut mouths = 0;
            doc.walk(|l| match &l.kind {
                LayerKind::Audio(_) => sounds += 1,
                LayerKind::Switch { children, switch } if l.name == "Mouth" => {
                    mouths += 1;
                    assert!(
                        switch.len() > 8,
                        "{}: a mouth with {} keys does not talk",
                        sample.name(),
                        switch.len()
                    );
                    assert!(
                        switch.windows(2).all(|w| w[0].frame < w[1].frame),
                        "{}",
                        sample.name()
                    );
                    assert!(
                        switch.iter().all(|k| children.iter().any(|c| c.name == k.value)),
                        "{}: a key names a mouth that is not there",
                        sample.name()
                    );
                }
                _ => {}
            });
            assert_eq!(sounds, 1, "{}", sample.name());
            assert!(mouths >= 1, "{}", sample.name());
        }
        // both characters of a scene speak: two rigs, two talking mouths
        for sample in [
            Sample::DressingRoom,
            Sample::PressConference,
            Sample::PenaltyShootout,
            Sample::Pundits,
        ] {
            let doc = sample.document();
            let (mut rigs, mut talking) = (0, 0);
            doc.walk(|l| match &l.kind {
                LayerKind::Bone { .. } => rigs += 1,
                LayerKind::Switch { switch, .. } if l.name == "Mouth" && switch.len() > 8 => talking += 1,
                _ => {}
            });
            assert_eq!((rigs, talking), (2, 2), "{}", sample.name());
        }
    }

    #[test]
    fn the_photo_mouths_include_the_nine_lip_sync_names_and_the_expressions() {
        let doc = Sample::DressingRoom.document();
        let mut names = Vec::new();
        doc.walk(|l| {
            if let (LayerKind::Switch { children, .. }, true) = (&l.kind, l.name == "Mouth") {
                names = children.iter().map(|c| c.name.clone()).collect();
            }
        });
        for letter in LETTERS {
            assert!(names.iter().any(|n| n == letter), "{letter} is missing");
        }
        for extra in ["Smile", "Surprised"] {
            assert!(names.iter().any(|n| n == extra), "{extra} is missing");
        }
    }
}
