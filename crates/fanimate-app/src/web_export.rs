//! Export for the web and for games: one frame as SVG, the animation as a
//! Lottie or dotLottie, and a sprite sheet. They are not video, so they do
//! not go through the render queue; each runs in a background thread on a
//! copy of the project (the editor stays responsive and keeps working) and
//! says what it could not carry when it finishes.

use std::path::PathBuf;
use std::sync::mpsc;

use fanimate_editor::StatusKind;
use fanimate_export::sheet::{self, Clip, Options};
use fanimate_export::{Report, lottie, svg};

use crate::app::Shell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The frame at the playhead.
    Svg,
    /// The whole animation; the file's extension picks Lottie or dotLottie.
    Lottie,
    /// The whole animation as one clip.
    Sheet,
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
