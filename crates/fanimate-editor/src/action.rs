//! Every command the user can run, and the tools.

/// A command. Menus, shortcuts, context menus and the command palette all
/// run the same actions, so behaviour never depends on how it was invoked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    // File
    New,
    /// A new file of a common size and frame rate.
    NewPreset(Preset),
    Open,
    /// Open a Moho or Adobe Animate project as a new Fanimate project.
    OpenOther,
    OpenSample(Sample),
    /// Open a project saved as a folder (older projects, and ones kept in Git).
    OpenFolder,
    /// Close the open document's tab.
    CloseDocument,
    /// Close every tab but this one (tabs with unsaved changes stay).
    CloseOtherDocuments,
    /// The next and the previous document tab.
    NextDocument,
    PreviousDocument,
    /// The Home tab: new files, recent files and templates.
    ShowHome,
    Save,
    SaveAs,
    /// Save the project as a folder of plain files (one per layer), which
    /// Git can diff, instead of one file.
    SaveAsFolder,
    /// Import pictures, Photoshop or SVG files, or sound.
    Import,
    /// Render the animation to a video, GIF or PNG sequence.
    ExportVideo,
    /// Show the render queue window.
    RenderQueue,
    /// Render the current frame as the export would make it, and show it.
    RenderPreviewFrame,
    /// Look for a newer version of Fanimate now.
    CheckForUpdates,
    /// Install a downloaded update and restart.
    InstallUpdate,
    /// Arrange the panels for a kind of work.
    Workspace(Workspace),
    Quit,
    // Edit
    Undo,
    Redo,
    /// Take the selected layer back to before its own last change.
    UndoLayer,
    /// Go to a state in the History panel; 0 is the oldest kept.
    GoToHistory(usize),
    Cut,
    Copy,
    Paste,
    /// Cut, copy and paste the selected layers, whatever has focus (plain
    /// Cut, Copy and Paste follow the focus: canvas points or timeline keys).
    CutLayers,
    CopyLayers,
    PasteLayers,
    Duplicate,
    /// Delete what the focus is on: the picked keys, points, shapes or
    /// bone. Never a layer: Backspace and Delete are too easy to press by
    /// accident for that.
    Delete,
    /// Delete the selected layers (Ctrl+Delete).
    DeleteLayers,
    SelectAll,
    SelectNone,
    Rename,
    /// Finish the path being drawn with the Pen tool.
    FinishPath,
    /// Boolean shape operations on the shapes with selected points.
    Union,
    Subtract,
    Intersect,
    /// Turn the selected rectangles into plain paths, to edit point by point.
    ExpandShape,
    Preferences,
    CommandPalette,
    // Layer
    NewVectorLayer,
    NewGroupLayer,
    NewBoneLayer,
    NewFramesLayer,
    NewSwitchLayer,
    GroupLayers,
    Ungroup,
    MoveLayerUp,
    MoveLayerDown,
    ToggleVisibility,
    LayerProperties,
    /// Give the selected layers this label colour.
    SetLabel(fanimate_core::Label),
    /// Bind shapes rigidly to the selected bone.
    BindToBone,
    /// Delete the selected bone: its children join its parent.
    DeleteBone,
    /// Bind shapes to the bones around them, with automatic weights.
    BindSmoothly,
    /// Stop shapes following bones.
    Unbind,
    /// A new action on the bone layer, opened for editing.
    NewAction,
    /// A new Smart Bone action for the selected bone, opened for editing.
    NewSmartBoneAction,
    /// Edit action n of the bone layer.
    EditAction(usize),
    /// Stop editing the action and go back to the timeline.
    FinishAction,
    /// Copy action n into the timeline at the current frame.
    InsertAction(usize),
    DeleteAction(usize),
    // Animation
    PlayPause,
    NextFrame,
    PrevFrame,
    FirstFrame,
    LastFrame,
    /// Frame 0, the rest pose: how things are drawn and set up, before the
    /// animation starts at frame 1.
    GoToRestPose,
    /// What the properties of the picked keys do after their last key.
    SetLoop(fanimate_core::LoopMode),
    NextKey,
    PrevKey,
    AddKey,
    DeleteKey,
    /// A blank drawing on the current frame of a frame-by-frame layer.
    NewDrawing,
    /// Show child n of the selected switch layer from the current frame.
    SetSwitch(usize),
    /// Key the selected switch layer's mouths from a sound, with Rhubarb.
    LipSync,
    /// The sub-layer the tools draw on in frame-by-frame layers.
    SetPart(fanimate_core::Part),
    /// How the keys on the current frame move on to the next key.
    SetInterpolation(fanimate_core::Interpolation),
    // View
    /// Show the drawings before and after this frame faintly.
    ToggleOnionSkin,
    /// Fade every layer but the selected ones.
    ToggleLightTable,
    ZoomIn,
    ZoomOut,
    FitView,
    ActualSize,
    /// Key one property of the selected layer on the current frame, holding
    /// its value there (the key buttons in Properties and the timeline's
    /// property rows).
    AddChannelKey(fanimate_core::ChannelId),
    /// Remove one property's key on the current frame.
    DeleteChannelKey(fanimate_core::ChannelId),
    /// How one property's key on the current frame moves on to the next.
    SetChannelInterpolation(fanimate_core::ChannelId, fanimate_core::Interpolation),
    /// How keys made from now on move on to the next key (the timeline's
    /// dropdown).
    SetKeyInterpolation(fanimate_core::Interpolation),
    /// Lock the selected layers, or unlock them when they all are locked.
    ToggleLock,
    /// Line the selected layers up with the canvas (one layer) or each other.
    Align(crate::editor::AlignTo),
    /// Show or hide one shape of a layer (the eye in the Properties panel's
    /// shape list).
    SetShapeVisible(fanimate_core::LayerId, usize, bool),
    /// Delete or copy the selected shapes.
    DeleteShapes,
    DuplicateShapes,
    /// Rename a shape in place, in the Properties panel's shape list.
    RenameShape(fanimate_core::LayerId, usize),
    /// Put the camera back over the whole canvas and remove its keys.
    ResetCamera,
    /// Show the canvas as the camera sees it (editing tools are off while it
    /// is on).
    LookThroughCamera,
    /// Turn the selected group, bone or switch layer into another of those.
    /// When that throws something away the app asks first.
    ConvertLayerType(crate::editor::ConvertTo),
    /// The same, once the question has been answered.
    ConfirmConvertLayerType(crate::editor::ConvertTo),
    // Tools
    Tool(Tool),
    /// Moho's `A`: Add Point on a vector layer, Add Bone on a bone layer.
    AddPointOrBone,
    /// Arm the pivot picker: the next click on the canvas places the pivot
    /// of the selected layer, or of the chosen points.
    PickPivot,
}

