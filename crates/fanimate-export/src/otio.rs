//! OpenTimelineIO: the project's timing as a timeline an editing or
//! finishing program can open (DaVinci Resolve, Premiere through an adapter,
//! Nuke Studio, Hiero and others).
//!
//! The timeline is the film's edit, not its drawings: one video track with a
//! clip for the rendered animation (a reference to a movie the user renders
//! from Fanimate, or a missing reference when none is named) and one audio
//! track per sound layer, with each sound placed where it starts, trimmed
//! to the project's frames, and referencing its own WAV file, which the
//! caller writes beside the timeline from [`Otio::media`]. Drawings, bones
//! and keys do not travel: OTIO describes editorial timing, so those are
//! counted in the [`Report`] as not carried.
//!
//! The JSON follows OpenTimelineIO's schema (`Timeline.1`, `Stack.1`,
//! `Track.1`, `Clip.1`, `Gap.1`, `ExternalReference.1`, `RationalTime.1`,
//! `TimeRange.1`), which every reader of OTIO 0.14 and later accepts.

use fanimate_core::{Document, Frame, Layer, LayerKind};
use serde_json::{Value, json};

use crate::report::Report;
use crate::sound;

/// Settings for [`timeline`].
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The movie the video clip points at, as the program that opens the
    /// timeline will find it (a path or `file://` URL). `None`: the clip's
    /// media is missing, to be linked there.
    pub video: Option<String>,
    /// The folder the audio files are looked for in, relative to the
    /// timeline file. Defaults to `media`.
    pub media_dir: Option<String>,
}

/// What [`timeline`] made.
#[derive(Debug)]
pub struct Otio {
    /// The OpenTimelineIO JSON.
    pub json: Value,
    /// The WAV files the timeline names (file name, bytes), to be written
    /// into the media folder beside it.
    pub media: Vec<(String, Vec<u8>)>,
    pub report: Report,
}

fn time(value: f64, rate: f64) -> Value {
    json!({"OTIO_SCHEMA": "RationalTime.1", "rate": rate, "value": value})
}

fn range(start: f64, duration: f64, rate: f64) -> Value {
    json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "start_time": time(start, rate),
        "duration": time(duration, rate),
    })
}

/// One audible sound layer: its name, file, where it starts and its volume.
struct Sound {
    name: String,
    media: String,
    start: Frame,
    volume: f32,
}

fn sounds(layers: &[Layer], shown: bool, out: &mut Vec<Sound>, hidden: &mut usize) {
    for l in layers {
        let visible = shown && l.visible;
        match &l.kind {
            LayerKind::Audio(a) if visible => out.push(Sound {
                name: l.name.clone(),
                media: a.media.clone(),
                start: a.start,
                volume: a.volume,
            }),
            LayerKind::Audio(_) => *hidden += 1,
            kind => {
                if let Some(children) = kind.children() {
                    sounds(children, visible, out, hidden);
                }
            }
        }
    }
}

