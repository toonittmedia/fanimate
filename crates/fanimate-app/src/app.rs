//! The app: window chrome, docking layout, dialogs and command dispatch.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, CursorIcon, Key, Modifiers};
use fanimate_core::LayerId;
use fanimate_editor::{Action, Chord, DrawStyle, Editor, Keymap, KeymapKind, Outcome, StatusKind};
use serde::{Deserialize, Serialize};

use crate::canvas_gpu::CanvasGpu;
use crate::docs::Startup;
use crate::layout::Layout;
use crate::panels::{self, Tab};
use crate::recovery::{self, Recovery};
use crate::theme::{self, Palette, Theme};
use crate::{dialogs, titlebar};

const SETTINGS_KEY: &str = "fanimate.settings";
const BENCH_DURATION: Duration = Duration::from_secs(10);

/// Remembered between sessions.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    keymap: String,
    theme: Theme,
    welcome_done: bool,
    draw: DrawStyle,
    onion: OnionSkin,
    /// The format Export Video chose last.
    export_format: fanimate_export::Format,
    /// The FFmpeg chosen in Preferences; none looks for one.
    ffmpeg: Option<PathBuf>,
    /// After a crash, offer to report it.
    crash_reports: bool,
    updates: crate::update::UpdateSettings,
    /// What opens when Fanimate starts.
    startup: Startup,
    /// Files opened or saved lately, newest first.
    recent: Vec<PathBuf>,
    /// The version that last ran, so a new one opens What's New once.
    last_seen_version: String,
    /// Folders saved or exported into lately, for the folder drop-downs.
    places: crate::places::Places,
    /// The folder the last export went to.
    last_export: Option<PathBuf>,
    /// The saved documents that were open, and the one in front, for
    /// "reopen last session".
    session: Vec<PathBuf>,
    session_front: usize,
    /// Grid and snap settings (per session).
    grid: fanimate_editor::GridSettings,
    snap: fanimate_editor::SnapSettings,
    /// Ruler unit (px, pt, mm, in).
    ruler_unit: crate::panels::rulers::RulerUnit,
}

/// Onion skin settings, remembered between sessions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnionSkin {
    pub on: bool,
    /// How many earlier and later frames or drawings show.
    pub before: usize,
    pub after: usize,
    /// Only frames with drawings or keys, rather than every frame.
    pub keys: bool,
    /// Every layer, rather than the selected ones.
    pub all_layers: bool,
}

impl Default for OnionSkin {
    fn default() -> Self {
        Self {
            on: false,
            before: 2,
            after: 1,
            keys: true,
            all_layers: false,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            keymap: KeymapKind::Fanimate.id().into(),
            theme: Theme::default(),
            welcome_done: false,
            draw: DrawStyle::default(),
            onion: OnionSkin::default(),
            export_format: Default::default(),
            ffmpeg: None,
            crash_reports: true,
            updates: Default::default(),
            startup: Startup::default(),
            recent: Vec::new(),
            last_seen_version: String::new(),
            places: Default::default(),
            last_export: None,
            session: Vec::new(),
            session_front: 0,
            grid: Default::default(),
            snap: Default::default(),
            ruler_unit: Default::default(),
        }
    }
}

/// Canvas camera: which document point is centred, and how zoomed.
#[derive(Clone)]
pub struct CanvasView {
    pub center: fanimate_core::Vec2,
    /// Screen points per document unit.
    pub zoom: f32,
    /// Fit the canvas to the panel on the next frame.
    pub fit_pending: bool,
}

impl CanvasView {
    pub fn fit(&mut self) {
        self.fit_pending = true;
    }
}

pub struct Bench {
    started: Instant,
    frames: u32,
    cpu: Duration,
}

/// What a set of snap lines was worked out for: the document's revision, the
/// selected layers and the frame.
pub type SnapKey = (u64, Vec<LayerId>, fanimate_core::Frame);

