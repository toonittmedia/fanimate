//! The Fanimate editing model.
//!
//! Everything the app does to a document lives here, independent of the UI
//! toolkit: selection, editing commands, undo and redo, the layer clipboard,
//! tools, and keyboard shortcut maps. The app crate only draws widgets and
//! turns clicks and key presses into calls on [`Editor`]. Keeping this
//! separate means every command is unit-tested without opening a window.

mod action;
mod editor;
mod history;
mod keymap;
pub mod snap;
mod tabs;

pub use action::{Action, Preset, Sample, Tool, ToolGroup, Workspace, search};
pub use editor::{
    AlignTo, BoneRef, ConvertTo, DrawStyle, Editor, Handle, HandleMods, Loss, MouthCue, Outcome, Placement,
    PointRef, RadiusTarget, SOUND_EXTENSIONS, SelectMode, SelectionFrame, ShapeBuilderState, SoundInfo,
    StatusKind, parse_rhubarb, sound_info, sound_to_wav,
};
pub use history::History;
pub use keymap::{Chord, Keymap, KeymapKind};
pub use snap::{GridSettings, Guide, SnapSettings};
pub use tabs::{TabInfo, Tabs};