impl Action {
    /// Whether the command changes the document or its history. These do
    /// nothing while the animation plays: an edit made at a moving playhead
    /// would key a different frame each time, and sprays keyframes.
    pub fn edits_document(self) -> bool {
        use Action::*;
        matches!(
            self,
            Undo | Redo
                | UndoLayer
                | GoToHistory(_)
                | Cut
                | Paste
                | CutLayers
                | PasteLayers
                | Duplicate
                | Delete
                | DeleteLayers
                | Rename
                | FinishPath
                | Union
                | Subtract
                | Intersect
                | ExpandShape
                | NewVectorLayer
                | NewGroupLayer
                | NewBoneLayer
                | NewFramesLayer
                | NewSwitchLayer
                | GroupLayers
                | Ungroup
                | MoveLayerUp
                | MoveLayerDown
                | ToggleVisibility
                | SetLabel(_)
                | BindToBone
                | DeleteBone
                | BindSmoothly
                | Unbind
                | NewAction
                | NewSmartBoneAction
                | EditAction(_)
                | FinishAction
                | InsertAction(_)
                | DeleteAction(_)
                | SetLoop(_)
                | AddKey
                | DeleteKey
                | NewDrawing
                | SetSwitch(_)
                | LipSync
                | SetInterpolation(_)
                | AddChannelKey(_)
                | DeleteChannelKey(_)
                | SetChannelInterpolation(..)
                | ToggleLock
                | Align(_)
                | SetShapeVisible(..)
                | DeleteShapes
                | DuplicateShapes
                | RenameShape(..)
                | ResetCamera
                | ConvertLayerType(_)
                | ConfirmConvertLayerType(_)
                | PickPivot
        )
    }