/// Transient UI state, not part of the document.
#[derive(Default)]
pub struct UiState {
    /// Layer being renamed in place, and the text so far.
    pub renaming: Option<(LayerId, String)>,
    /// Give the rename field focus (and select its text) on the next frame.
    pub rename_focus: bool,
    /// A bone being renamed in place: (layer_id, bone_index, current_text).
    pub bone_rename: Option<(fanimate_core::LayerId, usize, String)>,
    /// Give the bone rename field focus on the next frame.
    pub bone_rename_focus: bool,
    pub layer_filter: String,
    pub show_preferences: bool,
    pub prefs_page: dialogs::PrefsPage,
    pub show_shortcuts: bool,
    pub shortcut_filter: String,
    pub show_about: bool,
    pub show_welcome: bool,
    pub palette: Option<dialogs::PaletteState>,
    /// An action waiting for "save changes?" to be answered.
    pub confirm_discard: Option<Discard>,
    /// The Home tab's section.
    pub home_section: crate::home::Section,
    /// A selection frame handle or a corner radius being dragged.
    pub handle_gesture: Option<crate::panels::handles::Gesture>,
    /// Show a rectangle's corner radius as a percentage of its largest.
    pub radius_relative: bool,
    /// Edit each corner's radius on its own, not all four together.
    pub radius_each: bool,
    /// A Stroke Exposure drag is going on.
    pub exposure_active: bool,
    /// The selection frame as drawn last frame, which clicks are tested against.
    pub frame: Option<fanimate_editor::SelectionFrame>,
    /// The outline under the pointer.
    pub hover: Option<crate::panels::handles::HoverCache>,
    /// The floating selection toolbar's size last frame, to place it.
    pub selbar_size: Option<egui::Vec2>,
    /// How far the toolbar was dragged from where it would sit, and the
    /// selection that was for (a new selection puts it back).
    pub selbar_offset: egui::Vec2,
    pub selbar_for: Vec<LayerId>,
    /// Scroll the tab strip to the front tab on the next frame.
    pub scroll_tab_into_view: bool,
    /// Bring the Properties tab forward on the next frame.
    pub focus_properties: bool,
    /// Layer transform when a canvas drag started.
    pub drag_origin: Option<(LayerId, fanimate_core::Transform)>,
    /// Where a Rectangle or Ellipse drag started, in document space.
    pub shape_drag: Option<fanimate_core::Vec2>,
    /// A point tool drag in progress.
    pub point_gesture: Option<crate::panels::canvas::PointGesture>,
    /// Whether the pointer went down with the Pen on the canvas.
    pub pen_down: bool,
    /// Whether the pointer went down with the Curvature tool on the canvas.
    pub curvature_down: bool,
    /// The drag-start for Phase E shape tools (Line, Polygon, Star, Triangle).
    pub shape_drag_e: Option<fanimate_core::Vec2>,
    /// Magnet: whether a drag is in progress (checkpoint taken once).
    pub magnet_active: bool,
    /// Line Width: the screen height the press was at, while dragging.
    pub line_width_press: Option<f32>,
    /// Scale/Rotate Points: anchor point and last-drag position.
    pub points_transform_origin: Option<(fanimate_core::Vec2, fanimate_core::Vec2)>,
    /// The status message on screen, and when it appeared.
    pub status_since: Option<(String, Instant)>,
    /// Timeline zoom (pixels per frame) and horizontal scroll (frames).
    pub timeline_frame_width: f32,
    pub timeline_scroll: f32,
    /// Width of the timeline's frame area last frame, for "fit all".
    pub timeline_view_width: f32,
    /// The pointer is over the timeline, so zoom commands zoom it, not the canvas.
    pub timeline_hovered: bool,
    /// The playhead's frame when the timeline last drew, to scroll it into
    /// view when something else moves it (a step, a key jump, the Frame field).
    pub timeline_seen_frame: fanimate_core::Frame,
    /// The playhead is being dragged along the timeline, which scrolls at
    /// the edge instead.
    pub timeline_scrubbing: bool,
    /// Layers whose property rows are open in the timeline.
    pub timeline_expanded: std::collections::HashSet<LayerId>,
    /// The widget being edited in the Properties or Style panel, so one
    /// drag or one typed value makes one undo step.
    pub active_edit: Option<egui::Id>,
    /// Show (or reopen) a panel on the next frame.
    pub open_tab: Option<Tab>,
    pub reset_layout: bool,
    /// Show or hide the tools column on the next frame, and whether it shows
    /// (for the Window menu).
    pub toggle_tools: bool,
    pub tools_shown: bool,
    /// The docked panels on screen, for the Window menu.
    pub panels_shown: Vec<Tab>,
    pub window: titlebar::WindowState,
    /// The Export Video window, while open.
    pub export: Option<crate::export::ExportForm>,
    /// The Update Fanimate window.
    pub show_update: bool,
    /// The Save As dialog, while open.
    pub save_as: Option<crate::saveas::SaveForm>,
    /// The Render Preview dialog, while open.
    pub preview: Option<crate::preview::Preview>,
    /// The previews on the Home tab.
    pub thumbs: crate::thumbs::Thumbs,
    /// The What's New dialog, while open.
    pub whats_new: Option<crate::whatsnew::WhatsNew>,
    /// A layer type change waiting for the answer to "this removes ...".
    pub confirm_convert: Option<(fanimate_editor::ConvertTo, String)>,
    /// A shape being renamed in place in the Properties panel, and the text so far.
    pub shape_rename: Option<(LayerId, usize, String)>,
    /// Give the shape rename field focus on the next frame.
    pub shape_rename_focus: bool,
    /// A guide being dragged: its index.
    pub guide_drag: Option<usize>,
    /// Arrange the panels this way on the next frame.
    pub workspace: Option<fanimate_editor::Workspace>,
    /// The snap hint line to draw this frame (document space start, end).
    pub snap_hint: Vec<(fanimate_core::Vec2, fanimate_core::Vec2)>,
    /// The lines other shapes offer to snap to, and what they were worked out
    /// for (revision, selection, frame).
    pub snap_lines: Option<(SnapKey, fanimate_editor::snap::ShapeLines)>,
}

/// Unsaved changes are being asked about, before this happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discard {
    /// An action that closes every document: quitting, installing an update.
    Action(Action),
    /// The front tab closes.
    CloseTab,
}

