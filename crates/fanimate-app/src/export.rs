//! Export Video: a dialog that adds a render to the queue, and the render
//! progress shown in the status bar. The render itself runs in the render
//! queue's own window, so the animator keeps working once it is queued.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, vec2};
use fanimate_core::Frame;
use fanimate_editor::StatusKind;
use fanimate_export::{Format, Queue, Settings, State};

use crate::app::Shell;
use crate::formats::{self, VideoTools};
use crate::kit::{self, Backdrop};
use crate::render_queue;

/// The Export Video window's choices.
pub struct ExportForm {
    format: Format,
    width: u32,
    height: u32,
    /// Change the height with the width, keeping the canvas's shape.
    keep_aspect: bool,
    start: Frame,
    end: Frame,
    /// The folder to write into, and the file's name without its extension
    /// (the format adds that).
    dir: PathBuf,
    name: String,
    /// Replace a file that is already there.
    overwrite: bool,
}

/// Why a render cannot be queued yet.
struct Problem {
    text: String,
    /// Setting up FFmpeg in Preferences would solve it.
    needs_ffmpeg: bool,
}

impl Problem {
    fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            needs_ffmpeg: false,
        }
    }
}

impl ExportForm {
    pub fn new(shell: &Shell, format: Format) -> Self {
        let doc = &shell.editor.doc;
        let s = Settings::for_doc(doc, format);
        let project = shell.editor.path.as_deref().and_then(Path::parent);
        // where the last render went, else next to the project, else Videos
        let dir = shell
            .last_export
            .clone()
            .filter(|d| d.is_dir())
            .or_else(|| project.map(Path::to_path_buf))
            .or_else(videos_folder)
            .unwrap_or_default();
        let name = shell
            .editor
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(|| doc.name.clone(), |s| s.to_string_lossy().into_owned());
        Self {
            format,
            width: s.width,
            height: s.height,
            keep_aspect: true,
            start: s.start,
            end: s.end,
            dir,
            name: safe_name(&name),
            overwrite: false,
        }
    }

    /// The render's settings, with the FFmpeg chosen in Preferences, if any.
    fn settings(&self, ffmpeg: Option<&Path>) -> Settings {
        Settings {
            format: self.format,
            width: self.width,
            height: self.height,
            start: self.start,
            end: self.end,
            ffmpeg: ffmpeg.map(Path::to_path_buf),
            overwrite: self.overwrite,
        }
    }

    /// The file this would write. The format's extension is added to the
    /// name as typed, so a name with a dot in it ("take 2.5") keeps it.
    fn output(&self) -> PathBuf {
        let ext = self.format.extension();
        let name = self.name.trim();
        let typed = Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(ext));
        self.dir.join(if typed {
            name.to_owned()
        } else {
            format!("{name}.{ext}")
        })
    }

    /// Why the render cannot be queued yet.
    fn problem(&self, tools: &VideoTools) -> Option<Problem> {
        if tools.looking() {
            return Some(Problem::new("Checking which formats this computer can make…"));
        }
        if let Err(why) = tools.maker(self.format) {
            return Some(Problem {
                text: why,
                needs_ffmpeg: true,
            });
        }
        let (min_w, min_h) = self.format.min_size();
        if self.width < min_w || self.height < min_h {
            return Some(Problem::new(format!(
                "{} needs a size of at least {min_w}x{min_h}.",
                self.format.label()
            )));
        }
        if self.dir.as_os_str().is_empty() || !self.dir.is_dir() {
            return Some(Problem::new("Choose a folder that exists to save into."));
        }
        let name = self.name.trim();
        if name.is_empty() {
            return Some(Problem::new("Give the file a name."));
        }
        if safe_name(name) != name {
            return Some(Problem::new(
                "The name cannot hold any of these characters: < > : \" / \\ | ? *",
            ));
        }
        if !self.overwrite && fanimate_export::existing_output(&self.output(), self.format).is_some() {
            return Some(Problem::new(
                "A file with this name is already there. Tick Overwrite existing file to replace it.",
            ));
        }
        None
    }
}

/// The user's Videos folder, if there is one.
fn videos_folder() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    let videos = home.join(if cfg!(target_os = "macos") {
        "Movies"
    } else {
        "Videos"
    });
    Some(if videos.is_dir() { videos } else { home })
}

