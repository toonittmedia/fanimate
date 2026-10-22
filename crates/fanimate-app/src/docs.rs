//! Several documents open at once, as tabs.
//!
//! The live document stays where the whole app expects it (`shell.editor`,
//! `shell.view`, the timeline zoom, the crash journal). Switching tabs parks
//! those in the tab being left and brings back what the other tab parked,
//! so each document keeps its own undo history, selection, current frame,
//! canvas view, timeline zoom and journal. The tool, brush and colours,
//! keyboard shortcuts and panel layout are shared.
//!
//! The tab bookkeeping (order, which one is live, closing) is
//! `fanimate_editor::Tabs`, with its tests; this module moves state between
//! the shell and the tabs.

use std::path::{Path, PathBuf};

use fanimate_editor::{Action, Editor, StatusKind, TabInfo, Tabs};
use serde::{Deserialize, Serialize};

use crate::app::{CanvasView, Shell};
use crate::recovery::{self, Session};

/// How many files the Home tab remembers.
pub const MAX_RECENT: usize = 12;

/// What opens when Fanimate starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Startup {
    /// The Home tab.
    #[default]
    Home,
    /// Every tab that was open when Fanimate last closed.
    LastSession,
    /// A new, empty file.
    NewFile,
    /// The file used last.
    LastFile,
}

impl Startup {
    pub const ALL: [Startup; 4] = [Self::Home, Self::LastSession, Self::NewFile, Self::LastFile];

    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::LastSession => "Reopen last session",
            Self::NewFile => "New blank file",
            Self::LastFile => "Open the most recent file",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Home => "New file presets, recent files and templates.",
            Self::LastSession => "Every tab that was open, each as you left it.",
            Self::NewFile => "An empty project, ready to draw in.",
            Self::LastFile => "Only the file you used last.",
        }
    }
}

/// What tabs share: the brush look, the tool and the clipboards.
type Carry = (
    fanimate_editor::DrawStyle,
    fanimate_editor::Tool,
    fanimate_editor::Clipboards,
);

/// A document in a background tab.
pub struct Parked {
    editor: Editor,
    view: CanvasView,
    /// Timeline zoom (pixels per frame), scroll (frames) and the width of the
    /// names column, and the layers whose property rows are open.
    timeline: (f32, f32, f32),
    expanded: std::collections::HashSet<fanimate_core::LayerId>,
    session: Session,
}

impl Parked {
    /// End the document's journal: it was saved or deliberately closed.
    pub fn end(self) {
        self.session.close();
    }
}

/// What the tab strip shows for `editor`.
pub fn tab_info(editor: &Editor) -> TabInfo {
    let title = match &editor.path {
        Some(path) => path
            .file_stem()
            .map_or_else(|| editor.doc.name.clone(), |s| s.to_string_lossy().into_owned()),
        None => editor.doc.name.clone(),
    };
    TabInfo {
        title,
        path: editor.path.clone(),
        dirty: editor.dirty,
    }
}

/// Actions that make sense with the Home tab in front, where no document is
/// showing: creating and opening files, and the app's own windows.
pub fn works_on_home(action: Action) -> bool {
    matches!(
        action,
        Action::New
            | Action::NewPreset(_)
            | Action::Open
            | Action::OpenFolder
            | Action::OpenOther
            | Action::OpenSample(_)
            | Action::ShowHome
            | Action::NextDocument
            | Action::PreviousDocument
            | Action::RenderQueue
            | Action::CheckForUpdates
            | Action::InstallUpdate
            | Action::Workspace(_)
            | Action::Preferences
            | Action::CommandPalette
            | Action::Quit
    )
}