/// Everything except the dock layout (kept apart so the dock and its tab
/// viewer can be borrowed separately).
pub struct Shell {
    /// The front document (or, with no document open, a blank one that
    /// the Home tab hides). The other open documents are parked in `docs`.
    pub editor: Editor,
    pub docs: fanimate_editor::Tabs<crate::docs::Parked>,
    /// Files opened or saved lately, newest first.
    pub recent: Vec<PathBuf>,
    /// Folders saved or exported into lately.
    pub places: crate::places::Places,
    /// The folder the last export went to.
    pub last_export: Option<PathBuf>,
    /// What opens when Fanimate starts.
    pub startup: Startup,
    /// Grid display and spacing settings.
    pub grid: fanimate_editor::GridSettings,
    /// Snap settings.
    pub snap: fanimate_editor::SnapSettings,
    /// Show rulers along the canvas edges.
    pub show_rulers: bool,
    /// Show the canvas grid.
    pub show_grid: bool,
    /// Unit displayed in the rulers.
    pub ruler_unit: crate::panels::rulers::RulerUnit,
    /// Hands this Fanimate the files other launches ask for.
    instance: Option<crate::instance::Claim>,
    pub keymap: Keymap,
    pub theme: Theme,
    pub palette: Palette,
    pub canvas: Option<CanvasGpu>,
    /// The UI's GPU device, shared with the canvas renderer.
    pub render_state: Option<eframe::egui_wgpu::RenderState>,
    pub view: CanvasView,
    pub ui: UiState,
    /// Actions queued by widgets this frame, run after drawing.
    pub pending: Vec<Action>,
    pub gpu_name: String,
    pub bench: Option<Bench>,
    /// The app icon, for the title bar.
    pub icon: egui::TextureHandle,
    /// The crash recovery journal, and work recovered from a crash.
    pub recovery: Recovery,
    pub onion: OnionSkin,
    /// Fade every layer but the selected ones.
    pub light_table: bool,
    /// The canvas shows what the camera sees (a view, not saved).
    pub look_through_camera: bool,
    /// Plays audio layers along with playback.
    pub audio: crate::audio::Audio,
    /// A lip sync waiting on Rhubarb.
    pub lipsync: Option<crate::lipsync::Job>,
    pub export_format: fanimate_export::Format,
    /// What can make each export format here.
    pub video: crate::formats::VideoTools,
    /// Renders queued from here, for the status bar.
    pub renders: crate::export::Renders,
    /// A crash from an earlier session, waiting to be reported or not.
    pub crash: Option<crate::crash::Pending>,
    /// After a crash, offer to report it (Preferences).
    pub crash_reports: bool,
    /// This session's marker; a normal exit removes it.
    _crash_session: Option<crate::crash::Session>,
    pub updater: crate::update::Updater,
    /// How this copy was installed, which decides how it updates.
    pub install: crate::update::Install,
    pub updates: crate::update::UpdateSettings,
    /// Commands recently run from the palette, most recent first.
    pub recent_actions: Vec<Action>,
    /// The user asked for this check, so its result is shown.
    pub update_manual: bool,
    /// When the automatic check is due.
    update_due: Option<Instant>,
    last_tick: Instant,
    allow_close: bool,
}

pub struct FanimateApp {
    layout: Layout,
    shell: Shell,
    taskbar: crate::taskbar::Taskbar,
}

pub struct Options {
    pub files: Vec<PathBuf>,
    pub bench_bones: Option<usize>,
    pub claim: Option<crate::instance::Claim>,
}

impl FanimateApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, SETTINGS_KEY))
            .unwrap_or_default();
        let layout = cc
            .storage
            .and_then(|s| eframe::get_value::<Layout>(s, Layout::KEY))
            .unwrap_or_default();
        let palette = theme::apply(&cc.egui_ctx, settings.theme);

        // the blank document the Home tab hides until a document is opened
        let mut editor = Editor::untitled();
        editor.draw = settings.draw;

        let canvas = cc.wgpu_render_state.as_ref().map(CanvasGpu::new);
        let gpu_name = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| {
                let info = rs.adapter.get_info();
                format!("{} ({:?})", info.name, info.backend)
            })
            .unwrap_or_else(|| "no GPU".into());

        crate::crash::set_gpu(&gpu_name);
        let bench = opts.bench_bones.is_some();
        let (crash_session, mut crash) = match crate::crash::dir() {
            Some(dir) if !bench => crate::crash::begin(&dir),
            _ => (None, None),
        };
        if !settings.crash_reports
            && let Some(p) = crash.take()
        {
            crate::crash::dismiss(&p);
        }

        let icon = load_icon(&cc.egui_ctx);
        // the benchmark leaves no trace
        let recovery_root = if opts.bench_bones.is_some() {
            None
        } else {
            recovery::root()
        };
        let mut app = Self {
            layout,
            taskbar: Default::default(),
            shell: Shell {
                docs: Default::default(),
                recent: settings.recent.clone(),
                places: settings.places.clone(),
                last_export: settings.last_export.clone(),
                startup: settings.startup,
                grid: settings.grid.clone(),
                snap: settings.snap.clone(),
                show_rulers: true,
                show_grid: false,
                ruler_unit: settings.ruler_unit,
                instance: opts.claim,
                icon,
                recovery: Recovery::new(recovery_root),
                onion: settings.onion,
                light_table: false,
                look_through_camera: false,
                audio: Default::default(),
                lipsync: None,
                export_format: settings.export_format,
                video: crate::formats::VideoTools::new(settings.ffmpeg.clone()),
                renders: Default::default(),
                crash,
                crash_reports: settings.crash_reports,
                _crash_session: crash_session,
                updater: Default::default(),
                install: crate::update::Install::detect(),
                updates: settings.updates.clone(),
                recent_actions: Vec::new(),
                update_manual: false,
                update_due: (!bench).then(|| Instant::now() + Duration::from_secs(5)),
                editor,
                keymap: Keymap::new(KeymapKind::from_id(&settings.keymap).unwrap_or_default()),
                theme: settings.theme,
                palette,
                canvas,
                render_state: cc.wgpu_render_state.clone(),
                view: CanvasView {
                    center: Default::default(),
                    zoom: 1.0,
                    fit_pending: true,
                },
                ui: UiState {
                    show_welcome: !settings.welcome_done && opts.bench_bones.is_none(),
                    // after an update, once: what changed
                    whats_new: (opts.bench_bones.is_none()
                        && crate::whatsnew::show_after_update(&settings.last_seen_version))
                    .then(|| crate::whatsnew::WhatsNew::open(None))
                    .flatten(),
                    ..Default::default()
                },
                pending: Vec::new(),
                gpu_name,
                bench: opts.bench_bones.map(|_| Bench {
                    started: Instant::now(),
                    frames: 0,
                    cpu: Duration::ZERO,
                }),
                last_tick: Instant::now(),
                allow_close: false,
            },
        };
        match opts.bench_bones {
            Some(bones) => {
                let mut ed = Editor::new(fanimate_core::samples::benchmark(bones));
                ed.playing = true;
                app.shell.open_document(ed);
            }
            None => app.shell.open_startup(
                &opts.files,
                settings.startup,
                &settings.session,
                settings.session_front,
            ),
        }
        app
    }
}

