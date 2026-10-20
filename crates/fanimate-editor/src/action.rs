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
    Duplicate,
    Delete,
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
}

impl Action {
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
            Duplicate => "Duplicate",
            Delete => "Delete",
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
            Undo | Redo | UndoLayer | GoToHistory(_) | Cut | Copy | Paste | Duplicate | Delete
            | SelectAll | SelectNone | Rename | FinishPath | Union | Subtract | Intersect | ExpandShape
            | Preferences | CommandPalette => "Edit",
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
            | SetLabel(_)
            | SetShapeVisible(..)
            | DeleteShapes
            | DuplicateShapes
            | RenameShape(..)
            | ConvertLayerType(_)
            | ConfirmConvertLayerType(_)
            | BindToBone
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
            Duplicate,
            Delete,
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
            BindToBone,
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

/// The sample projects in File › Open Sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sample {
    Hello,
    Benchmark,
    StressTest,
}

impl Sample {
    pub const ALL: [Sample; 3] = [Self::Hello, Self::Benchmark, Self::StressTest];

    /// The name in the Open Sample menu.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hello => "Hello Fanimate",
            Self::Benchmark => "Benchmark rig (40 bones)",
            Self::StressTest => "Stress test (400 bones)",
        }
    }

    pub fn document(self) -> fanimate_core::Document {
        use fanimate_core::samples;
        match self {
            Self::Hello => samples::hello(),
            Self::Benchmark => samples::benchmark(40),
            Self::StressTest => samples::benchmark(400),
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
    Bone,
    Workspace,
}

impl ToolGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draw => "Draw",
            Self::Fill => "Fill",
            Self::Layer => "Layer",
            Self::Bone => "Bone",
            Self::Workspace => "Workspace",
        }
    }

    pub const ALL: [ToolGroup; 5] = [Self::Draw, Self::Fill, Self::Layer, Self::Bone, Self::Workspace];
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
    Magnet,
    ScalePoints,
    RotatePoints,
    DrawShape,
    FreehandWidth,
    LineWidth,
    StrokeExposure,
    ShapeBuilder,
    #[default]
    TransformLayer,
    SelectBone,
    AddBone,
    ManipulateBones,
    PaintWeights,
    Pan,
    Zoom,
}

impl Tool {
    pub const ALL: [Tool; 31] = [
        Self::SelectPoints,
        Self::TranslatePoints,
        Self::AddPoint,
        Self::Pen,
        Self::Curvature,
        Self::Freehand,
        Self::FreehandWidth,
        Self::Rectangle,
        Self::Ellipse,
        Self::Line,
        Self::Polygon,
        Self::Star,
        Self::Triangle,
        Self::Eraser,
        Self::DeleteEdge,
        Self::Magnet,
        Self::ScalePoints,
        Self::RotatePoints,
        Self::PaintBucket,
        Self::Eyedropper,
        Self::DrawShape,
        Self::LineWidth,
        Self::StrokeExposure,
        Self::ShapeBuilder,
        Self::TransformLayer,
        Self::SelectBone,
        Self::AddBone,
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
            Self::FreehandWidth => "Freehand with Width",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Polygon => "Polygon",
            Self::Star => "Star",
            Self::Triangle => "Triangle",
            Self::Eraser => "Eraser",
            Self::DeleteEdge => "Delete Edge",
            Self::Magnet => "Magnet",
            Self::ScalePoints => "Scale Points",
            Self::RotatePoints => "Rotate Points",
            Self::PaintBucket => "Paint Bucket",
            Self::Eyedropper => "Eyedropper",
            Self::DrawShape => "Draw Shape",
            Self::LineWidth => "Line Width",
            Self::StrokeExposure => "Stroke Exposure",
            Self::ShapeBuilder => "Shape Builder",
            Self::TransformLayer => "Transform Layer",
            Self::SelectBone => "Select Bone",
            Self::AddBone => "Add Bone",
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
            | Self::FreehandWidth
            | Self::Rectangle
            | Self::Ellipse
            | Self::Line
            | Self::Polygon
            | Self::Star
            | Self::Triangle
            | Self::Eraser
            | Self::DeleteEdge
            | Self::Magnet
            | Self::ScalePoints
            | Self::RotatePoints
            | Self::ShapeBuilder => ToolGroup::Draw,
            Self::PaintBucket
            | Self::Eyedropper
            | Self::DrawShape
            | Self::LineWidth
            | Self::StrokeExposure => ToolGroup::Fill,
            Self::TransformLayer => ToolGroup::Layer,
            Self::SelectBone | Self::AddBone | Self::ManipulateBones | Self::PaintWeights => ToolGroup::Bone,
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
                | Self::FreehandWidth
                | Self::Eraser
                | Self::Rectangle
                | Self::Ellipse
                | Self::Line
                | Self::Polygon
                | Self::Star
                | Self::Triangle
                | Self::DeleteEdge
                | Self::Magnet
                | Self::ScalePoints
                | Self::RotatePoints
                | Self::PaintBucket
                | Self::Eyedropper
                | Self::DrawShape
                | Self::LineWidth
                | Self::StrokeExposure
                | Self::ShapeBuilder
                | Self::TransformLayer
                | Self::SelectBone
                | Self::AddBone
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
                | Self::Magnet
                | Self::ScalePoints
                | Self::RotatePoints
        )
    }

    /// Status-bar help while the tool is active.
    pub fn hint(self) -> &'static str {
        match self {
            Self::TransformLayer => {
                "Drag to move the selected layer; drag the handles to scale and the round button to rotate (Shift: proportions or 15 degrees). The dot is the pivot. An animated layer gets a keyframe on this frame."
            }
            Self::Freehand => {
                "Draw brush strokes on the selected vector layer. A pen's pressure sets the width; set the size, smoothing and taper in the Style panel."
            }
            Self::FreehandWidth => {
                "Draw brush strokes whose width you drag on each segment. Set the base width in the Style panel."
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
            Self::Magnet => {
                "Drag near points to attract or repel them. Hold Alt to repel. The draw width sets the magnet radius."
            }
            Self::ScalePoints => {
                "Drag to scale the selected points around their centre. Shift keeps them on the same axes."
            }
            Self::RotatePoints => {
                "Drag to rotate the selected points around their centre. Shift snaps to 15-degree steps."
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
}
