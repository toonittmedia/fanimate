//! Render Preview Frame: the current frame rendered the way an export would
//! make it (the same renderer, off the pasteboard), shown in a dialog to look
//! at, save as a picture or copy.
//!
//! The render runs on its own thread with its own GPU device, so a large
//! frame never stalls the interface.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui::{self, RichText, vec2};
use fanimate_core::Document;
use fanimate_editor::StatusKind;

use crate::app::Shell;
use crate::kit::{self, Backdrop};

/// The longest side of a preview, in pixels.
const MAX_SIDE: f32 = 3840.0;

/// The Render Preview dialog's state.
pub struct Preview {
    frame: i32,
    /// The project's name, for the saved file's name.
    name: String,
    state: State,
    /// Show at the picture's own size (scrolling) rather than fitted.
    actual: bool,
}

enum State {
    Rendering(Receiver<Result<Rendered, String>>),
    Ready(Box<Rendered>, Option<egui::TextureHandle>),
    Failed(String),
}

struct Rendered {
    size: [usize; 2],
    /// Straight-alpha sRGB RGBA8.
    rgba: Vec<u8>,
}

/// The size the picture is rendered at: the project's own, no larger than
/// `MAX_SIDE` on its longest side.
pub fn preview_size(width: f32, height: f32) -> (u32, u32) {
    let longest = width.max(height).max(1.0);
    let scale = (MAX_SIDE / longest).min(1.0);
    (
        ((width * scale).round() as u32).max(1),
        ((height * scale).round() as u32).max(1),
    )
}

impl Preview {
    /// Start rendering the front document's current frame.
    pub fn start(shell: &Shell) -> Self {
        let editor = &shell.editor;
        let doc: Document = editor.doc.mainline().into_owned();
        let frame = editor.current_frame();
        let (w, h) = preview_size(doc.width, doc.height);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let result = fanimate_render::Gpu::headless()
                .map_err(|e| format!("There is no graphics adapter to render with: {e}"))
                .and_then(|gpu| {
                    fanimate_render::render_to_rgba(&gpu, &doc, frame as f32, w, h)
                        .map_err(|e| format!("The frame could not be rendered: {e}"))
                })
                .map(|rgba| Rendered {
                    size: [w as usize, h as usize],
                    rgba,
                });
            let _ = tx.send(result);
        });
        Self {
            frame,
            name: editor.doc.name.clone(),
            state: State::Rendering(rx),
            actual: false,
        }
    }
}

/// The dialog, while open.
pub fn window(ctx: &egui::Context, shell: &mut Shell) {
    let Some(mut preview) = shell.ui.preview.take() else {
        return;
    };
    let p = shell.palette;
    // take in the finished render
    if let State::Rendering(rx) = &preview.state {
        match rx.try_recv() {
            Ok(Ok(rendered)) => preview.state = State::Ready(Box::new(rendered), None),
            Ok(Err(e)) => preview.state = State::Failed(e),
            Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(TryRecvError::Disconnected) => {
                preview.state = State::Failed("The render stopped unexpectedly.".into());
            }
        }
    }
    if let State::Ready(rendered, texture @ None) = &mut preview.state {
        let image = egui::ColorImage::from_rgba_unmultiplied(rendered.size, &rendered.rgba);
        *texture = Some(ctx.load_texture("render-preview", image, egui::TextureOptions::LINEAR));
    }

    let mut save = false;
    let mut copy = false;
    let title = format!("Render Preview: frame {}", preview.frame);
    let closed = kit::modal(
        ctx,
        &p,
        "render-preview",
        &title,
        vec2(780.0, 560.0),
        Backdrop::Closes,
        |ui| {
            let room = ui.available_height() - 40.0;
            match &preview.state {
                State::Rendering(_) => {
                    ui.allocate_ui(vec2(ui.available_width(), room), |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Rendering the frame…");
                            });
                        });
                    });
                }
                State::Failed(why) => {
                    ui.allocate_ui(vec2(ui.available_width(), room), |ui| {
                        ui.label(RichText::new(why).color(p.error));
                    });
                }
                State::Ready(rendered, texture) => {
                    let Some(texture) = texture else { return };
                    let area = vec2(ui.available_width(), room);
                    let [w, h] = rendered.size;
                    let own = vec2(w as f32, h as f32) / ui.ctx().pixels_per_point();
                    let shown = if preview.actual {
                        own
                    } else {
                        own * (area.x / own.x).min(area.y / own.y).min(1.0)
                    };
                    egui::Frame::NONE.fill(p.pasteboard).show(ui, |ui| {
                        egui::ScrollArea::both()
                            .max_width(area.x)
                            .max_height(area.y)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.centered_and_justified(|ui| {
                                    ui.add(egui::Image::new((texture.id(), shown)));
                                });
                            });
                    });
                }
            }
            kit::footer(ui, |ui| {
                let ready = matches!(preview.state, State::Ready(..));
                if ui.add_enabled(ready, kit::primary(&p, "Save as PNG…")).clicked() {
                    save = true;
                }
                if ui.add_enabled(ready, egui::Button::new("Copy")).clicked() {
                    copy = true;
                }
                let label = if preview.actual {
                    "Fit to window"
                } else {
                    "Actual size"
                };
                if ui.add_enabled(ready, egui::Button::new(label)).clicked() {
                    preview.actual = !preview.actual;
                }
                if let State::Ready(r, _) = &preview.state {
                    ui.label(
                        RichText::new(format!("{} × {} px", r.size[0], r.size[1]))
                            .size(11.5)
                            .color(p.text_muted),
                    );
                }
            });
        },
    );

    if let State::Ready(rendered, _) = &preview.state {
        if copy {
            ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied(
                rendered.size,
                &rendered.rgba,
            ));
            shell
                .editor
                .set_status(StatusKind::Success, "Copied the frame to the clipboard");
        }
        if save {
            save_png(shell, &preview.name, preview.frame, rendered);
        }
    }
    if !closed {
        shell.ui.preview = Some(preview);
    }
}

