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
pub mod color;
pub mod eval;
pub mod frames;
pub mod health;
pub mod media;
pub mod path;
pub mod points;
pub mod samples;
pub mod scene;
pub mod transform;

pub use actions::RigAction;
pub use anim::{Channel, Frame, Interpolation, Key, Keying};
pub use bones::{Bone, Skeleton};
pub use boolean::BoolOp;
pub use brush::{BrushOptions, BrushSample, Stabiliser};
pub use color::Color;
pub use eval::{CAMERA_ZOOM, DrawItem, ImageItem, camera_at, camera_frame, evaluate};
pub use frames::{Cel, Part};
pub use glam::{Affine2, Vec2};
pub use media::{Audio, Image, Media, MediaFile};
pub use path::{Path, PathCmd, RectParams, Shape, Stroke, Style};
pub use scene::{Camera, ChannelId, Document, Guide, Label, Layer, LayerId, LayerKind};
pub use transform::{AnimTransform, Transform};
