//! Previews for the Home tab: the first frame of a template or of a project,
//! drawn by the same renderer as the canvas and the export.
//!
//! One background thread does all the drawing, one picture at a time (it
//! owns the only graphics device; several at once have crashed some
//! drivers), so the Home tab never waits for a picture.
//!
//! A project carries its own preview: every save draws a small picture of
//! the first frame and writes it into the file (`thumbnail.png`, see
//! `fanimate_fnm::save_with_thumbnail`). A project's preview is read from
//! the file first. Files saved by older versions have none; their preview is
//! drawn once and kept as a small PNG in Fanimate's data folder, named after
//! the file and when it last changed, so a saved change makes a fresh one.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::SystemTime;

use eframe::egui;
use fanimate_core::Document;

/// The longest side of a preview shown on the Home tab, in pixels.
const SIDE: f32 = 320.0;

/// The longest side of the preview kept in a project file, in pixels.
const EMBED_SIDE: f32 = 256.0;

/// When a project was last written and how big it was: if this is the same
/// later, nothing has saved over it since.
type Stamp = (Option<SystemTime>, u64);

/// What is asked of the drawing thread.
enum Job {
    Sample(fanimate_editor::Sample),
    File(PathBuf, PathBuf),
    /// Draw the document just saved to the path, for the file to keep.
    Embed(PathBuf, Stamp, Box<Document>),
}

/// A finished preview: its size and straight-alpha RGBA pixels.
struct Picture {
    size: [usize; 2],
    rgba: Vec<u8>,
}

/// A saved document and the preview drawn for it (none if it could not be).
struct Embedded {
    path: PathBuf,
    stamp: Stamp,
    doc: Box<Document>,
    png: Option<Vec<u8>>,
}

/// The previews asked for, drawn and kept.
pub struct Thumbs {
    /// Where finished previews of files are kept; none without a data folder.
    cache: Option<PathBuf>,
    jobs: Sender<(String, Job)>,
    done: Receiver<(String, Option<Picture>)>,
    embedded: Receiver<Embedded>,
    /// Saves whose preview has been asked for and not yet written.
    embeds_waiting: usize,
    asked: HashSet<String>,
    ready: HashMap<String, egui::TextureHandle>,
    /// Asked for but not drawn (the file could not be read, or no graphics
    /// adapter): left blank.
    failed: HashSet<String>,
}

impl Default for Thumbs {
    fn default() -> Self {
        let (jobs, job_rx) = channel::<(String, Job)>();
        let (done_tx, done) = channel();
        let (embedded_tx, embedded) = channel();
        std::thread::spawn(move || worker(job_rx, done_tx, embedded_tx));
        Self {
            cache: eframe::storage_dir("Fanimate").map(|d| d.join("thumbnails")),
            jobs,
            done,
            embedded,
            embeds_waiting: 0,
            asked: HashSet::new(),
            ready: HashMap::new(),
            failed: HashSet::new(),
        }
    }
}

/// The preview's size for a canvas of `width` by `height`, its longest side
/// `side` pixels.
fn fit_to(side: f32, width: f32, height: f32) -> (u32, u32) {
    let longest = width.max(height).max(1.0);
    let scale = side / longest;
    (
        ((width * scale).round() as u32).max(1),
        ((height * scale).round() as u32).max(1),
    )
}

/// The Home tab's preview size.
#[cfg(test)]
fn fit(width: f32, height: f32) -> (u32, u32) {
    fit_to(SIDE, width, height)
}

/// Draw the first frame of `doc`, its longest side `side` pixels.
fn draw_at(gpu: &fanimate_render::Gpu, doc: &Document, side: f32) -> Option<Picture> {
    let (w, h) = fit_to(side, doc.width, doc.height);
    let rgba = fanimate_render::render_to_rgba(gpu, doc, doc.start as f32, w, h).ok()?;
    Some(Picture {
        size: [w as usize, h as usize],
        rgba,
    })
}

/// Draw the first frame of `doc` at preview size.
fn draw(gpu: &fanimate_render::Gpu, doc: &Document) -> Option<Picture> {
    draw_at(gpu, doc, SIDE)
}

fn worker(
    jobs: Receiver<(String, Job)>,
    done: Sender<(String, Option<Picture>)>,
    embedded: Sender<Embedded>,
) {
    // the device is made when the first picture needs it
    let mut gpu: Option<Option<fanimate_render::Gpu>> = None;
    for (key, job) in jobs {
        let picture = match job {
            Job::File(path, cached) => read_embedded(&path).or_else(|| read_cached(&cached)).or_else(|| {
                let (doc, _) = fanimate_fnm::load(&path).ok()?;
                let gpu = gpu
                    .get_or_insert_with(|| fanimate_render::Gpu::headless().ok())
                    .as_ref()?;
                let picture = draw(gpu, &doc)?;
                write_cached(&cached, &picture);
                Some(picture)
            }),
            Job::Sample(sample) => {
                let gpu = gpu
                    .get_or_insert_with(|| fanimate_render::Gpu::headless().ok())
                    .as_ref();
                gpu.and_then(|gpu| draw(gpu, &sample.document()))
            }
            Job::Embed(path, stamp, doc) => {
                let png = gpu
                    .get_or_insert_with(|| fanimate_render::Gpu::headless().ok())
                    .as_ref()
                    .and_then(|gpu| draw_at(gpu, &doc, EMBED_SIDE))
                    .and_then(|picture| encode_png(&picture));
                let sent = embedded.send(Embedded {
                    path,
                    stamp,
                    doc,
                    png,
                });
                if sent.is_err() {
                    return;
                }
                continue;
            }
        };
        if done.send((key, picture)).is_err() {
            return;
        }
    }
}

