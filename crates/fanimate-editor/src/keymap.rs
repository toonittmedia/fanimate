//! Keyboard shortcut maps.
//!
//! Two presets ship:
//!
//! - **Fanimate** (the default): the conventions of mainstream editing apps.
//!   Ctrl+C / Ctrl+X / Ctrl+V copy, cut and paste whatever is selected,
//!   including layers; Ctrl+Y and Ctrl+Shift+Z both redo; Delete deletes;
//!   F2 renames; tool letters follow Adobe Animate and Illustrator.
//! - **Moho**: Moho Pro's own shortcuts, so Moho users keep their muscle
//!   memory. Redo is Ctrl+Shift+Z only, tools use Moho's letters (M, T, G,
//!   Z, …), and Home resets the view. A few keys Moho leaves unused (F1, F2)
//!   are added because they cannot conflict with anything.
//!
//! Key names are egui's `Key::name()` strings ("Z", "Left", "OpenBracket",
//! "F2"), so the app can match key presses without this crate depending on
//! egui. "Ctrl" means the platform command key (Cmd on macOS).

use crate::action::{Action, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: &'static str,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub fn new(key: &'static str, ctrl: bool, shift: bool, alt: bool) -> Self {
        Self {
            key,
            ctrl,
            shift,
            alt,
        }
    }

    /// Parse `"Ctrl+Shift+Z"`. Panics on a bad spec: specs are compile-time
    /// constants in this file, and a test parses every one of them.
    fn parse(spec: &'static str) -> Self {
        let mut chord = Self::new("", false, false, false);
        for part in spec.split('+') {
            match part {
                "Ctrl" => chord.ctrl = true,
                "Shift" => chord.shift = true,
                "Alt" => chord.alt = true,
                key if chord.key.is_empty() && !key.is_empty() => chord.key = key,
                _ => panic!("bad shortcut spec {spec:?}"),
            }
        }
        assert!(!chord.key.is_empty(), "shortcut spec {spec:?} has no key");
        chord
    }

    /// Human-readable form, e.g. `Ctrl+Shift+Z` or `⌘⇧Z` on macOS.
    pub fn display(&self) -> String {
        let key = match self.key {
            "Left" => "←",
            "Right" => "→",
            "Up" => "↑",
            "Down" => "↓",
            "OpenBracket" => "[",
            "CloseBracket" => "]",
            "Equals" => "=",
            "Minus" => "-",
            "Plus" => "+",
            "Comma" => ",",
            "Escape" => "Esc",
            "Delete" => "Del",
            k => k,
        };
        let mac = cfg!(target_os = "macos");
        let mut s = String::new();
        let sep = if mac { "" } else { "+" };
        if self.ctrl {
            s += if mac { "⌘" } else { "Ctrl" };
            s += sep;
        }
        if self.alt {
            s += if mac { "⌥" } else { "Alt" };
            s += sep;
        }
        if self.shift {
            s += if mac { "⇧" } else { "Shift" };
            s += sep;
        }
        s + key
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KeymapKind {
    #[default]
    Fanimate,
    Moho,
}

impl KeymapKind {
    pub const ALL: [KeymapKind; 2] = [Self::Fanimate, Self::Moho];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fanimate => "Fanimate (standard)",
            Self::Moho => "Moho",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Fanimate => {
                "Shortcuts like other editing apps: Ctrl+C, Ctrl+X and Ctrl+V work on layers, \
                 Ctrl+Y or Ctrl+Shift+Z redo, Delete deletes, F2 renames."
            }
            Self::Moho => {
                "Moho Pro's shortcuts: Ctrl+Shift+Z redoes, tools use Moho's letters \
                 (M transform layer, Z manipulate bones, T transform points), Home resets the view."
            }
        }
    }

    /// Stable id for settings files.
    pub fn id(self) -> &'static str {
        match self {
            Self::Fanimate => "fanimate",
            Self::Moho => "moho",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

pub struct Keymap {
    pub kind: KeymapKind,
    bindings: Vec<(Chord, Action)>,
}

impl Keymap {
    pub fn new(kind: KeymapKind) -> Self {
        let specs = match kind {
            KeymapKind::Fanimate => FANIMATE,
            KeymapKind::Moho => MOHO,
        };
        Self {
            kind,
            bindings: specs.iter().map(|(spec, a)| (Chord::parse(spec), *a)).collect(),
        }
    }

    pub fn lookup(&self, chord: Chord) -> Option<Action> {
        self.bindings.iter().find(|(c, _)| *c == chord).map(|(_, a)| *a)
    }

    /// The primary shortcut for `action`, for menus and tooltips.
    pub fn shortcut(&self, action: Action) -> Option<Chord> {
        let direct = self.bindings.iter().find(|(_, a)| *a == action).map(|(c, _)| *c);
        direct.or_else(|| match action {
            // Moho binds one letter to both; show it on each tool
            Action::Tool(Tool::AddPoint | Tool::AddBone) => self.shortcut(Action::AddPointOrBone),
            _ => None,
        })
    }

    pub fn shortcut_text(&self, action: Action) -> String {
        self.shortcut(action).map(|c| c.display()).unwrap_or_default()
    }

    pub fn bindings(&self) -> &[(Chord, Action)] {
        &self.bindings
    }
}

use Action::*;

const FANIMATE: &[(&str, Action)] = &[
    // File
    ("Ctrl+N", New),
    ("Ctrl+O", Open),
    ("Ctrl+Shift+O", OpenOther),
    ("Ctrl+W", CloseDocument),
    ("Ctrl+Tab", NextDocument),
    ("Ctrl+Shift+Tab", PreviousDocument),
    ("Ctrl+S", Save),
    ("Ctrl+Shift+S", SaveAs),
    // Adobe Animate's Import to Stage
    ("Ctrl+R", Import),
    ("Ctrl+Shift+R", ExportVideo),
    ("Ctrl+Q", Quit),
    // Edit
    ("Ctrl+Z", Undo),
    ("Ctrl+Y", Redo),
    ("Ctrl+Shift+Z", Redo),
    ("Ctrl+Alt+Z", UndoLayer),
    ("Ctrl+X", Cut),
    ("Ctrl+C", Copy),
    ("Ctrl+V", Paste),
    ("Ctrl+D", Duplicate),
    ("Delete", Delete),
    ("Backspace", Delete),
    ("Ctrl+A", SelectAll),
    ("Escape", SelectNone),
    ("Ctrl+Shift+A", SelectNone),
    ("F2", Rename),
    ("Enter", FinishPath),
    ("Ctrl+Comma", Preferences),
    ("Ctrl+Shift+P", CommandPalette),
    ("F1", CommandPalette),
    // Layer
    ("Ctrl+Shift+N", NewVectorLayer),
    ("Ctrl+Alt+N", NewFramesLayer),
    ("Ctrl+G", GroupLayers),
    ("Ctrl+Shift+G", Ungroup),
    ("Ctrl+CloseBracket", MoveLayerUp),
    ("Ctrl+OpenBracket", MoveLayerDown),
    ("Ctrl+Shift+H", ToggleVisibility),
    ("Ctrl+Shift+I", LayerProperties),
    ("Ctrl+B", BindToBone),
    ("Ctrl+Shift+B", BindSmoothly),
    // Animation (F6 / Shift+F6 as in Adobe Animate)
    ("Space", PlayPause),
    ("Right", NextFrame),
    ("Left", PrevFrame),
    ("Home", FirstFrame),
    ("End", LastFrame),
    ("Alt+Right", NextKey),
    ("Alt+Left", PrevKey),
    ("F6", AddKey),
    ("Shift+F6", DeleteKey),
    ("F7", NewDrawing),
    // View
    ("Alt+O", ToggleOnionSkin),
    ("Alt+L", ToggleLightTable),
    ("Ctrl+Equals", ZoomIn),
    ("Ctrl+Plus", ZoomIn),
    ("Ctrl+Minus", ZoomOut),
    ("Ctrl+0", FitView),
    ("Ctrl+1", ActualSize),
    // Tools (Adobe Animate / Illustrator letters)
    ("V", Tool(Tool::TransformLayer)),
    ("A", Tool(Tool::SelectPoints)),
    ("Q", Tool(Tool::TranslatePoints)),
    ("P", Tool(Tool::Pen)),
    ("Shift+P", Tool(Tool::AddPoint)),
    ("B", Tool(Tool::Freehand)),
    ("R", Tool(Tool::Rectangle)),
    ("O", Tool(Tool::Ellipse)),
    ("E", Tool(Tool::Eraser)),
    ("K", Tool(Tool::PaintBucket)),
    ("I", Tool(Tool::Eyedropper)),
    ("M", Tool(Tool::ManipulateBones)),
    ("Shift+M", Tool(Tool::AddBone)),
    ("J", Tool(Tool::SelectBone)),
    ("W", Tool(Tool::PaintWeights)),
    ("H", Tool(Tool::Pan)),
    ("Z", Tool(Tool::Zoom)),
];

const MOHO: &[(&str, Action)] = &[
    // File
    ("Ctrl+N", New),
    ("Ctrl+O", Open),
    ("Ctrl+Shift+O", OpenOther),
    ("Ctrl+W", CloseDocument),
    ("Ctrl+Tab", NextDocument),
    ("Ctrl+Shift+Tab", PreviousDocument),
    ("Ctrl+S", Save),
    ("Ctrl+Shift+S", SaveAs),
    // Moho's Export Animation
    ("Ctrl+Shift+R", ExportVideo),
    ("Ctrl+Q", Quit),
    // Edit (Moho: redo is Ctrl+Shift+Z only; Ctrl+Y is its tracing image)
    ("Ctrl+Z", Undo),
    ("Ctrl+Shift+Z", Redo),
    // not in Moho, but unused there
    ("Ctrl+Alt+Z", UndoLayer),
    ("Ctrl+X", Cut),
    ("Ctrl+C", Copy),
    ("Ctrl+V", Paste),
    ("Delete", Delete),
    ("Ctrl+A", SelectAll),
    ("Escape", SelectNone),
    ("Ctrl+Shift+I", LayerProperties),
    // not in Moho, but unused there, so safe to add
    ("F1", CommandPalette),
    ("F2", Rename),
    ("Enter", FinishPath),
    // Animation
    ("Space", PlayPause),
    ("Right", NextFrame),
    ("Left", PrevFrame),
    ("Shift+Left", FirstFrame),
    ("Shift+Right", LastFrame),
    ("Alt+Right", NextKey),
    ("Alt+Left", PrevKey),
    // View
    ("Home", FitView),
    ("Ctrl+Alt+9", ZoomIn),
    ("Ctrl+Alt+8", ZoomOut),
    // Tools (Moho letters)
    ("G", Tool(Tool::SelectPoints)),
    ("T", Tool(Tool::TranslatePoints)),
    ("A", AddPointOrBone),
    ("F", Tool(Tool::Freehand)),
    ("E", Tool(Tool::Eraser)),
    ("P", Tool(Tool::PaintBucket)),
    ("L", Tool(Tool::Eyedropper)),
    ("M", Tool(Tool::TransformLayer)),
    ("B", Tool(Tool::SelectBone)),
    ("Z", Tool(Tool::ManipulateBones)),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn chord(spec: &'static str) -> Chord {
        Chord::parse(spec)
    }

    #[test]
    fn every_spec_parses_and_no_chord_is_bound_twice() {
        for kind in KeymapKind::ALL {
            let map = Keymap::new(kind);
            let mut seen = HashMap::new();
            for (c, a) in map.bindings() {
                if let Some(prev) = seen.insert(*c, *a) {
                    panic!("{kind:?}: {} bound to both {prev:?} and {a:?}", c.display());
                }
            }
        }
    }

    #[test]
    fn fanimate_has_standard_editing_shortcuts() {
        let map = Keymap::new(KeymapKind::Fanimate);
        assert_eq!(map.lookup(chord("Ctrl+Y")), Some(Redo));
        assert_eq!(map.lookup(chord("Ctrl+Shift+Z")), Some(Redo));
        assert_eq!(map.lookup(chord("Ctrl+C")), Some(Copy));
        assert_eq!(map.lookup(chord("Ctrl+X")), Some(Cut));
        assert_eq!(map.lookup(chord("Ctrl+V")), Some(Paste));
        assert_eq!(map.lookup(chord("Delete")), Some(Delete));
        assert_eq!(map.lookup(chord("F2")), Some(Rename));
    }

    #[test]
    fn moho_keeps_mohos_shortcuts() {
        let map = Keymap::new(KeymapKind::Moho);
        assert_eq!(map.lookup(chord("Ctrl+Shift+Z")), Some(Redo));
        assert_eq!(map.lookup(chord("Ctrl+Y")), None);
        assert_eq!(map.lookup(chord("M")), Some(Tool(Tool::TransformLayer)));
        assert_eq!(map.lookup(chord("Z")), Some(Tool(Tool::ManipulateBones)));
        assert_eq!(map.lookup(chord("A")), Some(AddPointOrBone));
        assert_eq!(map.lookup(chord("Home")), Some(FitView));
        // Moho's contextual A shows on both tools
        assert_eq!(map.shortcut_text(Tool(Tool::AddBone)), "A");
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let map = Keymap::new(KeymapKind::Fanimate);
        assert_eq!(map.lookup(chord("Z")), Some(Tool(Tool::Zoom)));
        assert_eq!(map.lookup(chord("Ctrl+Z")), Some(Undo));
        assert_eq!(map.lookup(chord("Ctrl+Alt+Z")), Some(UndoLayer));
        assert_eq!(map.lookup(chord("Ctrl+Alt+Shift+Z")), None);
    }

    #[test]
    fn display_and_ids() {
        if !cfg!(target_os = "macos") {
            assert_eq!(chord("Ctrl+Shift+Z").display(), "Ctrl+Shift+Z");
            assert_eq!(chord("Ctrl+CloseBracket").display(), "Ctrl+]");
        }
        for k in KeymapKind::ALL {
            assert_eq!(KeymapKind::from_id(k.id()), Some(k));
        }
    }
}