/// A project name usable as a file name.
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        "Animation".into()
    } else {
        trimmed.into()
    }
}

/// The Export Video dialog, while open.
pub fn window(ctx: &egui::Context, shell: &mut Shell) {
    let Some(mut form) = shell.ui.export.take() else {
        return;
    };
    let p = shell.palette;
    let mut queue_it = false;
    let doc_aspect = shell.editor.doc.width / shell.editor.doc.height.max(1.0);
    let (doc_start, doc_end) = (shell.editor.doc.start, shell.editor.doc.end);
    let closed = kit::modal(
        ctx,
        &p,
        "export",
        "Export Video",
        vec2(500.0, 600.0),
        // a click beside the dialog must not throw away a typed name
        Backdrop::Ignored,
        |ui| {
            kit::section(ui, &p, "Format", |ui| {
                kit::form(ui, "export-format-form", |ui| {
                    ui.label("Format");
                    egui::ComboBox::from_id_salt("export-format")
                        .selected_text(form.format.label())
                        .width(260.0)
                        // every format at once, without scrolling
                        .height(560.0)
                        .show_ui(ui, |ui| {
                            for (i, (group, formats)) in formats::groups().into_iter().enumerate() {
                                if i > 0 {
                                    ui.separator();
                                }
                                ui.label(RichText::new(group).size(11.0).color(p.text_muted));
                                for f in formats.iter().copied() {
                                    // formats this computer cannot make yet stay listed,
                                    // so choosing one explains what they need
                                    let text = if shell.video.maker(f).is_ok() || shell.video.looking() {
                                        RichText::new(f.label())
                                    } else {
                                        RichText::new(format!("{} (needs FFmpeg)", f.label()))
                                            .color(p.text_muted)
                                    };
                                    ui.selectable_value(&mut form.format, f, text)
                                        .on_hover_text(f.hint());
                                }
                            }
                        });
                    ui.end_row();
                });
                ui.add_space(4.0);
                kit::note(ui, &p, form.format.hint());
                if let Ok(maker) = shell.video.maker(form.format) {
                    kit::note(ui, &p, maker);
                }
            });

            kit::section(ui, &p, "Size", |ui| {
                kit::form(ui, "export-size-form", |ui| {
                    kit::row(ui, "Picture size", |ui| {
                        let w = ui.add(
                            egui::DragValue::new(&mut form.width)
                                .range(2..=16384)
                                .suffix(" px"),
                        );
                        ui.label("×");
                        let h = ui.add(
                            egui::DragValue::new(&mut form.height)
                                .range(2..=16384)
                                .suffix(" px"),
                        );
                        if form.keep_aspect && w.changed() {
                            form.height = (form.width as f32 / doc_aspect).round().max(2.0) as u32;
                        }
                        if form.keep_aspect && h.changed() {
                            form.width = (form.height as f32 * doc_aspect).round().max(2.0) as u32;
                        }
                    });
                    kit::row(ui, "", |ui| {
                        ui.checkbox(&mut form.keep_aspect, "Keep shape");
                        for (label, scale) in [("100%", 1.0), ("50%", 0.5)] {
                            if ui.small_button(label).clicked() {
                                form.width = (shell.editor.doc.width * scale).round().max(2.0) as u32;
                                form.height = (shell.editor.doc.height * scale).round().max(2.0) as u32;
                            }
                        }
                    });
                });
            });

            kit::section(ui, &p, "Frames", |ui| {
                kit::form(ui, "export-frames-form", |ui| {
                    kit::row(ui, "Frames", |ui| {
                        let last = form.end;
                        ui.add(egui::DragValue::new(&mut form.start).range(0..=last));
                        ui.label("to");
                        let first = form.start;
                        ui.add(egui::DragValue::new(&mut form.end).range(first..=Frame::MAX));
                        let whole = (form.start, form.end) == (doc_start, doc_end);
                        if ui
                            .add_enabled(!whole, egui::Button::new("Entire animation"))
                            .on_hover_text("The first to the last frame of the project")
                            .clicked()
                        {
                            form.start = doc_start;
                            form.end = doc_end;
                        }
                    });
                });
            });

            kit::section(ui, &p, "Output", |ui| {
                kit::form(ui, "export-output-form", |ui| {
                    kit::row(ui, "Folder", |ui| {
                        crate::places::folder_picker(
                            ui,
                            "export-folder",
                            &mut form.dir,
                            &shell.places,
                            shell.editor.path.as_deref().and_then(Path::parent),
                            "Export into",
                        );
                    });
                    kit::row(ui, "File name", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(240.0));
                        ui.label(RichText::new(format!(".{}", form.format.extension())).color(p.text_muted));
                    });
                    kit::row(ui, "", |ui| {
                        ui.checkbox(&mut form.overwrite, "Overwrite existing file");
                    });
                });
            });

            let s = form.settings(None).sanitized();
            let seconds = s.frame_count() as f32 / shell.editor.doc.fps.max(1.0);
            let mut notes = vec![format!("{} frames, {seconds:.1} seconds", s.frame_count())];
            if form.format.has_sound() && !fanimate_export::sound::voices(&shell.editor.doc).is_empty() {
                notes.push("with sound".into());
            } else if !form.format.has_sound() {
                notes.push("no sound".into());
            }
            if (s.width, s.height) != (form.width, form.height) {
                notes.push(format!("made {}x{} for video", s.width, s.height));
            }
            kit::note(ui, &p, notes.join(", "));
            let problem = form.problem(&shell.video);
            if let Some(problem) = &problem {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&problem.text).size(11.5).color(p.error));
                    if problem.needs_ffmpeg && ui.small_button("Set up FFmpeg…").clicked() {
                        shell.ui.show_preferences = true;
                        shell.ui.prefs_page = crate::dialogs::PrefsPage::Video;
                    }
                });
            } else if fanimate_export::existing_output(&form.output(), form.format).is_some() {
                kit::note(ui, &p, "The file already there will be replaced.");
            }
            kit::footer(ui, |ui| {
                let add = ui.add_enabled(problem.is_none(), kit::primary(&p, "Add to Render Queue"));
                if add.clicked() {
                    queue_it = true;
                }
                ui.label(
                    RichText::new("Renders in its own window; you can keep working.")
                        .size(11.5)
                        .color(p.text_muted),
                );
            });
        },
    );
    shell.export_format = form.format;
    if queue_it {
        add_to_queue(shell, &form);
    } else if !closed {
        shell.ui.export = Some(form);
    }
}