    pub fn label(self) -> &'static str {
        use Action::*;
        match self {
            New => "New",
            NewPreset(p) => p.menu_label(),
            Open => "Open…",
            OpenOther => "Open Moho or Animate Project…",
            OpenSample(Sample::Hello) => "Open Sample: Hello Fanimate",
            OpenSample(Sample::Benchmark) => "Open Sample: Benchmark Rig",
            OpenSample(Sample::StressTest) => "Open Sample: Stress Test",
            OpenSample(Sample::Blank) => "Open Template: Blank 1080p 24 fps",
            OpenSample(Sample::WalkCycle) => "Open Template: Walk Cycle Rig",
            OpenSample(Sample::LipSync) => "Open Template: Lip Sync",
            OpenSample(Sample::FrameByFrame) => "Open Template: Frame by Frame",
            OpenFolder => "Open Project Folder…",
            CloseDocument => "Close",
            CloseOtherDocuments => "Close Other Tabs",
            NextDocument => "Next Tab",
            PreviousDocument => "Previous Tab",
            ShowHome => "Home",
            Save => "Save",
            SaveAs => "Save As…",
            SaveAsFolder => "Save As Folder…",
            Import => "Import…",
            ExportVideo => "Export Video…",
            RenderQueue => "Render Queue",
            RenderPreviewFrame => "Render Preview Frame",
            CheckForUpdates => "Check for Updates",
            InstallUpdate => "Install Update and Restart",
            Workspace(w) => w.label(),
            Quit => "Quit",
            Undo => "Undo",
            Redo => "Redo",
            UndoLayer => "Undo on Selected Layer",
            GoToHistory(_) => "Go to This Step",
            Cut => "Cut",
            Copy => "Copy",
            Paste => "Paste",
            CutLayers => "Cut Layers",
            CopyLayers => "Copy Layers",
            PasteLayers => "Paste Layers",
            Duplicate => "Duplicate",
            Delete => "Delete",
            DeleteLayers => "Delete Layer",
            SelectAll => "Select All Layers",
            SelectNone => "Select None",
            Rename => "Rename Layer",
            FinishPath => "Finish Path",
            Union => "Union Shapes",
            ExpandShape => "Expand Shape",
            Subtract => "Subtract Front Shape",
            Intersect => "Intersect Shapes",
            Preferences => "Preferences…",
            CommandPalette => "Command Palette…",
            NewVectorLayer => "New Vector Layer",
            NewGroupLayer => "New Group",
            NewBoneLayer => "New Bone Layer",
            NewFramesLayer => "New Frame-by-Frame Layer",
            NewSwitchLayer => "New Switch Layer",
            SetSwitch(_) => "Show This Layer",
            LipSync => "Lip Sync from Sound…",
            NewDrawing => "New Blank Drawing",
            SetPart(p) => match p {
                fanimate_core::Part::Rough => "Draw on Rough",
                fanimate_core::Part::Colour => "Draw on Colour",
                fanimate_core::Part::Line => "Draw on Line",
                fanimate_core::Part::Overlay => "Draw on Overlay",
            },
            GroupLayers => "Group Layers",
            Ungroup => "Ungroup",
            MoveLayerUp => "Move Layer Up",
            MoveLayerDown => "Move Layer Down",
            ToggleVisibility => "Show/Hide Layer",
            LayerProperties => "Layer Properties",
            SetLabel(l) => l.name(),
            BindToBone => "Bind to Selected Bone",
            DeleteBone => "Delete Bone",
            BindSmoothly => "Bind Smoothly",
            Unbind => "Release from Bones",
            NewAction => "New Action",
            NewSmartBoneAction => "New Smart Bone Action",
            EditAction(_) => "Edit Action",
            FinishAction => "Finish Editing Action",
            InsertAction(_) => "Insert Action Here",
            DeleteAction(_) => "Delete Action",
            PlayPause => "Play/Pause",
            NextFrame => "Next Frame",
            PrevFrame => "Previous Frame",
            FirstFrame => "First Frame",
            LastFrame => "Last Frame",
            GoToRestPose => "Go to Rest Pose",
            SetLoop(m) => match m {
                fanimate_core::LoopMode::Off => "Loop: Off",
                fanimate_core::LoopMode::Absolute => "Loop: Repeat",
                fanimate_core::LoopMode::Relative => "Loop: Repeat and Offset",
            },
            NextKey => "Next Keyframe",
            PrevKey => "Previous Keyframe",
            AddKey => "Add Keyframe",
            DeleteKey => "Delete Keyframe",
            SetInterpolation(i) => match i {
                fanimate_core::Interpolation::Smooth => "Interpolation: Smooth",
                fanimate_core::Interpolation::Linear => "Interpolation: Linear",
                fanimate_core::Interpolation::Step => "Interpolation: Step",
            },
            ToggleLock => "Lock/Unlock Layer",
            ResetCamera => "Reset Camera",
            LookThroughCamera => "Look Through Camera",
            Align(to) => to.label(),
            SetShapeVisible(_, _, true) => "Show Shape",
            SetShapeVisible(_, _, false) => "Hide Shape",
            DeleteShapes => "Delete Shape",
            DuplicateShapes => "Duplicate Shape",
            RenameShape(..) => "Rename Shape",
            ConvertLayerType(to) | ConfirmConvertLayerType(to) => match to {
                crate::editor::ConvertTo::Group => "Change to Group",
                crate::editor::ConvertTo::Bone => "Change to Bone Layer",
                crate::editor::ConvertTo::Switch => "Change to Switch Layer",
            },
            AddChannelKey(fanimate_core::ChannelId::FillColor(_)) => "Key Fill Colour Here",
            AddChannelKey(fanimate_core::ChannelId::StrokeColor(_)) => "Key Stroke Colour Here",
            AddChannelKey(fanimate_core::ChannelId::Trim(_)) => "Key Stroke Exposure Here",
            AddChannelKey(_) => "Add Keyframe on This Property",
            DeleteChannelKey(_) => "Delete Keyframe on This Property",
            SetChannelInterpolation(_, i) => match i {
                fanimate_core::Interpolation::Smooth => "Property Key: Smooth",
                fanimate_core::Interpolation::Linear => "Property Key: Linear",
                fanimate_core::Interpolation::Step => "Property Key: Step",
            },
            SetKeyInterpolation(i) => match i {
                fanimate_core::Interpolation::Smooth => "New Keys: Smooth",
                fanimate_core::Interpolation::Linear => "New Keys: Linear",
                fanimate_core::Interpolation::Step => "New Keys: Step",
            },
            ToggleOnionSkin => "Onion Skin",
            ToggleLightTable => "Light Table",
            ZoomIn => "Zoom In",
            ZoomOut => "Zoom Out",
            FitView => "Fit Canvas",
            ActualSize => "Actual Size (100%)",
            Tool(t) => t.label(),
            AddPointOrBone => "Add Point / Add Bone",
            PickPivot => "Set pivot by clicking",
        }
    }

    /// The menu an action belongs to, to group the shortcut list. Every
    /// action is named, so a new one has to choose.
    pub fn category(self) -> &'static str {
        use Action::*;
        match self {
            New | NewPreset(_) | Open | OpenOther | OpenSample(_) | OpenFolder | CloseDocument
            | CloseOtherDocuments | NextDocument | PreviousDocument | ShowHome | Save | SaveAs
            | SaveAsFolder | Import | ExportVideo | RenderQueue | RenderPreviewFrame | CheckForUpdates
            | InstallUpdate | Quit => "File",
            Undo | Redo | UndoLayer | GoToHistory(_) | Cut | Copy | Paste | CutLayers | CopyLayers
            | PasteLayers | Duplicate | Delete | DeleteLayers | SelectAll | SelectNone | Rename
            | FinishPath | Union | Subtract | Intersect | ExpandShape | Preferences | CommandPalette => {
                "Edit"
            }
            NewVectorLayer
            | NewGroupLayer
            | NewBoneLayer
            | NewFramesLayer
            | NewSwitchLayer
            | GroupLayers
            | Ungroup
            | MoveLayerUp
            | MoveLayerDown
            | ToggleVisibility
            | ToggleLock
            | Align(_)
            | LayerProperties
            | PickPivot
            | SetLabel(_)
            | SetShapeVisible(..)
            | DeleteShapes
            | DuplicateShapes
            | RenameShape(..)
            | ConvertLayerType(_)
            | ConfirmConvertLayerType(_)
            | BindToBone
            | DeleteBone
            | BindSmoothly
            | Unbind
            | NewAction
            | NewSmartBoneAction
            | EditAction(_)
            | FinishAction
            | InsertAction(_)
            | DeleteAction(_) => "Layer",
            PlayPause
            | NextFrame
            | PrevFrame
            | FirstFrame
            | LastFrame
            | GoToRestPose
            | SetLoop(_)
            | NextKey
            | PrevKey
            | AddKey
            | DeleteKey
            | NewDrawing
            | SetSwitch(_)
            | LipSync
            | SetPart(_)
            | SetInterpolation(_)
            | AddChannelKey(_)
            | DeleteChannelKey(_)
            | SetChannelInterpolation(..)
            | ResetCamera
            | SetKeyInterpolation(_) => "Animation",
            ToggleOnionSkin | ToggleLightTable | LookThroughCamera | ZoomIn | ZoomOut | FitView
            | ActualSize | Workspace(_) => "View",
            Tool(_) | AddPointOrBone => "Tools",
        }
    }

    /// Every action, for the command palette.
    pub fn all() -> Vec<Action> {
        use Action::*;
        let mut v = vec![
            New,
            Open,
            OpenFolder,
            OpenOther,
            CloseDocument,
            CloseOtherDocuments,
            NextDocument,
            PreviousDocument,
            ShowHome,
            Save,
            SaveAs,
            SaveAsFolder,
            Import,
            ExportVideo,
            RenderQueue,
            RenderPreviewFrame,
            CheckForUpdates,
            InstallUpdate,
            Quit,
            Undo,
            Redo,
            UndoLayer,
            Cut,
            Copy,
            Paste,
            CutLayers,
            CopyLayers,
            PasteLayers,
            Duplicate,
            Delete,
            DeleteLayers,
            SelectAll,
            SelectNone,
            Rename,
            FinishPath,
            Union,
            Subtract,
            Intersect,
            ExpandShape,
            Preferences,
            CommandPalette,
            NewVectorLayer,
            NewGroupLayer,
            NewBoneLayer,
            NewFramesLayer,
            NewSwitchLayer,
            GroupLayers,
            Ungroup,
            MoveLayerUp,
            MoveLayerDown,
            ToggleVisibility,
            LayerProperties,
            PickPivot,
            BindToBone,
            DeleteBone,
            BindSmoothly,
            Unbind,
            NewAction,
            NewSmartBoneAction,
            FinishAction,
            PlayPause,
            NextFrame,
            PrevFrame,
            FirstFrame,
            LastFrame,
            GoToRestPose,
            NextKey,
            PrevKey,
            AddKey,
            DeleteKey,
            NewDrawing,
            LipSync,
            ToggleOnionSkin,
            ToggleLightTable,
            LookThroughCamera,
            ZoomIn,
            ZoomOut,
            FitView,
            ActualSize,
        ];
        v.extend(fanimate_core::Interpolation::ALL.map(SetInterpolation));
        v.extend(fanimate_core::Interpolation::ALL.map(SetKeyInterpolation));
        v.extend(fanimate_core::LoopMode::ALL.map(SetLoop));
        v.extend(fanimate_core::Part::ALL.map(SetPart));
        v.extend(Sample::ALL.iter().map(|s| OpenSample(*s)));
        v.extend(Preset::ALL.iter().map(|p| NewPreset(*p)));
        v.extend(self::Workspace::ALL.map(Action::Workspace));
        v.extend(crate::action::Tool::ALL.iter().map(|t| Tool(*t)));
        v
    }

    /// Whether the action closes every open document, so unsaved changes
    /// must be saved or discarded first. Opening and creating documents do
    /// not: they open a new tab.
    pub fn closes_document(self) -> bool {
        matches!(self, Self::InstallUpdate | Self::Quit)
    }
}