fn load_icon(ctx: &egui::Context) -> egui::TextureHandle {
    let png = include_bytes!("../../../brand/app-icon/fanimate-64.png");
    let img = image::load_from_memory(png)
        .map(|i| i.into_rgba8())
        .unwrap_or_default();
    let (w, h) = img.dimensions();
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    ctx.load_texture("fanimate-icon", color, egui::TextureOptions::LINEAR)
}

impl eframe::App for FanimateApp {
    // here, not in `ui`: that is skipped while the window is minimised, which
    // is when the taskbar button matters most
    fn logic(&mut self, _ctx: &egui::Context, frame: &mut eframe::Frame) {
        use fanimate_oscodec::TaskbarProgress;
        let shell = &self.shell;
        let wanted = if let Some((_, fraction)) = shell.renders.active {
            TaskbarProgress::Fraction(fraction)
        } else if matches!(
            shell.updater.state(),
            crate::update::State::Downloading(_) | crate::update::State::Installing(_)
        ) {
            shell
                .updater
                .progress()
                .map_or(TaskbarProgress::Busy, TaskbarProgress::Fraction)
        } else {
            TaskbarProgress::Off
        };
        self.taskbar.show(frame, wanted);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let shell = &mut self.shell;

        // playback clock
        let now = Instant::now();
        let dt = (now - shell.last_tick).as_secs_f32().min(0.25);
        shell.last_tick = now;
        shell.editor.tick(dt);
        shell
            .audio
            .follow(&shell.editor.doc, shell.editor.playing, shell.editor.frame);
        crate::lipsync::poll(&ctx, shell);

        handle_close_request(&ctx, shell);
        titlebar::track_window(&ctx, &mut shell.ui.window);
        handle_input(&ctx, shell);
        handle_dropped_files(&ctx, shell);

        titlebar::show(ui, shell);
        crate::doctabs::show(ui, shell);
        panels::status_bar(ui, shell);
        poll_instance(&ctx, shell);

        if std::mem::take(&mut shell.ui.focus_properties) {
            self.layout.open(Tab::Properties);
        }

        // the timeline sets this while it draws, if the pointer is over it
        shell.ui.timeline_hovered = false;
        let style = dock_style(ui.style(), &shell.palette);
        if shell.docs.home_front() {
            crate::home::show(ui, shell);
        } else {
            crate::layout::show(ui, shell, &mut self.layout, &style);
            // what the panels leave is the canvas: no tab, no border
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(shell.palette.bg))
                .show(ui, |ui| panels::canvas_panel(ui, shell));
        }

        dialogs::show(&ctx, shell);
        if shell.video.poll() {
            ctx.request_repaint();
        } else if shell.video.looking() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        crate::export::window(&ctx, shell);
        crate::saveas::window(&ctx, shell);
        crate::preview::window(&ctx, shell);
        crate::whatsnew::window(&ctx, shell);
        dialogs::confirm_convert(&ctx, shell);
        {
            let Shell { renders, editor, .. } = shell;
            renders.poll(&ctx, &mut |kind, message| editor.set_status(kind, message));
        }
        titlebar::resize_edges(&ctx, &shell.ui.window);
        native_cursors(&ctx);

