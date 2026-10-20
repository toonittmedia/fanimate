//! Previews for the Home tab: the first frame of a template or of a recent
//! project, drawn by the same renderer as the canvas and the export.
//!
//! One background thread does all the drawing, one picture at a time (it
//! owns the only graphics device; several at once have crashed some
//! drivers), so the Home tab never waits for a picture. A recent project's
//! preview is kept as a small PNG in Fanimate's data folder, named after the
//! file and when it last changed, so it is drawn once and a saved change
//! makes a fresh one.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use fanimate_core::Document;

/// The longest side of a preview, in pixels.
const SIDE: f32 = 320.0;

/// What is asked of the drawing thread.
enum Job {
    Sample(fanimate_editor::Sample),
    File(PathBuf, PathBuf),
}

/// A finished preview: its size and straight-alpha RGBA pixels.
struct Picture {
    size: [usize; 2],
    rgba: Vec<u8>,
}

/// The previews asked for, drawn and kept.
pub struct Thumbs {
    /// Where finished previews of files are kept; none without a data folder.
    cache: Option<PathBuf>,
    jobs: Sender<(String, Job)>,
    done: Receiver<(String, Option<Picture>)>,
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
        std::thread::spawn(move || worker(job_rx, done_tx));
        Self {
            cache: eframe::storage_dir("Fanimate").map(|d| d.join("thumbnails")),
            jobs,
            done,
            asked: HashSet::new(),
            ready: HashMap::new(),
            failed: HashSet::new(),
        }
    }
}

/// The preview's size for a canvas of `width` by `height`.
fn fit(width: f32, height: f32) -> (u32, u32) {
    let longest = width.max(height).max(1.0);
    let scale = SIDE / longest;
    (
        ((width * scale).round() as u32).max(1),
        ((height * scale).round() as u32).max(1),
    )
}

/// Draw the first frame of `doc` at preview size.
fn draw(gpu: &fanimate_render::Gpu, doc: &Document) -> Option<Picture> {
    let (w, h) = fit(doc.width, doc.height);
    let rgba = fanimate_render::render_to_rgba(gpu, doc, doc.start as f32, w, h).ok()?;
    Some(Picture {
        size: [w as usize, h as usize],
        rgba,
    })
}

fn worker(jobs: Receiver<(String, Job)>, done: Sender<(String, Option<Picture>)>) {
    // the device is made when the first picture needs it
    let mut gpu: Option<Option<fanimate_render::Gpu>> = None;
    for (key, job) in jobs {
        let picture = match job {
            Job::File(path, cached) => read_cached(&cached).or_else(|| {
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
        };
        if done.send((key, picture)).is_err() {
            return;
        }
    }
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
    }
}