/// Panel arrangements for kinds of work, in Window › Workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Workspace {
    /// Everything: tools, canvas, timeline, properties and layers.
    Animate,
    /// A large canvas with the Style panel at hand and a short timeline.
    Draw,
    /// Properties and Actions forward, for bones and Smart Bones.
    Rig,
    /// The X-sheet beside a tall timeline, for drawn animation.
    FrameByFrame,
    /// The canvas and the timeline only, for watching and checking.
    Review,
}

impl Workspace {
    pub const ALL: [Workspace; 5] = [
        Self::Animate,
        Self::Draw,
        Self::Rig,
        Self::FrameByFrame,
        Self::Review,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Animate => "Workspace: Animate",
            Self::Draw => "Workspace: Draw",
            Self::Rig => "Workspace: Rig",
            Self::FrameByFrame => "Workspace: Frame by Frame",
            Self::Review => "Workspace: Review",
        }
    }

    /// The name in the Window › Workspace menu.
    pub fn name(self) -> &'static str {
        self.label().trim_start_matches("Workspace: ")
    }
}

/// Command palette search: the actions whose labels match `query`, best
/// first. A label that starts with the query comes first, then labels
/// with a word starting with each query word, then ones containing every
/// word, then ones holding the query's letters in order ("nvl" finds New
/// Vector Layer). Within each, recently used actions (`recent`, most recent
/// first) lead. An empty query lists the recent actions, then the rest.
pub fn search(all: &[Action], query: &str, recent: &[Action]) -> Vec<Action> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let rank = |a: &Action| -> Option<u8> {
        if words.is_empty() {
            return Some(0);
        }
        let label = a.label().to_lowercase();
        let label_words: Vec<&str> = label
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        if label.starts_with(&query) {
            Some(0)
        } else if words.iter().all(|q| label_words.iter().any(|w| w.starts_with(q))) {
            Some(1)
        } else if words.iter().all(|q| label.contains(q)) {
            Some(2)
        } else {
            let mut letters = label.chars();
            let compact: String = query.chars().filter(|c| !c.is_whitespace()).collect();
            compact.chars().all(|q| letters.any(|c| c == q)).then_some(3)
        }
    };
    let recency = |a: &Action| recent.iter().position(|r| r == a).unwrap_or(usize::MAX);
    let mut found: Vec<(u8, usize, usize, Action)> = all
        .iter()
        .enumerate()
        .filter_map(|(i, a)| rank(a).map(|r| (r, recency(a), i, *a)))
        .collect();
    found.sort_by_key(|f| (f.0, f.1, f.2));
    found.into_iter().map(|(.., a)| a).collect()
}

