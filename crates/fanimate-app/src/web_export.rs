//! Export for the web, games and editing programs: one frame as SVG, the
//! animation as a Lottie, dotLottie, animated SVG or runtime file, a sprite
//! sheet, and an OpenTimelineIO timeline. They are not video, so they do
//! not go through the render queue; each runs in a background thread on a
//! copy of the project (the editor stays responsive and keeps working) and
//! says what it could not carry when it finishes.

use std::path::PathBuf;
use std::sync::mpsc;

use fanimate_editor::StatusKind;
use fanimate_export::sheet::{self, Clip, Options};
use fanimate_export::{Report, lottie, ora, otio, runtime, svg, svg_animated};

use crate::app::Shell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The frame at the playhead.
    Svg,
    /// The whole animation; the file's extension picks Lottie or dotLottie.
    Lottie,
    /// The whole animation as one clip.
    Sheet,
    /// The whole animation as an SVG that browsers play.
    AnimatedSvg,
    /// The whole animation as a runtime file for games and the web.
    Runtime,
    /// The timing and sounds as an OpenTimelineIO timeline.
    Otio,
    /// The frame at the playhead as raster and picture layers for a painting program.
    OpenRaster,
}

/// An export in progress.
pub struct Job {
    what: &'static str,
    result: mpsc::Receiver<Result<(String, Report), String>>,
}

/// The longest side of a sprite sheet's frames when the project is larger.
const SHEET_FRAME: f32 = 256.0;

/// Ask where to save, then start. Does nothing if the user cancels.
pub fn start(shell: &mut Shell, kind: Kind) {
    if shell.web_export.is_some() {
        shell
            .editor
            .set_status(StatusKind::Info, "An export is already running");
        return;
    }
    let name = shell.editor.doc.name.clone();
    let name = if name.trim().is_empty() {
        "Fanimate".to_owned()
    } else {
        name
    };
    let dialog = rfd::FileDialog::new();
    let dialog = match kind {
        Kind::Svg => dialog
            .set_title("Export this frame as SVG")
            .set_file_name(format!("{name}.svg"))
            .add_filter("SVG", &["svg"]),
        Kind::Lottie => dialog
            .set_title("Export as Lottie")
            .set_file_name(format!("{name}.lottie"))
            .add_filter("dotLottie", &["lottie"])
            .add_filter("Lottie JSON", &["json"]),
        Kind::Sheet => dialog
            .set_title("Export a sprite sheet")
            .set_file_name(format!("{name}.png"))
            .add_filter("PNG", &["png"]),
        Kind::AnimatedSvg => dialog
            .set_title("Export as animated SVG")
            .set_file_name(format!("{name}.svg"))
            .add_filter("SVG", &["svg"]),
        Kind::Runtime => dialog
            .set_title("Export a runtime file")
            .set_file_name(format!("{name}.fnmr"))
            .add_filter("Fanimate runtime", &["fnmr"]),
        Kind::Otio => dialog
            .set_title("Export an OpenTimelineIO timeline")
            .set_file_name(format!("{name}.otio"))
            .add_filter("OpenTimelineIO", &["otio"]),
        Kind::OpenRaster => dialog
            .set_title("Export paint layers as OpenRaster")
            .set_file_name(format!("{name}.ora"))
            .add_filter("OpenRaster", &["ora"]),
    };
    let Some(path) = dialog.save_file() else {
        return;
    };
    let doc = shell.editor.doc.mainline().into_owned();
    let frame = shell.editor.current_frame();
    let (tx, rx) = mpsc::channel();
    let what = match kind {
        Kind::Svg => "SVG",
        Kind::Lottie => "Lottie",
        Kind::Sheet => "Sprite sheet",
        Kind::AnimatedSvg => "Animated SVG",
        Kind::Runtime => "Runtime file",
        Kind::Otio => "Timeline",
        Kind::OpenRaster => "OpenRaster",
    };
    std::thread::spawn(move || {
        let _ = tx.send(run(kind, &doc, frame, &path));
    });
    shell
        .editor
        .set_status(StatusKind::Info, format!("Exporting {what}…"));
    shell.web_export = Some(Job { what, result: rx });
}

