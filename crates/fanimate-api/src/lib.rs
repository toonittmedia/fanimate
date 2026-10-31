//! The Fanimate scripting API, as Rust.
//!
//! Scripts, plugins, the headless mode and the AI tools need to name what
//! they change ("layer 7"), where the interface works on what is selected.
//! This crate is that surface: explicit-target queries and commands over an
//! [`Editor`], with no UI toolkit in sight. Every command runs through
//! [`Editor::transaction`], so a call is one undo step, all or nothing, and
//! refused while the animation plays. The Python, TypeScript and WASM hosts
//! (Phase 2, WS6) are thin layers over this crate and speak its data as JSON.
//!
//! The data types mirror the project file format ([`fanimate_fnm`]) rather
//! than the engine's internals, so a plugin author reads one specification.
//! No egui, wgpu or winit type ever appears here: their versions are coupled
//! to Fanimate's and change with it.
//!
//! **The API is unstable until Fanimate 1.0** ([`API_VERSION`] ends in
//! `-unstable`). Changing it is a visible diff: `api-surface.txt` lists the
//! public surface, and a test fails until the file is updated on purpose.

use std::fmt;

pub mod rpc;

use fanimate_core::{Layer, LayerId, LayerKind, Provenance, Skeleton};
use fanimate_editor::{Editor, TxError};
use serde::Serialize;

/// The API's own version, not Fanimate's. `0.x` is unstable; the freeze at
/// 1.0 makes it `1.0.0`, after which changes follow semantic versioning and
/// deprecated calls keep working for two major releases.
pub const API_VERSION: &str = "0.1.0-unstable";

/// The longest layer name the API accepts, in characters.
pub const MAX_NAME_CHARS: usize = 200;

/// Why a call failed. The strings from [`ErrorCode::as_str`] are part of the
/// API: scripts match on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    /// Nothing has the id the call named.
    NotFound,
    /// An argument is empty, too long or otherwise unusable.
    InvalidArgument,
    /// The animation is playing, and nothing is edited then.
    Playing,
    /// A file could not be read or written.
    Io,
    /// The host refused the call: a connection that has not shown the right token.
    Unauthorized,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::InvalidArgument => "invalid_argument",
            Self::Playing => "playing",
            Self::Io => "io_error",
            Self::Unauthorized => "unauthorized",
        }
    }
}

/// A failed call: a stable code for programs and a message for people.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn no_layer(id: u32) -> Self {
        Self::new(ErrorCode::NotFound, format!("there is no layer {id}"))
    }

    fn no_bone(layer: u32, bone: u32) -> Self {
        Self::new(ErrorCode::NotFound, format!("layer {layer} has no bone {bone}"))
    }
}

/// A name as the API takes it: trimmed, not empty, not too long.
fn check_name<'n>(name: &'n str, what: &str) -> Result<&'n str, ApiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidArgument,
            format!("{what} name cannot be empty"),
        ));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::new(
            ErrorCode::InvalidArgument,
            format!("{what} name has at most {MAX_NAME_CHARS} characters"),
        ));
    }
    Ok(name)
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ApiError {}

/// The project as a whole.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocumentInfo {
    pub name: String,
    pub width: f32,
    pub height: f32,
    pub fps: f32,
    /// First and last frame that plays; frames count from 1.
    pub start: i32,
    pub end: i32,
    /// Top-level layers, not counting those inside groups.
    pub layers: usize,
    /// The `.fnm` format version this build writes.
    pub format_version: u32,
    pub api_version: &'static str,
}

/// One layer, wherever it sits in the tree.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayerInfo {
    pub id: u32,
    pub name: String,
    /// The layer's type in the file format: `"vector"`, `"group"`, `"bone"`,
    /// `"image"`, `"frames"`, `"switch"` or `"audio"`.
    pub kind: &'static str,
    /// The group, bone layer or switch layer holding it; `null` at the top.
    pub parent: Option<u32>,
    pub visible: bool,
    pub locked: bool,
    /// Layers directly inside it.
    pub children: usize,
    /// What made its content, when a tool rather than the artist did.
    pub provenance: Option<Provenance>,
}

/// One bone of a bone layer. Bones are named by `id`, which never changes,
/// not by their place in the list, which does when a bone is reparented.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoneInfo {
    pub id: u32,
    pub name: String,
    /// The parent bone's id; `null` for a root bone.
    pub parent: Option<u32>,
    pub length: f32,
}

/// A text layer's words and how they are set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextInfo {
    pub content: String,
    pub family: String,
    pub size: f32,
    pub weight: f32,
    /// Extra space after every character, in thousandths of the size.
    pub tracking: f32,
    pub line_height: f32,
    /// The wrap width; `null` for text that only breaks where the words do.
    pub width: Option<f32>,
    pub characters: usize,
    pub words: usize,
    pub lines: usize,
    pub animators: Vec<AnimatorInfo>,
    /// Whether the text runs along a path (see `set_text_path`).
    pub on_path: bool,
}

/// The highest frame the API takes: a day at 24 frames a second, more than any
/// animation needs and far short of what overflows a frame number.
pub const MAX_FRAME: i32 = 2_000_000;

/// Changes to the project as a whole. A field left out is left as it is.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct DocumentSettings {
    pub name: Option<String>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub fps: Option<f32>,
    /// The first and last frame that plays; frames count from 1.
    pub start: Option<i32>,
    pub end: Option<i32>,
}

/// Changes to a layer's transform and opacity. A field left out is left as it
/// is; the angle is in degrees, clockwise on screen.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct TransformChange {
    /// The frame to set it on; 0, the rest pose, when left out.
    pub frame: Option<i32>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub rotation: Option<f32>,
    pub scale_x: Option<f32>,
    pub scale_y: Option<f32>,
    /// From 0 (clear) to 1 (opaque).
    pub opacity: Option<f32>,
}

/// A shape to draw on a vector layer: its box, and how it looks.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct ShapeSpec {
    /// `"rectangle"` or `"ellipse"`.
    pub kind: String,
    pub name: Option<String>,
    /// The box's top left, in the layer's space, and its size.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The corner radius of a rectangle.
    pub radius: Option<f32>,
    /// A colour like `#RRGGBB` or `#RRGGBBAA`; black when there is no stroke either.
    pub fill: Option<String>,
    pub stroke: Option<String>,
    /// The outline width, 4 when there is a stroke colour and no width.
    pub stroke_width: Option<f32>,
}

/// A value for `set_text_animator_value`: a number, a pair `[x, y]`, or a
/// colour like `"#RRGGBB"`.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum AnimatorValue {
    Number(f32),
    Pair([f32; 2]),
    Colour(String),
}