fn add_to_queue(shell: &mut Shell, form: &ExportForm) {
    let Some(queue) = render_queue::queue() else {
        shell.editor.set_status(
            StatusKind::Error,
            "There is no folder to keep the render queue in",
        );
        return;
    };
    let output = form.output();
    shell.last_export = Some(form.dir.clone());
    shell.places.remember(&form.dir);
    let doc = shell.editor.doc.mainline();
    match queue.add(&doc, &output, &form.settings(shell.video.chosen.as_deref())) {
        Ok(id) => {
            shell.renders.watch(id);
            match render_queue::open(shell.theme) {
                Ok(()) => shell.editor.set_status(
                    StatusKind::Info,
                    format!("Rendering {} in the render queue", file_name(&output)),
                ),
                Err(e) => shell.editor.set_status(StatusKind::Error, e),
            }
        }
        Err(e) => shell
            .editor
            .set_status(StatusKind::Error, format!("The render could not be queued: {e}")),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |f| f.to_string_lossy().into_owned(),
    )
}

/// Renders this editor queued, followed for the status bar.
#[derive(Default)]
pub struct Renders {
    watching: Vec<String>,
    polled: Option<Instant>,
    /// The render in progress: file name and how far along.
    pub active: Option<(String, f32)>,
}

impl Renders {
    pub fn watch(&mut self, id: String) {
        self.watching.push(id);
        self.polled = None;
    }