/// Each frame: report a finished export.
pub fn poll(ctx: &eframe::egui::Context, shell: &mut Shell) {
    let Some(job) = &shell.web_export else {
        return;
    };
    let outcome = match job.result.try_recv() {
        Ok(r) => r,
        Err(mpsc::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }
        Err(mpsc::TryRecvError::Disconnected) => Err("the export stopped unexpectedly".to_owned()),
    };
    let Some(job) = shell.web_export.take() else {
        return;
    };
    match outcome {
        Ok((message, report)) if report.is_empty() => shell.editor.set_status(StatusKind::Success, message),
        Ok((message, report)) => shell.editor.set_status(
            StatusKind::Warning,
            format!("{message}; left out or simplified: {}", report.summary()),
        ),
        Err(e) => shell
            .editor
            .set_status(StatusKind::Error, format!("{} export failed: {e}", job.what)),
    }
}

/// Make the file. Returns the status message and what could not be carried.
fn run(
    kind: Kind,
    doc: &fanimate_core::Document,
    frame: fanimate_core::Frame,
    path: &PathBuf,
) -> Result<(String, Report), String> {
    let write = |bytes: &[u8]| std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()));
    match kind {
        Kind::Svg => {
            let (text, report) = svg::frame(doc, frame as f32);
            write(text.as_bytes())?;
            Ok((format!("Exported frame {frame} as SVG"), report))
        }
        Kind::Lottie => {
            let dot = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lottie"));
            let (bytes, report) = if dot {
                lottie::dotlottie(doc, None)?
            } else {
                let l = lottie::lottie(doc, None)?;
                (serde_json::to_vec(&l.json).map_err(|e| e.to_string())?, l.report)
            };
            write(&bytes)?;
            let kb = bytes.len().div_ceil(1024);
            Ok((
                format!("Exported {} ({kb} KB)", if dot { "dotLottie" } else { "Lottie" }),
                report,
            ))
        }
        Kind::AnimatedSvg => {
            let (text, report) = svg_animated::animated(doc, None)?;
            write(text.as_bytes())?;
            let kb = text.len().div_ceil(1024);
            Ok((format!("Exported an animated SVG ({kb} KB)"), report))
        }
        Kind::Runtime => {
            let (bytes, report, keys) = runtime::fnmr(doc, None)?;
            write(&bytes)?;
            let kb = bytes.len().div_ceil(1024);
            Ok((format!("Exported a runtime file ({kb} KB, {keys} keys)"), report))
        }
        Kind::OpenRaster => {
            let (bytes, report) = ora::write(doc, frame)?;
            write(&bytes)?;
            let kb = bytes.len().div_ceil(1024);
            Ok((
                format!("Exported paint layers as OpenRaster ({kb} KB, as on frame {frame})"),
                report,
            ))
        }
        Kind::Otio => {
            let stem = path
                .file_stem()
                .map_or_else(|| "timeline".to_owned(), |s| s.to_string_lossy().into_owned());
            let media_dir = format!("{stem}-media");
            let options = otio::Options {
                video: None,
                media_dir: Some(media_dir.clone()),
            };
            let made = otio::timeline(doc, &options);
            let text = serde_json::to_string_pretty(&made.json).map_err(|e| e.to_string())?;
            write(text.as_bytes())?;
            if !made.media.is_empty() {
                let folder = path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(&media_dir);
                std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
                for (name, bytes) in &made.media {
                    if fanimate_fnm::is_media_name(name) {
                        let file = folder.join(name);
                        std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
                    }
                }
            }
            Ok((
                format!(
                    "Exported a timeline with {} audio file(s); link its video clip to your rendered movie",
                    made.media.len()
                ),
                made.report,
            ))
        }
        Kind::Sheet => {
            let scale = (SHEET_FRAME / doc.width.max(doc.height)).min(1.0);
            let opts = Options {
                width: ((doc.width * scale).round() as u32).max(1),
                height: ((doc.height * scale).round() as u32).max(1),
                ..Options::default()
            };
            let clips = [Clip {
                name: "all".to_owned(),
                start: doc.start,
                end: doc.end,
            }];
            let image_name = path
                .file_name()
                .map_or_else(|| "sheet.png".to_owned(), |n| n.to_string_lossy().into_owned());
            let made = sheet::render(doc, &clips, &opts, &image_name)?;
            image::save_buffer(
                path,
                &made.rgba,
                made.width,
                made.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| format!("{}: {e}", path.display()))?;
            let atlas = path.with_extension("json");
            let text = serde_json::to_string_pretty(&made.atlas).map_err(|e| e.to_string())?;
            std::fs::write(&atlas, text).map_err(|e| format!("{}: {e}", atlas.display()))?;
            Ok((
                format!(
                    "Exported the sprite sheet and its atlas ({} frames share a picture)",
                    made.shared
                ),
                made.report,
            ))
        }
    }
}