/// The sample projects and templates in File › Open Sample and on the Home
/// tab. Each opens as a new, unsaved file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sample {
    Hello,
    Benchmark,
    StressTest,
    /// An empty 1080p file at 24 fps.
    Blank,
    /// A stick figure on bones with a walk cycle.
    WalkCycle,
    /// A face and a switch layer of mouth shapes, ready for Lip Sync.
    LipSync,
    /// A frame-by-frame layer with twelve empty drawings.
    FrameByFrame,
}

impl Sample {
    pub const ALL: [Sample; 7] = [
        Self::Blank,
        Self::WalkCycle,
        Self::LipSync,
        Self::FrameByFrame,
        Self::Hello,
        Self::Benchmark,
        Self::StressTest,
    ];

    /// The name in the Open Sample menu.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hello => "Hello Fanimate",
            Self::Benchmark => "Benchmark rig (40 bones)",
            Self::StressTest => "Stress test (400 bones)",
            Self::Blank => "Blank 1080p 24 fps",
            Self::WalkCycle => "Walk cycle rig",
            Self::LipSync => "Lip sync",
            Self::FrameByFrame => "Frame by frame",
        }
    }

    /// One line about what it holds, for the Home tab.
    pub fn detail(self) -> &'static str {
        match self {
            Self::Hello => "A bone-rigged letter",
            Self::Benchmark => "40 bones, the speed test",
            Self::StressTest => "400 bones, for stress",
            Self::Blank => "1920 × 1080 px, an empty layer",
            Self::WalkCycle => "A stick figure with a walk",
            Self::LipSync => "A face and nine mouth shapes",
            Self::FrameByFrame => "Twelve empty drawings",
        }
    }

    pub fn document(self) -> fanimate_core::Document {
        use fanimate_core::samples;
        match self {
            Self::Hello => samples::hello(),
            Self::Benchmark => samples::benchmark(40),
            Self::StressTest => samples::benchmark(400),
            Self::Blank => samples::blank(),
            Self::WalkCycle => samples::walk_cycle(),
            Self::LipSync => samples::lip_sync(),
            Self::FrameByFrame => samples::frame_by_frame(),
        }
    }
}

/// A common size and frame rate for a new file, shown on the Home tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    Hd1080,
    Hd720,
    Uhd4k,
    Square,
    Vertical,
    Social,
}

impl Preset {
    pub const ALL: [Preset; 6] = [
        Self::Hd1080,
        Self::Hd720,
        Self::Uhd4k,
        Self::Square,
        Self::Vertical,
        Self::Social,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Hd1080 => "HD 1080p",
            Self::Hd720 => "HD 720p",
            Self::Uhd4k => "4K UHD",
            Self::Square => "Square",
            Self::Vertical => "Vertical",
            Self::Social => "Social 4:5",
        }
    }

    /// The menu and command palette wording.
    pub fn menu_label(self) -> &'static str {
        match self {
            Self::Hd1080 => "New: HD 1080p",
            Self::Hd720 => "New: HD 720p",
            Self::Uhd4k => "New: 4K UHD",
            Self::Square => "New: Square",
            Self::Vertical => "New: Vertical",
            Self::Social => "New: Social 4:5",
        }
    }

    /// Width and height in pixels.
    pub fn size(self) -> (f32, f32) {
        match self {
            Self::Hd1080 => (1920.0, 1080.0),
            Self::Hd720 => (1280.0, 720.0),
            Self::Uhd4k => (3840.0, 2160.0),
            Self::Square => (1080.0, 1080.0),
            Self::Vertical => (1080.0, 1920.0),
            Self::Social => (1080.0, 1350.0),
        }
    }

    pub fn fps(self) -> f32 {
        24.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolGroup {
    Draw,
    Fill,
    Layer,
    Camera,
    Bone,
    Workspace,
}

impl ToolGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draw => "Draw",
            Self::Fill => "Fill",
            Self::Layer => "Layer",
            Self::Camera => "Camera",
            Self::Bone => "Bone",
            Self::Workspace => "Workspace",
        }
    }

    pub const ALL: [ToolGroup; 6] = [
        Self::Draw,
        Self::Fill,
        Self::Layer,
        Self::Camera,
        Self::Bone,
        Self::Workspace,
    ];

    /// What kind of layer the group's tools need, for the status message when
    /// a layer change takes the active tool away. `None` for the groups that
    /// work on any layer (or none): drawing with another kind of layer
    /// selected, or none, makes a new vector layer instead.
    pub fn needs(self) -> Option<&'static str> {
        match self {
            Self::Bone => Some("a bone layer"),
            Self::Draw | Self::Fill | Self::Layer | Self::Camera | Self::Workspace => None,
        }
    }
}

