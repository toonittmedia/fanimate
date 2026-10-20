//! The render queue window (`fanimate --render-queue`): a small window of
//! its own process that runs exports one after another, so a render goes
//! on after the editor closes.
//!
//! The editor adds jobs to the queue folder (`fanimate_export::Queue`) and
//! opens this window if it is not open already. The window shows each
//! job's progress and lets the animator reorder, cancel and remove jobs and
//! open what was made. A worker thread renders; the window only reads the
//! queue and asks the worker to stop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui::{self, Id, RichText};
use fanimate_export::queue::RunnerLock;
use fanimate_export::{Entry, ExportError, Queue, State, Status};

use crate::icons::{self, Icon};
use crate::menus;
use crate::theme::{self, Palette, Theme};

/// How often the window reads the queue.
const POLL: Duration = Duration::from_millis(250);
/// How often the worker writes progress.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);

/// The queue folder, beside the crash recovery folder.
pub fn root() -> Option<PathBuf> {
    eframe::storage_dir("Fanimate").map(|dir| dir.join("render-queue"))
}

pub fn queue() -> Option<Queue> {
    root().map(Queue::new)
}

/// Open the queue window in its own process, unless it is open. Its theme
/// follows the editor's.
pub fn open(theme: Theme) -> Result<(), String> {
    let queue = queue().ok_or("there is no folder to keep the render queue in")?;
    if queue.has_runner() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["--render-queue", "--theme", theme.id()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // its own process group, so a terminal's Ctrl+C for the editor does not
    // stop the render
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("the render queue could not start: {e}"))?;
    // reap it when it ends, so it leaves no zombie behind while the editor runs
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Run the queue window until it is closed.
pub fn run(theme: Theme) -> eframe::Result {
    let Some(queue) = queue() else {
        crate::report("Fanimate: there is no folder to keep the render queue in.");
        return Ok(());
    };
    // another queue window is open and will run the jobs
    let Some(lock) = queue.try_run() else {
        return Ok(());
    };
    eframe::run_native(
        // its own name, so its saved window never touches the editor's
        "Fanimate Render Queue",
        native_options(),
        Box::new(move |cc| Ok(Box::new(QueueApp::new(cc, queue, lock, theme)))),
    )
}

fn native_options() -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Render Queue - Fanimate")
        .with_app_id("fanimate-render-queue")
        .with_inner_size([560.0, 420.0])
        .with_min_inner_size([420.0, 260.0]);
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../brand/app-icon/fanimate-256.png"))
    {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    eframe::NativeOptions {
        viewport,
        wgpu_options: crate::wgpu_options(),
        // closed while minimised (from the taskbar), eframe would save the
        // minimised window and open it shrunk into a corner from then on;
        // `QueueApp::save` clears what older versions saved
        persist_window: false,
        ..Default::default()
    }
}

/// The rendering thread, and how to stop it.
struct Worker {
    /// Stop the job being rendered.
    cancel: Arc<AtomicBool>,
    /// Stop altogether: the window is closing.
    quit: Arc<AtomicBool>,
    /// The job being rendered.
    current: Arc<Mutex<Option<String>>>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    fn start(queue: Queue, ctx: egui::Context) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let quit = Arc::new(AtomicBool::new(false));
        let current = Arc::new(Mutex::new(None));
        let handle = {
            let (cancel, quit, current) = (cancel.clone(), quit.clone(), current.clone());
            std::thread::Builder::new()
                .name("fanimate-render".into())
                .spawn(move || work(&queue, &ctx, &cancel, &quit, &current))
                .ok()
        };
        Self {
            cancel,
            quit,
            current,
            handle,
        }
    }

    fn current(&self) -> Option<String> {
        self.current.lock().ok().and_then(|c| c.clone())
    }

    fn stop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Render waiting jobs in order until told to quit.
fn work(
    queue: &Queue,
    ctx: &egui::Context,
    cancel: &AtomicBool,
    quit: &AtomicBool,
    current: &Mutex<Option<String>>,
) {
    // a window closed or crashed mid-render left its job half done
    queue.recover();
    while !quit.load(Ordering::Relaxed) {
        let Some(job) = queue.next_waiting() else {
            std::thread::sleep(Duration::from_millis(400));
            continue;
        };
        if let Ok(mut c) = current.lock() {
            *c = Some(job.id.clone());
        }
        cancel.store(false, Ordering::Relaxed);
        let total = job.settings.sanitized().frame_count();
        let rendering = |done| Status {
            state: State::Rendering,
            done,
            total,
            message: None,
        };
        let _ = queue.set_status(&job.id, &rendering(0));
        ctx.request_repaint();

        let mut last = Instant::now();
        let result = match fanimate_fnm::load(queue.project(&job.id)) {
            Ok((doc, _)) => fanimate_export::export(&doc, &job.settings, &job.output, &mut |done, _| {
                if last.elapsed() >= PROGRESS_EVERY {
                    let _ = queue.set_status(&job.id, &rendering(done));
                    ctx.request_repaint();
                    last = Instant::now();
                }
                !cancel.load(Ordering::Relaxed) && !quit.load(Ordering::Relaxed)
            })
            .map_err(|e| match e {
                ExportError::Cancelled => None,
                e => Some(e.to_string()),
            }),
            Err(e) => Err(Some(format!(
                "the queued copy of the project could not be opened: {e}"
            ))),
        };
        let end = match result {
            Ok(()) => Status {
                state: State::Done,
                done: total,
                total,
                message: None,
            },
            // closing the window pauses the queue: the job starts over next time
            Err(None) if quit.load(Ordering::Relaxed) => Status {
                total,
                ..Default::default()
            },
            Err(None) => Status {
                state: State::Cancelled,
                total,
                ..Default::default()
            },
            Err(Some(message)) => Status {
                state: State::Failed,
                done: 0,
                total,
                message: Some(message),
            },
        };
        let _ = queue.set_status(&job.id, &end);
        if let Ok(mut c) = current.lock() {
            *c = None;
        }
        ctx.request_repaint();
    }
}

struct QueueApp {
    queue: Queue,
    palette: Palette,
    entries: Vec<Entry>,
    polled: Instant,
    worker: Worker,
    /// Asking whether to stop rendering and close.
    confirm_close: bool,
    allow_close: bool,
    /// The job being dragged to a new place.
    dragging: Option<String>,
    taskbar: crate::taskbar::Taskbar,
    _lock: RunnerLock,
}

impl QueueApp {
    fn new(cc: &eframe::CreationContext<'_>, queue: Queue, lock: RunnerLock, theme: Theme) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let palette = theme::apply(&cc.egui_ctx, theme);
        let worker = Worker::start(queue.clone(), cc.egui_ctx.clone());
        Self {
            entries: queue.jobs(),
            queue,
            palette,
            polled: Instant::now(),
            worker,
            confirm_close: false,
            allow_close: false,
            dragging: None,
            taskbar: Default::default(),
            _lock: lock,
        }
    }

    /// The taskbar button: how far the render in progress is; a bar that
    /// waits (jobs queued, none running yet) moves without a position; red
    /// when the last thing that ended failed.
    fn show_taskbar_progress(&mut self, frame: &eframe::Frame) {
        use fanimate_export::State;
        use fanimate_oscodec::TaskbarProgress;
        let wanted = if let Some(e) = self.entries.iter().find(|e| e.status.state == State::Rendering) {
            TaskbarProgress::Fraction(e.status.fraction())
        } else if self.entries.iter().any(|e| e.status.state == State::Waiting) {
            TaskbarProgress::Busy
        } else if self.entries.iter().any(|e| e.status.state == State::Failed) {
            TaskbarProgress::Failed(1.0)
        } else {
            TaskbarProgress::Off
        };
        self.taskbar.show(frame, wanted);
    }

    fn refresh(&mut self) {
        self.entries = self.queue.jobs();
        self.polled = Instant::now();
    }

    fn rendering(&self) -> Option<&Entry> {
        let current = self.worker.current()?;
        self.entries.iter().find(|e| e.job.id == current)
    }

    /// Jobs rendering or waiting, read from the queue itself: the list on
    /// screen can be a poll behind, or not drawn at all while minimised.
    fn unfinished(&self) -> usize {
        let waiting = self
            .queue
            .jobs()
            .iter()
            .filter(|e| !e.status.state.is_finished())
            .count();
        // the worker may have taken a job before its status says so
        waiting.max(usize::from(self.worker.current().is_some()))
    }

    /// Stop job `id`: the running one through the worker, a waiting one
    /// by marking it.
    fn cancel(&mut self, id: &str) {
        if self.worker.current().as_deref() == Some(id) {
            self.worker.cancel.store(true, Ordering::Relaxed);
        } else if let Some(e) = self.entries.iter().find(|e| e.job.id == id) {
            let status = Status {
                state: State::Cancelled,
                ..e.status.clone()
            };
            let _ = self.queue.set_status(id, &status);
        }
        self.refresh();
    }

    /// Put a cancelled or failed job back in line.
    fn retry(&mut self, id: &str) {
        if let Some(e) = self.entries.iter().find(|e| e.job.id == id) {
            let status = Status {
                total: e.status.total,
                ..Default::default()
            };
            let _ = self.queue.set_status(id, &status);
        }
        self.refresh();
    }

    fn remove(&mut self, id: &str) {
        if self.worker.current().as_deref() != Some(id) {
            let _ = self.queue.remove(id);
        }
        self.refresh();
    }

    fn clear_finished(&mut self) {
        for e in self.entries.iter().filter(|e| e.status.state.is_finished()) {
            let _ = self.queue.remove(&e.job.id);
        }
        self.refresh();
    }
}

impl eframe::App for QueueApp {
    // here, not in `ui`: eframe skips `ui` while the window is minimised or
    // hidden, and closing it from the taskbar then must still ask
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.show_taskbar_progress(frame);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.unfinished() > 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            // bring the window back so the question is seen
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.confirm_close = true;
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.polled.elapsed() >= POLL {
            self.refresh();
        }
        let busy = self.entries.iter().any(|e| !e.status.state.is_finished());
        if busy {
            ctx.request_repaint_after(POLL);
        }

        let p = self.palette;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.bg).inner_margin(12.0))
            .show(ui, |ui| {
                self.header(ui);
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(ui.available_height() - 28.0)
                    .show(ui, |ui| self.list(ui));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Rendering goes on when the editor closes.")
                        .size(11.0)
                        .color(p.text_muted),
                );
            });
        if self.confirm_close {
            self.confirm_close_dialog(&ctx);
        }
    }

    fn on_exit(&mut self) {
        self.worker.stop();
    }

    // the window opens at its default size every time (`persist_window` is
    // off), so drop any size an earlier version saved: one saved while
    // minimised opens the window shrunk into a corner. eframe loads it
    // even when not saving it; "window" is its (private) key.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("window", String::new());
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

