//! The spoken dialogue of the templates: a WAV made with the computer's own
//! voices and the mouth cues worked out from them (`scripts/template-speech`).

use std::sync::Arc;

use fanimate_core::{Audio, Document, Frame, Layer, LayerKind, MediaFile};
use serde::Deserialize;

use crate::editor::{MouthCue, labelled};

/// One spoken line.
#[derive(Debug, Deserialize)]
pub struct Line {
    pub who: String,
    /// Seconds from the start of the sound.
    pub start: f32,
    pub end: f32,
    /// Mouth cues, in seconds from the start of the line.
    pub cues: Vec<MouthCue>,
}

#[derive(Debug, Deserialize)]
pub struct Script {
    pub seconds: f32,
    pub lines: Vec<Line>,
}

/// A scene's sound and what is said in it.
pub struct Speech {
    pub script: Script,
    wav: &'static [u8],
}

impl Speech {
    pub fn new(json: &str, wav: &'static [u8]) -> Self {
        // the files ship with Fanimate and a test reads each one; a damaged
        // one gives a silent scene rather than a failed template
        let script = serde_json::from_str(json).unwrap_or(Script {
            seconds: 0.0,
            lines: Vec::new(),
        });
        Self { script, wav }
    }

    /// The last frame of a document that holds this sound from frame
    /// `start`, with `tail` seconds of quiet after it.
    pub fn end_frame(&self, start: Frame, fps: f32, tail: f32) -> Frame {
        start + ((self.script.seconds + tail) * fps).ceil() as Frame
    }

    /// The sound as an audio layer starting on `start`; its WAV joins the
    /// document's media.
    pub fn audio_layer(&self, doc: &mut Document, name: &str, start: Frame) -> Layer {
        let media = MediaFile::name_for(self.wav, "wav");
        doc.media.entry(media.clone()).or_insert_with(|| {
            Arc::new(MediaFile {
                bytes: self.wav.to_vec(),
            })
        });
        let mut audio = Audio::new(media);
        audio.start = start;
        labelled(doc.new_layer(name, LayerKind::Audio(audio)))
    }

    /// Every cue of one speaker, in seconds from the start of the sound, each
    /// line ending with the mouth at rest.
    pub fn cues_for(&self, who: &str) -> Vec<MouthCue> {
        let mut out = Vec::new();
        for line in self.script.lines.iter().filter(|l| l.who == who) {
            out.extend(line.cues.iter().map(|c| MouthCue {
                start: line.start + c.start,
                value: c.value.clone(),
            }));
            out.push(MouthCue {
                start: line.end,
                value: "X".to_owned(),
            });
        }
        out
    }

    /// The spans of one speaker's lines as frames, for the body to act on.
    pub fn spans_for(&self, who: &str, start: Frame, fps: f32) -> Vec<(Frame, Frame)> {
        self.script
            .lines
            .iter()
            .filter(|l| l.who == who)
            .map(|l| {
                (
                    start + (l.start * fps).round() as Frame,
                    start + (l.end * fps).round() as Frame,
                )
            })
            .collect()
    }

    /// The spans of every other speaker's lines.
    pub fn spans_not(&self, who: &str, start: Frame, fps: f32) -> Vec<(Frame, Frame)> {
        self.script
            .lines
            .iter()
            .filter(|l| l.who != who)
            .map(|l| {
                (
                    start + (l.start * fps).round() as Frame,
                    start + (l.end * fps).round() as Frame,
                )
            })
            .collect()
    }
}
