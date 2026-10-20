//! Rendering a project's frames and writing them out.
//!
//! Every format writes to a `.part` file (or one per frame) beside the
//! output and renames it into place only once complete, so a cancelled or
//! failed export never leaves a broken file under the name asked for, and
//! never replaces a good one.

use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use fanimate_core::Document;
use fanimate_render::{Gpu, Offscreen};

use crate::ffmpeg::{self, Ffmpeg};
use crate::job::{Format, Settings, Writer};
use crate::sound;

/// The sound's sample rate in exported video.
pub(crate) const SOUND_RATE: u32 = 48_000;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(
        "{} needs FFmpeg, a free program Fanimate does not include: set it up in Edit › Preferences › Video formats",
        .0.label()
    )]
    NoFfmpeg(Format),
    #[error("the FFmpeg chosen (version {version}) cannot make {}; try a full build of FFmpeg", .format.label())]
    FfmpegLacks { format: Format, version: String },
    #[error("the export was cancelled")]
    Cancelled,
    #[error("{} already exists; choose another name or tick Overwrite existing file", .0.display())]
    Exists(PathBuf),
    #[error("the frames could not be rendered: {0}")]
    Render(String),
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: std::io::Error },
    #[error("FFmpeg could not encode the video: {0}")]
    Ffmpeg(String),
    #[error("the video could not be written: {0}")]
    Video(String),
    #[error("the GIF could not be written: {0}")]
    Gif(String),
}

pub(crate) type Result<T> = std::result::Result<T, ExportError>;

pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ExportError + '_ {
    move |source| ExportError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Who writes an export's file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    BuiltIn,
    System,
    Ffmpeg { path: PathBuf, encoder: &'static str },
}

/// How `format` would be written, given whether the system's H.264 encoder
/// is there and the FFmpeg found, if any. The first writer that can, of
/// those the format lists, best first.
pub fn route(format: Format, system_h264: bool, ffmpeg: Option<&Ffmpeg>) -> Result<Route> {
    for w in format.writers() {
        match w {
            Writer::BuiltIn => return Ok(Route::BuiltIn),
            Writer::System if system_h264 => return Ok(Route::System),
            Writer::System => {}
            Writer::Ffmpeg(_) => {
                if let Some(f) = ffmpeg
                    && let Some(encoder) = f.encoder_for(format)
                {
                    return Ok(Route::Ffmpeg {
                        path: f.path.clone(),
                        encoder,
                    });
                }
            }
        }
    }
    Err(match ffmpeg {
        Some(f) => ExportError::FfmpegLacks {
            format,
            version: f.version.clone(),
        },
        None => ExportError::NoFfmpeg(format),
    })
}

/// Export `doc` as `settings` say to `out` (for a picture sequence, the
/// frames are named after it: `scene.png` gives `scene_0000.png` and on).
/// `progress` hears how many of how many frames are done, and returning
/// `false` from it cancels the export.
pub fn export(
    doc: &Document,
    settings: &Settings,
    out: &Path,
    progress: &mut dyn FnMut(u32, u32) -> bool,
) -> Result<()> {
    let s = settings.sanitized();
    let (min_w, min_h) = s.format.min_size();
    if s.width < min_w || s.height < min_h {
        return Err(ExportError::Video(format!(
            "{} needs pictures at least {min_w} x {min_h} pixels",
            s.format.label()
        )));
    }
    if !s.overwrite
        && let Some(existing) = existing_output(out, s.format)
    {
        return Err(ExportError::Exists(existing));
    }
    // decided before anything is rendered, so a missing writer says so at once
    let system = s.format.writers().contains(&Writer::System) && fanimate_oscodec::h264_available();
    let needs_ffmpeg = s.format.writers().iter().any(|w| matches!(w, Writer::Ffmpeg(_)));
    let ffmpeg = if needs_ffmpeg && !system {
        ffmpeg::find(s.ffmpeg.as_deref()).and_then(|p| Ffmpeg::probe(&p).ok())
    } else {
        None
    };
    let route = route(s.format, system, ffmpeg.as_ref())?;
    let gpu = Gpu::headless().map_err(|e| ExportError::Render(e.to_string()))?;
    let mut frames = Frames {
        gpu: &gpu,
        target: Offscreen::new(&gpu, s.width, s.height),
        doc,
        start: s.start,
    };
    let total = s.frame_count();
    let mut temps = Temps::default();
    let sound = if s.format.has_sound() {
        sound::mix(doc, s.start, s.end, SOUND_RATE)
    } else {
        None
    };
    match route {
        Route::System => system_video(
            &mut frames,
            &s,
            doc.fps,
            sound.as_deref(),
            out,
            &mut temps,
            progress,
        )?,
        Route::Ffmpeg { path, encoder } => video(
            &mut frames,
            &s,
            doc,
            sound.as_deref(),
            &path,
            encoder,
            out,
            &mut temps,
            progress,
        )?,
        Route::BuiltIn => match s.format {
            Format::Gif => gif(&mut frames, &s, doc.fps, out, &mut temps, progress)?,
            Format::Apng => crate::apng::write(&mut frames, &s, doc.fps, out, &mut temps, progress)?,
            Format::Avi => crate::avi::write(
                &mut frames,
                &s,
                doc.fps,
                sound.as_deref(),
                out,
                &mut temps,
                progress,
            )?,
            Format::Av1 => crate::av1::write(&mut frames, &s, doc.fps, out, &mut temps, progress)?,
            format => crate::sequence::write(&mut frames, format, total, out, &mut temps, progress)?,
        },
    }
    progress(total, total);
    Ok(())
}