impl QueueApp {
    fn header(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let count = |s: State| self.entries.iter().filter(|e| e.status.state == s).count();
        let (waiting, rendering, done) = (count(State::Waiting), count(State::Rendering), count(State::Done));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Render Queue").size(16.0).strong());
            let summary = match (rendering, waiting) {
                (0, 0) if done > 0 => "All done".to_owned(),
                (0, 0) => "Nothing queued".to_owned(),
                (_, 0) => "Rendering".to_owned(),
                (_, w) => format!("{w} waiting"),
            };
            ui.label(RichText::new(summary).color(p.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let finished = self.entries.iter().any(|e| e.status.state.is_finished());
                if ui
                    .add_enabled(finished, egui::Button::new("Clear Finished"))
                    .clicked()
                {
                    self.clear_finished();
                }
            });
        });
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        if self.entries.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No renders yet.").color(p.text_muted));
                ui.label(
                    RichText::new("Add one from the editor with File > Export Video…")
                        .size(11.0)
                        .color(p.text_muted),
                );
            });
            return;
        }
        let entries = self.entries.clone();
        let mut rows = Vec::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            rows.push(self.row(ui, e, i, entries.len()));
            ui.add_space(6.0);
        }
        // drag a row by its handle to a new place
        if let Some(id) = self.dragging.clone() {
            let pointer = ui.ctx().pointer_interact_pos();
            if let Some(pos) = pointer {
                let target = rows
                    .iter()
                    .position(|r| pos.y < r.center().y)
                    .unwrap_or(rows.len());
                let line = rows
                    .get(target)
                    .map_or_else(|| rows[rows.len() - 1].bottom() + 3.0, |r| r.top() - 3.0);
                let x = rows[0].x_range();
                ui.painter().hline(x, line, egui::Stroke::new(2.0, p.accent));
                if ui.input(|i| i.pointer.any_released()) {
                    if let Some(from) = entries.iter().position(|e| e.job.id == id) {
                        let to = if target > from { target - 1 } else { target };
                        let _ = self.queue.move_by(&id, to as isize - from as isize);
                    }
                    self.dragging = None;
                    self.refresh();
                }
            } else {
                self.dragging = None;
            }
        }
    }

    /// One job: name, where it goes, progress and what can be done with it.
    /// Returns the row's rectangle.
    fn row(&mut self, ui: &mut egui::Ui, e: &Entry, index: usize, count: usize) -> egui::Rect {
        let p = self.palette;
        let id = e.job.id.clone();
        let state = e.status.state;
        let frame = egui::Frame::NONE
            .fill(p.panel)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(6.0)
            .inner_margin(10.0);
        let mut action: Option<RowAction> = None;
        let resp = clickable_row(ui, Id::new(("queue-row", &id)), frame, |ui| {
            ui.set_min_width(ui.available_width());
            // buttons first, from the right, so long names and folders are
            // cut short before them rather than running underneath
            let size = egui::vec2(ui.available_width(), 36.0);
            ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Down, 24.0, false, index + 1 == count)
                    .on_hover_text("Render later")
                    .clicked()
                {
                    action = Some(RowAction::Move(1));
                }
                if icons::button(ui, Icon::Up, 24.0, false, index == 0)
                    .on_hover_text("Render sooner")
                    .clicked()
                {
                    action = Some(RowAction::Move(-1));
                }
                match state {
                    State::Waiting | State::Rendering => {
                        if ui.button("Cancel").clicked() {
                            action = Some(RowAction::Cancel);
                        }
                    }
                    State::Done => {
                        if ui.button("Show in Folder").clicked() {
                            action = Some(RowAction::ShowFolder);
                        }
                        if ui.button("Open").clicked() {
                            action = Some(RowAction::Open);
                        }
                    }
                    State::Failed | State::Cancelled => {
                        if ui.button("Render Again").clicked() {
                            action = Some(RowAction::Retry);
                        }
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let (grip, handle) = ui.allocate_exact_size(egui::vec2(16.0, 32.0), egui::Sense::drag());
                    icons::paint(ui.painter(), grip, Icon::Grip, p.text_muted);
                    if handle.on_hover_text("Drag to change the order").drag_started() {
                        self.dragging = Some(id.clone());
                    }
                    ui.vertical(|ui| {
                        let title =
                            format!("{}  {}", file_name(&e.job.output), e.job.settings.format.label());
                        ui.add(egui::Label::new(RichText::new(title).strong()).truncate());
                        let s = &e.job.settings;
                        let detail = format!(
                            "{}x{}, frames {} to {}, in {}",
                            s.width,
                            s.height,
                            s.start,
                            s.end,
                            folder(&e.job.output)
                        );
                        ui.add(
                            egui::Label::new(RichText::new(detail.clone()).size(11.0).color(p.text_muted))
                                .truncate(),
                        )
                        .on_hover_text(format!("{detail}\nfrom the project \"{}\"", e.job.name));
                    });
                });
            });
            ui.add_space(6.0);
            let (text, color) = status_text(&e.status, p);
            let bar = egui::ProgressBar::new(e.status.fraction())
                .desired_height(6.0)
                .fill(if state == State::Failed { p.error } else { p.accent });
            ui.add(bar);
            ui.label(RichText::new(text).size(11.0).color(color));
        });
        let rect = resp.rect;
        menus::context_menu(&resp, |ui| {
            let mut item = |ui: &mut egui::Ui, label: &str, enabled: bool, a: RowAction| {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    action = Some(a);
                    ui.close();
                }
            };
            item(ui, "Open", state == State::Done, RowAction::Open);
            item(ui, "Show in Folder", state == State::Done, RowAction::ShowFolder);
            ui.separator();
            item(ui, "Render Sooner", index > 0, RowAction::Move(-1));
            item(ui, "Render Later", index + 1 < count, RowAction::Move(1));
            ui.separator();
            item(ui, "Cancel", !state.is_finished(), RowAction::Cancel);
            item(
                ui,
                "Render Again",
                matches!(state, State::Failed | State::Cancelled),
                RowAction::Retry,
            );
            item(
                ui,
                "Remove from Queue",
                state != State::Rendering,
                RowAction::Remove,
            );
        });
        match action {
            Some(RowAction::Move(by)) => {
                let _ = self.queue.move_by(&id, by);
                self.refresh();
            }
            Some(RowAction::Cancel) => self.cancel(&id),
            Some(RowAction::Retry) => self.retry(&id),
            Some(RowAction::Remove) => self.remove(&id),
            Some(RowAction::Open) => open_path(&e.job.output),
            Some(RowAction::ShowFolder) => show_in_folder(&e.job.output),
            None => {}
        }
        rect
    }

    fn confirm_close_dialog(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        let text = match self.rendering() {
            Some(e) => format!(
                "{} is not finished. Closing stops it; it starts again, with the jobs still waiting, the next time the render queue opens.",
                file_name(&e.job.output)
            ),
            None => {
                "Renders are still waiting. Closing leaves them for the next time the render queue opens."
                    .to_owned()
            }
        };
        let modal = egui::Modal::new(Id::new("confirm-close")).show(ctx, |ui| {
            ui.set_max_width(360.0);
            ui.label(RichText::new("Stop rendering?").size(15.0).strong());
            ui.add_space(4.0);
            ui.label(text);
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(RichText::new("Keep Rendering").color(p.on_button)).fill(p.button))
                    .clicked()
                {
                    self.confirm_close = false;
                }
                if ui.button("Stop and Close").clicked() {
                    self.allow_close = true;
                    self.confirm_close = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
        if modal.should_close() {
            self.confirm_close = false;
        }
    }
}

#[derive(Clone, Copy)]
enum RowAction {
    Move(isize),
    Cancel,
    Retry,
    Remove,
    Open,
    ShowFolder,
}

/// A framed row that senses clicks (for its context menu) below the widgets
/// inside it, so its buttons still get their own clicks. An interact added
/// over the row after its contents would sit on top and take them.
fn clickable_row(
    ui: &mut egui::Ui,
    id: Id,
    frame: egui::Frame,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let row = egui::UiBuilder::new().id_salt(id).sense(egui::Sense::click());
    ui.scope_builder(row, |ui| frame.show(ui, contents)).response
}

fn status_text(s: &Status, p: Palette) -> (String, egui::Color32) {
    match s.state {
        State::Waiting => ("Waiting".into(), p.text_muted),
        State::Rendering => (format!("Rendering frame {} of {}", s.done + 1, s.total), p.text),
        State::Done => ("Done".into(), p.success),
        State::Cancelled => ("Cancelled".into(), p.text_muted),
        State::Failed => (
            format!("Failed: {}", s.message.as_deref().unwrap_or("no reason given")),
            p.error,
        ),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |f| f.to_string_lossy().into_owned(),
    )
}

fn folder(path: &Path) -> String {
    path.parent()
        .map_or_else(String::new, |d| d.display().to_string())
}

/// Open a file or folder with the system's default app.
fn open_path(path: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(path).spawn() {
        log::warn!("could not open {}: {e}", path.display());
    }
}

/// Open the folder holding `file` with the file selected, where the system
/// can (Explorer, Finder); elsewhere just open the folder.
fn show_in_folder(file: &Path) {
    let mut cmd;
    #[cfg(windows)]
    {
        // Explorer wants `/select,"path"`; the standard quoting wraps the
        // whole argument instead, so pass it as written
        use std::os::windows::process::CommandExt;
        let mut select = std::ffi::OsString::from("/select,\"");
        select.push(file);
        select.push("\"");
        cmd = std::process::Command::new("explorer");
        cmd.raw_arg(select);
    }
    #[cfg(target_os = "macos")]
    {
        cmd = std::process::Command::new("open");
        cmd.arg("-R").arg(file);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let Some(dir) = file.parent() else { return };
        cmd = std::process::Command::new("xdg-open");
        cmd.arg(dir);
    }
    if let Err(e) = cmd.spawn() {
        log::warn!("could not show {}: {e}", file.display());
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;
    use eframe::egui_wgpu::WgpuSetup;

    /// The queue window uses the editor's GPU backends: eframe's default
    /// loads Vulkan, which crashed the window on open on an Intel HD 520.
    #[test]
    fn queue_window_uses_the_renderer_backends() {
        let WgpuSetup::CreateNew(setup) = super::native_options().wgpu_options.wgpu_setup else {
            panic!("the queue window should create its own wgpu instance");
        };
        assert_eq!(setup.instance_descriptor.backends, fanimate_render::backends());
    }

    /// Closing the window mid-render ends the render for good: the worker
    /// stops within a frame, the encoder (the system's or FFmpeg) is
    /// stopped, its unfinished file is removed and the job waits to start
    /// over.
    #[test]
    fn stopping_the_worker_ends_the_render() {
        use fanimate_export::ffmpeg::{Ffmpeg, find};
        use fanimate_export::{Format, Queue, Settings, State};
        use std::time::{Duration, Instant};
        let ffmpeg = find(None).and_then(|p| Ffmpeg::probe(&p).ok());
        let mp4 = fanimate_export::route(Format::Mp4, fanimate_oscodec::h264_available(), ffmpeg.as_ref());
        if fanimate_render::Gpu::headless().is_err() || mp4.is_err() {
            eprintln!("no GPU adapter or nothing to make MP4 with; skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("fanimate-queue-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let queue = Queue::new(dir.join("queue"));
        let out = dir.join("long.mp4");
        // far longer than the test waits
        let settings = Settings {
            format: Format::Mp4,
            width: 320,
            height: 180,
            start: 0,
            end: 100_000,
            ffmpeg: None,
            overwrite: true,
        };
        let id = queue
            .add(&fanimate_core::samples::hello(), &out, &settings)
            .unwrap();
        let status = || queue.jobs().into_iter().find(|e| e.job.id == id).unwrap().status;

        let mut worker = super::Worker::start(queue.clone(), egui::Context::default());
        let started = Instant::now();
        while status().done < 10 {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "the render never got going"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let stopping = Instant::now();
        worker.stop();
        assert!(
            stopping.elapsed() < Duration::from_secs(5),
            "the render took too long to stop"
        );

        assert_eq!(status().state, State::Waiting, "it starts over next time");
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n != "queue")
            .collect();
        assert!(left.is_empty(), "nothing is left beside the output: {left:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Click `button` at `pos` (press, then release) over two frames.
    fn click(pos: egui::Pos2, button: egui::PointerButton) -> [Vec<egui::Event>; 2] {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        [
            vec![egui::Event::PointerMoved(pos), press(true)],
            vec![press(false)],
        ]
    }

    /// A row's buttons get their clicks and the row its right-clicks. The
    /// Open and Show in Folder buttons once did nothing: the row's own
    /// click area, added after them, was on top.
    #[test]
    fn buttons_in_a_row_get_their_clicks() {
        use std::cell::Cell;
        let ctx = egui::Context::default();
        let (button_rect, row_rect) = (Cell::new(egui::Rect::NOTHING), Cell::new(egui::Rect::NOTHING));
        let (button_clicks, row_clicks, row_menus) = (Cell::new(0), Cell::new(0), Cell::new(0));
        let run = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let row = super::clickable_row(
                    ui,
                    egui::Id::new("row"),
                    egui::Frame::NONE.inner_margin(10.0),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Hello Fanimate.mp4");
                            let open = ui.button("Open");
                            button_rect.set(open.rect);
                            button_clicks.set(button_clicks.get() + usize::from(open.clicked()));
                        });
                    },
                );
                row_rect.set(row.rect);
                row_clicks.set(row_clicks.get() + usize::from(row.clicked()));
                row_menus.set(row_menus.get() + usize::from(row.secondary_clicked()));
            });
            // no renderer to upload the font atlas to
            out.textures_delta.clear();
        };
        // a first frame to lay the row out, so the clicks find its widgets
        run(Vec::new());
        for events in click(button_rect.get().center(), egui::PointerButton::Primary) {
            run(events);
        }
        // in the row's margin, away from its widgets
        let margin = row_rect.get().left_top() + egui::vec2(4.0, 4.0);
        for events in click(margin, egui::PointerButton::Secondary) {
            run(events);
        }
        assert_eq!(
            (button_clicks.get(), row_clicks.get()),
            (1, 0),
            "the button takes its click"
        );
        assert_eq!(row_menus.get(), 1, "a right-click still reaches the row");
    }
}