/// The picture kept inside the project at `path`, if it has one.
fn read_embedded(path: &Path) -> Option<Picture> {
    let png = fanimate_fnm::read_thumbnail(path)?;
    let img = image::load_from_memory(&png).ok()?.into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(Picture {
        size,
        rgba: img.into_raw(),
    })
}

/// `picture` as a PNG small enough for a project to keep.
fn encode_png(picture: &Picture) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        &picture.rgba,
        picture.size[0] as u32,
        picture.size[1] as u32,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .ok()?;
    let bytes = out.into_inner();
    (bytes.len() <= fanimate_fnm::MAX_THUMBNAIL_BYTES).then_some(bytes)
}

/// When the project at `path` (a file, or a folder) was last written.
fn stamp(path: &Path) -> Option<Stamp> {
    let written = if path.is_dir() {
        path.join("document.json")
    } else {
        path.to_path_buf()
    };
    let meta = std::fs::metadata(written).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

fn read_cached(path: &Path) -> Option<Picture> {
    let img = image::open(path).ok()?.into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(Picture {
        size,
        rgba: img.into_raw(),
    })
}

fn write_cached(path: &Path, picture: &Picture) {
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let part = path.with_extension("part");
        if image::save_buffer_with_format(
            &part,
            &picture.rgba,
            picture.size[0] as u32,
            picture.size[1] as u32,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .is_ok()
        {
            let _ = std::fs::rename(&part, path);
        } else {
            let _ = std::fs::remove_file(&part);
        }
    }
}

/// A file name for the preview of `path` as it is now: it changes when the
/// file does.
pub fn cache_name(path: &Path, modified_secs: u64, len: u64) -> String {
    // FNV-1a over the path: short, stable, and no path characters
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.to_string_lossy().bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}-{modified_secs}-{len}.png")
}