/// One of a bone layer's actions.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ActionInfo {
    pub index: usize,
    pub name: String,
    /// `"action"`, `"pose"` (a one-frame action) or `"smart_bone"`.
    pub kind: &'static str,
    pub length: i32,
}

/// One key of a curve.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KeyInfo {
    pub frame: i32,
    /// In the units the graph shows: degrees for angles, a percentage for
    /// opacity, the file's own numbers for the rest.
    pub value: f32,
    /// How the curve leaves this key, as the project file writes it:
    /// `"linear"`, `{"ease":"bounce"}`, `{"bezier":[250,100,750,1000]}`.
    pub interp: serde_json::Value,
}

/// One curve of a keyed property: a number a property holds, against frame.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CurveInfo {
    /// `"position"`, `"rotation"`, `"scale"`, `"opacity"`, `"order"` or
    /// `"bone:<id>"`.
    pub channel: String,
    /// 0 for a number, 0 (x) or 1 (y) for a pair.
    pub component: usize,
    pub keys: Vec<KeyInfo>,
}

/// One master controller of a bone layer.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ControllerInfo {
    pub index: usize,
    pub name: String,
    /// `"slider"` or `"pad"`.
    pub kind: &'static str,
    /// The values, 0 to 1, on the frame asked for.
    pub x: f32,
    pub y: f32,
    pub links: Vec<ControllerLinkInfo>,
}

/// A controller's hold on an action.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ControllerLinkInfo {
    /// The action's index in `actions`.
    pub action: usize,
    /// `"x"` or `"y"`.
    pub axis: &'static str,
    pub from: f32,
    pub to: f32,
}

fn axis_of(axis: &str) -> Result<fanimate_core::actions::ControllerAxis, ApiError> {
    match axis {
        "x" => Ok(fanimate_core::actions::ControllerAxis::X),
        "y" => Ok(fanimate_core::actions::ControllerAxis::Y),
        other => Err(ApiError::new(
            ErrorCode::InvalidArgument,
            format!("an axis is \"x\" or \"y\", not \"{other}\""),
        )),
    }
}

/// The most bones a bone layer takes through the API.
pub const MAX_BONES: usize = 2_000;

/// How a spring behaves; a field left out takes the default (stiffness 120,
/// damping 8, inertia 1).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct SpringSettings {
    /// How hard a bone is pulled back to its key, 0 to 10000.
    pub stiffness: Option<f32>,
    /// How fast the swing dies away, 0 to 1000.
    pub damping: Option<f32>,
    /// How much the bones above throw it, 0 to 10.
    pub inertia: Option<f32>,
    /// How hard gravity pulls bones down the canvas, 0 to 200 (10 droops a loose tail gently).
    pub gravity: Option<f32>,
    /// How hard a steady wind blows bones to the right, -200 to 200 (negative blows left).
    pub wind: Option<f32>,
}

/// What importing captions made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CaptionInfo {
    /// The switch layer holding the captions.
    pub layer: u32,
    pub captions: usize,
    /// Captions in the file that could not be read (a bad time or no words).
    pub skipped: usize,
    /// Captions past the limit of 50,000 that were not read.
    pub cut_off: usize,
}

/// One of a text layer's animators.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnimatorInfo {
    pub name: String,
    /// What it acts on: `"character"`, `"word"` or `"line"`.
    pub unit: &'static str,
}

/// Changes to a text layer's look. A field left out is left as it is; the
/// numbers must be numbers (they are refused otherwise, not held in range).
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct TextStyle {
    pub family: Option<String>,
    pub size: Option<f32>,
    pub weight: Option<f32>,
    pub tracking: Option<f32>,
    pub line_height: Option<f32>,
    /// Wrap lines at this width.
    pub width: Option<f32>,
    /// `false` turns wrapping off (a missing value cannot say so in JSON: `null`
    /// and left out are the same thing).
    pub wrap: Option<bool>,
}
/// A handle on an editor that scripts act through.
pub struct Api<'a> {
    editor: &'a mut Editor,
}

impl<'a> Api<'a> {
    pub fn new(editor: &'a mut Editor) -> Self {
        Self { editor }
    }

    // ------------------------------------------------------------------
    // Queries: nothing changes

    pub fn document(&self) -> DocumentInfo {
        let doc = &self.editor.doc;
        DocumentInfo {
            name: doc.name.clone(),
            width: doc.width,
            height: doc.height,
            fps: doc.fps,
            start: doc.start,
            end: doc.end,
            layers: doc.layers.len(),
            format_version: fanimate_fnm::FORMAT_VERSION,
            api_version: API_VERSION,
        }
    }

    /// Every layer, depth first, a layer before the layers inside it.
    pub fn layers(&self) -> Vec<LayerInfo> {
        let mut out = Vec::new();
        for layer in &self.editor.doc.layers {
            collect(layer, None, &mut out);
        }
        out
    }

    pub fn layer(&self, id: u32) -> Result<LayerInfo, ApiError> {
        self.layers()
            .into_iter()
            .find(|l| l.id == id)
            .ok_or_else(|| ApiError::no_layer(id))
    }

    /// The bones of a bone layer, parents before children, each named by its
    /// id. Fails with `not_found` if the layer is not a bone layer.
    pub fn bones(&self, layer: u32) -> Result<Vec<BoneInfo>, ApiError> {
        let skeleton = self.skeleton(layer)?;
        Ok(skeleton
            .bones
            .iter()
            .map(|b| BoneInfo {
                id: b.id,
                name: b.name.clone(),
                parent: b.parent.and_then(|p| skeleton.bones.get(p)).map(|p| p.id),
                length: b.length,
            })
            .collect())
    }

    /// The words and look of a text layer, and its animators.
    pub fn text(&self, layer: u32) -> Result<TextInfo, ApiError> {
        let text = self
            .editor
            .text(LayerId(layer))
            .ok_or_else(|| self.not_text(layer))?;
        let look = text.look();
        let built = text.built();
        Ok(TextInfo {
            content: look.content.clone(),
            family: look.family.clone(),
            size: look.size,
            weight: look.weight,
            tracking: look.tracking,
            line_height: look.line_height,
            width: look.width,
            characters: built.count(fanimate_core::TextUnit::Character),
            words: built.count(fanimate_core::TextUnit::Word),
            lines: built.count(fanimate_core::TextUnit::Line),
            on_path: look.path.is_some(),
            animators: text
                .animators
                .iter()
                .map(|a| AnimatorInfo {
                    name: a.name.clone(),
                    unit: match a.unit {
                        fanimate_core::TextUnit::Character => "character",
                        fanimate_core::TextUnit::Word => "word",
                        fanimate_core::TextUnit::Line => "line",
                    },
                })
                .collect(),
        })
    }