        // run queued actions now that no widget borrows the editor
        for action in std::mem::take(&mut shell.pending) {
            dispatch(&ctx, shell, action);
        }
        recovery::tick(&ctx, shell);
        update_tick(&ctx, shell);
        if let Some(tab) = shell.ui.open_tab.take() {
            self.layout.open(tab);
        }
        if std::mem::take(&mut shell.ui.toggle_tools) {
            self.layout.toggle_tools();
        }
        if std::mem::take(&mut shell.ui.reset_layout) {
            self.layout.reset(fanimate_editor::Workspace::Animate);
        }
        if let Some(w) = shell.ui.workspace.take() {
            self.layout.reset(w);
        }
        shell.ui.tools_shown = self.layout.show_tools;
        shell.ui.panels_shown = Tab::ALL.into_iter().filter(|t| self.layout.is_open(*t)).collect();

        if let Some(b) = &mut shell.bench {
            b.frames += 1;
            if let Some(c) = &shell.canvas {
                b.cpu += c.cpu_time;
            }
            if b.started.elapsed() >= BENCH_DURATION {
                let secs = b.started.elapsed().as_secs_f64();
                crate::report(&format!(
                    "benchmark: {} ({} shapes) on {}\n  {:.1} fps average over {:.1} s (UI included)\n  {:.3} ms CPU per frame (evaluate + tessellate)",
                    shell.editor.doc.name,
                    shell.canvas.as_ref().map_or(0, |c| c.shapes),
                    shell.gpu_name,
                    b.frames as f64 / secs,
                    secs,
                    b.cpu.as_secs_f64() * 1000.0 / b.frames.max(1) as f64,
                ));
                shell.allow_close = true;
                shell.bench = None; // report once; a frame may still run before closing
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        if shell.editor.playing || shell.bench.is_some() {
            ctx.request_repaint();
        }
        let title = match shell.docs.home_front() {
            true => "Fanimate".to_owned(),
            false => format!("{} - Fanimate", window_title(shell)),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let s = &self.shell;
        let (session, session_front) = s.session();
        eframe::set_value(
            storage,
            SETTINGS_KEY,
            &Settings {
                keymap: s.keymap.kind.id().into(),
                theme: s.theme,
                welcome_done: !s.ui.show_welcome,
                draw: s.editor.draw,
                onion: s.onion,
                export_format: s.export_format,
                ffmpeg: s.video.chosen.clone(),
                crash_reports: s.crash_reports,
                updates: s.updates.clone(),
                startup: s.startup,
                recent: s.recent.clone(),
                last_seen_version: env!("CARGO_PKG_VERSION").to_owned(),
                places: s.places.clone(),
                last_export: s.last_export.clone(),
                session,
                session_front,
                grid: s.grid.clone(),
                snap: s.snap.clone(),
                ruler_unit: s.ruler_unit,
            },
        );
        eframe::set_value(storage, Layout::KEY, &self.layout);
    }

    fn on_exit(&mut self) {
        recovery::end(&mut self.shell);
    }
}

/// Dock colours from the theme tokens rather than egui_dock's defaults.
fn dock_style(egui_style: &egui::Style, p: &Palette) -> egui_dock::Style {
    let mut style = egui_dock::Style::from_egui(egui_style);
    style.main_surface_border_stroke = egui::Stroke::NONE;
    // a panel can be dragged as narrow as its region allows (the tools
    // column and the region sizes are the layout's to set)
    style.separator.extra = 60.0;
    style.tab_bar.bg_fill = p.bg;
    style.tab_bar.height = 26.0;
    style.tab_bar.hline_color = p.border;
    style.tab.tab_body.bg_fill = p.panel;
    style.tab.tab_body.stroke = egui::Stroke::new(1.0, p.border);
    for (state, bg, text) in [
        (&mut style.tab.active, p.panel, p.text),
        (&mut style.tab.focused, p.panel, p.text),
        (&mut style.tab.inactive, p.bg, p.text_muted),
        (&mut style.tab.inactive_with_kb_focus, p.bg, p.text),
        (&mut style.tab.hovered, p.raised, p.text),
    ] {
        state.bg_fill = bg;
        state.text_color = text;
        state.outline_color = p.border;
    }
    // panels meet at a hairline, not a gap; the wider grab area stays
    style.separator.color_idle = p.border;
    style.separator.color_hovered = p.accent;
    style.separator.width = 1.0;
    style.separator.extra_interact_width = 4.0;
    style
}

/// What the title bar and the window show: the front document's name, or
/// Home.
pub fn window_title(shell: &Shell) -> String {
    if shell.docs.home_front() {
        return "Home".into();
    }
    let ed = &shell.editor;
    format!("{}{}", ed.doc.name, if ed.dirty { " •" } else { "" })
}

/// Whether any open document has unsaved changes.
pub fn any_unsaved(shell: &Shell) -> bool {
    shell.editor.dirty || shell.docs.any_dirty()
}

/// Intercept closing the window when there are unsaved changes.
fn handle_close_request(ctx: &egui::Context, shell: &mut Shell) {
    if ctx.input(|i| i.viewport().close_requested()) && !shell.allow_close && any_unsaved(shell) {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        shell.ui.confirm_discard = Some(Discard::Action(Action::Quit));
    }
}

/// Open what other launches of Fanimate asked this one to open (a .fnm
/// double-clicked in the file manager), and bring the window forward.
fn poll_instance(ctx: &egui::Context, shell: &mut Shell) {
    let Some(claim) = &shell.instance else { return };
    let requests = claim.requests();
    // look again soon: a launch is waiting for its answer
    ctx.request_repaint_after(Duration::from_millis(400));
    if requests.is_empty() {
        return;
    }
    for files in requests {
        for file in files {
            shell.open_path(&file);
        }
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

/// egui and egui_dock show a grab hand over anything draggable, such as
/// dock tabs and layer rows. Desktop apps keep the normal arrow there, and
/// Windows has no hand cursor at all: winit substitutes the four-way move
/// arrow, which reads as "this moves the window".
fn native_cursors(ctx: &egui::Context) {
    ctx.output_mut(|o| {
        let grabbing_shows_move = cfg!(windows) && o.cursor_icon == CursorIcon::Grabbing;
        if o.cursor_icon == CursorIcon::Grab || grabbing_shows_move {
            o.cursor_icon = CursorIcon::Default;
        }
    });
}

fn any_modal_open(shell: &Shell) -> bool {
    let u = &shell.ui;
    u.palette.is_some()
        || u.confirm_discard.is_some()
        || u.show_welcome
        || u.show_preferences
        || u.show_shortcuts
        || u.show_about
        || u.show_update
        || u.export.is_some()
        || u.save_as.is_some()
        || u.preview.is_some()
        || u.whats_new.is_some()
        || u.confirm_convert.is_some()
        || shell.crash.is_some()
        || !shell.recovery.offered.is_empty()
}

/// Keyboard shortcuts and clipboard events.
fn handle_input(ctx: &egui::Context, shell: &mut Shell) {
    // a text field has focus, or a dialog or menu is up: keys belong to it
    if ctx.egui_wants_keyboard_input() || any_modal_open(shell) || egui::Popup::is_any_open(ctx) {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    for event in events {
        match event {
            // egui turns Ctrl+C/X/V into these instead of key events
            // a paste needs a document in front, so not on Home
            egui::Event::Paste(_) if shell.docs.home_front() => {}
            egui::Event::Copy => shell.pending.push(Action::Copy),
            egui::Event::Cut => shell.pending.push(Action::Cut),
            egui::Event::Paste(text) if !shell.editor.paste(Some(&text)) => {
                shell.editor.set_status(StatusKind::Info, "Nothing to paste");
            }
            egui::Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers,
                ..
            } => {
                let chord = chord_of(key, modifiers);
                if let Some(action) = shell.keymap.lookup(chord) {
                    // only frame stepping repeats while a key is held
                    let repeats = matches!(action, Action::NextFrame | Action::PrevFrame);
                    if !repeat || repeats {
                        shell.pending.push(action);
                    }
                }
            }
            _ => {}
        }
    }
}

fn chord_of(key: Key, m: Modifiers) -> Chord {
    Chord::new(key.name(), m.command, m.shift, m.alt)
}

/// Run an action from any source: menu, shortcut, context menu, palette.
pub fn dispatch(ctx: &egui::Context, shell: &mut Shell, action: Action) {
    // with Home in front there is no document to act on
    if shell.docs.home_front() && !crate::docs::works_on_home(action) {
        return;
    }
    // actions that would throw away unsaved work ask first
    if action.closes_document() && any_unsaved(shell) {
        shell.ui.confirm_discard = Some(Discard::Action(action));
        return;
    }
    run_unchecked(ctx, shell, action);
}

/// Run an action without the unsaved-changes check (already answered).
pub fn run_unchecked(ctx: &egui::Context, shell: &mut Shell, action: Action) {
    match shell.editor.run(action) {
        Outcome::Done => {}
        Outcome::CopyText(text) => ctx.copy_text(text),
        Outcome::App(a) => match a {
            Action::New => shell.new_file(Editor::untitled()),
            Action::NewPreset(preset) => shell.new_file(Editor::untitled_preset(preset)),
            Action::Open => open_dialog(shell),
            Action::OpenFolder => open_folder_dialog(shell),
            Action::OpenOther => open_other_dialog(shell),
            Action::OpenSample(sample) => shell.open_document(Editor::new(sample.document())),
            Action::CloseDocument => {
                if let Some(tab) = shell.docs.active() {
                    shell.request_close_tab(tab);
                }
            }
            Action::CloseOtherDocuments => {
                if let Some(tab) = shell.docs.active() {
                    shell.close_other_tabs(tab);
                }
            }
            Action::NextDocument => shell.cycle_tab(true),
            Action::PreviousDocument => shell.cycle_tab(false),
            Action::ShowHome => shell.show_home(),
            Action::Save => {
                // a file that has never been saved needs a place first
                if shell.editor.path.is_some() {
                    save(shell, false);
                } else {
                    shell.ui.save_as = Some(crate::saveas::SaveForm::new(shell));
                }
            }
            Action::SaveAs => shell.ui.save_as = Some(crate::saveas::SaveForm::new(shell)),
            Action::SaveAsFolder => save_as_folder(shell),
            Action::Import => import_dialog(shell),
            Action::LipSync => crate::lipsync::start(shell),
            Action::ExportVideo => {
                shell.ui.export = Some(crate::export::ExportForm::new(shell, shell.export_format));
            }
            Action::RenderPreviewFrame => shell.ui.preview = Some(crate::preview::Preview::start(shell)),
            // the editor asks before throwing away a skeleton or a switch's choices
            Action::ConvertLayerType(to) => {
                if let Some(id) = shell.editor.primary()
                    && let Some(loss) = shell.editor.conversion_loss(id, to)
                {
                    shell.ui.confirm_convert = Some((to, loss.describe()));
                }
            }
            Action::RenderQueue => {
                if let Err(e) = crate::render_queue::open(shell.theme) {
                    shell.editor.set_status(StatusKind::Error, e);
                }
            }
            Action::ToggleOnionSkin => shell.onion.on = !shell.onion.on,
            Action::ToggleLightTable => shell.light_table = !shell.light_table,
            Action::LookThroughCamera => shell.look_through_camera = !shell.look_through_camera,
            Action::Quit => {
                shell.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Action::Preferences => shell.ui.show_preferences = true,
            Action::CheckForUpdates => {
                shell.update_manual = true;
                shell.ui.show_update = true;
                shell.updater.check(true, &shell.install);
            }
            Action::InstallUpdate => install_update(shell),
            Action::Workspace(w) => shell.ui.workspace = Some(w),
            Action::CommandPalette => shell.ui.palette = Some(Default::default()),
            Action::LayerProperties => shell.ui.focus_properties = true,
            Action::ZoomIn if shell.ui.timeline_hovered => panels::timeline::zoom_by(shell, 1.25),
            Action::ZoomOut if shell.ui.timeline_hovered => panels::timeline::zoom_by(shell, 0.8),
            Action::FitView if shell.ui.timeline_hovered => panels::timeline::fit_all(shell),
            Action::ActualSize if shell.ui.timeline_hovered => panels::timeline::reset_zoom(shell),
            Action::ZoomIn => zoom_by(shell, 1.25),
            Action::ZoomOut => zoom_by(shell, 0.8),
            Action::FitView => shell.view.fit(),
            Action::ActualSize => {
                shell.view.zoom = 1.0 / ctx.pixels_per_point();
                shell.view.center = shell.editor.doc.size() / 2.0;
            }
            other => log::warn!("unhandled app action {other:?}"),
        },
    }
}

/// Each frame: start the automatic update check when due, and report
/// what a check found.
fn update_tick(ctx: &egui::Context, shell: &mut Shell) {
    use crate::update::State;
    if let Some(due) = shell.update_due
        && Instant::now() >= due
    {
        shell.update_due = None;
        if shell.updates.check {
            shell.updater.check(false, &shell.install);
        }
    }
    match shell.updater.state() {
        State::Checking { .. } | State::Downloading(_) | State::Installing(_) => {
            ctx.request_repaint_after(Duration::from_millis(300))
        }
        State::Installed(release) => {
            log::info!("installing Fanimate {}", release.version);
            shell.updater.reset();
            shell.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        State::UpToDate => {
            if std::mem::take(&mut shell.update_manual) {
                let v = crate::update::Version::current();
                shell
                    .editor
                    .set_status(StatusKind::Success, format!("Fanimate {v} is the newest version"));
                shell.ui.show_update = false;
            }
            shell.updater.reset();
        }
        State::Failed(e) => {
            // an automatic check fails quietly: offline is normal
            if std::mem::take(&mut shell.update_manual) || shell.ui.show_update {
                shell.editor.set_status(StatusKind::Error, e);
            }
            shell.ui.show_update = false;
            shell.updater.reset();
        }
        State::Available(_) => shell.update_manual = false,
        State::Idle | State::Ready(..) => {}
    }
}

/// Install a downloaded update: wait for renders, then hand over to the
/// installer and quit. Unsaved changes were already asked about.
fn install_update(shell: &mut Shell) {
    let crate::update::State::Ready(release, file) = shell.updater.state() else {
        shell.editor.set_status(
            StatusKind::Info,
            "No update is ready yet. Use Help > Check for Updates.",
        );
        return;
    };
    // jobs waiting with no queue window open resume in the new version
    let rendering = crate::render_queue::queue()
        .is_some_and(|q| q.has_runner() && q.jobs().iter().any(|e| !e.status.state.is_finished()));
    if rendering {
        shell.editor.set_status(
            StatusKind::Warning,
            "The render queue is still working. Install the update when it has finished.",
        );
        return;
    }
    // a failure is reported even though the update window has closed
    shell.update_manual = true;
    shell.updater.start_install(shell.install.clone(), release, file);
}

fn zoom_by(shell: &mut Shell, factor: f32) {
    shell.view.zoom = (shell.view.zoom * factor).clamp(0.02, 64.0);
}

/// Pick pictures, Photoshop or SVG files, or sounds, and import them as
/// layers.
/// The kinds of file File > Import (and dropping a file on the window) brings
/// in as layers: pictures, Photoshop and SVG files, and sound.
fn importable_extensions() -> Vec<&'static str> {
    let mut all = vec!["png", "jpg", "jpeg", "psd", "svg"];
    all.extend(fanimate_editor::SOUND_EXTENSIONS);
    all
}

/// Whether `path` is something to import (rather than a project to open).
fn is_importable(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| importable_extensions().contains(&e.as_str()))
}

/// Import one file as layers of the front document. Errors say what went
/// wrong, in the status bar.
fn import_file(shell: &mut Shell, path: &Path) -> bool {
    let name = path.display().to_string();
    let result = std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "psd" => shell.editor.import_psd(&name, &bytes),
            "svg" => shell.editor.import_svg(&name, &bytes),
            e if fanimate_editor::SOUND_EXTENSIONS.contains(&e) => shell.editor.import_audio(&name, bytes),
            _ => shell.editor.import_picture(&name, bytes),
        }
    });
    match result {
        Ok(_) => true,
        Err(e) => {
            let file = path
                .file_name()
                .map_or(name.clone(), |f| f.to_string_lossy().into_owned());
            shell
                .editor
                .set_status(StatusKind::Error, format!("Could not import {file}: {e}"));
            false
        }
    }
}

fn import_dialog(shell: &mut Shell) {
    let Some(paths) = rfd::FileDialog::new()
        .set_title("Import pictures, a Photoshop or SVG file, or sound")
        .add_filter(
            "Pictures, Photoshop and SVG files, and sound",
            &importable_extensions(),
        )
        .pick_files()
    else {
        return;
    };
    for path in paths {
        if !import_file(shell, &path) {
            return;
        }
    }
}

/// Files dropped on the window: projects open as tabs, and pictures, Photoshop
/// and SVG files and sound are imported into the front document.
fn handle_dropped_files(ctx: &egui::Context, shell: &mut Shell) {
    let dropped: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    for path in dropped {
        if is_importable(&path) {
            if shell.docs.home_front() {
                shell
                    .editor
                    .set_status(StatusKind::Info, "Open a project first, then drop the file on it");
            } else if !import_file(shell, &path) {
                break;
            }
        } else {
            shell.open_path(&path);
        }
    }
}

/// Save the project as a folder of plain files: choose a name, and a folder
/// of that name is made.
fn save_as_folder(shell: &mut Shell) {
    let mut dialog = rfd::FileDialog::new()
        .set_title("Save the project as a folder")
        .set_file_name(format!("{}.fnm", shell.editor.doc.name));
    if let Some(dir) = shell
        .editor
        .path
        .as_deref()
        .and_then(Path::parent)
        .or_else(|| shell.places.latest())
    {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut path) = dialog.save_file() else {
        return;
    };
    if path.extension().is_none_or(|e| e != "fnm") {
        path.set_extension("fnm");
    }
    // a folder is only ever made fresh: an existing file or folder of that name is left alone
    if path.exists() {
        shell.editor.set_status(
            StatusKind::Error,
            format!(
                "{} already exists. Choose a new name for the folder.",
                path.display()
            ),
        );
        return;
    }
    match shell.editor.save_folder_to(&path) {
        Ok(()) => {
            shell.remember(&path);
            recovery::record(shell);
        }
        Err(e) => shell
            .editor
            .set_status(StatusKind::Error, format!("Could not save: {e}")),
    }
}

fn open_dialog(shell: &mut Shell) {
    let Some(paths) = rfd::FileDialog::new()
        .set_title("Open Fanimate projects")
        .add_filter("Fanimate project", &["fnm"])
        .pick_files()
    else {
        return;
    };
    for path in paths {
        shell.open_path(&path);
    }
}

/// Projects saved as folders (older Fanimate versions, or kept in Git) are
/// opened by their folder.
fn open_folder_dialog(shell: &mut Shell) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Open a Fanimate project folder")
        .pick_folder()
    else {
        return;
    };
    shell.open_path(&path);
}