/// Ask where, then write the picture. The folder is remembered like an
/// export's.
fn save_png(shell: &mut Shell, name: &str, frame: i32, rendered: &Rendered) {
    let file = format!("{}_{frame:04}.png", crate::saveas::clean_name(name));
    let mut dialog = rfd::FileDialog::new()
        .set_title("Save the frame as a PNG")
        .add_filter("PNG picture", &["png"])
        .set_file_name(file);
    if let Some(dir) = shell
        .last_export
        .as_deref()
        .filter(|d| d.is_dir())
        .or_else(|| shell.places.latest())
    {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut path) = dialog.save_file() else {
        return;
    };
    if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("png")) {
        path.set_extension("png");
    }
    match write_png(&path, rendered) {
        Ok(()) => {
            if let Some(dir) = path.parent() {
                shell.last_export = Some(dir.to_path_buf());
                shell.places.remember(dir);
            }
            shell.editor.set_status(
                StatusKind::Success,
                format!(
                    "Saved {}",
                    path.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                ),
            );
        }
        Err(e) => shell
            .editor
            .set_status(StatusKind::Error, format!("Could not save the picture: {e}")),
    }
}

/// Write the picture beside its final name and rename it into place, so a
/// failed write never leaves half a file where a good one was.
fn write_png(path: &Path, rendered: &Rendered) -> Result<(), String> {
    let [w, h] = rendered.size;
    let mut part: PathBuf = path.into();
    let mut name = part.file_name().unwrap_or_default().to_owned();
    name.push(".part");
    part.set_file_name(name);
    image::save_buffer_with_format(
        &part,
        &rendered.rgba,
        w as u32,
        h as u32,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| {
        let _ = std::fs::remove_file(&part);
        e.to_string()
    })?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, path).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_the_projects_size_but_not_huge() {
        assert_eq!(preview_size(1920.0, 1080.0), (1920, 1080));
        // a 7680 px wide project is halved to fit the limit
        assert_eq!(preview_size(7680.0, 4320.0), (3840, 2160));
        assert_eq!(preview_size(1080.0, 7680.0), (540, 3840));
        assert_eq!(preview_size(0.0, 0.0), (1, 1));
    }

    #[test]
    fn a_picture_is_written_in_one_piece() {
        let dir = std::env::temp_dir().join(format!("fanimate-preview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rendered = Rendered {
            size: [2, 2],
            rgba: vec![255; 16],
        };
        let path = dir.join("frame.png");
        write_png(&path, &rendered).unwrap();
        // writing again replaces it, and leaves no part file behind
        write_png(&path, &rendered).unwrap();
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        let back = image::open(&path).unwrap().into_rgba8();
        assert_eq!(back.dimensions(), (2, 2));
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Needs a real graphics adapter. Windows only: on the Linux CI runner the
    // software Vulkan driver crashes the test process at exit once a second
    // device has been made in it, and the renderer's own tests (in
    // fanimate-render) cover drawing on every platform.
    #[cfg(windows)]
    #[test]
    fn a_frame_renders_to_a_picture() {
        // skips where there is no graphics adapter
        let Ok(gpu) = fanimate_render::Gpu::headless() else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let doc = fanimate_core::samples::hello();
        let (w, h) = preview_size(doc.width, doc.height);
        let rgba = fanimate_render::render_to_rgba(&gpu, &doc, 0.0, w, h).unwrap();
        assert_eq!(rgba.len(), (w * h * 4) as usize);
    }
}