/// The timeline of `doc`.
pub fn timeline(doc: &Document, options: &Options) -> Otio {
    let mut report = Report::default();
    let fps = f64::from(doc.fps.max(1.0));
    let count = (i64::from(doc.end) - i64::from(doc.start) + 1).max(1) as f64;
    let media_dir = options.media_dir.clone().unwrap_or_else(|| "media".to_owned());

    let reference = match &options.video {
        Some(url) => json!({
            "OTIO_SCHEMA": "ExternalReference.1",
            "name": "",
            "target_url": url,
            "available_range": range(0.0, count, fps),
            "metadata": {},
        }),
        None => json!({"OTIO_SCHEMA": "MissingReference.1", "name": "", "metadata": {}}),
    };
    let video = json!({
        "OTIO_SCHEMA": "Track.1",
        "name": "Animation",
        "kind": "Video",
        "source_range": null,
        "metadata": {},
        "children": [{
            "OTIO_SCHEMA": "Clip.1",
            "name": doc.name,
            "source_range": range(0.0, count, fps),
            "media_reference": reference,
            "metadata": {"fanimate": {"first_frame": doc.start, "last_frame": doc.end}},
        }],
    });
    let mut tracks = vec![video];

    let mut found = Vec::new();
    let mut hidden = 0;
    sounds(&doc.layers, true, &mut found, &mut hidden);
    report.add_many("hidden sound layers left out", hidden);
    let mut media: Vec<(String, Vec<u8>)> = Vec::new();
    // bottom of the stack first, as the audio mix does
    for s in found {
        let Some(file) = doc.media.get(&s.media) else {
            report.add("sounds with no audio file left out");
            continue;
        };
        let Some(decoded) = sound::decode(&file.bytes) else {
            report.add("sounds that could not be read left out");
            continue;
        };
        let rate = f64::from(decoded.rate);
        let length = decoded.frames() as f64 / rate;
        // seconds from the first frame of the project to where the sound starts
        let offset = (f64::from(s.start) - f64::from(doc.start)) / fps;
        let total = count / fps;
        let (lead_gap, trim_start) = if offset >= 0.0 {
            (offset, 0.0)
        } else {
            (0.0, -offset)
        };
        let remaining = (length - trim_start).max(0.0);
        let room = (total - lead_gap).max(0.0);
        let shown = remaining.min(room);
        if shown <= 0.0 {
            report.add("sounds outside the project's frames left out");
            continue;
        }
        let mut children = Vec::new();
        if lead_gap > 0.0 {
            children.push(json!({
                "OTIO_SCHEMA": "Gap.1",
                "name": "",
                "source_range": range(0.0, (lead_gap * fps).round(), fps),
                "metadata": {},
            }));
        }
        children.push(json!({
            "OTIO_SCHEMA": "Clip.1",
            "name": s.name,
            "source_range": range((trim_start * rate).round(), (shown * rate).round(), rate),
            "media_reference": {
                "OTIO_SCHEMA": "ExternalReference.1",
                "name": "",
                "target_url": format!("{media_dir}/{}", s.media),
                "available_range": range(0.0, decoded.frames() as f64, rate),
                "metadata": {},
            },
            "metadata": {"fanimate": {"volume": s.volume}},
        }));
        if (s.volume - 1.0).abs() > 1.0e-6 {
            report.add("sound volumes kept as notes only (OTIO has no volume)");
        }
        if !media.iter().any(|(n, _)| *n == s.media) {
            media.push((s.media.clone(), file.bytes.clone()));
        }
        tracks.push(json!({
            "OTIO_SCHEMA": "Track.1",
            "name": s.name,
            "kind": "Audio",
            "source_range": null,
            "metadata": {},
            "children": children,
        }));
    }

    let mut drawn = 0usize;
    doc.walk(|l| {
        if !matches!(l.kind, LayerKind::Audio(_)) && l.kind.children().is_none() {
            drawn += 1;
        }
    });
    report.add_many(
        "layers of artwork not carried (a timeline holds timing, not drawings)",
        drawn,
    );

    let json = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": doc.name,
        "global_start_time": time(f64::from(doc.start), fps),
        "metadata": {"fanimate": {"width": doc.width, "height": doc.height}},
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "name": "tracks",
            "source_range": null,
            "metadata": {},
            "children": tracks,
        },
    });
    Otio { json, media, report }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::media::{Audio, MediaFile};
    use std::sync::Arc;

    /// A mono WAV of `seconds` of silence at 8000 Hz.
    fn wav(seconds: f32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut bytes), spec).unwrap();
            for _ in 0..(8000.0 * seconds) as usize {
                w.write_sample(0i16).unwrap();
            }
            w.finalize().unwrap();
        }
        bytes
    }

    fn doc_with_sound(start: Frame, seconds: f32) -> Document {
        let mut doc = Document::new("Film", 1920.0, 1080.0, 24.0);
        doc.start = 1;
        doc.end = 48; // two seconds
        let bytes = wav(seconds);
        let name = MediaFile::name_for(&bytes, "wav");
        doc.media.insert(name.clone(), Arc::new(MediaFile { bytes }));
        let mut audio = Audio::new(name);
        audio.start = start;
        let layer = doc.new_layer("Dialogue", LayerKind::Audio(audio));
        doc.layers.push(layer);
        doc
    }

    fn tracks(o: &Otio) -> &Vec<Value> {
        o.json["tracks"]["children"].as_array().unwrap()
    }

    #[test]
    fn the_timeline_has_the_projects_rate_start_and_a_video_clip() {
        let doc = Document::new("Film", 1920.0, 1080.0, 24.0);
        let o = timeline(&doc, &Options::default());
        assert_eq!(o.json["OTIO_SCHEMA"], "Timeline.1");
        assert_eq!(o.json["global_start_time"]["rate"], 24.0);
        assert_eq!(o.json["global_start_time"]["value"], 1.0);
        let video = &tracks(&o)[0];
        assert_eq!(video["kind"], "Video");
        let clip = &video["children"][0];
        assert_eq!(
            clip["source_range"]["duration"]["value"],
            f64::from(doc.end - doc.start + 1)
        );
        assert_eq!(clip["media_reference"]["OTIO_SCHEMA"], "MissingReference.1");
        // naming the movie links it
        let linked = timeline(
            &doc,
            &Options {
                video: Some("film.mp4".into()),
                ..Options::default()
            },
        );
        let reference = &tracks(&linked)[0]["children"][0]["media_reference"];
        assert_eq!(reference["target_url"], "film.mp4");
        assert_eq!(
            reference["available_range"]["duration"]["value"],
            f64::from(doc.end - doc.start + 1)
        );
    }

    #[test]
    fn a_sound_is_placed_after_a_gap_and_carries_its_file() {
        // starts on frame 13: half a second (12 frames) into the project
        let doc = doc_with_sound(13, 1.0);
        let o = timeline(&doc, &Options::default());
        assert_eq!(tracks(&o).len(), 2);
        let audio = &tracks(&o)[1];
        assert_eq!(audio["kind"], "Audio");
        let children = audio["children"].as_array().unwrap();
        assert_eq!(children[0]["OTIO_SCHEMA"], "Gap.1");
        assert_eq!(children[0]["source_range"]["duration"]["value"], 12.0);
        assert_eq!(children[0]["source_range"]["duration"]["rate"], 24.0);
        let clip = &children[1];
        assert_eq!(clip["name"], "Dialogue");
        // the whole second of sound, in samples
        assert_eq!(clip["source_range"]["duration"]["value"], 8000.0);
        assert_eq!(clip["source_range"]["duration"]["rate"], 8000.0);
        let url = clip["media_reference"]["target_url"].as_str().unwrap();
        assert!(url.starts_with("media/") && url.ends_with(".wav"), "{url}");
        assert_eq!(o.media.len(), 1);
        assert!(url.ends_with(&o.media[0].0));
    }

    #[test]
    fn a_sound_that_starts_early_is_trimmed_and_one_that_runs_long_is_cut_at_the_end() {
        // starts a quarter second before the project (6 frames before frame 1)
        let early = timeline(&doc_with_sound(-5, 1.0), &Options::default());
        let clip = &tracks(&early)[1]["children"][0];
        assert_eq!(clip["source_range"]["start_time"]["value"], 2000.0);
        assert_eq!(clip["source_range"]["duration"]["value"], 6000.0);
        // a long sound from frame 25 (one second in) is cut to the second that is left
        let long = timeline(&doc_with_sound(25, 10.0), &Options::default());
        let clip = &tracks(&long)[1]["children"][1];
        assert_eq!(clip["source_range"]["duration"]["value"], 8000.0);
    }

    #[test]
    fn what_cannot_be_carried_is_reported_and_hidden_or_missing_sounds_are_left_out() {
        let mut doc = doc_with_sound(1, 1.0);
        if let LayerKind::Audio(a) = &mut doc.layers[0].kind {
            a.volume = 0.5;
        }
        let drawn = doc.new_layer("art", LayerKind::vector(vec![]));
        doc.layers.push(drawn);
        let o = timeline(&doc, &Options::default());
        assert_eq!(
            o.report
                .count("sound volumes kept as notes only (OTIO has no volume)"),
            1
        );
        assert_eq!(
            o.report
                .count("layers of artwork not carried (a timeline holds timing, not drawings)"),
            1
        );
        assert_eq!(
            tracks(&o)[1]["children"][0]["metadata"]["fanimate"]["volume"],
            0.5
        );
        // hidden
        doc.layers[0].visible = false;
        let hidden = timeline(&doc, &Options::default());
        assert_eq!(tracks(&hidden).len(), 1);
        assert_eq!(hidden.report.count("hidden sound layers left out"), 1);
        // a sound whose file is not in the project
        let mut broken = doc_with_sound(1, 1.0);
        broken.media.clear();
        let o = timeline(&broken, &Options::default());
        assert_eq!(tracks(&o).len(), 1);
        assert_eq!(o.report.count("sounds with no audio file left out"), 1);
        // a sound entirely after the end
        let late = timeline(&doc_with_sound(500, 1.0), &Options::default());
        assert_eq!(
            late.report.count("sounds outside the project's frames left out"),
            1
        );
    }
}
