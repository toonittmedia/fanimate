//! Fanimate core engine.
//!
//! The scene graph, animation curves, bones and frame evaluation. This crate
//! has no UI or GPU code, so the same engine runs in the desktop app, in the
//! browser and headless on a render server.
//!
//! The main entry points are [`Document`], which holds a project, and
//! [`eval::evaluate`], which turns a document at a given frame into a flat
//! list of shapes ready to draw.

pub mod actions;
pub mod anim;
pub mod bones;
pub mod boolean;
pub mod brush;
pub mod captions;
pub mod color;
pub mod easing;
pub mod eval;
pub mod expr;
pub mod frames;
pub mod graph;
pub mod health;
mod hide_edge;
pub mod media;
pub mod path;
pub mod physics;
pub mod points;
pub mod policy;
pub mod poses;
pub mod provenance;
pub mod retarget;
pub mod samples;
pub mod scene;
mod shift;
pub mod stateful;
pub mod styles;
pub mod text;
pub mod transform;
pub mod trim;

pub use actions::RigAction;
pub use anim::{Channel, Frame, Interpolation, Key, Keying, LoopMode, REST_FRAME};
pub use bones::{Bone, Skeleton};
pub use boolean::BoolOp;
pub use brush::{BrushOptions, BrushSample, Stabiliser};
pub use color::Color;
pub use easing::{Bezier, Easing};
pub use eval::{CAMERA_ZOOM, DrawItem, ImageItem, camera_at, camera_frame, evaluate};
pub use frames::{Cel, Part};
pub use glam::{Affine2, Vec2};
pub use media::{Audio, Image, Media, MediaFile};
pub use path::{Path, PathCmd, RectParams, Shape, Stroke, Style};
pub use policy::Policy;
pub use provenance::{MAX_PROVENANCE_TEXT, Provenance};
pub use scene::{Camera, ChannelId, Document, Guide, Label, Layer, LayerId, LayerKind};
pub use styles::{MAX_STYLE_NAME, MAX_STYLES, StylePreset};
pub use text::{
    Animator, Look, Selector, SelectorShape, Text, TextAlign, TextDirection, TextFeature, TextPreset,
    TextUnit,
};
pub use transform::{AnimTransform, Transform};