/// Renders frames one after another into the same target.
pub(crate) struct Frames<'a> {
    gpu: &'a Gpu,
    pub(crate) target: Offscreen,
    doc: &'a Document,
    pub(crate) start: i32,
}

impl Frames<'_> {
    /// Frame `i` of the export, as RGBA pixels.
    pub(crate) fn get(&mut self, i: u32) -> Result<Vec<u8>> {
        self.target
            .draw(self.gpu, self.doc, (self.start + i as i32) as f32);
        self.target
            .read_rgba(self.gpu)
            .map_err(|e| ExportError::Render(e.to_string()))
    }
}

/// Files to remove however the export ends, unless finished.
#[derive(Default)]
pub(crate) struct Temps(Vec<PathBuf>);

impl Temps {
    pub(crate) fn add(&mut self, path: PathBuf) -> PathBuf {
        self.0.push(path.clone());
        path
    }

    /// Move finished `part` into place as `out`.
    pub(crate) fn finish(&mut self, part: &Path, out: &Path) -> Result<()> {
        // a rename does not replace a file everywhere (Windows)
        if out.exists() {
            std::fs::remove_file(out).map_err(io(out))?;
        }
        std::fs::rename(part, out).map_err(io(out))?;
        self.0.retain(|p| p != part);
        Ok(())
    }
}

impl Drop for Temps {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// The file (or, for a picture sequence, the first picture) an export to
/// `out` would replace, if there is one.
pub fn existing_output(out: &Path, format: Format) -> Option<PathBuf> {
    if !format.is_sequence() {
        return out.exists().then(|| out.to_path_buf());
    }
    let (dir, stem) = (out.parent()?, out.file_stem()?);
    let prefix = format!("{}_", stem.to_string_lossy());
    let suffix = format!(".{}", format.extension());
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        (name.starts_with(&prefix) && name.ends_with(&suffix)).then(|| e.path())
    })
}

pub(crate) fn part(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_owned();
    name.push(".part");
    out.with_file_name(name)
}

/// Remove the unfinished files an export to `out` left when its process
/// was killed before it could clean up (a stop or failure removes them
/// itself). Finished files are never touched.
pub(crate) fn remove_parts(out: &Path, format: Format) {
    if !format.is_sequence() {
        let _ = std::fs::remove_file(part(out));
        return;
    }
    // a sequence writes one frame at a time, named after `out`
    let (Some(dir), Some(stem)) = (out.parent(), out.file_stem()) else {
        return;
    };
    let prefix = format!("{}_", stem.to_string_lossy());
    let suffix = format!(".{}.part", format.extension());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(&suffix) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn gif(
    frames: &mut Frames,
    s: &Settings,
    fps: f32,
    out: &Path,
    temps: &mut Temps,
    progress: &mut dyn FnMut(u32, u32) -> bool,
) -> Result<()> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let tmp = temps.add(part(out));
    let file = std::fs::File::create(&tmp).map_err(io(&tmp))?;
    let gif_err = |e: image::ImageError| ExportError::Gif(e.to_string());
    {
        // speed 10 of 30: close to the best palette at a fraction of the time
        let mut enc = GifEncoder::new_with_speed(BufWriter::new(file), 10);
        enc.set_repeat(Repeat::Infinite).map_err(gif_err)?;
        let delay = image::Delay::from_numer_denom_ms(1_000_000, (fps.max(0.001) * 1000.0) as u32);
        for i in 0..s.frame_count() {
            if !progress(i, s.frame_count()) {
                return Err(ExportError::Cancelled);
            }
            let pixels = frames.get(i)?;
            let img = image::RgbaImage::from_raw(s.width, s.height, pixels)
                .ok_or_else(|| ExportError::Render("a frame came back the wrong size".into()))?;
            enc.encode_frame(image::Frame::from_parts(img, 0, 0, delay))
                .map_err(gif_err)?;
        }
    }
    temps.finish(&tmp, out)
}