/// Pick "Untitled", or "Untitled 2" and so on, so tabs are told apart.
pub fn untitled_name(tabs: &Tabs<Parked>) -> String {
    let taken = |name: &str| tabs.infos().any(|t| t.path.is_none() && t.title == name);
    if !taken("Untitled") {
        return "Untitled".into();
    }
    (2..)
        .map(|n| format!("Untitled {n}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| "Untitled".into())
}

impl Shell {
    /// Move the live document out into a parked state, leaving a
    /// placeholder editor behind that the caller replaces at once.
    fn take_live(&mut self) -> Parked {
        self.editor.playing = false;
        Parked {
            editor: std::mem::replace(&mut self.editor, Editor::untitled()),
            view: self.view.clone(),
            timeline: (
                self.ui.timeline_frame_width,
                self.ui.timeline_scroll,
                self.ui.timeline_names_width,
            ),
            expanded: std::mem::take(&mut self.ui.timeline_expanded),
            session: std::mem::take(&mut self.recovery.session),
        }
    }

    /// Make a parked document the live one. `carry` is what tabs share: the
    /// brush and the tool.
    fn install(&mut self, parked: Parked, carry: Carry) {
        let Parked {
            mut editor,
            view,
            timeline,
            expanded,
            session,
        } = parked;
        self.adopt(&mut editor, carry);
        self.editor = editor;
        self.view = view;
        (
            self.ui.timeline_frame_width,
            self.ui.timeline_scroll,
            self.ui.timeline_names_width,
        ) = timeline;
        self.ui.timeline_expanded = expanded;
        self.recovery.session = session;
        self.reset_gestures();
    }

    fn adopt(&self, editor: &mut Editor, carry: Carry) {
        editor.draw = carry.0;
        let _ = editor.run(Action::Tool(carry.1));
        editor.adopt_clipboards(carry.2);
    }

    fn carry(&self) -> Carry {
        (self.editor.draw, self.editor.tool, self.editor.clipboards())
    }

    /// Half-done gestures belong to the document they started in.
    fn reset_gestures(&mut self) {
        let ui = &mut self.ui;
        ui.renaming = None;
        ui.drag_origin = None;
        ui.shape_drag = None;
        ui.shape_drag_e = None;
        ui.point_gesture = None;
        ui.pen_down = false;
        ui.curvature_down = false;
        ui.magnet_active = false;
        ui.line_width_press = None;
        ui.active_edit = None;
    }

    /// Open `editor` as a new tab and bring it to the front. The old
    /// document stays open in its own tab.
    pub fn open_document(&mut self, mut editor: Editor) {
        let carry = self.carry();
        let info = tab_info(&editor);
        let live = self.docs.active().is_some().then(|| self.take_live());
        self.docs.open(live, info);
        self.ui.scroll_tab_into_view = true;
        self.adopt(&mut editor, carry);
        // the project's own styles join the app's, so a project made on
        // another computer keeps its looks
        self.styles.merge(&editor.doc.styles);
        self.editor = editor;
        self.view.fit();
        self.ui.timeline_scroll = 0.0;
        self.ui.timeline_expanded.clear();
        self.reset_gestures();
        // a new journal for it; the old document's travelled with its tab
        recovery::start(self);
    }

    /// Open a new, empty file as a tab.
    pub fn new_file(&mut self, mut editor: Editor) {
        editor.doc.name = untitled_name(&self.docs);
        self.open_document(editor);
    }

    /// Bring a tab to the front.
    pub fn switch_tab(&mut self, tab: usize) {
        let carry = self.carry();
        self.ui.scroll_tab_into_view = true;
        let mut tabs = std::mem::take(&mut self.docs);
        let next = tabs.switch(tab, || self.take_live());
        self.docs = tabs;
        if let Some(parked) = next {
            self.install(parked, carry);
        }
    }

    /// Show the Home tab. The document behind it stays open.
    pub fn show_home(&mut self) {
        self.editor.playing = false;
        self.docs.show_home();
    }

    /// Switch to the next or previous tab.
    pub fn cycle_tab(&mut self, forward: bool) {
        if let Some(tab) = self.docs.neighbour(forward) {
            self.switch_tab(tab);
        }
    }

    /// Close a tab, asking first if it has unsaved changes.
    pub fn request_close_tab(&mut self, tab: usize) {
        let dirty = if self.docs.active() == Some(tab) {
            self.editor.dirty
        } else {
            self.docs.info(tab).is_some_and(|t| t.dirty)
        };
        if dirty {
            // the question names the document in front
            self.switch_tab(tab);
            self.ui.confirm_discard = Some(crate::app::Discard::CloseTab);
        } else {
            self.close_tab(tab);
        }
    }

    /// Close every tab but `keep`. Tabs with unsaved changes stay open.
    pub fn close_other_tabs(&mut self, keep: usize) {
        let mut keep = keep;
        let mut kept_dirty = 0;
        // from the end, so the tabs still to look at keep their numbers
        for tab in (0..self.docs.len()).rev() {
            if tab == keep {
                continue;
            }
            let dirty = if self.docs.active() == Some(tab) {
                self.editor.dirty
            } else {
                self.docs.info(tab).is_some_and(|t| t.dirty)
            };
            if dirty {
                kept_dirty += 1;
                continue;
            }
            self.close_tab(tab);
            if tab < keep {
                keep -= 1;
            }
        }
        self.switch_tab(keep);
        if kept_dirty > 0 {
            let (s, were) = if kept_dirty == 1 {
                ("", "was")
            } else {
                ("s", "were")
            };
            self.editor.set_status(
                StatusKind::Info,
                format!("{kept_dirty} tab{s} with unsaved changes {were} left open"),
            );
        }
    }

    /// Close a tab without asking: it was saved, or its changes are not wanted.
    pub fn close_tab(&mut self, tab: usize) {
        if self.docs.active() == Some(tab) {
            let carry = self.carry();
            // the document is saved or discarded: nothing to recover
            std::mem::take(&mut self.recovery.session).close();
            match self.docs.close_active() {
                Some(parked) => self.install(parked, carry),
                None => {
                    // nothing open: Home shows, over a blank placeholder
                    self.editor = Editor::untitled();
                    self.editor.draw = carry.0;
                    self.editor.adopt_clipboards(carry.2);
                    self.view.fit();
                    self.reset_gestures();
                }
            }
        } else if let Some(parked) = self.docs.close_parked(tab) {
            parked.end();
        }
    }

    /// Put `path` first in the recent files.
    pub fn remember(&mut self, path: &Path) {
        self.places.remember_file(path);
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    /// Open a project, a folder project, or a Moho or Animate file as a tab,
    /// or bring its tab forward if it is open already.
    pub fn open_path(&mut self, path: &Path) {
        if let Some(tab) = self.docs.find_path(path) {
            self.switch_tab(tab);
            return;
        }
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |f| f.to_string_lossy().into_owned(),
        );
        if fanimate_import::can_import(path) {
            match Editor::import_project(path) {
                Ok(editor) => self.open_document(editor),
                Err(e) => self
                    .editor
                    .set_status(StatusKind::Error, format!("Could not open {file}: {e}")),
            }
            return;
        }
        match Editor::open(path) {
            Ok((editor, report)) => {
                self.open_document(editor);
                self.remember(path);
                if report.is_clean() {
                    self.editor
                        .set_status(StatusKind::Info, format!("Opened {}", path.display()));
                } else {
                    let n = report.issues.len();
                    let fixed = report.repaired_count();
                    self.editor.set_status(
                        StatusKind::Warning,
                        format!("Opened with {n} issue(s); {fixed} repaired automatically. Save to keep the repairs."),
                    );
                }
            }
            Err(e) => {
                if !path.exists() {
                    // gone from disk: stop offering it
                    self.recent.retain(|p| p != path);
                }
                self.editor
                    .set_status(StatusKind::Error, format!("Could not open {file}: {e}"));
            }
        }
    }

    /// Open what Fanimate was started with, or what the user chose to start
    /// with.
    pub fn open_startup(&mut self, files: &[PathBuf], startup: Startup, session: &[PathBuf], front: usize) {
        if !files.is_empty() {
            for file in files {
                self.open_path(file);
            }
            return;
        }
        match startup {
            Startup::Home => {}
            Startup::NewFile => self.new_file(Editor::untitled()),
            Startup::LastFile => {
                if let Some(path) = self.recent.first().cloned() {
                    self.open_path(&path);
                }
            }
            Startup::LastSession => {
                for path in session {
                    self.open_path(path);
                }
                if let Some(tab) = (front < self.docs.len()).then_some(front) {
                    self.switch_tab(tab);
                }
            }
        }
    }

    /// Every open document that is saved somewhere, in tab order, and which
    /// of them is in front: what "reopen last session" opens next time.
    pub fn session(&self) -> (Vec<PathBuf>, usize) {
        let mut paths = Vec::new();
        let mut front = 0;
        for (i, info) in self.docs.infos().enumerate() {
            if let Some(path) = &info.path {
                if self.docs.active() == Some(i) {
                    front = paths.len();
                }
                paths.push(path.clone());
            }
        }
        (paths, front)
    }

    /// Keep the tab strip's picture of the live document current.
    pub fn sync_tab_info(&mut self) {
        let info = tab_info(&self.editor);
        self.docs.set_active_info(info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_titles_are_file_names_or_project_names() {
        let mut ed = Editor::untitled();
        assert_eq!(tab_info(&ed).title, "Untitled");
        ed.path = Some(PathBuf::from("/work/walk cycle.fnm"));
        ed.dirty = true;
        let info = tab_info(&ed);
        assert_eq!(info.title, "walk cycle");
        assert!(info.dirty);
    }

    #[test]
    fn new_files_are_numbered_so_tabs_are_told_apart() {
        let mut tabs: Tabs<Parked> = Tabs::new();
        assert_eq!(untitled_name(&tabs), "Untitled");
        for expected in ["Untitled", "Untitled 2", "Untitled 3"] {
            let name = untitled_name(&tabs);
            assert_eq!(name, expected);
            tabs.open(
                None,
                TabInfo {
                    title: name,
                    ..Default::default()
                },
            );
        }
    }

    #[test]
    fn only_global_actions_work_with_home_in_front() {
        for a in [
            Action::New,
            Action::NewPreset(fanimate_editor::Preset::Hd1080),
            Action::Open,
            Action::Quit,
            Action::Preferences,
            Action::ShowHome,
        ] {
            assert!(works_on_home(a), "{a:?}");
        }
        for a in [
            Action::Save,
            Action::Undo,
            Action::Paste,
            Action::PlayPause,
            Action::ExportVideo,
        ] {
            assert!(!works_on_home(a), "{a:?}");
        }
    }

    #[test]
    fn startup_choices_are_all_named() {
        for s in Startup::ALL {
            assert!(!s.label().is_empty() && !s.description().is_empty());
        }
        assert_eq!(Startup::default(), Startup::Home);
    }
}