/// The tool groups the Tools panel shows for the active layer (`None` when
/// nothing is selected). Draw, Fill, Layer, Camera and Workspace are always
/// there, as in Moho: drawing with no vector layer selected makes a new one.
/// Only Bone needs a bone layer.
pub fn tool_groups_for(kind: Option<&fanimate_core::LayerKind>) -> Vec<ToolGroup> {
    tool_groups_in(kind, false)
}

/// As [`tool_groups_for`], for a layer that may sit inside a bone layer
/// (`in_rig`): the bone tools work on it too, as Paint Weights brushes a
/// vector layer inside the rig and Manipulate Bones poses its bones.
pub fn tool_groups_in(kind: Option<&fanimate_core::LayerKind>, in_rig: bool) -> Vec<ToolGroup> {
    use fanimate_core::LayerKind;
    ToolGroup::ALL
        .into_iter()
        .filter(|group| match group {
            ToolGroup::Bone => in_rig || matches!(kind, Some(LayerKind::Bone { .. })),
            ToolGroup::Draw
            | ToolGroup::Fill
            | ToolGroup::Layer
            | ToolGroup::Camera
            | ToolGroup::Workspace => true,
        })
        .collect()
}

/// Canvas tools, grouped as in Moho Pro's Tools panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    SelectPoints,
    TranslatePoints,
    AddPoint,
    Pen,
    /// Click adds smooth points along a curve; Alt+click adds a corner.
    Curvature,
    Freehand,
    Rectangle,
    Ellipse,
    Line,
    Polygon,
    Star,
    Triangle,
    Eraser,
    PaintBucket,
    Eyedropper,
    DeleteEdge,
    /// Click a segment to leave its stroke undrawn (Alt+click shows it).
    HideEdge,
    Magnet,
    DrawShape,
    LineWidth,
    StrokeExposure,
    ShapeBuilder,
    #[default]
    TransformLayer,
    /// Moves the document's camera (the picture follows the pointer).
    TrackCamera,
    /// Zooms the document's camera.
    ZoomCamera,
    /// Rolls the document's camera about its centre.
    RollCamera,
    SelectBone,
    AddBone,
    /// Drag from a bone to the bone that becomes its parent.
    ReparentBone,
    ManipulateBones,
    PaintWeights,
    Pan,
    Zoom,
}