impl Thumbs {
    /// Take in the pictures the drawing thread has finished.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, picture)) = self.done.try_recv() {
            match picture {
                Some(p) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(p.size, &p.rgba);
                    let texture = ctx.load_texture(&key, image, egui::TextureOptions::LINEAR);
                    self.ready.insert(key, texture);
                }
                None => {
                    self.failed.insert(key);
                }
            }
            ctx.request_repaint();
        }
        if self.ready.len() + self.failed.len() < self.asked.len() {
            // some are still being drawn
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    /// Ask for a preview of `doc`, just saved to `path`, to be drawn and
    /// written into the project. Nothing is waited for: the save is already
    /// complete, and the preview follows within a moment (see
    /// [`Thumbs::finish_saves`]).
    pub fn after_save(&mut self, path: &Path, doc: &Document) {
        let Some(stamp) = stamp(path) else {
            return;
        };
        // the picture shows the timeline, not an action being edited
        let doc = doc.mainline().into_owned();
        if self
            .jobs
            .send((
                String::new(),
                Job::Embed(path.to_path_buf(), stamp, Box::new(doc)),
            ))
            .is_ok()
        {
            self.embeds_waiting += 1;
        }
    }

    /// Write the previews that have been drawn into their projects. Runs on
    /// the interface thread, like every save, so it cannot cross a save the
    /// user makes meanwhile; and a project that has been written since is left
    /// alone (that save asked for a preview of its own).
    pub fn finish_saves(&mut self, ctx: &egui::Context) {
        loop {
            let done = match self.embedded.try_recv() {
                Ok(done) => done,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // the drawing thread is gone: nothing more will come
                    self.embeds_waiting = 0;
                    break;
                }
            };
            self.embeds_waiting = self.embeds_waiting.saturating_sub(1);
            let Some(png) = done.png else {
                continue;
            };
            if stamp(&done.path) != Some(done.stamp) {
                continue;
            }
            // the project was complete before this, so a failure loses nothing
            if let Err(e) = fanimate_fnm::save_with_thumbnail(&done.doc, &done.path, Some(&png)) {
                log::warn!("could not add a preview to {}: {e}", done.path.display());
            }
        }
        if self.embeds_waiting > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// The preview of a template, asking for it the first time.
    pub fn sample(&mut self, sample: fanimate_editor::Sample) -> Option<&egui::TextureHandle> {
        let key = format!("sample:{}", sample.name());
        if self.asked.insert(key.clone()) {
            let _ = self.jobs.send((key.clone(), Job::Sample(sample)));
        }
        self.ready.get(&key)
    }

    /// The preview of a project file, asking for it the first time (and again
    /// when the file has changed).
    pub fn file(&mut self, path: &Path) -> Option<&egui::TextureHandle> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let name = cache_name(path, modified, meta.len());
        let key = format!("file:{name}");
        if self.asked.insert(key.clone()) {
            let cached = self.cache.clone().unwrap_or_else(std::env::temp_dir).join(&name);
            let _ = self
                .jobs
                .send((key.clone(), Job::File(path.to_path_buf(), cached)));
        }
        self.ready.get(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_fit_the_longest_side() {
        assert_eq!(fit(1920.0, 1080.0), (320, 180));
        assert_eq!(fit(1080.0, 1920.0), (180, 320));
        assert_eq!(fit(100.0, 100.0), (320, 320));
        assert_eq!(fit(0.0, 0.0), (1, 1));
        // the picture a project keeps is smaller
        assert_eq!(fit_to(EMBED_SIDE, 1920.0, 1080.0), (256, 144));
        assert_eq!(fit_to(EMBED_SIDE, 1080.0, 1350.0), (205, 256));
    }

    #[test]
    fn a_changed_file_gets_a_new_preview_name() {
        let path = Path::new("/work/walk.fnm");
        let a = cache_name(path, 1000, 5000);
        assert_eq!(a, cache_name(path, 1000, 5000), "stable");
        assert_ne!(a, cache_name(path, 1001, 5000), "saved again");
        assert_ne!(a, cache_name(path, 1000, 5001));
        assert_ne!(a, cache_name(Path::new("/work/run.fnm"), 1000, 5000));
        assert!(a.ends_with(".png") && !a.contains('/') && !a.contains('\\'));
    }

    #[test]
    fn a_picture_is_kept_and_read_back_whole() {
        let dir = std::env::temp_dir().join(format!("fanimate-thumbs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("a.png");
        let picture = Picture {
            size: [2, 2],
            rgba: (0..16).collect(),
        };
        write_cached(&path, &picture);
        let back = read_cached(&path).expect("it was written");
        assert_eq!(back.size, [2, 2]);
        assert_eq!(back.rgba, picture.rgba);
        assert!(
            !path.with_extension("part").exists(),
            "no half-written file is left"
        );
        // an unreadable cache is a miss, not a failure
        std::fs::write(&path, b"not a picture").unwrap();
        assert!(read_cached(&path).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_picture_inside_a_project_is_read_back() {
        let dir = std::env::temp_dir().join(format!("fanimate-thumbs-embed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scene.fnm");
        let doc = fanimate_core::samples::blank();
        // 4 by 2, every pixel opaque red
        let picture = Picture {
            size: [4, 2],
            rgba: [255, 0, 0, 255].repeat(8),
        };
        let png = encode_png(&picture).expect("a tiny picture fits");
        fanimate_fnm::save_with_thumbnail(&doc, &path, Some(&png)).unwrap();
        let back = read_embedded(&path).expect("the project kept its picture");
        assert_eq!(back.size, [4, 2]);
        assert_eq!(back.rgba, picture.rgba);
        // a project with none is a miss, not a failure
        let bare = dir.join("bare.fnm");
        fanimate_fnm::save(&doc, &bare).unwrap();
        assert!(read_embedded(&bare).is_none());
        assert!(read_embedded(&dir.join("missing.fnm")).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_stamp_changes_when_the_project_is_written_again() {
        let dir = std::env::temp_dir().join(format!("fanimate-thumbs-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scene.fnm");
        assert_eq!(stamp(&path), None, "nothing there yet");
        let mut doc = fanimate_core::samples::blank();
        fanimate_fnm::save(&doc, &path).unwrap();
        let first = stamp(&path).expect("saved");
        assert_eq!(stamp(&path), Some(first), "stable while nothing is written");
        doc.name = "A much longer name than before, so the file is bigger".into();
        fanimate_fnm::save(&doc, &path).unwrap();
        assert_ne!(stamp(&path), Some(first));
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Needs a real graphics adapter. Windows only: on the Linux CI runner the
    // software Vulkan driver crashes the test process at exit once a second
    // device has been made in it, and the renderer's own tests (in
    // fanimate-render) cover drawing on every platform.
    #[cfg(windows)]
    #[test]
    fn a_template_is_drawn_when_there_is_a_graphics_adapter() {
        // skips where there is none
        let Ok(gpu) = fanimate_render::Gpu::headless() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let picture = draw(&gpu, &fanimate_core::samples::hello()).expect("drawn");
        assert_eq!(picture.size, [320, 180]);
        assert_eq!(picture.rgba.len(), 320 * 180 * 4);
        // not a blank sheet: the artwork and the background differ
        let first = &picture.rgba[..4];
        assert!(picture.rgba.chunks(4).any(|p| p != first));
        // the picture a project keeps is small
        let kept = draw_at(&gpu, &fanimate_core::samples::hello(), EMBED_SIDE).expect("drawn");
        assert_eq!(kept.size, [256, 144]);
        let png = encode_png(&kept).expect("it fits");
        assert!(png.len() <= fanimate_fnm::MAX_THUMBNAIL_BYTES);
    }
}