    // ------------------------------------------------------------------
    // Commands: each is one undo step, all or nothing

    pub fn rename_layer(&mut self, id: u32, name: &str) -> Result<(), ApiError> {
        let name = check_name(name, "a layer")?;
        self.require(id)?;
        self.edit("Rename layer", |ed| {
            ed.rename(LayerId(id), name);
            Ok(())
        })
    }

    /// Rename a bone, named by the id `bones` gave it.
    pub fn rename_bone(&mut self, layer: u32, bone: u32, name: &str) -> Result<(), ApiError> {
        let name = check_name(name, "a bone")?;
        if self.skeleton(layer)?.index_of(bone).is_none() {
            return Err(ApiError::no_bone(layer, bone));
        }
        self.edit("Rename bone", |ed| {
            if let Some(LayerKind::Bone { skeleton, .. }) =
                ed.doc.find_mut(LayerId(layer)).map(|l| &mut l.kind)
                && let Some(i) = skeleton.index_of(bone)
            {
                skeleton.bones[i].name = name.to_owned();
            }
            Ok(())
        })
    }

    pub fn set_layer_visible(&mut self, id: u32, visible: bool) -> Result<(), ApiError> {
        self.require(id)?;
        self.edit(if visible { "Show layer" } else { "Hide layer" }, |ed| {
            ed.set_visible(LayerId(id), visible);
            Ok(())
        })
    }

