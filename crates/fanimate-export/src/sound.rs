//! The project's sound: decoding WAV files and mixing audio layers, for
//! playback in the app and for video export.

use std::path::Path;

use fanimate_core::{Document, Frame, Layer, LayerKind};

/// A sound, decoded to interleaved samples from -1 to 1.
pub struct Decoded {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    /// The sound at `t` seconds, mixed down to (left, right). Silence
    /// outside it.
    pub fn at(&self, t: f64) -> (f32, f32) {
        let pos = t * self.rate as f64;
        if pos < 0.0 {
            return (0.0, 0.0);
        }
        let i = pos as usize;
        if i + 1 >= self.frames() {
            return (0.0, 0.0);
        }
        let frac = (pos - i as f64) as f32;
        let c = self.channels.max(1) as usize;
        let get = |frame: usize, ch: usize| self.samples[frame * c + ch.min(c - 1)];
        let lerp = |ch| get(i, ch) + (get(i + 1, ch) - get(i, ch)) * frac;
        (lerp(0), lerp(1))
    }
}

/// Decode a WAV file. `None` if it cannot be read.
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).ok()?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 * scale)
                .collect()
        }
    };
    (spec.channels > 0 && spec.sample_rate > 0).then_some(Decoded {
        rate: spec.sample_rate,
        channels: spec.channels,
        samples,
    })
}

/// An audio layer that can be heard: shown, inside shown layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub media: String,
    /// The frame the sound starts on.
    pub start: Frame,
    pub volume: f32,
}

/// The audible audio layers of `doc`, bottom of the stack first.
pub fn voices(doc: &Document) -> Vec<Voice> {
    fn walk(layers: &[Layer], out: &mut Vec<Voice>) {
        for l in layers.iter().filter(|l| l.visible) {
            match &l.kind {
                LayerKind::Audio(a) => out.push(Voice {
                    media: a.media.clone(),
                    start: a.start,
                    volume: a.volume,
                }),
                kind => {
                    if let Some(children) = kind.children() {
                        walk(children, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.layers, &mut out);
    out
}

/// The sound of frames `start..=end`, as stereo samples at `rate`, or
/// `None` when nothing is heard there. Frame numbers are the document's own;
/// a sound is placed by its distance from `start`, so the first exported
/// frame is time zero whatever it is numbered.
pub fn mix(doc: &Document, start: Frame, end: Frame, rate: u32) -> Option<Vec<f32>> {
    let fps = doc.fps.max(1.0) as f64;
    let sounds: Vec<(Decoded, f64, f32)> = voices(doc)
        .into_iter()
        .filter_map(|v| {
            let d = decode(&doc.media.get(&v.media)?.bytes)?;
            Some((d, v.start as f64 / fps, v.volume))
        })
        .collect();
    if sounds.is_empty() {
        return None;
    }
    let from = start as f64 / fps;
    let seconds = (end - start + 1).max(0) as f64 / fps;
    let n = (seconds * rate as f64).round() as usize;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = from + i as f64 / rate as f64;
        let (mut l, mut r) = (0.0f32, 0.0f32);
        for (d, offset, volume) in &sounds {
            let (a, b) = d.at(t - offset);
            l += a * volume;
            r += b * volume;
        }
        out.push(l.clamp(-1.0, 1.0));
        out.push(r.clamp(-1.0, 1.0));
    }
    out.iter().any(|s| *s != 0.0).then_some(out)
}

/// Write stereo `samples` at `rate` as a 16-bit WAV file.
pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for s in samples {
        w.write_sample((s * i16::MAX as f32) as i16)?;
    }
    w.finalize()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use fanimate_core::{Audio, MediaFile};
    use std::sync::Arc;

    pub(crate) fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut bytes = Vec::new();
        {
            let mut w = hound::WavWriter::new(std::io::Cursor::new(&mut bytes), spec).unwrap();
            for s in samples {
                w.write_sample(*s).unwrap();
            }
            w.finalize().unwrap();
        }
        bytes
    }

    /// A document with a one-second tone starting on frame `start`.
    pub(crate) fn with_sound(start: Frame) -> Document {
        let mut doc = Document::new("s", 64.0, 32.0, 10.0);
        doc.end = 19;
        let bytes = wav(1000, &[16384; 1000]);
        let name = MediaFile::name_for(&bytes, "wav");
        doc.media.insert(name.clone(), Arc::new(MediaFile { bytes }));
        let mut audio = Audio::new(name);
        audio.start = start;
        let layer = doc.new_layer("line", LayerKind::Audio(audio));
        doc.layers.push(layer);
        doc
    }

    #[test]
    fn decodes_and_samples_between_frames() {
        let d = decode(&wav(100, &[0, 16384, 0, -16384])).unwrap();
        assert_eq!(d.frames(), 4);
        let (l, r) = d.at(0.005);
        assert!((l - 0.25).abs() < 1e-3 && l == r);
        assert_eq!(d.at(-1.0), (0.0, 0.0));
        assert_eq!(d.at(10.0), (0.0, 0.0));
        assert!(decode(b"nope").is_none());
    }

    #[test]
    fn mixes_from_the_layer_start_and_skips_hidden_layers() {
        let doc = with_sound(5);
        // 10 fps, 100 samples a second: frame 5 is sample 50
        let samples = mix(&doc, 0, 19, 100).unwrap();
        assert_eq!(samples.len(), 2 * 200);
        assert_eq!(samples[2 * 49], 0.0);
        assert!((samples[2 * 51] - 0.5).abs() < 1e-3);
        let mut hidden = doc.clone();
        hidden.layers[0].visible = false;
        assert!(mix(&hidden, 0, 19, 100).is_none());
        // a range without the sound in it is silent
        assert!(mix(&doc, 16, 19, 100).is_none());
    }
}