    /// Read the queue now and then; report renders that finished.
    pub fn poll(&mut self, ctx: &egui::Context, shell_status: &mut dyn FnMut(StatusKind, String)) {
        if self.watching.is_empty() {
            self.active = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        if self
            .polled
            .is_some_and(|t| t.elapsed() < Duration::from_millis(500))
        {
            return;
        }
        self.polled = Some(Instant::now());
        let Some(queue) = render_queue::queue() else {
            return;
        };
        self.update(&queue, shell_status);
    }

    fn update(&mut self, queue: &Queue, status: &mut dyn FnMut(StatusKind, String)) {
        let jobs = queue.jobs();
        self.active = None;
        self.watching.retain(|id| {
            let Some(e) = jobs.iter().find(|e| &e.job.id == id) else {
                return false; // removed in the queue window
            };
            let name = file_name(&e.job.output);
            match e.status.state {
                State::Done => {
                    status(StatusKind::Success, format!("Exported {name}"));
                    false
                }
                State::Failed => {
                    let why = e.status.message.as_deref().unwrap_or("no reason given");
                    status(StatusKind::Error, format!("Could not export {name}: {why}"));
                    false
                }
                State::Cancelled => false,
                State::Rendering => {
                    self.active = Some((name, e.status.fraction()));
                    true
                }
                State::Waiting => true,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_names_become_safe_file_names() {
        assert_eq!(safe_name("Walk: take 2?"), "Walk_ take 2_");
        assert_eq!(safe_name("  "), "Animation");
        assert_eq!(safe_name("end."), "end");
    }

    fn form(dir: &Path, name: &str) -> ExportForm {
        ExportForm {
            format: Format::Gif,
            width: 64,
            height: 36,
            keep_aspect: true,
            start: 0,
            end: 9,
            dir: dir.to_path_buf(),
            name: name.into(),
            overwrite: false,
        }
    }

    #[test]
    fn the_extension_is_added_to_the_name_as_typed() {
        let d = Path::new("/out");
        assert_eq!(form(d, "walk").output(), d.join("walk.gif"));
        // a dot in the name is part of it
        assert_eq!(form(d, "take 2.5").output(), d.join("take 2.5.gif"));
        // typing the extension does not double it
        assert_eq!(form(d, "walk.GIF").output(), d.join("walk.GIF"));
        assert_eq!(form(d, "  walk ").output(), d.join("walk.gif"));
    }

    #[test]
    fn a_file_already_there_stops_the_export_unless_it_may_be_replaced() {
        let dir = std::env::temp_dir().join(format!("fanimate-export-form-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // not looking for encoders, so GIF (made by Fanimate) is simply fine
        let tools = VideoTools::default();
        let mut f = form(&dir, "walk");
        let problem = |f: &ExportForm| f.problem(&tools).map(|p| p.text);
        assert_eq!(problem(&f), None);
        std::fs::write(dir.join("walk.gif"), b"old").unwrap();
        let blocked = problem(&f).expect("the file is there");
        assert!(blocked.contains("Overwrite existing file"), "{blocked}");
        f.overwrite = true;
        assert_eq!(problem(&f), None);
        f.name = "a/b".into();
        assert!(problem(&f).unwrap().contains("cannot hold"));
        f.name = "".into();
        assert!(problem(&f).unwrap().contains("name"));
        f.dir = dir.join("missing");
        f.name = "walk".into();
        assert!(problem(&f).unwrap().contains("folder"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn finished_renders_are_reported_once() {
        let dir = std::env::temp_dir().join(format!("fanimate-renders-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let queue = Queue::new(&dir);
        let doc = fanimate_core::samples::hello();
        let s = Settings::for_doc(&doc, Format::Gif);
        let id = queue.add(&doc, Path::new("/x/walk.gif"), &s).unwrap();
        let mut renders = Renders::default();
        renders.watch(id.clone());
        let mut said = Vec::new();
        let rendering = fanimate_export::Status {
            state: State::Rendering,
            done: 12,
            total: 48,
            message: None,
        };
        queue.set_status(&id, &rendering).unwrap();
        renders.update(&queue, &mut |_, m| said.push(m));
        assert_eq!(renders.active, Some(("walk.gif".into(), 0.25)));
        let done = fanimate_export::Status {
            state: State::Done,
            ..rendering
        };
        queue.set_status(&id, &done).unwrap();
        renders.update(&queue, &mut |_, m| said.push(m));
        renders.update(&queue, &mut |_, m| said.push(m));
        assert_eq!(said, ["Exported walk.gif"]);
        assert!(renders.active.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