/// H.264 and AAC in an MP4, by the operating system's own encoder.
fn system_video(
    frames: &mut Frames,
    s: &Settings,
    fps: f32,
    sound: Option<&[f32]>,
    out: &Path,
    temps: &mut Temps,
    progress: &mut dyn FnMut(u32, u32) -> bool,
) -> Result<()> {
    let tmp = temps.add(part(out));
    let options = fanimate_oscodec::Options {
        width: s.width,
        height: s.height,
        fps,
        bit_rate: ffmpeg::bit_rate(s, fps),
        sound_rate: sound.map(|_| SOUND_RATE),
    };
    let video_err = |e: fanimate_oscodec::Error| ExportError::Video(e.to_string());
    let mut writer = fanimate_oscodec::H264Writer::create(&tmp, options).map_err(video_err)?;
    let total = s.frame_count();
    for i in 0..total {
        if !progress(i, total) {
            return Err(ExportError::Cancelled);
        }
        let pixels = frames.get(i)?;
        let nv12 = crate::yuv::i420(&pixels, s.width as usize, s.height as usize).nv12();
        writer
            .frame(i, &nv12, crate::avi::sound_for_frame(sound, i, fps))
            .map_err(video_err)?;
    }
    writer.finish().map_err(video_err)?;
    temps.finish(&tmp, out)
}

/// FFmpeg, stopped if the export ends early.
struct Encoder(Child);