impl Tool {
    pub const ALL: [Tool; 33] = [
        Self::SelectPoints,
        Self::TranslatePoints,
        Self::AddPoint,
        Self::Pen,
        Self::Curvature,
        Self::Freehand,
        Self::Rectangle,
        Self::Ellipse,
        Self::Line,
        Self::Polygon,
        Self::Star,
        Self::Triangle,
        Self::Eraser,
        Self::DeleteEdge,
        Self::HideEdge,
        Self::Magnet,
        Self::PaintBucket,
        Self::Eyedropper,
        Self::DrawShape,
        Self::LineWidth,
        Self::StrokeExposure,
        Self::ShapeBuilder,
        Self::TransformLayer,
        Self::TrackCamera,
        Self::ZoomCamera,
        Self::RollCamera,
        Self::SelectBone,
        Self::AddBone,
        Self::ReparentBone,
        Self::ManipulateBones,
        Self::PaintWeights,
        Self::Pan,
        Self::Zoom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SelectPoints => "Select Points",
            Self::TranslatePoints => "Transform Points",
            Self::AddPoint => "Add Point",
            Self::Pen => "Pen",
            Self::Curvature => "Curvature",
            Self::Freehand => "Freehand",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Polygon => "Polygon",
            Self::Star => "Star",
            Self::Triangle => "Triangle",
            Self::Eraser => "Eraser",
            Self::DeleteEdge => "Delete Edge",
            Self::HideEdge => "Hide Edge",
            Self::Magnet => "Magnet",
            Self::PaintBucket => "Paint Bucket",
            Self::Eyedropper => "Eyedropper",
            Self::DrawShape => "Draw Shape",
            Self::LineWidth => "Line Width",
            Self::StrokeExposure => "Stroke Exposure",
            Self::ShapeBuilder => "Shape Builder",
            Self::TransformLayer => "Transform Layer",
            Self::TrackCamera => "Track camera",
            Self::ZoomCamera => "Zoom camera",
            Self::RollCamera => "Roll camera",
            Self::SelectBone => "Select Bone",
            Self::AddBone => "Add Bone",
            Self::ReparentBone => "Reparent Bone",
            Self::ManipulateBones => "Manipulate Bones",
            Self::PaintWeights => "Paint Weights",
            Self::Pan => "Pan",
            Self::Zoom => "Zoom",
        }
    }

    pub fn group(self) -> ToolGroup {
        match self {
            Self::SelectPoints
            | Self::TranslatePoints
            | Self::AddPoint
            | Self::Pen
            | Self::Curvature
            | Self::Freehand
            | Self::Rectangle
            | Self::Ellipse
            | Self::Line
            | Self::Polygon
            | Self::Star
            | Self::Triangle
            | Self::Eraser
            | Self::DeleteEdge
            | Self::HideEdge
            | Self::Magnet
            | Self::ShapeBuilder => ToolGroup::Draw,
            Self::PaintBucket
            | Self::Eyedropper
            | Self::DrawShape
            | Self::LineWidth
            | Self::StrokeExposure => ToolGroup::Fill,
            Self::TransformLayer => ToolGroup::Layer,
            Self::TrackCamera | Self::ZoomCamera | Self::RollCamera => ToolGroup::Camera,
            Self::SelectBone
            | Self::AddBone
            | Self::ReparentBone
            | Self::ManipulateBones
            | Self::PaintWeights => ToolGroup::Bone,
            Self::Pan | Self::Zoom => ToolGroup::Workspace,
        }
    }

    /// Whether the tool works yet. Others are shown so the layout is
    /// complete, and say which milestone brings them.
    pub fn is_available(self) -> bool {
        matches!(
            self,
            Self::SelectPoints
                | Self::TranslatePoints
                | Self::AddPoint
                | Self::Pen
                | Self::Curvature
                | Self::Freehand
                | Self::Eraser
                | Self::Rectangle
                | Self::Ellipse
                | Self::Line
                | Self::Polygon
                | Self::Star
                | Self::Triangle
                | Self::DeleteEdge
                | Self::HideEdge
                | Self::Magnet
                | Self::PaintBucket
                | Self::Eyedropper
                | Self::DrawShape
                | Self::LineWidth
                | Self::StrokeExposure
                | Self::ShapeBuilder
                | Self::TransformLayer
                | Self::TrackCamera
                | Self::ZoomCamera
                | Self::RollCamera
                | Self::SelectBone
                | Self::AddBone
                | Self::ReparentBone
                | Self::ManipulateBones
                | Self::PaintWeights
                | Self::Pan
                | Self::Zoom
        )
    }

    /// Whether the tool works on points, so Delete and Select All act on
    /// points rather than layers.
    pub fn edits_points(self) -> bool {
        matches!(
            self,
            Self::SelectPoints
                | Self::TranslatePoints
                | Self::AddPoint
                | Self::DeleteEdge
                | Self::HideEdge
                | Self::Magnet
        )
    }

    /// Status-bar help while the tool is active.
    pub fn hint(self) -> &'static str {
        match self {
            Self::TransformLayer => {
                "Drag to move the selected layer; drag the handles to scale and the round button to rotate (Shift: proportions or 15 degrees). The dot is the pivot. An animated layer gets a keyframe on this frame."
            }
            Self::Freehand => {
                "Draw brush strokes on the selected vector layer. With pen pressure on, a pen's pressure sets the width; set the size, smoothing and taper in the Style panel."
            }
            Self::Rectangle => "Drag to draw a rectangle. Shift makes a square; Alt draws from the centre.",
            Self::Ellipse => "Drag to draw an ellipse. Shift makes a circle; Alt draws from the centre.",
            Self::Line => "Drag to draw a straight line. Shift snaps to 15-degree steps.",
            Self::Polygon => {
                "Drag to draw a regular polygon. Scroll the mouse wheel to change the number of sides."
            }
            Self::Star => {
                "Drag to draw a star. Scroll the mouse wheel to change the number of points. Alt adjusts the inner radius."
            }
            Self::Triangle => "Drag to draw an equilateral triangle. Shift snaps the angle.",
            Self::PaintBucket => {
                "Click a shape to fill it with the fill colour, or a brush stroke to recolour it with the line colour."
            }
            Self::Eyedropper => "Click a shape to use its colours for new artwork.",
            Self::Eraser => {
                "Drag to erase lines and brush strokes on the selected layer. Shapes it covers completely are removed."
            }
            Self::Pan => "Drag to pan the canvas. Middle-drag pans with any tool.",
            Self::Zoom => "Click to zoom in, Alt-click to zoom out. Ctrl+scroll zooms with any tool.",
            Self::SelectPoints => {
                "Click a shape to select it whole, or a point to select just that. Shift or Ctrl adds. Drag a shape or selected points to move them; drag a box to pick the shapes it touches (Alt: only the points inside)."
            }
            Self::TranslatePoints => {
                "Drag to move the selected points, or the nearest point if none are selected."
            }
            Self::AddPoint => {
                "Click a line to add a point on it. Drag from the end of an open path to extend it, or from empty space to start a new line. Drop on the other end to close the shape."
            }
            Self::Pen => {
                "Click to add corner points, drag to add smooth ones. Click the first point to close the shape; Enter or Esc finishes an open path."
            }
            Self::Curvature => {
                "Click to add smooth points along a curve; Alt+click adds a corner. Click the first point to close. Esc finishes an open path."
            }
            Self::DeleteEdge => "Click a segment to delete it, splitting the shape into two open paths.",
            Self::HideEdge => {
                "Click a segment to leave its stroke undrawn; the points and fill stay. Click it again, or Alt+click, to show it."
            }
            Self::Magnet => {
                "Drag near points to attract or repel them. Hold Alt to repel. The draw width sets the magnet radius."
            }
            Self::TrackCamera => {
                "Drag to move the camera: the picture follows the pointer. Sets a keyframe on a later frame."
            }
            Self::ZoomCamera => {
                "Drag up to zoom the camera in, down to zoom out. Sets a keyframe on a later frame."
            }
            Self::RollCamera => {
                "Drag around the camera's centre to roll it (Shift: 15-degree steps). Sets a keyframe on a later frame."
            }
            Self::DrawShape => {
                "Drag around an area to sketch a shape; let go to close it and fill it with the current Fill and Line colours."
            }
            Self::LineWidth => {
                "Press next to a point on a stroke and drag up to widen it, down to narrow it. Selected points change together."
            }
            Self::StrokeExposure => {
                "Press on a stroke and drag along it to show less or more of it. Alt+drag moves its start. Animate it by doing the same on another frame."
            }
            Self::ShapeBuilder => {
                "Drag across overlapping regions to unite them; Alt+drag subtracts them. Release to see the result."
            }
            Self::SelectBone => {
                "Click a bone to select it. Drag its tip to change its length and rest angle, or its joint to move it."
            }
            Self::AddBone => {
                "Drag to draw a bone on the selected bone layer. It joins the selected bone, so drag from its tip to build a chain."
            }
            Self::ReparentBone => {
                "Drag from a bone onto its new parent; drag to empty space for no parent. The bone stays where it is."
            }
            Self::ManipulateBones => {
                "Drag a bone to pose its chain (inverse kinematics; a pinned bone stops it). Alt-drag turns one bone only. Sets keyframes on the current frame."
            }
            Self::PaintWeights => {
                "Select a bone, then brush over points of a bound vector layer to make them follow that bone more. Alt brushes them away from it."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_belongs_to_a_menu() {
        const MENUS: [&str; 6] = ["File", "Edit", "Layer", "Animation", "View", "Tools"];
        for action in Action::all() {
            assert!(MENUS.contains(&action.category()), "{action:?}");
        }
    }

    fn every_kind() -> Vec<(&'static str, fanimate_core::LayerKind)> {
        use fanimate_core::{Audio, Image, LayerKind, Skeleton};
        vec![
            ("vector", LayerKind::Vector { shapes: vec![] }),
            ("group", LayerKind::Group { children: vec![] }),
            (
                "bone",
                LayerKind::Bone {
                    skeleton: Skeleton::default(),
                    children: vec![],
                },
            ),
            (
                "image",
                LayerKind::Image(Image {
                    media: "a.png".into(),
                    width: 1.0,
                    height: 1.0,
                    grid: [1, 1],
                    bone: None,
                    skin: None,
                }),
            ),
            ("frames", LayerKind::Frames { cels: vec![] }),
            (
                "switch",
                LayerKind::Switch {
                    children: vec![],
                    switch: vec![],
                },
            ),
            (
                "audio",
                LayerKind::Audio(Audio {
                    media: "a.wav".into(),
                    start: 0,
                    volume: 1.0,
                }),
            ),
        ]
    }

    #[test]
    fn tool_groups_follow_the_layer_type() {
        use ToolGroup::*;
        let always = vec![Draw, Fill, Layer, Camera, Workspace];
        assert_eq!(tool_groups_for(None), always);
        for (name, kind) in every_kind() {
            let groups = tool_groups_for(Some(&kind));
            let expected = match name {
                "bone" => vec![Draw, Fill, Layer, Camera, Bone, Workspace],
                _ => always.clone(),
            };
            assert_eq!(groups, expected, "{name}");
        }
    }

    #[test]
    fn a_layer_inside_a_bone_layer_keeps_the_bone_tools() {
        use ToolGroup::*;
        let vector = fanimate_core::LayerKind::Vector { shapes: vec![] };
        assert_eq!(
            tool_groups_in(Some(&vector), true),
            vec![Draw, Fill, Layer, Camera, Bone, Workspace]
        );
        assert_eq!(
            tool_groups_in(Some(&vector), false),
            tool_groups_for(Some(&vector))
        );
        assert_eq!(tool_groups_in(None, false), tool_groups_for(None));
    }

    #[test]
    fn the_always_groups_never_leave() {
        for (name, kind) in every_kind() {
            let groups = tool_groups_for(Some(&kind));
            for group in [
                ToolGroup::Draw,
                ToolGroup::Fill,
                ToolGroup::Layer,
                ToolGroup::Camera,
                ToolGroup::Workspace,
            ] {
                assert!(groups.contains(&group), "{name} lost {group:?}");
                assert_eq!(group.needs(), None);
            }
        }
    }

    #[test]
    fn every_tool_is_listed_once_in_a_known_group() {
        for tool in Tool::ALL {
            assert_eq!(
                Tool::ALL.iter().filter(|t| **t == tool).count(),
                1,
                "{tool:?} is listed more than once"
            );
            assert!(ToolGroup::ALL.contains(&tool.group()), "{tool:?}");
        }
        // every group has a tool, or the panel would show an empty heading
        for group in ToolGroup::ALL {
            assert!(Tool::ALL.iter().any(|t| t.group() == group), "{group:?} is empty");
        }
        // and a match without a wildcard keeps this list in step with the enum
        for tool in Tool::ALL {
            let _: usize = match tool {
                Tool::SelectPoints
                | Tool::TranslatePoints
                | Tool::AddPoint
                | Tool::Pen
                | Tool::Curvature
                | Tool::Freehand
                | Tool::Rectangle
                | Tool::Ellipse
                | Tool::Line
                | Tool::Polygon
                | Tool::Star
                | Tool::Triangle
                | Tool::Eraser
                | Tool::PaintBucket
                | Tool::Eyedropper
                | Tool::DeleteEdge
                | Tool::HideEdge
                | Tool::Magnet
                | Tool::DrawShape
                | Tool::LineWidth
                | Tool::StrokeExposure
                | Tool::ShapeBuilder
                | Tool::TransformLayer
                | Tool::TrackCamera
                | Tool::ZoomCamera
                | Tool::RollCamera
                | Tool::SelectBone
                | Tool::AddBone
                | Tool::ReparentBone
                | Tool::ManipulateBones
                | Tool::PaintWeights
                | Tool::Pan
                | Tool::Zoom => 1,
            };
        }
        assert_eq!(Tool::ALL.len(), 33);
    }

    #[test]
    fn camera_tools_are_in_the_camera_group() {
        for tool in [Tool::TrackCamera, Tool::ZoomCamera, Tool::RollCamera] {
            assert_eq!(tool.group(), ToolGroup::Camera);
            assert!(tool.is_available());
            assert!(!tool.edits_points());
            assert!(!tool.hint().is_empty());
        }
        assert_eq!(ToolGroup::Camera.label(), "Camera");
    }
}
