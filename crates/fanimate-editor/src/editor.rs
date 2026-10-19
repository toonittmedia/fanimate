//! The editor: an open document plus everything about editing it.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use fanimate_core::anim::Animatable;
use fanimate_core::bones::Skeleton;
use fanimate_core::{
    Affine2, Channel, Document, Frame, Interpolation, Label, Layer, LayerId, LayerKind, Transform, samples,
};
use fanimate_fnm::{FnmError, HistoryRecord, Report};
use serde::{Deserialize, Serialize};

use crate::action::{Action, Tool};
use crate::history::History;

mod actions;
mod bones;
mod booleans;
mod construct;
mod draw;
mod handles;
mod import;
mod points;
mod shapes;
mod shapesel;
mod sound;
mod svg;
mod tools_e;
pub use bones::BoneRef;
pub use draw::DrawStyle;
pub use handles::{Handle, HandleMods, SelectionFrame};
pub use points::PointRef;
pub use shapesel::RadiusTarget;
pub use sound::{MouthCue, SoundInfo, parse_rhubarb, sound_info};
pub use tools_e::ShapeBuilderState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    /// Plain click: select only this.
    Replace,
    /// Ctrl-click: add or remove this.
    Toggle,
    /// Shift-click: everything between the last clicked layer and this.
    Range,
}

/// Where dropped layers go relative to the layer under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Above,
    Below,
    /// Inside a group or bone layer, on top of its children.
    Into,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
}

/// What running an action asks of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Handled by the editor.
    Done,
    /// Handled; put this text on the system clipboard.
    CopyText(String),
    /// Needs the UI: a dialog, the view, or quitting.
    App(Action),
}

/// Layers on the clipboard, as JSON. Tagged so pasting arbitrary text is
/// recognised and ignored.
#[derive(Serialize, Deserialize)]
struct ClipboardData {
    fanimate: String,
    version: u32,
    layers: Vec<Layer>,
}

const CLIPBOARD_TAG: &str = "layers";

pub struct Editor {
    pub doc: Document,
    /// Where the project is saved; `None` until the first save.
    pub path: Option<PathBuf>,
    /// Unsaved changes.
    pub dirty: bool,
    /// Counts edits, undos and redos, so the app can tell when the document
    /// changed (for the crash recovery journal).
    revision: u64,
    /// Selected layers in click order; the last is the primary selection.
    pub selection: Vec<LayerId>,
    /// Selected points, for the point tools.
    pub points: BTreeSet<PointRef>,
    /// Current time; fractional while playing.
    pub frame: f32,
    pub playing: bool,
    pub tool: Tool,
    pub history: History,
    /// Groups collapsed in the Layers panel.
    pub collapsed: HashSet<LayerId>,
    /// Set when the UI should start renaming a layer in place.
    pub rename_request: Option<LayerId>,
    pub status: Option<(StatusKind, String)>,
    /// Colours and brush settings for new artwork.
    pub draw: DrawStyle,
    /// The brush stroke being drawn.
    stroke: Option<draw::StrokeInProgress>,
    /// The eraser gesture in progress.
    erase: Option<draw::EraseInProgress>,
    /// The Add Point drag in progress.
    add_point: Option<construct::AddPointDrag>,
    /// The path being drawn with the Pen.
    pen: Option<construct::PenPath>,
    /// The path being drawn with the Curvature tool.
    curvature: Option<tools_e::CurvaturePath>,
    /// Polygon side count (3..=32), Star point count (2..=16), for the drag tools.
    pub poly_sides: u32,
    pub star_points: u32,
    /// Star inner-radius fraction (inner/outer), 0..1.
    pub star_inner: f32,
    /// Shape Builder drag state.
    pub shape_builder: Option<ShapeBuilderState>,
    clipboard: Option<String>,
    /// Last plainly clicked layer, the anchor for Shift-click ranges.
    anchor: Option<LayerId>,
    /// Keys being dragged along the timeline.
    key_drag: Option<KeyDrag>,
    /// The selected bone, for the bone tools and binding.
    bone: Option<BoneRef>,
    /// The bone gesture in progress.
    bone_drag: Option<bones::BoneDrag>,
    /// The sub-layer the tools draw on in frame-by-frame layers.
    pub part: fanimate_core::Part,
    /// The selection frame handle being dragged, and what to show beside it.
    handle_drag: Option<handles::HandleDrag>,
    handle_readout: Option<String>,
}