    /// Record what made a layer's content, or clear the record with `None`.
    /// The tools that generate layers call this so the file says so.
    pub fn set_layer_provenance(&mut self, id: u32, provenance: Option<Provenance>) -> Result<(), ApiError> {
        if provenance.as_ref().is_some_and(|p| !p.is_sound()) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "a provenance record needs a tool, text no longer than 200 characters and a date like 2026-10-30",
            ));
        }
        self.require(id)?;
        self.edit("Set provenance", |ed| {
            if let Some(layer) = ed.doc.find_mut(LayerId(id)) {
                layer.provenance = provenance;
            }
            Ok(())
        })
    }

    /// Add a text layer with its box's top left at (`x`, `y`) in the document.
    /// Returns its id.
    pub fn add_text_layer(&mut self, content: &str, x: f32, y: f32) -> Result<u32, ApiError> {
        if !(x.is_finite() && y.is_finite()) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "a position must be numbers",
            ));
        }
        self.edit("New text layer", |ed| {
            ed.add_text_layer(fanimate_core::Vec2::new(x, y), content)
                .map(|id| id.0)
                .ok_or_else(|| ApiError::new(ErrorCode::InvalidArgument, "the text layer could not be made"))
        })
    }

    /// Replace the words of a text layer.
    pub fn set_text_content(&mut self, layer: u32, content: &str) -> Result<(), ApiError> {
        self.require_text(layer)?;
        self.edit("Edit text", |ed| {
            ed.edit_text(LayerId(layer), "Edit text", |l| l.content = content.to_owned());
            Ok(())
        })
    }

    /// Change how a text layer is set. Numbers that are not numbers are
    /// refused; ones out of range are held within it.
    pub fn set_text_style(&mut self, layer: u32, style: TextStyle) -> Result<(), ApiError> {
        self.require_text(layer)?;
        let numbers = [
            style.size,
            style.weight,
            style.tracking,
            style.line_height,
            style.width,
        ];
        if numbers.iter().flatten().any(|n| !n.is_finite()) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "the text style needs numbers",
            ));
        }
        self.edit("Change text", |ed| {
            ed.edit_text(LayerId(layer), "Change text", |l| {
                if let Some(family) = style.family {
                    l.family = family;
                }
                l.size = style.size.unwrap_or(l.size);
                l.weight = style.weight.unwrap_or(l.weight);
                l.tracking = style.tracking.unwrap_or(l.tracking);
                l.line_height = style.line_height.unwrap_or(l.line_height);
                if let Some(width) = style.width {
                    l.width = Some(width);
                }
                if style.wrap == Some(false) {
                    l.width = None;
                }
            });
            Ok(())
        })
    }

    /// Put a ready-made animator on a text layer, named by one of
    /// `"fade_in"`, `"typewriter"`, `"rise_in"`, `"pop_in"` and `"wave"`. It
    /// starts on the playhead frame and runs for a second. Returns its index.
    pub fn add_text_animator(&mut self, layer: u32, preset: &str) -> Result<usize, ApiError> {
        let preset = match preset {
            "fade_in" => fanimate_core::TextPreset::FadeIn,
            "typewriter" => fanimate_core::TextPreset::Typewriter,
            "rise_in" => fanimate_core::TextPreset::RiseIn,
            "pop_in" => fanimate_core::TextPreset::PopIn,
            "wave" => fanimate_core::TextPreset::Wave,
            other => {
                return Err(ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("there is no animator preset \"{other}\""),
                ));
            }
        };
        self.require_text(layer)?;
        self.edit("Add text animator", |ed| {
            ed.add_text_animator(LayerId(layer), preset).ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidArgument,
                    "a text layer has at most 64 animators",
                )
            })
        })
    }

    /// Read an SRT or WebVTT file's text into a new layer of captions: a
    /// switch layer holding one text layer per caption, shown at its time.
    /// The animation is lengthened to the last caption if it is shorter.
    /// One undo step.
    pub fn import_captions(&mut self, content: &str, name: &str) -> Result<CaptionInfo, ApiError> {
        let name = if name.trim().is_empty() {
            "Captions"
        } else {
            check_name(name, "a layer")?
        };
        self.edit("Import captions", |ed| {
            ed.import_captions(content, name)
                .map(|made| CaptionInfo {
                    layer: made.layer.0,
                    captions: made.cues,
                    skipped: made.skipped,
                    cut_off: made.cut_off,
                })
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// The captions of a layer made by `import_captions` as text in the
    /// named format, `"srt"` or `"vtt"`.
    pub fn export_captions(&self, layer: u32, format: &str) -> Result<String, ApiError> {
        let webvtt = match format {
            "srt" => false,
            "vtt" => true,
            other => {
                return Err(ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("there is no caption format \"{other}\"; use \"srt\" or \"vtt\""),
                ));
            }
        };
        self.require(layer)?;
        self.editor
            .export_captions(LayerId(layer), webvtt)
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("layer {layer} shows no captions"),
                )
            })
    }

    /// Add an empty layer of `kind` (`"vector"`, `"group"` or `"switch"`)
    /// called `name`, on top of the layers beside it: at the top level, or
    /// inside `parent` when that is a group or a switch layer. Returns its id.
    pub fn add_layer(&mut self, kind: &str, name: &str, parent: Option<u32>) -> Result<u32, ApiError> {
        let name = check_name(name, "a layer")?;
        let kind = match kind {
            "vector" => LayerKind::vector(vec![]),
            "group" => LayerKind::Group { children: vec![] },
            "switch" => LayerKind::Switch {
                children: vec![],
                switch: vec![],
            },
            other => {
                return Err(ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("a layer is added as \"vector\", \"group\" or \"switch\", not \"{other}\""),
                ));
            }
        };
        if let Some(parent) = parent {
            match self.editor.doc.find(LayerId(parent)).map(|l| &l.kind) {
                Some(LayerKind::Group { .. } | LayerKind::Switch { .. }) => {}
                Some(_) => {
                    return Err(ApiError::new(
                        ErrorCode::InvalidArgument,
                        format!("layer {parent} cannot hold layers: only groups and switch layers can"),
                    ));
                }
                None => return Err(ApiError::no_layer(parent)),
            }
        }
        self.edit("Add layer", |ed| {
            let mut layer = ed.doc.new_layer(name, kind);
            layer.label = Some(fanimate_core::Label::for_id(layer.id));
            let id = layer.id;
            let parent = parent.map(LayerId);
            let at = match parent {
                Some(p) => ed.doc.find(p).and_then(|l| l.kind.children()).map_or(0, Vec::len),
                None => ed.doc.layers.len(),
            };
            ed.doc.insert(parent, at, layer);
            Ok(id.0)
        })
    }

    /// Delete a layer and everything inside it.
    pub fn delete_layer(&mut self, id: u32) -> Result<(), ApiError> {
        self.require(id)?;
        self.edit("Delete layer", |ed| {
            ed.doc.remove(LayerId(id));
            ed.selection.retain(|s| ed.doc.find(*s).is_some());
            Ok(())
        })
    }

    /// Change the project's name, size, frame rate or frame range. A field
    /// left out is left as it is.
    pub fn set_document(&mut self, settings: DocumentSettings) -> Result<(), ApiError> {
        let doc = &self.editor.doc;
        let (width, height) = (
            settings.width.unwrap_or(doc.width),
            settings.height.unwrap_or(doc.height),
        );
        let fps = settings.fps.unwrap_or(doc.fps);
        let (start, end) = (
            settings.start.unwrap_or(doc.start),
            settings.end.unwrap_or(doc.end),
        );
        let sized = |v: f32| v.is_finite() && (1.0..=16384.0).contains(&v);
        if !(sized(width) && sized(height)) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "the width and height are between 1 and 16384",
            ));
        }
        if !(fps.is_finite() && (1.0..=240.0).contains(&fps)) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "the frame rate is between 1 and 240",
            ));
        }
        if !(1..=MAX_FRAME).contains(&start) || !(start..=MAX_FRAME).contains(&end) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!(
                    "frames count from 1, the end is not before the start, and neither passes {MAX_FRAME}"
                ),
            ));
        }
        let name = match &settings.name {
            Some(name) => Some(check_name(name, "a project")?.to_owned()),
            None => None,
        };
        self.edit("Change project settings", |ed| {
            if let Some(name) = name {
                ed.doc.name = name;
            }
            (ed.doc.width, ed.doc.height, ed.doc.fps) = (width, height, fps);
            (ed.doc.start, ed.doc.end) = (start, end);
            Ok(())
        })
    }

    /// Move, turn, scale or fade a layer on `frame` (the rest pose, frame 0,
    /// when none is given), by the keying rule: on frame 0 the value itself
    /// changes, on a later frame a key is set and one is seeded on frame 0.
    /// Angles are in degrees, clockwise on screen. A field left out is left
    /// as it is.
    pub fn set_layer_transform(&mut self, id: u32, change: TransformChange) -> Result<(), ApiError> {
        self.require(id)?;
        let frame = change.frame.unwrap_or(0);
        if !(0..=MAX_FRAME).contains(&frame) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("a frame is from 0 (the rest pose) to {MAX_FRAME}"),
            ));
        }
        let numbers = [
            change.x,
            change.y,
            change.rotation,
            change.scale_x,
            change.scale_y,
            change.opacity,
        ];
        if numbers.iter().flatten().any(|n| !n.is_finite()) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "a transform needs numbers",
            ));
        }
        if self.editor.is_locked(LayerId(id)) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("layer {id} is locked"),
            ));
        }
        self.edit("Transform layer", |ed| {
            let keying = ed.keying();
            let Some(layer) = ed.doc.find_mut(LayerId(id)) else {
                return Err(ApiError::no_layer(id));
            };
            let at = layer.transform.sample(frame as f32);
            if change.x.is_some() || change.y.is_some() {
                let to = fanimate_core::Vec2::new(
                    change.x.unwrap_or(at.position.x),
                    change.y.unwrap_or(at.position.y),
                );
                layer.transform.position.write(frame, to, keying);
            }
            if let Some(degrees) = change.rotation {
                layer
                    .transform
                    .rotation
                    .write(frame, degrees.to_radians(), keying);
            }
            if change.scale_x.is_some() || change.scale_y.is_some() {
                let to = fanimate_core::Vec2::new(
                    change.scale_x.unwrap_or(at.scale.x),
                    change.scale_y.unwrap_or(at.scale.y),
                );
                layer.transform.scale.write(frame, to, keying);
            }
            if let Some(opacity) = change.opacity {
                layer.opacity.write(frame, opacity.clamp(0.0, 1.0), keying);
            }
            Ok(())
        })
    }

    /// Draw a shape on a vector layer, on top of its others. Returns its
    /// index in the layer.
    pub fn add_shape(&mut self, layer: u32, shape: ShapeSpec) -> Result<usize, ApiError> {
        match self.editor.doc.find(LayerId(layer)).map(|l| &l.kind) {
            Some(LayerKind::Vector { .. }) => {}
            Some(_) => {
                return Err(ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("layer {layer} is not a vector layer"),
                ));
            }
            None => return Err(ApiError::no_layer(layer)),
        }
        let colour = |hex: &Option<String>, what: &str| -> Result<Option<fanimate_core::Color>, ApiError> {
            hex.as_deref()
                .map(|h| {
                    fanimate_core::Color::from_hex(h).map_err(|_| {
                        ApiError::new(
                            ErrorCode::InvalidArgument,
                            format!("the {what} \"{h}\" is not a colour like #RRGGBB or #RRGGBBAA"),
                        )
                    })
                })
                .transpose()
        };
        let fill = colour(&shape.fill, "fill")?;
        let stroke = colour(&shape.stroke, "stroke")?;
        let width = shape
            .stroke_width
            .unwrap_or(if stroke.is_some() { 4.0 } else { 0.0 });
        let numbers = [
            shape.x,
            shape.y,
            shape.width,
            shape.height,
            width,
            shape.radius.unwrap_or(0.0),
        ];
        if numbers.iter().any(|n| !n.is_finite() || n.abs() > 1.0e6)
            || shape.width <= 0.0
            || shape.height <= 0.0
            || width < 0.0
        {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "a shape needs a position, a positive width and height, and a stroke width that is not negative",
            ));
        }
        let style = fanimate_core::Style {
            fill: fill.or(if stroke.is_none() {
                Some(fanimate_core::Color::BLACK)
            } else {
                None
            }),
            stroke: stroke.map(|color| fanimate_core::Stroke { color, width }),
        };
        let min = fanimate_core::Vec2::new(shape.x, shape.y);
        let size = fanimate_core::Vec2::new(shape.width, shape.height);
        let built = match shape.kind.as_str() {
            "rectangle" => fanimate_core::Shape::rectangle(
                shape.name.clone().unwrap_or_else(|| "Rectangle".into()),
                min,
                size,
                shape.radius.unwrap_or(0.0).max(0.0),
                style,
            ),
            "ellipse" => fanimate_core::Shape::new(
                shape.name.clone().unwrap_or_else(|| "Ellipse".into()),
                fanimate_core::Path::ellipse(min + size * 0.5, size * 0.5),
                style,
            ),
            other => {
                return Err(ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("a shape is a \"rectangle\" or an \"ellipse\", not \"{other}\""),
                ));
            }
        };
        self.edit("Add shape", |ed| {
            match ed.doc.find_mut(LayerId(layer)).map(|l| &mut l.kind) {
                Some(LayerKind::Vector { shapes }) => {
                    shapes.push(built);
                    Ok(shapes.len() - 1)
                }
                _ => Err(ApiError::no_layer(layer)),
            }
        })
    }

    /// Add an empty bone layer called `name`, on top of the layers beside it.
    /// Returns its id.
    pub fn add_bone_layer(&mut self, name: &str) -> Result<u32, ApiError> {
        let name = check_name(name, "a layer")?;
        self.edit("Add bone layer", |ed| {
            let mut layer = ed.doc.new_layer(
                name,
                LayerKind::Bone {
                    skeleton: Skeleton::default(),
                    children: vec![],
                },
            );
            layer.label = Some(fanimate_core::Label::for_id(layer.id));
            let id = layer.id;
            let at = ed.doc.layers.len();
            ed.doc.insert(None, at, layer);
            Ok(id.0)
        })
    }

    /// Add a bone to a bone layer: under `parent` (a bone id) or as a root,
    /// its joint at (`x`, `y`) in the parent's space (the layer's for a root)
    /// and `length` long, pointing along the parent's direction. Returns its id.
    pub fn add_bone(
        &mut self,
        layer: u32,
        name: &str,
        parent: Option<u32>,
        x: f32,
        y: f32,
        length: f32,
    ) -> Result<u32, ApiError> {
        let name = check_name(name, "a bone")?;
        if !(x.is_finite() && y.is_finite() && length.is_finite() && x.abs() < 1.0e6 && y.abs() < 1.0e6)
            || !(0.0..=1.0e6).contains(&length)
        {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "a bone needs a position and a length that are numbers, the length not negative",
            ));
        }
        let skeleton = self.skeleton(layer)?;
        if skeleton.bones.len() >= MAX_BONES {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("a bone layer has at most {MAX_BONES} bones"),
            ));
        }
        let parent_index = match parent {
            Some(id) => Some(
                skeleton
                    .index_of(id)
                    .ok_or_else(|| ApiError::no_bone(layer, id))?,
            ),
            None => None,
        };
        self.edit("Add bone", |ed| {
            match ed.doc.find_mut(LayerId(layer)).map(|l| &mut l.kind) {
                Some(LayerKind::Bone { skeleton, .. }) => {
                    let i = skeleton.add(name, parent_index, fanimate_core::Vec2::new(x, y), length, 0.0);
                    Ok(skeleton.bones[i].id)
                }
                _ => Err(ApiError::no_layer(layer)),
            }
        })
    }

    /// Turn a bone to `degrees` relative to its parent (clockwise on screen) on
    /// `frame` (the rest pose, frame 0, when none is given), by the keying rule.
    pub fn set_bone_angle(
        &mut self,
        layer: u32,
        bone: u32,
        degrees: f32,
        frame: Option<i32>,
    ) -> Result<(), ApiError> {
        let frame = frame.unwrap_or(0);
        if !degrees.is_finite() || !(0..=MAX_FRAME).contains(&frame) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("an angle must be a number, on a frame from 0 to {MAX_FRAME}"),
            ));
        }
        let index = self
            .skeleton(layer)?
            .index_of(bone)
            .ok_or_else(|| ApiError::no_bone(layer, bone))?;
        self.edit("Turn bone", |ed| {
            let keying = ed.keying();
            if let Some(LayerKind::Bone { skeleton, .. }) =
                ed.doc.find_mut(LayerId(layer)).map(|l| &mut l.kind)
            {
                skeleton.bones[index]
                    .angle
                    .write(frame, degrees.to_radians(), keying);
            }
            Ok(())
        })
    }

    /// Make bones follow their keys on a spring, thrown about by the bones
    /// above them, and write the swing as keys on every frame from `start` to
    /// `end` (the whole animation when left out), replacing their keys there.
    /// Bones are named by id; order does not matter. Returns how many keys
    /// were written.
    pub fn bake_spring(
        &mut self,
        layer: u32,
        bones: &[u32],
        settings: SpringSettings,
        start: Option<i32>,
        end: Option<i32>,
    ) -> Result<usize, ApiError> {
        self.skeleton(layer)?;
        let params = fanimate_core::physics::SpringParams {
            stiffness: settings
                .stiffness
                .unwrap_or(fanimate_core::physics::SpringParams::default().stiffness),
            damping: settings
                .damping
                .unwrap_or(fanimate_core::physics::SpringParams::default().damping),
            inertia: settings
                .inertia
                .unwrap_or(fanimate_core::physics::SpringParams::default().inertia),
            gravity: settings.gravity.unwrap_or(0.0),
            wind: settings.wind.unwrap_or(0.0),
        };
        if !params.is_sound() {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "the stiffness is 0 to 10000, the damping 0 to 1000, the inertia 0 to 10, the gravity 0 to 200 and the wind -200 to 200",
            ));
        }
        let (first, last) = (
            start.unwrap_or(self.editor.doc.start),
            end.unwrap_or(self.editor.doc.end),
        );
        self.edit("Bake spring motion", |ed| {
            ed.bake_spring(LayerId(layer), bones, params, first, last)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// The actions and poses of a bone layer, in order.
    pub fn actions(&self, layer: u32) -> Result<Vec<ActionInfo>, ApiError> {
        let skeleton = self.skeleton(layer)?;
        Ok(skeleton
            .actions
            .iter()
            .enumerate()
            .map(|(index, a)| ActionInfo {
                index,
                name: a.name.clone(),
                kind: if a.driver.is_some() {
                    "smart_bone"
                } else if a.is_pose() {
                    "pose"
                } else {
                    "action"
                },
                length: a.length,
            })
            .collect())
    }

    /// Save the rig of a bone layer as it is on `frame` (the rest pose, frame 0,
    /// when none is given) as a pose named `name` (or "Pose N" when left out).
    /// Returns the pose's index in `actions`.
    pub fn save_pose(
        &mut self,
        layer: u32,
        name: Option<&str>,
        frame: Option<i32>,
    ) -> Result<usize, ApiError> {
        self.skeleton(layer)?;
        let name = match name {
            Some(n) => check_name(n, "a pose")?,
            None => "",
        };
        let frame = frame.unwrap_or(0);
        if !(0..=MAX_FRAME).contains(&frame) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("a frame is from 0 (the rest pose) to {MAX_FRAME}"),
            ));
        }
        self.edit("Save pose", |ed| {
            let before = ed.frame;
            ed.frame = frame as f32;
            let made = ed.save_pose_named(LayerId(layer), name);
            ed.frame = before;
            made.map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Make the left-for-right mirror image of pose `index` as a new pose
    /// after it, by the bones' names (`arm L` swaps with `arm R`). Returns its
    /// index.
    pub fn mirror_pose(&mut self, layer: u32, index: usize) -> Result<usize, ApiError> {
        self.skeleton(layer)?;
        self.edit("Mirror pose", |ed| {
            ed.mirror_pose_at(LayerId(layer), index)
                .map(|(i, _)| i)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Put the keys of action or pose `index` into the timeline starting on
    /// `frame`. A Smart Bone action cannot be inserted: it plays by itself.
    pub fn insert_action(&mut self, layer: u32, index: usize, frame: i32) -> Result<(), ApiError> {
        self.skeleton(layer)?;
        if !(0..=MAX_FRAME).contains(&frame) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("a frame is from 0 to {MAX_FRAME}"),
            ));
        }
        let count = self.skeleton(layer)?.actions.len();
        if index >= count {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("layer {layer} has no action {index} (it has {count})"),
            ));
        }
        self.edit("Insert action", |ed| {
            ed.insert_action_on(LayerId(layer), index, frame)
                .map(|_| ())
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Import the text of an SVG file as layers, centred on the canvas: each
    /// top-level group becomes a vector layer, and `<text>` elements become text
    /// layers (with their font, size, colour, anchor and transform). `name`
    /// names the group the layers go in. Returns the new layer's id.
    pub fn import_svg(&mut self, svg: &str, name: &str) -> Result<u32, ApiError> {
        let name = check_name(name, "an import")?;
        self.edit("Import SVG file", |ed| {
            ed.import_svg(name, svg.as_bytes())
                .map(|id| id.0)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// The master controllers of a bone layer, with their values on `frame`
    /// (the rest pose, frame 0, when none is given).
    pub fn controllers(&self, layer: u32, frame: Option<i32>) -> Result<Vec<ControllerInfo>, ApiError> {
        let skeleton = self.skeleton(layer)?;
        let frame = frame.unwrap_or(0) as f32;
        Ok(skeleton
            .controllers
            .iter()
            .enumerate()
            .map(|(index, c)| ControllerInfo {
                index,
                name: c.name.clone(),
                kind: match c.kind {
                    fanimate_core::actions::ControllerKind::Slider => "slider",
                    fanimate_core::actions::ControllerKind::Pad => "pad",
                },
                x: c.value(fanimate_core::actions::ControllerAxis::X, frame),
                y: c.value(fanimate_core::actions::ControllerAxis::Y, frame),
                links: c
                    .links
                    .iter()
                    .map(|l| ControllerLinkInfo {
                        action: l.action,
                        axis: if l.axis == fanimate_core::actions::ControllerAxis::X {
                            "x"
                        } else {
                            "y"
                        },
                        from: l.from,
                        to: l.to,
                    })
                    .collect(),
            })
            .collect())
    }

    /// Add a master controller to a bone layer: `kind` is `"slider"` (one
    /// value) or `"pad"` (two). Returns its index.
    pub fn add_controller(&mut self, layer: u32, name: &str, kind: &str) -> Result<usize, ApiError> {
        let kind = match kind {
            "slider" => fanimate_core::actions::ControllerKind::Slider,
            "pad" => fanimate_core::actions::ControllerKind::Pad,
            other => {
                return Err(ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("a controller is a \"slider\" or a \"pad\", not \"{other}\""),
                ));
            }
        };
        self.skeleton(layer)?;
        self.edit("Add controller", |ed| {
            ed.add_controller(LayerId(layer), name, kind)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Every keyed curve of a layer: the transform, opacity and layer order,
    /// and the bones' angles, one curve per number (a position is two).
    pub fn curves(&self, layer: u32) -> Result<Vec<CurveInfo>, ApiError> {
        self.require(layer)?;
        let id = LayerId(layer);
        let mut out = Vec::new();
        for (ch, n) in self.editor.graph_channels(id) {
            let Some(name) = self.channel_name(layer, ch) else {
                continue;
            };
            let scale = fanimate_core::graph::display_scale(ch);
            for component in 0..n {
                let keys = self.editor.curve_keys(id, ch, component);
                if keys.is_empty() {
                    continue;
                }
                out.push(CurveInfo {
                    channel: name.clone(),
                    component,
                    keys: keys
                        .into_iter()
                        .map(|k| KeyInfo {
                            frame: k.frame,
                            value: k.value * scale,
                            interp: serde_json::to_value(k.interp).unwrap_or(serde_json::Value::Null),
                        })
                        .collect(),
                });
            }
        }
        Ok(out)
    }

    /// Move the key on `frame` of a property to `to_frame` and set curve
    /// `component` of it to `value` (in the units `curves` reports). The key
    /// keeps to the room between its neighbours; the frame it ended on is
    /// returned.
    pub fn move_key(
        &mut self,
        layer: u32,
        channel: &str,
        component: usize,
        frame: i32,
        to_frame: i32,
        value: f32,
    ) -> Result<i32, ApiError> {
        self.require(layer)?;
        let ch = self.channel_of(layer, channel)?;
        let scale = fanimate_core::graph::display_scale(ch);
        if !value.is_finite() || !(-MAX_FRAME..=MAX_FRAME).contains(&to_frame) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("a value must be a number, on a frame within {MAX_FRAME} of 0"),
            ));
        }
        self.edit("Move key", |ed| {
            ed.change_key(LayerId(layer), ch, component, frame, to_frame, value / scale)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Change how the curve leaves the key on `frame` of a property:
    /// `interp` is written as in the project file (`"smooth"`, `"linear"`,
    /// `{"ease":"overshoot"}`, `{"bezier":[x1, y1, x2, y2]}` in thousandths).
    /// Returns whether it changed.
    pub fn set_key_interpolation(
        &mut self,
        layer: u32,
        channel: &str,
        frame: i32,
        interp: serde_json::Value,
    ) -> Result<bool, ApiError> {
        self.require(layer)?;
        let ch = self.channel_of(layer, channel)?;
        let interp: fanimate_core::Interpolation = serde_json::from_value(interp).map_err(|e| {
            ApiError::new(
                ErrorCode::InvalidArgument,
                format!("that is not an interpolation: {e}"),
            )
        })?;
        if let fanimate_core::Interpolation::Bezier(b) = interp
            && !b.is_sound()
        {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "a curve's x values are 0 to 1000 and its y values -2000 to 3000",
            ));
        }
        self.edit("Change key curve", |ed| {
            ed.change_key_curve(LayerId(layer), ch, frame, interp)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// The property a name stands for on a layer (see [`CurveInfo::channel`]).
    fn channel_of(&self, layer: u32, name: &str) -> Result<fanimate_core::ChannelId, ApiError> {
        use fanimate_core::ChannelId;
        Ok(match name {
            "position" => ChannelId::Position,
            "rotation" => ChannelId::Rotation,
            "scale" => ChannelId::Scale,
            "opacity" => ChannelId::Opacity,
            "order" => ChannelId::Order,
            other => {
                let bad = || {
                    ApiError::new(
                        ErrorCode::InvalidArgument,
                        format!(
                            "\"{other}\" is not a property: use position, rotation, scale, opacity, order or bone:<id>"
                        ),
                    )
                };
                let id: u32 = other
                    .strip_prefix("bone:")
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(bad)?;
                let index = self
                    .skeleton(layer)?
                    .index_of(id)
                    .ok_or_else(|| ApiError::no_bone(layer, id))?;
                ChannelId::Bone(index)
            }
        })
    }

    /// The name of a property on a layer, or `None` for one the API does not name.
    fn channel_name(&self, layer: u32, ch: fanimate_core::ChannelId) -> Option<String> {
        use fanimate_core::ChannelId;
        Some(match ch {
            ChannelId::Position => "position".to_owned(),
            ChannelId::Rotation => "rotation".to_owned(),
            ChannelId::Scale => "scale".to_owned(),
            ChannelId::Opacity => "opacity".to_owned(),
            ChannelId::Order => "order".to_owned(),
            ChannelId::Bone(i) => format!("bone:{}", self.skeleton(layer).ok()?.bones.get(i)?.id),
            _ => return None,
        })
    }

    /// Rename a master controller (1 to 200 characters).
    pub fn rename_controller(&mut self, layer: u32, index: usize, name: &str) -> Result<(), ApiError> {
        let name = check_name(name, "a controller")?;
        self.skeleton(layer)?;
        self.edit("Rename controller", |ed| {
            ed.rename_controller(LayerId(layer), index, name)
                .map(|_| ())
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Remove a master controller.
    pub fn remove_controller(&mut self, layer: u32, index: usize) -> Result<(), ApiError> {
        self.skeleton(layer)?;
        self.edit("Remove controller", |ed| {
            ed.remove_controller(LayerId(layer), index)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Make controller `controller` play action `action` (an index in
    /// `actions`) as its `axis` (`"x"` or `"y"`) goes from `from` (the action
    /// on its first frame) to `to` (its last). Linking the same action and
    /// axis again replaces the link.
    pub fn link_controller(
        &mut self,
        layer: u32,
        controller: usize,
        action: usize,
        axis: &str,
        from: f32,
        to: f32,
    ) -> Result<(), ApiError> {
        let axis = axis_of(axis)?;
        self.skeleton(layer)?;
        self.edit("Link controller", |ed| {
            ed.link_controller(LayerId(layer), controller, action, axis, from, to)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Take away a controller's hold on an action. Returns whether it had one.
    pub fn unlink_controller(
        &mut self,
        layer: u32,
        controller: usize,
        action: usize,
    ) -> Result<bool, ApiError> {
        self.skeleton(layer)?;
        self.edit("Unlink controller", |ed| {
            ed.unlink_controller(LayerId(layer), controller, action)
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Set one value of a controller, 0 to 1, on `frame` (the rest pose when
    /// none is given) by the keying rule.
    pub fn set_controller_value(
        &mut self,
        layer: u32,
        controller: usize,
        axis: &str,
        value: f32,
        frame: Option<i32>,
    ) -> Result<(), ApiError> {
        let axis = axis_of(axis)?;
        self.skeleton(layer)?;
        let frame = frame.unwrap_or(0);
        if !(0..=MAX_FRAME).contains(&frame) || !value.is_finite() {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("a value is a number, on a frame from 0 (the rest pose) to {MAX_FRAME}"),
            ));
        }
        self.edit("Set controller", |ed| {
            let before = ed.frame;
            ed.frame = frame as f32;
            let result = ed.set_controller_value(LayerId(layer), controller, axis, value);
            ed.frame = before;
            result
                .map(|_| ())
                .map_err(|e| ApiError::new(ErrorCode::InvalidArgument, e))
        })
    }

    /// Undo the last step. Returns whether there was one to undo; refused
    /// while the animation plays.
    pub fn undo(&mut self) -> Result<bool, ApiError> {
        self.history_step(fanimate_editor::Action::Undo)
    }

    /// Redo the last undone step. Returns whether there was one.
    pub fn redo(&mut self) -> Result<bool, ApiError> {
        self.history_step(fanimate_editor::Action::Redo)
    }

    fn history_step(&mut self, action: fanimate_editor::Action) -> Result<bool, ApiError> {
        if self.editor.playing {
            return Err(ApiError::new(ErrorCode::Playing, fanimate_editor::PAUSE_TO_EDIT));
        }
        if !self.editor.can_run(action) {
            return Ok(false);
        }
        self.editor.run(action);
        Ok(true)
    }

    /// Set one value of a text animator on `frame` (the rest pose, frame 0, when
    /// none is given), by the keying rule. `property` is one of
    /// `"range_start"`, `"range_end"`, `"range_offset"` (percent of the text),
    /// `"position"` and `"anchor"` (a pair of document units), `"rotation"`
    /// (degrees), `"scale"` (a pair, 1 is no change), `"opacity"` (a multiplier,
    /// 1 is no change), `"tracking"` (thousandths of the size),
    /// `"stroke_width"` (added to the outline), and `"fill"` and
    /// `"stroke_color"` (colours like `#RRGGBB`).
    pub fn set_text_animator_value(
        &mut self,
        layer: u32,
        index: usize,
        property: &str,
        value: AnimatorValue,
        frame: Option<i32>,
    ) -> Result<(), ApiError> {
        use fanimate_editor::{AnimatorProp as P, PropValue as V};
        self.require_text(layer)?;
        let frame = frame.unwrap_or(0);
        if !(0..=MAX_FRAME).contains(&frame) {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                format!("a frame is from 0 (the rest pose) to {MAX_FRAME}"),
            ));
        }
        let count = self.editor.text(LayerId(layer)).map_or(0, |t| t.animators.len());
        if index >= count {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                format!("layer {layer} has no animator {index} (it has {count})"),
            ));
        }
        let (prop, kind) = match property {
            "range_start" => (P::RangeStart, 'n'),
            "range_end" => (P::RangeEnd, 'n'),
            "range_offset" => (P::RangeOffset, 'n'),
            "position" => (P::Position, 'p'),
            "anchor" => (P::Anchor, 'p'),
            "rotation" => (P::Rotation, 'r'),
            "scale" => (P::Scale, 'p'),
            "opacity" => (P::Opacity, 'n'),
            "tracking" => (P::Tracking, 'n'),
            "stroke_width" => (P::StrokeWidth, 'n'),
            "fill" => (P::Fill, 'c'),
            "stroke_color" => (P::StrokeColour, 'c'),
            other => {
                return Err(ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("a text animator has no property \"{other}\""),
                ));
            }
        };
        let wrong = || {
            ApiError::new(
                ErrorCode::InvalidArgument,
                format!(
                    "\"{property}\" takes {}",
                    match kind {
                        'p' => "a pair of numbers, [x, y]",
                        'c' => "a colour like \"#RRGGBB\"",
                        _ => "a number",
                    }
                ),
            )
        };
        let value = match (kind, value) {
            ('n', AnimatorValue::Number(n)) => V::Number(n),
            ('r', AnimatorValue::Number(degrees)) => V::Number(degrees.to_radians()),
            ('p', AnimatorValue::Pair([x, y])) => V::Pair(fanimate_core::Vec2::new(x, y)),
            ('c', AnimatorValue::Colour(hex)) => {
                V::Colour(fanimate_core::Color::from_hex(&hex).map_err(|_| {
                    ApiError::new(
                        ErrorCode::InvalidArgument,
                        format!("\"{hex}\" is not a colour like #RRGGBB or #RRGGBBAA"),
                    )
                })?)
            }
            _ => return Err(wrong()),
        };
        let finite = match value {
            V::Number(n) => n.is_finite(),
            V::Pair(p) => p.is_finite(),
            V::Colour(_) => true,
        };
        if !finite {
            return Err(wrong());
        }
        self.edit("Set text animator value", |ed| {
            let before = ed.frame;
            ed.frame = frame as f32;
            ed.set_animator_value(LayerId(layer), index, prop, value);
            ed.frame = before;
            Ok(())
        })
    }

    /// Make a text layer run along the first shape of a vector layer, as it
    /// stands on the playhead frame. The path is copied, and stays where it
    /// is on the canvas.
    pub fn set_text_path(&mut self, layer: u32, shape_layer: u32) -> Result<(), ApiError> {
        self.require_text(layer)?;
        self.require(shape_layer)?;
        self.edit("Set text path", |ed| {
            if ed.set_text_path_from(LayerId(layer), LayerId(shape_layer)) {
                Ok(())
            } else {
                Err(ApiError::new(
                    ErrorCode::InvalidArgument,
                    format!("layer {shape_layer} has no shape the text can follow"),
                ))
            }
        })
    }

    /// Set a text layer straight again. Nothing to do is not an error.
    pub fn clear_text_path(&mut self, layer: u32) -> Result<(), ApiError> {
        self.require_text(layer)?;
        self.edit("Remove text path", |ed| {
            ed.clear_text_path(LayerId(layer));
            Ok(())
        })
    }

    /// Where along its path the text starts, as a percentage of the path's
    /// length, on the playhead frame (keyed like any animated value).
    pub fn set_text_path_offset(&mut self, layer: u32, percent: f32) -> Result<(), ApiError> {
        self.require_text(layer)?;
        if !percent.is_finite() {
            return Err(ApiError::new(
                ErrorCode::InvalidArgument,
                "an offset must be a number",
            ));
        }
        self.edit("Set text path offset", |ed| {
            ed.set_text_path_offset(LayerId(layer), percent);
            Ok(())
        })
    }

    // ------------------------------------------------------------------

    fn skeleton(&self, layer: u32) -> Result<&Skeleton, ApiError> {
        match self.editor.doc.find(LayerId(layer)).map(|l| &l.kind) {
            Some(LayerKind::Bone { skeleton, .. }) => Ok(skeleton),
            Some(_) => Err(ApiError::new(
                ErrorCode::NotFound,
                format!("layer {layer} is not a bone layer"),
            )),
            None => Err(ApiError::no_layer(layer)),
        }
    }

    fn require(&self, id: u32) -> Result<(), ApiError> {
        match self.editor.doc.find(LayerId(id)) {
            Some(_) => Ok(()),
            None => Err(ApiError::no_layer(id)),
        }
    }

    fn not_text(&self, layer: u32) -> ApiError {
        match self.editor.doc.find(LayerId(layer)) {
            Some(_) => ApiError::new(ErrorCode::NotFound, format!("layer {layer} is not a text layer")),
            None => ApiError::no_layer(layer),
        }
    }

    fn require_text(&self, layer: u32) -> Result<(), ApiError> {
        match self.editor.text(LayerId(layer)) {
            Some(_) => Ok(()),
            None => Err(self.not_text(layer)),
        }
    }

    fn edit<T>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Editor) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        self.editor.transaction(label, f).map_err(|e| match e {
            TxError::Playing => ApiError::new(ErrorCode::Playing, fanimate_editor::PAUSE_TO_EDIT),
            TxError::Failed(e) => e,
        })
    }
}

fn collect(layer: &Layer, parent: Option<u32>, out: &mut Vec<LayerInfo>) {
    let children = layer.kind.children().map_or(&[][..], Vec::as_slice);
    out.push(LayerInfo {
        id: layer.id.0,
        name: layer.name.clone(),
        kind: layer.kind.type_name(),
        parent,
        visible: layer.visible,
        locked: layer.locked,
        children: children.len(),
        provenance: layer.provenance.clone(),
    });
    for child in children {
        collect(child, Some(layer.id.0), out);
    }
}