/// Pick Moho or Animate projects and open each as a new Fanimate project.
fn open_other_dialog(shell: &mut Shell) {
    let Some(paths) = rfd::FileDialog::new()
        .set_title("Open Moho or Animate projects")
        .add_filter("Moho and Animate projects", &fanimate_import::EXTENSIONS)
        .pick_files()
    else {
        return;
    };
    for path in paths {
        shell.open_path(&path);
    }
}

/// Save, asking for a location if needed. Returns whether it saved.
pub fn save(shell: &mut Shell, ask: bool) -> bool {
    let path = match (&shell.editor.path, ask) {
        (Some(p), false) => p.clone(),
        _ => {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Save Fanimate project")
                .set_file_name(format!("{}.fnm", shell.editor.doc.name))
                .add_filter("Fanimate project", &["fnm"]);
            if let Some(dir) = shell
                .editor
                .path
                .as_deref()
                .and_then(Path::parent)
                .or_else(|| shell.places.latest())
            {
                dialog = dialog.set_directory(dir);
            }
            let Some(mut p) = dialog.save_file() else {
                return false;
            };
            if p.extension().is_none_or(|e| e != "fnm") {
                p.set_extension("fnm");
            }
            p
        }
    };
    match shell.editor.save_to(&path) {
        Ok(()) => {
            shell.remember(&path);
            // saved: a crash now would lose nothing
            recovery::record(shell);
            true
        }
        Err(e) => {
            shell
                .editor
                .set_status(StatusKind::Error, format!("Could not save: {e}"));
            false
        }
    }
}

impl Shell {
    pub fn set_theme(&mut self, ctx: &egui::Context, theme: Theme) {
        self.theme = theme;
        self.palette = theme::apply(ctx, theme);
    }

    pub fn set_keymap(&mut self, kind: KeymapKind) {
        self.keymap = Keymap::new(kind);
        self.editor
            .set_status(StatusKind::Info, format!("Keyboard shortcuts: {}", kind.label()));
    }
}