/// Retiming keys by dragging them on the timeline. The selected layers are
/// kept as they were when the drag started, so a key dragged across
/// another and back leaves it unharmed.
struct KeyDrag {
    from: Frame,
    to: Frame,
    originals: Vec<Layer>,
    recorded: bool,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let frame = doc.start as f32;
        Self {
            doc,
            path: None,
            dirty: false,
            revision: 0,
            selection: Vec::new(),
            points: BTreeSet::new(),
            frame,
            playing: false,
            tool: Tool::default(),
            history: History::default(),
            collapsed: HashSet::new(),
            rename_request: None,
            status: None,
            draw: DrawStyle::default(),
            stroke: None,
            erase: None,
            add_point: None,
            pen: None,
            curvature: None,
            poly_sides: 6,
            star_points: 5,
            star_inner: 0.4,
            shape_builder: None,
            clipboard: None,
            anchor: None,
            key_drag: None,
            bone: None,
            bone_drag: None,
            part: fanimate_core::Part::default(),
            handle_drag: None,
            handle_readout: None,
        }
    }

    /// A new, empty 1280x720 project with one vector layer.
    pub fn untitled() -> Self {
        Self::untitled_sized("Untitled", 1280.0, 720.0, 24.0)
    }

    /// A new, empty project for a [`Preset`](crate::Preset).
    pub fn untitled_preset(preset: crate::Preset) -> Self {
        let (width, height) = preset.size();
        Self::untitled_sized("Untitled", width, height, preset.fps())
    }

    /// A new, empty project of a chosen size and frame rate, five seconds
    /// long, with one vector layer. Sizes are kept within what the format
    /// and the renderer accept.
    pub fn untitled_sized(name: &str, width: f32, height: f32, fps: f32) -> Self {
        let fps = fps.clamp(1.0, 120.0);
        let mut doc = Document::new(name, width.clamp(16.0, 16384.0), height.clamp(16.0, 16384.0), fps);
        doc.end = (fps * 5.0).round() as i32 - 1;
        let layer = labelled(doc.new_layer("Layer 1", LayerKind::vector(vec![])));
        let id = layer.id;
        doc.layers.push(layer);
        let mut ed = Self::new(doc);
        ed.selection = vec![id];
        ed
    }

    pub fn sample() -> Self {
        Self::new(samples::hello())
    }

    /// Open a project. Damage the validator can fix is repaired (and marks
    /// the project as changed, so the repair is saved).
    pub fn open(path: &Path) -> Result<(Self, Report), FnmError> {
        let (doc, report) = fanimate_fnm::load_and_repair(path)?;
        let mut ed = Self::new(doc);
        ed.path = Some(path.to_path_buf());
        ed.dirty = report.repaired_count() > 0;
        Ok((ed, report))
    }

    /// Open a Moho or Animate project as a new, unsaved Fanimate project.
    /// The status says what did not come through exactly.
    pub fn import_project(path: &Path) -> Result<Self, String> {
        let mut im = fanimate_import::import(path)?;
        fanimate_fnm::repair(&mut im.doc);
        im.doc.walk_mut(|l| {
            if l.label.is_none() {
                l.label = Some(Label::for_id(l.id));
            }
        });
        let mut ed = Self::new(im.doc);
        ed.dirty = true;
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |f| f.to_string_lossy().into_owned(),
        );
        if im.notes.is_empty() {
            ed.set_status(
                StatusKind::Success,
                format!("Imported {file}. Save to keep it as a Fanimate project."),
            );
        } else {
            ed.set_status(
                StatusKind::Warning,
                format!("Imported {file}; {}. Save to keep it.", im.notes.join("; ")),
            );
        }
        Ok(ed)
    }

    /// Adopt work recovered after a crash. It is unsaved until saved
    /// again, to `path` if it had been saved before.
    /// Its undo history comes back too, when the journal kept it.
    pub fn recovered(doc: Document, path: Option<PathBuf>, history: Option<HistoryRecord>) -> Self {
        let mut ed = Self::new(doc);
        ed.path = path;
        if let Some(history) = history.and_then(History::from_record) {
            ed.history = history;
        }
        ed.dirty = true;
        ed.set_status(StatusKind::Success, "Recovered unsaved work. Save to keep it.");
        ed
    }

    /// See the `revision` field.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The undo history with the document as it is now, for the crash
    /// recovery journal.
    pub fn history_record(&mut self) -> HistoryRecord {
        self.history.record(&self.doc, &self.selection)
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), FnmError> {
        fanimate_fnm::save(&self.doc, path)?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.set_status(StatusKind::Success, format!("Saved {}", path.display()));
        Ok(())
    }

    pub fn set_status(&mut self, kind: StatusKind, message: impl Into<String>) {
        self.status = Some((kind, message.into()));
    }

    /// Record the state before an edit. Call once per user action: for a
    /// drag, at drag start, not on every movement.
    pub fn checkpoint(&mut self, label: &str) {
        self.history.push(label, &self.doc, &self.selection);
        self.dirty = true;
        self.revision += 1;
    }

    // ------------------------------------------------------------------
    // Actions

    pub fn run(&mut self, action: Action) -> Outcome {
        use Action::*;
        match action {
            Undo => self.undo(),
            Redo => self.redo(),
            UndoLayer => self.undo_layer(),
            GoToHistory(state) => self.go_to_history(state),
            Copy => {
                return match self.copy() {
                    Some(text) => Outcome::CopyText(text),
                    None => Outcome::Done,
                };
            }
            Cut => {
                return match self.cut() {
                    Some(text) => Outcome::CopyText(text),
                    None => Outcome::Done,
                };
            }
            Paste => {
                self.paste(None);
            }
            Duplicate => self.duplicate(),
            // with a point tool, editing commands act on points
            Delete if self.tool.edits_points() && !self.points.is_empty() => {
                self.delete_points();
            }
            // with a bone tool, Delete deletes the selected bone
            Delete
                if self.tool.group() == crate::action::ToolGroup::Bone && self.selected_bone().is_some() =>
            {
                self.delete_selected_bone();
            }
            BindToBone => {
                self.bind_to_bone();
            }
            NewAction => {
                self.new_action(false);
            }
            NewFramesLayer => self.new_layer(NewKind::Frames),
            NewSwitchLayer => self.new_layer(NewKind::Switch),
            SetSwitch(i) => {
                self.set_switch(i);
            }
            NewDrawing => {
                self.new_drawing(false);
            }
            SetPart(p) => {
                self.part = p;
                self.points.clear();
            }
            NewSmartBoneAction => {
                self.new_action(true);
            }
            EditAction(i) => {
                self.edit_action(i);
            }
            FinishAction => {
                self.finish_action();
            }
            InsertAction(i) => {
                self.insert_action(i);
            }
            DeleteAction(i) => {
                self.delete_action(i);
            }
            BindSmoothly => {
                self.bind_smoothly();
            }
            Unbind => {
                self.unbind();
            }
            // Esc ends a Pen or Curvature path, keeping it, as in Illustrator
            SelectNone | FinishPath if self.is_drawing_curvature() => self.finish_curvature(),
            SelectNone | FinishPath if self.is_drawing_path() => self.finish_path(),
            FinishPath => {}
            Union => {
                self.combine_shapes(fanimate_core::BoolOp::Union);
            }
            Subtract => {
                self.combine_shapes(fanimate_core::BoolOp::Subtract);
            }
            Intersect => {
                self.combine_shapes(fanimate_core::BoolOp::Intersect);
            }
            ExpandShape => self.expand_shape(),
            SelectAll if self.tool.edits_points() => self.select_all_points(),
            SelectNone if self.tool.edits_points() => self.points.clear(),
            Delete => self.delete_selected(),
            SelectAll => self.select_all(),
            SelectNone => self.selection.clear(),
            Rename => self.rename_request = self.primary(),
            SetLabel(label) => self.set_label(label),
            NewVectorLayer => self.new_layer(NewKind::Vector),
            NewGroupLayer => self.new_layer(NewKind::Group),
            NewBoneLayer => self.new_layer(NewKind::Bone),
            GroupLayers => self.group_selected(),
            Ungroup => self.ungroup_selected(),
            MoveLayerUp => self.move_selected(true),
            MoveLayerDown => self.move_selected(false),
            ToggleVisibility => self.toggle_visibility(),
            PlayPause => self.playing = !self.playing,
            NextFrame => self.step(1),
            PrevFrame => self.step(-1),
            FirstFrame => self.go_to(self.doc.start),
            LastFrame => self.go_to(self.doc.end),
            NextKey => self.jump_key(true),
            PrevKey => self.jump_key(false),
            AddKey => self.add_key(),
            DeleteKey => self.delete_key(),
            SetInterpolation(interp) => self.set_interpolation(interp),
            Tool(t) => self.set_tool(t),
            AddPointOrBone => {
                let bone = self
                    .primary()
                    .and_then(|id| self.doc.find(id))
                    .is_some_and(|l| matches!(l.kind, LayerKind::Bone { .. }));
                // full path: `Tool` here is the Action variant
                use crate::action::Tool as T;
                self.set_tool(if bone { T::AddBone } else { T::AddPoint });
            }
            New | NewPreset(_) | Open | OpenFolder | OpenOther | OpenSample(_) | CloseDocument
            | CloseOtherDocuments | NextDocument | PreviousDocument | ShowHome | Save | SaveAs | Import
            | ExportVideo | RenderQueue | LipSync | CheckForUpdates | InstallUpdate | Workspace(_) | Quit
            | ToggleOnionSkin | ToggleLightTable | Preferences | CommandPalette | LayerProperties
            | ZoomIn | ZoomOut | FitView | ActualSize => return Outcome::App(action),
        }
        Outcome::Done
    }

    /// Whether `action` can run now, for greying out menu items.
    pub fn can_run(&self, action: Action) -> bool {
        use Action::*;
        let has_sel = !self.selection.is_empty();
        match action {
            Undo => self.history.undo_label().is_some(),
            Redo => self.history.redo_label().is_some(),
            UndoLayer => self
                .primary()
                .is_some_and(|id| self.history.before_last_change(id, &self.doc).is_some()),
            GoToHistory(state) => state != self.history.cursor(),
            Cut | Copy | Duplicate | Delete | GroupLayers | MoveLayerUp | MoveLayerDown
            | ToggleVisibility | AddKey | DeleteKey => has_sel,
            Rename | LayerProperties => self.primary().is_some(),
            SetLabel(_) => has_sel,
            BindToBone | BindSmoothly | Unbind => self.can_bind(),
            NewAction => self.actions_layer().is_some(),
            NewSmartBoneAction => self
                .actions_layer()
                .is_some_and(|l| self.selected_bone().is_some_and(|b| b.layer == l)),
            FinishAction => self.editing_action().is_some(),
            SetSwitch(_) => self.switch_layer().is_some(),
            LipSync => self.switch_layer().is_some(),
            NewDrawing => self.primary().is_some_and(|id| {
                matches!(self.doc.find(id).map(|l| &l.kind), Some(LayerKind::Frames { .. }))
            }),
            EditAction(i) | InsertAction(i) | DeleteAction(i) => self
                .actions_layer()
                .is_some_and(|l| i < self.rig_actions(l).len()),
            Paste => self.clipboard.is_some(),
            FinishPath => self.is_drawing_path(),
            Union | Subtract | Intersect => self.can_combine(),
            ExpandShape => self.expandable_shapes() > 0,
            SetInterpolation(_) => self.key_interpolation().is_some(),
            Ungroup => self.selected_roots().iter().any(|id| {
                self.doc
                    .find(*id)
                    .is_some_and(|l| matches!(l.kind, LayerKind::Group { .. }))
            }),
            _ => true,
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        if tool != Tool::Pen {
            self.finish_path();
        }
        if tool != Tool::Curvature {
            self.cancel_curvature();
        }
        self.tool = tool;
        if !tool.is_available() {
            self.set_status(StatusKind::Info, format!("{}: {}", tool.label(), tool.hint()));
        }
    }

    // ------------------------------------------------------------------
    // Undo

    fn undo(&mut self) {
        match self.history.undo(&mut self.doc, &mut self.selection) {
            Some(label) => {
                self.dirty = true;
                self.revision += 1;
                self.prune_selection();
                self.set_status(StatusKind::Info, format!("Undid {label}"));
            }
            None => self.set_status(StatusKind::Info, "Nothing to undo"),
        }
    }

    fn redo(&mut self) {
        match self.history.redo(&mut self.doc, &mut self.selection) {
            Some(label) => {
                self.dirty = true;
                self.revision += 1;
                self.prune_selection();
                self.set_status(StatusKind::Info, format!("Redid {label}"));
            }
            None => self.set_status(StatusKind::Info, "Nothing to redo"),
        }
    }

    /// Go straight to a state in the History panel (0 is the oldest).
    fn go_to_history(&mut self, state: usize) {
        if self.history.go_to(state, &mut self.doc, &mut self.selection) {
            self.dirty = true;
            self.revision += 1;
            self.prune_selection();
        }
    }

    /// Take the selected layer back to before its own last change, leaving
    /// everything else as it is. Itself an undoable step.
    fn undo_layer(&mut self) {
        let Some(id) = self.primary() else { return };
        let Some((old, label)) = self.history.before_last_change(id, &self.doc) else {
            self.set_status(StatusKind::Info, "Nothing to undo on this layer");
            return;
        };
        let name = self.doc.find(id).map(|l| l.name.clone()).unwrap_or_default();
        self.checkpoint(&format!("Undo {label} on {name}"));
        if let Some(l) = self.doc.find_mut(id) {
            *l = old;
        }
        self.prune_selection();
        self.set_status(StatusKind::Info, format!("Undid {label} on \"{name}\""));
    }

    fn prune_selection(&mut self) {
        let doc = &self.doc;
        self.selection.retain(|id| doc.find(*id).is_some());
        // a point selection that no longer fits (the shape has other points now)
        // would be a half of a shape: better none
        let before = self.points.len();
        self.prune_points();
        if self.points.len() != before {
            self.points.clear();
        }
    }

    // ------------------------------------------------------------------
    // Selection and layer order

    pub fn primary(&self) -> Option<LayerId> {
        self.selection.last().copied()
    }

    pub fn is_selected(&self, id: LayerId) -> bool {
        self.selection.contains(&id)
    }

    /// Layers as the Layers panel lists them: top of the stack first,
    /// children indented under their parent, collapsed groups' children
    /// hidden. Pairs of (layer, depth).
    pub fn display_rows(&self) -> Vec<(LayerId, usize)> {
        self.rows(true)
    }

    fn rows(&self, honour_collapsed: bool) -> Vec<(LayerId, usize)> {
        fn go(
            layers: &[Layer],
            depth: usize,
            collapsed: Option<&HashSet<LayerId>>,
            out: &mut Vec<(LayerId, usize)>,
        ) {
            // last in the list is drawn on top, so it is listed first
            for l in layers.iter().rev() {
                out.push((l.id, depth));
                if let Some(children) = l.kind.children()
                    && !collapsed.is_some_and(|c| c.contains(&l.id))
                {
                    go(children, depth + 1, collapsed, out);
                }
            }
        }
        let mut out = Vec::new();
        go(
            &self.doc.layers,
            0,
            honour_collapsed.then_some(&self.collapsed),
            &mut out,
        );
        out
    }

    pub fn select(&mut self, id: LayerId, mode: SelectMode) {
        match mode {
            SelectMode::Replace => {
                self.selection = vec![id];
                self.anchor = Some(id);
            }
            SelectMode::Toggle => {
                if let Some(i) = self.selection.iter().position(|s| *s == id) {
                    self.selection.remove(i);
                } else {
                    self.selection.push(id);
                }
                self.anchor = Some(id);
            }
            SelectMode::Range => {
                let rows = self.display_rows();
                let pos = |x: LayerId| rows.iter().position(|(r, _)| *r == x);
                match (self.anchor.and_then(pos), pos(id)) {
                    (Some(a), Some(b)) => {
                        let (lo, hi) = (a.min(b), a.max(b));
                        self.selection = rows[lo..=hi].iter().map(|(r, _)| *r).collect();
                        // keep the clicked layer primary
                        self.selection.retain(|r| *r != id);
                        self.selection.push(id);
                    }
                    _ => self.select(id, SelectMode::Replace),
                }
            }
        }
    }

    pub fn select_all(&mut self) {
        self.selection = self.rows(false).into_iter().map(|(id, _)| id).collect();
    }

    /// Selected layers that are not inside another selected layer, in
    /// display order (top first). Commands act on these, so a group and
    /// its child are never copied twice.
    pub fn selected_roots(&self) -> Vec<LayerId> {
        self.rows(false)
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| self.is_selected(*id))
            .filter(|id| {
                !self
                    .selection
                    .iter()
                    .any(|other| other != id && self.doc.is_within(*id, *other))
            })
            .collect()
    }

    /// Where new and pasted layers go: just above the primary selection,
    /// in the same parent; otherwise on top of the stack.
    fn insertion_point(&self) -> (Option<LayerId>, usize) {
        self.primary()
            .and_then(|id| self.doc.locate(id))
            .map(|(parent, index)| (parent, index + 1))
            .unwrap_or((None, self.doc.layers.len()))
    }

    // ------------------------------------------------------------------
    // Layer commands

    fn new_layer(&mut self, kind: NewKind) {
        self.checkpoint("New layer");
        let (word, kind) = match kind {
            NewKind::Vector => ("Layer", LayerKind::vector(vec![])),
            NewKind::Group => ("Group", LayerKind::Group { children: vec![] }),
            NewKind::Frames => ("Animation", LayerKind::Frames { cels: vec![] }),
            NewKind::Switch => (
                "Switch",
                LayerKind::Switch {
                    children: vec![],
                    switch: vec![],
                },
            ),
            NewKind::Bone => (
                "Bones",
                LayerKind::Bone {
                    skeleton: Skeleton::default(),
                    children: vec![],
                },
            ),
        };
        let name = self.unused_name(word);
        let layer = labelled(self.doc.new_layer(name, kind));
        let id = layer.id;
        let (parent, index) = self.insertion_point();
        self.doc.insert(parent, index, layer);
        self.selection = vec![id];
        self.anchor = Some(id);
        self.rename_request = Some(id);
    }

    fn delete_selected(&mut self) {
        let roots = self.selected_roots();
        if roots.is_empty() {
            return;
        }
        self.checkpoint("Delete");
        for id in &roots {
            self.doc.remove(*id);
        }
        self.selection.clear();
        let n = roots.len();
        self.set_status(StatusKind::Info, format!("Deleted {n} layer{}", plural(n)));
    }

    fn duplicate(&mut self) {
        let roots = self.selected_roots();
        if roots.is_empty() {
            return;
        }
        self.checkpoint("Duplicate");
        let mut new_sel = Vec::new();
        for id in roots {
            let Some(original) = self.doc.find(id).cloned() else {
                continue;
            };
            let Some((parent, index)) = self.doc.locate(id) else {
                continue;
            };
            let mut copy = original;
            self.doc.reassign_ids(&mut copy);
            copy.name = format!("{} copy", copy.name);
            new_sel.push(copy.id);
            self.doc.insert(parent, index + 1, copy);
        }
        self.selection = new_sel;
    }

    /// Copy the selected layers; returns clipboard text.
    pub fn copy(&mut self) -> Option<String> {
        let layers: Vec<Layer> = self
            .selected_roots()
            .into_iter()
            .rev() // document order, bottom first
            .filter_map(|id| self.doc.find(id).cloned())
            .collect();
        if layers.is_empty() {
            return None;
        }
        let n = layers.len();
        let data = ClipboardData {
            fanimate: CLIPBOARD_TAG.into(),
            version: fanimate_fnm::FORMAT_VERSION,
            layers,
        };
        let text = serde_json::to_string(&data).ok()?;
        self.clipboard = Some(text.clone());
        self.set_status(StatusKind::Info, format!("Copied {n} layer{}", plural(n)));
        Some(text)
    }

    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        self.checkpoint("Cut");
        for id in self.selected_roots() {
            self.doc.remove(id);
        }
        self.selection.clear();
        Some(text)
    }

    /// Paste layers from `text` (the system clipboard) if it holds Fanimate
    /// layers, else from the internal clipboard. Returns whether anything
    /// was pasted.
    pub fn paste(&mut self, text: Option<&str>) -> bool {
        let parse = |t: &str| {
            serde_json::from_str::<ClipboardData>(t)
                .ok()
                .filter(|d| d.fanimate == CLIPBOARD_TAG && d.version <= fanimate_fnm::FORMAT_VERSION)
        };
        let data = text
            .and_then(parse)
            .or_else(|| self.clipboard.as_deref().and_then(parse));
        let Some(data) = data else {
            return false;
        };
        if data.layers.is_empty() {
            return false;
        }
        self.checkpoint("Paste");
        let (parent, mut index) = self.insertion_point();
        let mut pasted = Vec::new();
        for mut layer in data.layers {
            self.doc.reassign_ids(&mut layer);
            pasted.push(layer.id);
            if !self.doc.insert(parent, index, layer.clone()) {
                self.doc.insert(None, self.doc.layers.len(), layer);
            }
            index += 1;
        }
        self.selection = pasted;
        true
    }

    /// Move each selected layer one step up (towards the top of the stack)
    /// or down among its siblings.
    fn move_selected(&mut self, up: bool) {
        let mut roots = self.selected_roots();
        if roots.is_empty() {
            return;
        }
        // move the leading layer first so neighbours do not block each other
        if !up {
            roots.reverse();
        }
        let mut moved = false;
        let mut checkpointed = false;
        for id in roots {
            let Some((parent, index)) = self.doc.locate(id) else {
                continue;
            };
            let Some(len) = self.doc.siblings_mut(parent).map(|s| s.len()) else {
                continue;
            };
            let target = if up { index + 1 } else { index.wrapping_sub(1) };
            if target >= len {
                continue;
            }
            let neighbour = self.doc.siblings_mut(parent).map(|s| s[target].id);
            if neighbour.is_some_and(|n| self.is_selected(n)) {
                continue;
            }
            if !checkpointed {
                self.checkpoint(if up { "Move layer up" } else { "Move layer down" });
                checkpointed = true;
            }
            if let Some(list) = self.doc.siblings_mut(parent) {
                list.swap(index, target);
                moved = true;
            }
        }
        if !moved {
            self.set_status(StatusKind::Info, "Already at the end of its group");
        }
    }

    /// Put the selected layers into a new group where the primary
    /// selection was.
    fn group_selected(&mut self) {
        let roots = self.selected_roots();
        let Some(anchor) = self
            .primary()
            .filter(|p| roots.contains(p))
            .or(roots.first().copied())
        else {
            return;
        };
        let Some((parent, _)) = self.doc.locate(anchor) else {
            return;
        };
        self.checkpoint("Group layers");
        // the group takes the place of the topmost selected layer in `parent`
        let index = roots
            .iter()
            .filter_map(|id| self.doc.locate(*id))
            .filter(|(p, _)| *p == parent)
            .map(|(_, i)| i)
            .max()
            .unwrap_or(0);
        let removed_below = roots
            .iter()
            .filter_map(|id| self.doc.locate(*id))
            .filter(|(p, i)| *p == parent && *i < index)
            .count();
        let mut children: Vec<Layer> = Vec::new();
        for id in roots.iter().rev() {
            if let Some(layer) = self.doc.remove(*id) {
                children.push(layer);
            }
        }
        let name = self.unused_name("Group");
        let group = labelled(self.doc.new_layer(name, LayerKind::Group { children }));
        let gid = group.id;
        self.doc.insert(parent, index - removed_below, group);
        self.selection = vec![gid];
    }

    /// Replace selected groups with their contents. Only groups with no
    /// transform or opacity of their own, so nothing visibly moves.
    fn ungroup_selected(&mut self) {
        let groups: Vec<LayerId> = self
            .selected_roots()
            .into_iter()
            .filter(|id| {
                self.doc
                    .find(*id)
                    .is_some_and(|l| matches!(l.kind, LayerKind::Group { .. }))
            })
            .collect();
        let (plain, transformed): (Vec<_>, Vec<_>) = groups.into_iter().partition(|id| {
            self.doc.find(*id).is_some_and(|l| {
                !l.transform.is_animated()
                    && l.transform.sample(0.0) == Transform::IDENTITY
                    && !l.opacity.is_animated()
                    && l.opacity.base == 1.0
            })
        });
        if !transformed.is_empty() {
            self.set_status(
                StatusKind::Warning,
                "A group with its own transform or opacity can't be ungrouped yet; reset it first",
            );
        }
        if plain.is_empty() {
            return;
        }
        self.checkpoint("Ungroup");
        let mut new_sel = Vec::new();
        for id in plain {
            let Some((parent, index)) = self.doc.locate(id) else {
                continue;
            };
            let Some(group) = self.doc.remove(id) else {
                continue;
            };
            if let LayerKind::Group { children } = group.kind {
                for (offset, child) in children.into_iter().enumerate() {
                    new_sel.push(child.id);
                    self.doc.insert(parent, index + offset, child);
                }
            }
        }
        self.selection = new_sel;
    }

    /// Move layers next to, or into, `target` (drag and drop in the Layers
    /// panel). Layers keep their relative order. Refuses to move a layer
    /// into itself or its own descendants.
    pub fn move_relative(&mut self, ids: &[LayerId], target: LayerId, placement: Placement) -> bool {
        let roots: Vec<LayerId> = self
            .rows(false)
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| ids.contains(id))
            .filter(|id| !ids.iter().any(|o| o != id && self.doc.is_within(*id, *o)))
            .collect();
        if roots.is_empty() || roots.iter().any(|r| self.doc.is_within(target, *r)) {
            return false;
        }
        if placement == Placement::Into && self.doc.find(target).is_none_or(|l| l.kind.children().is_none()) {
            return false;
        }
        self.checkpoint("Move layers");
        // remove bottom-most first; collect in document order (bottom first)
        let mut moving: Vec<Layer> = roots.iter().rev().filter_map(|id| self.doc.remove(*id)).collect();
        let (parent, index) = match placement {
            Placement::Into => {
                let len = self
                    .doc
                    .find(target)
                    .and_then(|l| l.kind.children())
                    .map_or(0, |c| c.len());
                (Some(target), len)
            }
            Placement::Above | Placement::Below => {
                let Some((parent, i)) = self.doc.locate(target) else {
                    return false;
                };
                (parent, if placement == Placement::Above { i + 1 } else { i })
            }
        };
        for (offset, layer) in moving.drain(..).enumerate() {
            self.doc.insert(parent, index + offset, layer);
        }
        self.selection = roots;
        true
    }

    /// Transform from a layer's parent space to document space at the
    /// current frame. Used to turn canvas drags into layer movement.
    pub fn parent_world(&self, id: LayerId) -> Affine2 {
        let mut chain = Vec::new();
        let mut current = self.doc.locate(id).and_then(|(p, _)| p);
        while let Some(c) = current {
            chain.push(c);
            current = self.doc.locate(c).and_then(|(p, _)| p);
        }
        chain
            .iter()
            .rev()
            .filter_map(|id| self.doc.find(*id))
            .fold(Affine2::IDENTITY, |acc, l| {
                acc * l.transform.sample(self.frame).to_affine()
            })
    }

    /// Transform from a layer's own space to document space.
    pub fn world(&self, id: LayerId) -> Affine2 {
        let own = self
            .doc
            .find(id)
            .map_or(Affine2::IDENTITY, |l| l.transform.sample(self.frame).to_affine());
        self.parent_world(id) * own
    }

    /// `"{word} N"` with the lowest N not already used by a layer.
    fn unused_name(&self, word: &str) -> String {
        let mut used = HashSet::new();
        self.doc.walk(|l| {
            if let Some(n) = l
                .name
                .strip_prefix(word)
                .and_then(|r| r.trim().parse::<usize>().ok())
            {
                used.insert(n);
            }
        });
        let n = (1..).find(|n| !used.contains(n)).unwrap_or(1);
        format!("{word} {n}")
    }

    pub fn rename(&mut self, id: LayerId, name: &str) {
        let name = name.trim();
        if name.is_empty() || self.doc.find(id).is_none_or(|l| l.name == name) {
            return;
        }
        self.checkpoint("Rename");
        if let Some(l) = self.doc.find_mut(id) {
            l.name = name.to_owned();
        }
    }

    /// Give the selected layers a label colour.
    fn set_label(&mut self, label: Label) {
        let ids: Vec<LayerId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| self.doc.find(*id).is_some_and(|l| l.label != Some(label)))
            .collect();
        if ids.is_empty() {
            return;
        }
        self.checkpoint("Label colour");
        for id in ids {
            if let Some(l) = self.doc.find_mut(id) {
                l.label = Some(label);
            }
        }
    }

    pub fn set_visible(&mut self, id: LayerId, visible: bool) {
        if self.doc.find(id).is_none_or(|l| l.visible == visible) {
            return;
        }
        self.checkpoint(if visible { "Show layer" } else { "Hide layer" });
        if let Some(l) = self.doc.find_mut(id) {
            l.visible = visible;
        }
    }

    fn toggle_visibility(&mut self) {
        let roots = self.selected_roots();
        let Some(first) = roots.first().and_then(|id| self.doc.find(*id)) else {
            return;
        };
        // like most apps: if the first is visible, hide all; else show all
        let visible = !first.visible;
        self.checkpoint(if visible { "Show layers" } else { "Hide layers" });
        for id in roots {
            if let Some(l) = self.doc.find_mut(id) {
                l.visible = visible;
            }
        }
    }

    // ------------------------------------------------------------------
    // Animation

    pub fn current_frame(&self) -> Frame {
        self.frame.floor() as Frame
    }

    pub fn go_to(&mut self, frame: Frame) {
        self.playing = false;
        self.frame = frame.clamp(self.doc.start, self.doc.end) as f32;
    }

    fn step(&mut self, by: Frame) {
        self.go_to(self.current_frame() + by);
    }

    /// Advance playback by `seconds`, looping over the frame range.
    pub fn tick(&mut self, seconds: f32) {
        if !self.playing {
            return;
        }
        let n = self.doc.frame_count().max(1) as f32;
        let t = self.frame - self.doc.start as f32 + seconds * self.doc.fps;
        self.frame = self.doc.start as f32 + t.rem_euclid(n);
    }

    /// The frames onion skins show: up to `before` frames before the current
    /// one and `after` after it, nearest first. With `keys`, only frames
    /// with a drawing or key on the selected layers (all layers when none is
    /// selected), so the previous and next drawings show whatever their
    /// timing.
    pub fn onion_frames(&self, before: usize, after: usize, keys: bool) -> (Vec<Frame>, Vec<Frame>) {
        let now = self.current_frame();
        let (start, end) = (self.doc.start, self.doc.end);
        if !keys {
            let past = (1..=before as Frame)
                .map(|k| now - k)
                .filter(|f| *f >= start)
                .collect();
            let future = (1..=after as Frame)
                .map(|k| now + k)
                .filter(|f| *f <= end)
                .collect();
            return (past, future);
        }
        let ids: Vec<LayerId> = if self.selection.is_empty() {
            self.rows(false).into_iter().map(|(id, _)| id).collect()
        } else {
            self.selection.clone()
        };
        let mut frames: Vec<Frame> = ids.iter().flat_map(|id| self.key_frames(*id)).collect();
        frames.sort_unstable();
        frames.dedup();
        // the drawing or pose held now starts at the last key at or before
        // the current frame; onion skins are the ones before and after it
        let held = frames.partition_point(|f| *f <= now);
        let past = frames[..held.saturating_sub(1)]
            .iter()
            .rev()
            .take(before)
            .copied()
            .collect();
        let future = frames[held..].iter().take(after).copied().collect();
        (past, future)
    }

    /// Frames with any keyframe on this layer (its transform and opacity,
    /// a bone layer's bones and a vector layer's point keys), sorted.
    pub fn key_frames(&self, id: LayerId) -> Vec<Frame> {
        self.doc.find(id).map(Layer::key_frames).unwrap_or_default()
    }

    fn jump_key(&mut self, forward: bool) {
        let ids: Vec<LayerId> = if self.selection.is_empty() {
            self.rows(false).into_iter().map(|(id, _)| id).collect()
        } else {
            self.selection.clone()
        };
        let now = self.current_frame();
        let frames = ids.iter().flat_map(|id| self.key_frames(*id));
        let target = if forward {
            frames.filter(|f| *f > now).min()
        } else {
            frames.filter(|f| *f < now).max()
        };
        match target {
            Some(f) => self.go_to(f),
            None => self.set_status(StatusKind::Info, "No more keyframes"),
        }
    }

    /// Key every transform channel and opacity of the selected layers at
    /// the current frame, holding their current values.
    fn add_key(&mut self) {
        let roots = self.selected_roots();
        if roots.is_empty() {
            return;
        }
        self.checkpoint("Add keyframe");
        let frame = self.current_frame();
        for id in roots {
            // on a frame-by-frame layer a key is a drawing: a copy of the
            // one showing, to change (Animate's F6)
            if let Some(LayerKind::Frames { cels }) = self.doc.find_mut(id).map(|l| &mut l.kind) {
                if !cels.iter().any(|c| c.frame == frame) {
                    let mut cel = fanimate_core::frames::cel_at(cels, frame as f32)
                        .map(|i| cels[i].clone())
                        .unwrap_or_default();
                    cel.frame = frame;
                    let at = cels.partition_point(|c| c.frame < frame);
                    cels.insert(at, cel);
                }
                continue;
            }
            if let Some(l) = self.doc.find_mut(id) {
                key_now(&mut l.transform.position, frame);
                key_now(&mut l.transform.rotation, frame);
                key_now(&mut l.transform.scale, frame);
                key_now(&mut l.opacity, frame);
                // a vector layer's drawings too, so they can change shape
                if let LayerKind::Vector { shapes } = &mut l.kind {
                    for shape in shapes {
                        let path = shape.path_at(frame as f32).into_owned();
                        shape.key_path(frame, path, Interpolation::Smooth);
                    }
                }
            }
        }
    }

    /// Remove keys on the current frame from the selected layers,
    /// including their bones.
    fn delete_key(&mut self) {
        let roots = self.selected_roots();
        let frame = self.current_frame();
        let any = roots.iter().any(|id| self.key_frames(*id).contains(&frame));
        if !any {
            self.set_status(StatusKind::Info, "No keyframe on this frame");
            return;
        }
        self.checkpoint("Delete keyframe");
        for id in roots {
            if let Some(l) = self.doc.find_mut(id) {
                l.key_lists_mut(|k| {
                    k.delete_key(frame);
                });
            }
        }
    }

    /// The interpolation of the keys on the current frame of the selected
    /// layers, if there are keys there and they all agree; `Some(None)`
    /// when they differ.
    pub fn key_interpolation(&self) -> Option<Option<Interpolation>> {
        let frame = self.current_frame();
        let mut found: Option<Option<Interpolation>> = None;
        for id in &self.selection {
            let Some(l) = self.doc.find(*id) else { continue };
            l.key_lists(|k| {
                if let Some(i) = k.interp_at(frame) {
                    found = Some(match found {
                        None => Some(i),
                        Some(Some(j)) if j == i => Some(i),
                        Some(_) => None,
                    });
                }
            });
        }
        found
    }

    /// Set how the selected layers' keys on the current frame move on to
    /// the next key.
    fn set_interpolation(&mut self, interp: Interpolation) {
        if self.key_interpolation().is_none() {
            self.set_status(StatusKind::Info, "No keyframe on this frame");
            return;
        }
        if self.key_interpolation() == Some(Some(interp)) {
            return;
        }
        self.checkpoint("Change interpolation");
        let frame = self.current_frame();
        for id in self.selection.clone() {
            if let Some(l) = self.doc.find_mut(id) {
                l.key_lists_mut(|k| {
                    k.set_interp(frame, interp);
                });
            }
        }
    }

    /// Start dragging the keys on frame `from` of the selected layers along
    /// the timeline. Returns false when none of them has a key there.
    pub fn begin_key_drag(&mut self, from: Frame) -> bool {
        let originals: Vec<Layer> = self
            .selection
            .iter()
            .filter_map(|id| self.doc.find(*id))
            .filter(|l| l.key_frames().contains(&from))
            .cloned()
            .collect();
        if originals.is_empty() {
            return false;
        }
        self.key_drag = Some(KeyDrag {
            from,
            to: from,
            originals,
            recorded: false,
        });
        true
    }

    /// Move the dragged keys to `to`. A key already on `to` is replaced, as
    /// when the drag ends there. The whole drag is one undo step.
    pub fn drag_keys_to(&mut self, to: Frame) {
        let to = to.max(0);
        let Some(drag) = &self.key_drag else { return };
        if drag.to == to {
            return;
        }
        if !drag.recorded {
            self.checkpoint("Move keyframe");
        }
        let Some(drag) = &mut self.key_drag else { return };
        drag.recorded = true;
        drag.to = to;
        let from = drag.from;
        for original in &drag.originals {
            if let Some(l) = self.doc.find_mut(original.id) {
                *l = original.clone();
                if to != from {
                    l.key_lists_mut(|k| {
                        k.move_key(from, to);
                    });
                }
            }
        }
        self.go_to(to);
    }

    pub fn end_key_drag(&mut self) {
        self.key_drag = None;
    }

    pub fn is_dragging_keys(&self) -> bool {
        self.key_drag.is_some()
    }

    /// A layer's transform at the current frame.
    pub fn layer_transform(&self, id: LayerId) -> Option<Transform> {
        self.doc.find(id).map(|l| l.transform.sample(self.frame))
    }

    /// Change a layer's transform at the current frame, with auto-keying:
    /// a channel that is already animated gets a keyframe here; a channel
    /// that is not animated has its value changed for the whole animation.
    /// Does not checkpoint; callers checkpoint once per gesture.
    pub fn set_layer_transform(&mut self, id: LayerId, t: Transform) {
        let frame = self.current_frame();
        let Some(l) = self.doc.find_mut(id) else { return };
        let old = l.transform.sample(frame as f32);
        if old.position != t.position {
            write_channel(&mut l.transform.position, frame, t.position);
        }
        if old.rotation != t.rotation {
            write_channel(&mut l.transform.rotation, frame, t.rotation);
        }
        if old.scale != t.scale {
            write_channel(&mut l.transform.scale, frame, t.scale);
        }
        self.dirty = true;
    }

    pub fn layer_opacity(&self, id: LayerId) -> Option<f32> {
        self.doc.find(id).map(|l| l.opacity.sample(self.frame))
    }

    /// Like [`set_layer_transform`](Self::set_layer_transform), for opacity.
    pub fn set_layer_opacity(&mut self, id: LayerId, opacity: f32) {
        let frame = self.current_frame();
        if let Some(l) = self.doc.find_mut(id) {
            write_channel(&mut l.opacity, frame, opacity.clamp(0.0, 1.0));
            self.dirty = true;
        }
    }
}