impl Drop for Encoder {
    fn drop(&mut self) {
        // already exited after a finished export; harmless then
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[allow(clippy::too_many_arguments)]
fn video(
    frames: &mut Frames,
    s: &Settings,
    doc: &Document,
    sound: Option<&[f32]>,
    ffmpeg: &Path,
    encoder: &str,
    out: &Path,
    temps: &mut Temps,
    progress: &mut dyn FnMut(u32, u32) -> bool,
) -> Result<()> {
    static RUNS: AtomicU32 = AtomicU32::new(0);
    let audio = match sound {
        Some(samples) => {
            let path = temps.add(std::env::temp_dir().join(format!(
                "fanimate-export-{}-{}.wav",
                std::process::id(),
                RUNS.fetch_add(1, Ordering::Relaxed)
            )));
            sound::write_wav(&path, samples, SOUND_RATE).map_err(|e| ExportError::Io {
                path: path.clone(),
                source: std::io::Error::other(e),
            })?;
            Some(path)
        }
        None => None,
    };
    let tmp = temps.add(part(out));
    let child = ffmpeg::command(ffmpeg)
        .args(ffmpeg::video_args(s, doc.fps, encoder, audio.as_deref(), &tmp))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ExportError::Ffmpeg(format!("it could not start ({e})")))?;
    let mut enc = Encoder(child);
    // read FFmpeg's complaints as they come, so its pipe never fills
    let errors = enc.0.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = err.read_to_string(&mut text);
            text
        })
    });
    let mut stdin = enc.0.stdin.take();
    for i in 0..s.frame_count() {
        if !progress(i, s.frame_count()) {
            return Err(ExportError::Cancelled);
        }
        let pixels = frames.get(i)?;
        // a write fails when FFmpeg has stopped; its message says why
        if stdin.as_mut().is_none_or(|w| w.write_all(&pixels).is_err()) {
            break;
        }
    }
    drop(stdin);
    let status = enc.0.wait().map_err(|e| ExportError::Ffmpeg(e.to_string()))?;
    let complaint = errors.and_then(|t| t.join().ok()).unwrap_or_default();
    if !status.success() {
        let line = complaint.lines().rev().find(|l| !l.trim().is_empty());
        return Err(ExportError::Ffmpeg(line.map_or_else(
            || format!("it stopped ({status})"),
            |l| l.trim().to_owned(),
        )));
    }
    temps.finish(&tmp, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::samples;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-export-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_export_that_would_replace_a_file_stops_unless_overwrite_is_on() {
        let dir = temp_dir("overwrite");
        let doc = samples::hello();
        let out = dir.join("walk.gif");
        std::fs::write(&out, b"precious").unwrap();
        let mut settings = Settings::for_doc(&doc, Format::Gif);
        settings.overwrite = false;
        let err = export(&doc, &settings, &out, &mut |_, _| true).unwrap_err();
        assert!(matches!(&err, ExportError::Exists(p) if *p == out), "{err}");
        assert_eq!(std::fs::read(&out).unwrap(), b"precious", "the file is untouched");
        // before this setting existed an export replaced the file, and a
        // queued job without the setting still does
        let queued: Settings =
            serde_json::from_str(r#"{"format":"gif","width":8,"height":8,"start":0,"end":1}"#).unwrap();
        assert!(queued.overwrite);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn existing_output_finds_a_file_or_a_picture_of_a_sequence() {
        let dir = temp_dir("existing");
        let one = dir.join("scene.mp4");
        assert_eq!(existing_output(&one, Format::Mp4), None);
        std::fs::write(&one, b"x").unwrap();
        assert_eq!(existing_output(&one, Format::Mp4), Some(one));
        let seq = dir.join("scene.png");
        assert_eq!(existing_output(&seq, Format::Png), None);
        std::fs::write(dir.join("scene_0003.png"), b"x").unwrap();
        // an unfinished `.part` picture is not an existing one
        std::fs::write(dir.join("other_0001.png"), b"x").unwrap();
        assert_eq!(
            existing_output(&seq, Format::Png),
            Some(dir.join("scene_0003.png"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Held by each GPU test, so they take turns: creating devices on
    /// several threads at once has crashed Windows' software renderer (the
    /// one CI runners use) with an access violation.
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The GPU, for one test at a time; `None` (and the test skips cleanly)
    /// where no adapter exists.
    fn gpu() -> Option<std::sync::MutexGuard<'static, ()>> {
        let turn = GPU.lock().unwrap_or_else(|e| e.into_inner());
        if Gpu::headless().is_err() {
            eprintln!("no GPU adapter; skipping");
            return None;
        }
        Some(turn)
    }

    fn small(format: Format, frames: i32) -> Settings {
        Settings {
            format,
            width: 64,
            height: 36,
            start: 0,
            end: frames - 1,
            ffmpeg: None,
            overwrite: true,
        }
    }

    #[test]
    fn png_sequences_number_their_frames() {
        let Some(_turn) = gpu() else { return };
        let dir = temp_dir("png");
        let mut seen = Vec::new();
        export(
            &samples::hello(),
            &small(Format::Png, 3),
            &dir.join("hello.png"),
            &mut |d, t| {
                seen.push((d, t));
                true
            },
        )
        .unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["hello_0000.png", "hello_0001.png", "hello_0002.png"]);
        assert_eq!(seen.last(), Some(&(3, 3)));
        let img = image::open(dir.join("hello_0001.png")).unwrap();
        assert_eq!((img.width(), img.height()), (64, 36));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gifs_loop_their_frames() {
        let Some(_turn) = gpu() else { return };
        let dir = temp_dir("gif");
        let out = dir.join("hello.gif");
        export(&samples::hello(), &small(Format::Gif, 4), &out, &mut |_, _| true).unwrap();
        let file = std::fs::File::open(&out).unwrap();
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)).unwrap();
        use image::AnimationDecoder;
        assert_eq!(decoder.into_frames().count(), 4);
        assert!(!part(&out).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancelling_leaves_nothing_behind_and_keeps_the_old_file() {
        let Some(_turn) = gpu() else { return };
        let dir = temp_dir("cancel");
        let out = dir.join("hello.gif");
        std::fs::write(&out, b"the previous export").unwrap();
        let err = export(&samples::hello(), &small(Format::Gif, 10), &out, &mut |d, _| {
            d < 3
        })
        .unwrap_err();
        assert!(matches!(err, ExportError::Cancelled));
        assert_eq!(std::fs::read(&out).unwrap(), b"the previous export");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn has(bytes: &[u8], what: &[u8]) -> bool {
        bytes.windows(what.len()).any(|w| w == what)
    }

    #[test]
    fn built_in_formats_write_their_files() {
        let Some(_turn) = gpu() else { return };
        let dir = temp_dir("built-in");
        let doc = sound::tests::with_sound(0);
        for (format, magic) in [
            (Format::Exr, &[0x76, 0x2f, 0x31, 0x01][..]),
            (Format::Tiff, &[0x49, 0x49, 0x2a, 0x00][..]),
            (Format::Apng, &[0x89, 0x50, 0x4e, 0x47][..]),
            (Format::Avi, &b"RIFF"[..]),
            (Format::Av1, &b"ftyp"[..]),
        ] {
            let out = dir.join(format!("hello.{}", format.extension()));
            let mut s = small(format, 4);
            s.width = 63; // AV1 makes it even; the others keep it
            export(&doc, &s, &out, &mut |_, _| true).unwrap();
            let file = if format.is_sequence() {
                crate::sequence::frame_file(&out, format, 1, 4)
            } else {
                out.clone()
            };
            let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert!(
                bytes.starts_with(magic) || has(&bytes[..16], magic),
                "{format:?} header"
            );
            match format {
                Format::Apng => assert!(has(&bytes, b"acTL"), "APNG is animated"),
                Format::Avi => assert!(has(&bytes, b"01wb"), "AVI has sound"),
                Format::Av1 => assert!(has(&bytes, b"av01"), "MP4 holds AV1"),
                _ => {}
            }
            assert!(!part(&file).exists());
        }
        // the sequences decode back as pictures of the size asked for
        let exr = image::open(crate::sequence::frame_file(
            &dir.join("hello.exr"),
            Format::Exr,
            0,
            4,
        ))
        .unwrap();
        assert_eq!((exr.width(), exr.height()), (63, 36));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mp4_is_written_by_the_system_encoder_where_there_is_one() {
        let Some(_turn) = gpu() else { return };
        if !fanimate_oscodec::h264_available() {
            eprintln!("no system H.264 encoder; skipping");
            return;
        }
        let dir = temp_dir("system-mp4");
        let out = dir.join("tone.mp4");
        let mut s = small(Format::Mp4, 10);
        s.width = 63; // made even
        export(&sound::tests::with_sound(0), &s, &out, &mut |_, _| true).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert!(has(&bytes[..16], b"ftyp") && has(&bytes, b"avc1") && has(&bytes, b"mp4a"));
        assert!(!part(&out).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn formats_say_what_they_need() {
        // with no FFmpeg, FFmpeg-only formats say so; built-in ones never ask
        assert!(matches!(
            route(Format::ProRes4444, true, None),
            Err(ExportError::NoFfmpeg(_))
        ));
        assert_eq!(route(Format::Exr, false, None).unwrap(), Route::BuiltIn);
        assert_eq!(route(Format::Mp4, true, None).unwrap(), Route::System);
        let lacking = Ffmpeg {
            path: "ffmpeg".into(),
            version: "6.0".into(),
            encoders: ["aac".to_owned()].into(),
        };
        let e = route(Format::Hevc, true, Some(&lacking)).unwrap_err();
        assert!(e.to_string().contains("version 6.0"), "{e}");
        // MP4 falls back to FFmpeg where the system has no encoder
        let full = Ffmpeg {
            encoders: ["libx264".to_owned()].into(),
            ..lacking
        };
        assert!(matches!(
            route(Format::Mp4, false, Some(&full)),
            Ok(Route::Ffmpeg {
                encoder: "libx264",
                ..
            })
        ));
    }

    #[test]
    fn ffmpeg_formats_are_written_by_the_users_ffmpeg() {
        let Some(_turn) = gpu() else { return };
        let Some(found) = ffmpeg::find(None).and_then(|p| Ffmpeg::probe(&p).ok()) else {
            eprintln!("no ffmpeg; skipping");
            return;
        };
        let dir = temp_dir("ffmpeg");
        // the containers name their codecs; read them from the file
        for (format, video, audio) in [
            (Format::Mkv, &b"V_MPEG4/ISO/AVC"[..], &b"A_AAC"[..]),
            (Format::ProRes4444, &b"ap4h"[..], &b"sowt"[..]),
            (Format::Dnxhr, &b"AVdh"[..], &b"sowt"[..]),
        ] {
            if found.encoder_for(format).is_none() {
                eprintln!("this ffmpeg cannot make {format:?}; skipping it");
                continue;
            }
            let out = dir.join(format!("tone.{}", format.extension()));
            let mut s = small(format, 10);
            s.width = 63; // made even
            if format == Format::Dnxhr {
                let tiny = export(&sound::tests::with_sound(0), &s, &out, &mut |_, _| true).unwrap_err();
                assert!(tiny.to_string().contains("at least 256 x 120"), "{tiny}");
                (s.width, s.height) = (256, 144);
            }
            export(&sound::tests::with_sound(0), &s, &out, &mut |_, _| true)
                .unwrap_or_else(|e| panic!("{format:?}: {e}"));
            let bytes = std::fs::read(&out).unwrap();
            assert!(has(&bytes, video), "{format:?} video");
            assert!(has(&bytes, audio), "{format:?} sound");
            assert!(!part(&out).exists());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