enum NewKind {
    Vector,
    Group,
    Frames,
    Switch,
    Bone,
}

/// A new layer with its own label colour, the next in turn, so it is saved
/// with the project and stays when layers are reordered.
pub(crate) fn labelled(mut layer: Layer) -> Layer {
    layer.label = Some(Label::for_id(layer.id));
    layer
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn write_channel<T: Animatable>(ch: &mut Channel<T>, frame: Frame, value: T) {
    if ch.is_animated() {
        ch.set_key(frame, value, Interpolation::Smooth);
    } else {
        ch.base = value;
    }
}

fn key_now<T: Animatable>(ch: &mut Channel<T>, frame: Frame) {
    let v = ch.sample(frame as f32);
    ch.set_key(frame, v, Interpolation::Smooth);
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::Vec2;

    /// Editor with layers (bottom to top) A, B, C, all selected none.
    fn three() -> (Editor, [LayerId; 3]) {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let ids = ["A", "B", "C"].map(|n| {
            let l = doc.new_layer(n, LayerKind::vector(vec![]));
            let id = l.id;
            doc.layers.push(l);
            id
        });
        (Editor::new(doc), ids)
    }

    fn names(ed: &Editor) -> Vec<String> {
        ed.doc.layers.iter().map(|l| l.name.clone()).collect()
    }

    #[test]
    fn new_layer_goes_above_selection_and_undoes() {
        let (mut ed, [a, ..]) = three();
        ed.select(a, SelectMode::Replace);
        ed.run(Action::NewVectorLayer);
        assert_eq!(names(&ed), ["A", "Layer 1", "B", "C"]);
        assert_eq!(ed.rename_request, ed.primary());
        assert!(ed.dirty);
        ed.run(Action::Undo);
        assert_eq!(names(&ed), ["A", "B", "C"]);
        assert_eq!(ed.selection, vec![a]);
        ed.run(Action::Redo);
        assert_eq!(names(&ed), ["A", "Layer 1", "B", "C"]);
    }

    #[test]
    fn copy_paste_through_text_gives_fresh_ids() {
        let (mut ed, [_, b, _]) = three();
        ed.select(b, SelectMode::Replace);
        let Outcome::CopyText(text) = ed.run(Action::Copy) else {
            panic!()
        };
        // paste into a different editor, as between two windows
        let (mut other, [.., c]) = three();
        other.select(c, SelectMode::Replace);
        assert!(other.paste(Some(&text)));
        assert_eq!(names(&other), ["A", "B", "C", "B"]);
        let mut ids = Vec::new();
        other.doc.walk(|l| ids.push(l.id));
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
        assert!(fanimate_fnm::validate(&other.doc).is_clean());
    }

    #[test]
    fn paste_ignores_foreign_text_and_falls_back_to_internal_clipboard() {
        let (mut ed, [a, ..]) = three();
        assert!(!ed.paste(Some("hello")));
        ed.select(a, SelectMode::Replace);
        ed.run(Action::Copy);
        assert!(ed.paste(Some("not layers")));
        assert_eq!(names(&ed), ["A", "A", "B", "C"]);
    }

    #[test]
    fn cut_removes_and_paste_restores() {
        let (mut ed, [a, b, _]) = three();
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        ed.run(Action::Cut);
        assert_eq!(names(&ed), ["C"]);
        ed.run(Action::Paste);
        assert_eq!(names(&ed), ["C", "A", "B"]);
    }

    #[test]
    fn delete_and_duplicate() {
        let (mut ed, [a, b, _]) = three();
        ed.select(b, SelectMode::Replace);
        ed.run(Action::Duplicate);
        assert_eq!(names(&ed), ["A", "B", "B copy", "C"]);
        ed.select(a, SelectMode::Replace);
        ed.run(Action::Delete);
        assert_eq!(names(&ed), ["B", "B copy", "C"]);
        assert!(ed.selection.is_empty());
    }

    #[test]
    fn move_up_and_down_stop_at_the_ends() {
        let (mut ed, [a, _, c]) = three();
        ed.select(a, SelectMode::Replace);
        ed.run(Action::MoveLayerUp);
        assert_eq!(names(&ed), ["B", "A", "C"]);
        ed.select(c, SelectMode::Replace);
        ed.run(Action::MoveLayerUp);
        assert_eq!(names(&ed), ["B", "A", "C"]);
        ed.run(Action::MoveLayerDown);
        assert_eq!(names(&ed), ["B", "C", "A"]);
    }

    #[test]
    fn group_then_ungroup_round_trips() {
        let (mut ed, [a, b, _]) = three();
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        ed.run(Action::GroupLayers);
        assert_eq!(names(&ed), ["Group 1", "C"]);
        let g = ed.primary().unwrap();
        let LayerKind::Group { children } = &ed.doc.find(g).unwrap().kind else {
            panic!()
        };
        assert_eq!(
            children.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
            ["A", "B"]
        );
        ed.run(Action::Ungroup);
        assert_eq!(names(&ed), ["A", "B", "C"]);
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
    }

    #[test]
    fn range_select_follows_display_order() {
        let (mut ed, [a, _, c]) = three();
        ed.select(c, SelectMode::Replace);
        ed.select(a, SelectMode::Range);
        assert_eq!(ed.selection.len(), 3);
        assert_eq!(ed.primary(), Some(a));
        ed.select(a, SelectMode::Toggle);
        assert_eq!(ed.selection.len(), 2);
    }

    #[test]
    fn display_rows_list_top_first_and_hide_collapsed() {
        let (mut ed, [a, b, _]) = three();
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        ed.run(Action::GroupLayers);
        let g = ed.primary().unwrap();
        let rows: Vec<_> = ed
            .display_rows()
            .iter()
            .map(|(id, d)| (ed.doc.find(*id).unwrap().name.clone(), *d))
            .collect();
        assert_eq!(
            rows,
            [
                ("C".into(), 0),
                ("Group 1".into(), 0),
                ("B".into(), 1),
                ("A".into(), 1)
            ]
        );
        ed.collapsed.insert(g);
        assert_eq!(ed.display_rows().len(), 2);
    }

    #[test]
    fn auto_key_only_keys_animated_channels() {
        let (mut ed, [a, ..]) = three();
        ed.go_to(10);
        let mut t = ed.layer_transform(a).unwrap();
        t.position = Vec2::new(5.0, 0.0);
        ed.set_layer_transform(a, t);
        // not animated: the base value changes, no key
        let l = ed.doc.find(a).unwrap();
        assert!(!l.transform.position.is_animated());
        assert_eq!(l.transform.position.base, Vec2::new(5.0, 0.0));

        ed.select(a, SelectMode::Replace);
        ed.run(Action::AddKey);
        ed.go_to(20);
        t.position = Vec2::new(50.0, 0.0);
        ed.set_layer_transform(a, t);
        assert_eq!(ed.key_frames(a), [10, 20]);
        ed.go_to(15);
        assert_eq!(ed.layer_transform(a).unwrap().position.x, 27.5);
    }

    #[test]
    fn key_navigation_and_deletion() {
        let (mut ed, [a, ..]) = three();
        ed.select(a, SelectMode::Replace);
        for f in [5, 12] {
            ed.go_to(f);
            ed.run(Action::AddKey);
        }
        ed.go_to(0);
        ed.run(Action::NextKey);
        assert_eq!(ed.current_frame(), 5);
        ed.run(Action::NextKey);
        assert_eq!(ed.current_frame(), 12);
        ed.run(Action::PrevKey);
        assert_eq!(ed.current_frame(), 5);
        ed.run(Action::DeleteKey);
        assert_eq!(ed.key_frames(a), [12]);
    }

    #[test]
    fn onion_frames_are_neighbours_or_neighbouring_keys() {
        let (mut ed, [a, ..]) = three();
        ed.select(a, SelectMode::Replace);
        for f in [2, 6, 10, 14] {
            ed.go_to(f);
            ed.run(Action::AddKey);
        }
        ed.go_to(8);
        assert_eq!(ed.onion_frames(2, 1, false), (vec![7, 6], vec![9]));
        // held: the key on 6; skins: the keys before it and after now
        assert_eq!(ed.onion_frames(2, 2, true), (vec![2], vec![10, 14]));
        ed.go_to(0);
        assert_eq!(ed.onion_frames(3, 1, false), (vec![], vec![1]));
    }

    #[test]
    fn dragging_keys_retimes_them_in_one_undo_step() {
        let (mut ed, [a, b, _]) = three();
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        for f in [5, 12] {
            ed.go_to(f);
            ed.run(Action::AddKey);
        }
        ed.select(a, SelectMode::Replace);
        assert!(!ed.begin_key_drag(7));
        assert!(ed.begin_key_drag(5));
        // across the key on 12 and on, then back: 12 survives
        for f in [6, 12, 15, 9] {
            ed.drag_keys_to(f);
        }
        ed.end_key_drag();
        assert_eq!(ed.key_frames(a), [9, 12]);
        assert_eq!(ed.key_frames(b), [5, 12]);
        assert_eq!(ed.current_frame(), 9);
        ed.run(Action::Undo);
        assert_eq!(ed.key_frames(a), [5, 12]);
        // dropping on another key replaces it
        assert!(ed.begin_key_drag(5));
        ed.drag_keys_to(12);
        ed.end_key_drag();
        assert_eq!(ed.key_frames(a), [12]);
        // a drag that goes nowhere records nothing
        let steps = ed.history.undo_label().map(str::to_owned);
        assert!(ed.begin_key_drag(12));
        ed.drag_keys_to(12);
        ed.end_key_drag();
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps);
    }

    #[test]
    fn interpolation_is_set_on_the_current_frames_keys() {
        let (mut ed, [a, ..]) = three();
        ed.select(a, SelectMode::Replace);
        assert!(!ed.can_run(Action::SetInterpolation(Interpolation::Step)));
        ed.go_to(4);
        ed.run(Action::AddKey);
        assert_eq!(ed.key_interpolation(), Some(Some(Interpolation::Smooth)));
        ed.run(Action::SetInterpolation(Interpolation::Step));
        assert_eq!(ed.key_interpolation(), Some(Some(Interpolation::Step)));
        let l = ed.doc.find(a).unwrap();
        assert_eq!(l.transform.position.keys[0].interp, Interpolation::Step);
        ed.run(Action::Undo);
        assert_eq!(ed.key_interpolation(), Some(Some(Interpolation::Smooth)));
    }

    #[test]
    fn playback_loops_and_frames_clamp() {
        let (mut ed, _) = three();
        ed.doc.end = 23;
        ed.playing = true;
        ed.tick(1.5); // 36 frames at 24 fps over a 24-frame range
        assert_eq!(ed.current_frame(), 12);
        ed.go_to(500);
        assert_eq!(ed.current_frame(), 23);
        assert!(!ed.playing);
    }

    #[test]
    fn moho_a_picks_tool_by_layer_type() {
        let (mut ed, [a, ..]) = three();
        ed.select(a, SelectMode::Replace);
        ed.run(Action::AddPointOrBone);
        assert_eq!(ed.tool, Tool::AddPoint);
        ed.run(Action::NewBoneLayer);
        ed.run(Action::AddPointOrBone);
        assert_eq!(ed.tool, Tool::AddBone);
    }

    #[test]
    fn drag_and_drop_moves_layers() {
        let (mut ed, [a, b, c]) = three();
        assert!(ed.move_relative(&[a], c, Placement::Above));
        assert_eq!(names(&ed), ["B", "C", "A"]);
        ed.run(Action::NewGroupLayer);
        let g = ed.primary().unwrap();
        assert!(ed.move_relative(&[b, c], g, Placement::Into));
        let LayerKind::Group { children } = &ed.doc.find(g).unwrap().kind else {
            panic!()
        };
        assert_eq!(
            children.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
            ["B", "C"]
        );
        // a group cannot go inside itself, and a vector layer holds nothing
        assert!(!ed.move_relative(&[g], b, Placement::Above));
        assert!(!ed.move_relative(&[b], a, Placement::Into));
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
    }

    #[test]
    fn world_transforms_compose_parents() {
        let (mut ed, [a, b, _]) = three();
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        ed.run(Action::GroupLayers);
        let g = ed.primary().unwrap();
        ed.doc.find_mut(g).unwrap().transform.position.base = Vec2::new(10.0, 0.0);
        ed.doc.find_mut(a).unwrap().transform.position.base = Vec2::new(0.0, 5.0);
        let p = ed.world(a).transform_point2(Vec2::ZERO);
        assert!(p.abs_diff_eq(Vec2::new(10.0, 5.0), 1e-5));
        assert!(
            ed.parent_world(a)
                .transform_point2(Vec2::ZERO)
                .abs_diff_eq(Vec2::new(10.0, 0.0), 1e-5)
        );
    }

    #[test]
    fn opening_a_sample_is_left_to_the_app_and_opens_a_tab() {
        let mut ed = Editor::untitled();
        let open = Action::OpenSample(crate::Sample::Hello);
        assert_eq!(ed.run(open), Outcome::App(open));
        // opening adds a tab, so no unsaved work is at risk; quitting and
        // updating close every document, so the app asks first
        assert!(!open.closes_document());
        assert!(!Action::New.closes_document() && !Action::Open.closes_document());
        assert!(Action::Quit.closes_document() && Action::InstallUpdate.closes_document());
        assert!(!Action::Save.closes_document());
        for tab in [
            Action::CloseDocument,
            Action::CloseOtherDocuments,
            Action::NextDocument,
            Action::PreviousDocument,
            Action::ShowHome,
            Action::OpenFolder,
        ] {
            assert_eq!(ed.run(tab), Outcome::App(tab));
        }
    }

    #[test]
    fn a_new_file_takes_its_size_and_frame_rate() {
        let ed = Editor::untitled_sized("Short", 1920.0, 1080.0, 30.0);
        assert_eq!((ed.doc.width, ed.doc.height, ed.doc.fps), (1920.0, 1080.0, 30.0));
        // five seconds, counted from frame 0
        assert_eq!((ed.doc.start, ed.doc.end), (0, 149));
        assert_eq!(ed.doc.name, "Short");
        assert_eq!(ed.doc.layers.len(), 1);
        assert!(ed.path.is_none() && !ed.dirty);
        // sizes the renderer cannot take are brought into range
        let huge = Editor::untitled_sized("x", 1.0e9, 0.0, 0.0);
        assert_eq!(
            (huge.doc.width, huge.doc.height, huge.doc.fps),
            (16384.0, 16.0, 1.0)
        );
        assert!(fanimate_fnm::validate(&huge.doc).is_clean());
        let hd = Editor::untitled_preset(crate::Preset::Hd1080);
        assert_eq!((hd.doc.width, hd.doc.height, hd.doc.fps), (1920.0, 1080.0, 24.0));
        // every preset is a size the app accepts, and has its own name
        for preset in crate::Preset::ALL {
            let ed = Editor::untitled_preset(preset);
            assert!(fanimate_fnm::validate(&ed.doc).is_clean(), "{preset:?}");
            assert!(preset.menu_label().ends_with(preset.name()), "{preset:?}");
        }
        // the plain new file is as it always was
        let plain = Editor::untitled();
        assert_eq!(
            (plain.doc.width, plain.doc.height, plain.doc.end),
            (1280.0, 720.0, 119)
        );
    }

    #[test]
    fn revision_counts_edits_undos_and_redos() {
        let (mut ed, _) = three();
        let r0 = ed.revision();
        ed.run(Action::NewVectorLayer);
        assert_eq!(ed.revision(), r0 + 1);
        ed.run(Action::Undo);
        ed.run(Action::Redo);
        assert_eq!(ed.revision(), r0 + 3);
        // nothing to undo past the start: no change
        ed.run(Action::Undo);
        ed.run(Action::Undo);
        let r = ed.revision();
        ed.run(Action::Undo);
        assert_eq!(ed.revision(), r);
    }

    #[test]
    fn recovered_work_is_unsaved_and_keeps_its_path() {
        let ed = Editor::recovered(samples::hello(), Some(PathBuf::from("scene.fnm")), None);
        assert!(ed.dirty);
        assert_eq!(ed.path.as_deref(), Some(Path::new("scene.fnm")));
    }

    #[test]
    fn undo_on_a_layer_leaves_the_others_alone() {
        let (mut ed, [a, b, _]) = three();
        ed.rename(a, "A1");
        ed.rename(b, "B1");
        ed.select(a, SelectMode::Replace);
        assert!(ed.can_run(Action::UndoLayer));
        ed.run(Action::UndoLayer);
        assert_eq!(names(&ed), ["A", "B1", "C"]);
        assert_eq!(ed.history.undo_label(), Some("Undo Rename on A1"));
        // and that is undoable like any step
        ed.run(Action::Undo);
        assert_eq!(names(&ed), ["A1", "B1", "C"]);
        let (_, [.., c]) = three();
        ed.select(c, SelectMode::Replace);
        assert!(!ed.can_run(Action::UndoLayer));
    }

    #[test]
    fn history_survives_recovery() {
        let (mut ed, [a, ..]) = three();
        ed.rename(a, "A1");
        ed.rename(a, "A2");
        let record = ed.history_record();
        let mut back = Editor::recovered(ed.doc.clone(), None, Some(record));
        back.run(Action::Undo);
        assert_eq!(names(&back), ["A1", "B", "C"]);
        back.run(Action::GoToHistory(0));
        assert_eq!(names(&back), ["A", "B", "C"]);
    }

    #[test]
    fn new_layers_get_labels_in_turn_and_labels_can_be_changed() {
        let (mut ed, [a, ..]) = three();
        ed.select(a, SelectMode::Replace);
        ed.run(Action::NewVectorLayer);
        let first = ed.primary().unwrap();
        ed.run(Action::NewVectorLayer);
        let second = ed.primary().unwrap();
        let label = |ed: &Editor, id| ed.doc.find(id).unwrap().label;
        assert!(label(&ed, first).is_some());
        assert_ne!(label(&ed, first), label(&ed, second));
        ed.select(first, SelectMode::Toggle);
        ed.run(Action::SetLabel(Label::Blue));
        assert_eq!(label(&ed, first), Some(Label::Blue));
        assert_eq!(label(&ed, second), Some(Label::Blue));
        ed.run(Action::Undo);
        assert_ne!(label(&ed, second), Some(Label::Blue));
    }

    #[test]
    fn can_run_reflects_state() {
        let (mut ed, [a, ..]) = three();
        assert!(!ed.can_run(Action::Undo));
        assert!(!ed.can_run(Action::Copy));
        assert!(!ed.can_run(Action::Paste));
        ed.select(a, SelectMode::Replace);
        ed.run(Action::Copy);
        assert!(ed.can_run(Action::Paste));
        assert!(!ed.can_run(Action::Ungroup));
    }

    #[test]
    fn importing_a_moho_project_opens_it_unsaved_with_a_report() {
        let dir = std::env::temp_dir().join(format!("fanimate-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Walk.mohoproj");
        let json = r#"{"project_data": {"width": 320, "height": 240, "fps": 30.0, "start_frame": 1, "end_frame": 10},
            "layers": [{"type": "GroupLayer", "name": "Body", "layers": []}, {"type": "TextLayer", "name": "Title"}]}"#;
        std::fs::write(&file, json).unwrap();
        let ed = Editor::import_project(&file).unwrap();
        assert!(ed.dirty && ed.path.is_none());
        assert_eq!(
            (ed.doc.name.as_str(), ed.doc.width, ed.doc.fps),
            ("Walk", 320.0, 30.0)
        );
        assert!(ed.doc.layers[0].label.is_some());
        let (kind, msg) = ed.status.clone().unwrap();
        assert_eq!(kind, StatusKind::Warning);
        assert!(msg.contains("1 text layer left out"), "{msg}");
        // a damaged file is an error, not a panic
        std::fs::write(&file, "{").unwrap();
        assert!(Editor::import_project(&file).is_err());
        assert!(Editor::import_project(&dir.join("missing.fla")).is_err());
        std::fs::remove_dir_all(&dir).ok();
        assert!(!Action::OpenOther.closes_document());
    }

    #[test]
    fn palette_search_ranks_and_remembers() {
        use crate::action::search;
        let all = Action::all();
        let first = |q: &str, recent: &[Action]| search(&all, q, recent).first().copied();
        // a label that starts with the query wins
        assert_eq!(first("undo", &[]), Some(Action::Undo));
        // word starts, in any order
        assert_eq!(first("layer vector", &[]), Some(Action::NewVectorLayer));
        // letters in order
        assert!(search(&all, "nvl", &[]).contains(&Action::NewVectorLayer));
        assert!(search(&all, "zzzz", &[]).is_empty());
        // recently used leads among equals, and fills an empty query
        let recent = [Action::Redo];
        assert_eq!(first("", &recent), Some(Action::Redo));
        assert_eq!(search(&all, "", &[]).len(), all.len());
        let workspaces = search(&all, "workspace", &[]);
        assert_eq!(workspaces.len(), crate::Workspace::ALL.len());
    }
}
