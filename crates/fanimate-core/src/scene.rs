//! The scene graph: a document holds a tree of layers.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::Color;
use crate::anim::{Channel, Frame, KeyList};
use crate::bones::Skeleton;
use crate::path::Shape;
use crate::transform::AnimTransform;

/// Stable identifier for a layer, unique within a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u32);

/// A guide: a horizontal or vertical line at a fixed position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    /// A vertical line at `position` on the x axis, or else a horizontal
    /// line at `position` on the y axis.
    pub vertical: bool,
    /// Position in document units.
    pub position: f32,
    /// Locked guides cannot be dragged.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
}

/// The camera: what renders look at the canvas through (Moho's camera).
///
/// It is part of the project, not a layer. It starts over the middle of the
/// canvas, seeing all of it; its `position` moves it, `zoom` brings it closer
/// (2 shows half the canvas, twice as big; 1 is all of it) and `roll` turns it
/// (radians, positive is clockwise). Each is animated like any channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub position: Channel<Vec2>,
    pub zoom: Channel<f32>,
    pub roll: Channel<f32>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: Channel::new(Vec2::ZERO),
            zoom: Channel::new(1.0),
            roll: Channel::new(0.0),
        }
    }
}

impl Camera {
    /// Whether it still sees the whole canvas and has no keys, so a project
    /// that never used it writes nothing about it.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether any of its channels has keys.
    pub fn is_animated(&self) -> bool {
        self.position.is_animated() || self.zoom.is_animated() || self.roll.is_animated()
    }

    /// The keys of one of the camera's channels (`None` for any other).
    pub fn keys(&self, ch: ChannelId) -> Option<&dyn KeyList> {
        match ch {
            ChannelId::CameraPosition => Some(&self.position.keys),
            ChannelId::CameraZoom => Some(&self.zoom.keys),
            ChannelId::CameraRoll => Some(&self.roll.keys),
            _ => None,
        }
    }

    pub fn keys_mut(&mut self, ch: ChannelId) -> Option<&mut dyn KeyList> {
        match ch {
            ChannelId::CameraPosition => Some(&mut self.position.keys),
            ChannelId::CameraZoom => Some(&mut self.zoom.keys),
            ChannelId::CameraRoll => Some(&mut self.roll.keys),
            _ => None,
        }
    }

    /// Frames with a key in any channel, sorted.
    pub fn key_frames(&self) -> Vec<Frame> {
        let mut frames = Vec::new();
        for ch in ChannelId::CAMERA {
            if let Some(keys) = self.keys(ch) {
                keys.frames_into(&mut frames);
            }
        }
        frames.sort_unstable();
        frames.dedup();
        frames
    }
}

/// A Fanimate project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub name: String,
    /// Canvas size in document units (pixels at 100% zoom).
    pub width: f32,
    pub height: f32,
    pub fps: f32,
    /// Inclusive frame range that plays and renders.
    pub start: Frame,
    pub end: Frame,
    pub background: Color,
    /// Top-level layers, drawn first to last (last is on top).
    pub layers: Vec<Layer>,
    /// Next unused layer id.
    pub next_layer_id: u32,
    /// Version 5: guides placed from the rulers, saved with the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Version 5: the camera renders look through.
    #[serde(default, skip_serializing_if = "Camera::is_default")]
    pub camera: Camera,
    /// The action being edited, if any (see [`crate::actions`]). Never in
    /// a saved project: saving writes [`Document::mainline`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_edit: Option<crate::actions::ActionEdit>,
    /// Image files used by image layers (see [`crate::media`]). Saved as
    /// files beside the layers, not in the document's own data.
    #[serde(skip)]
    pub media: crate::media::Media,
}

impl Document {
    pub fn new(name: impl Into<String>, width: f32, height: f32, fps: f32) -> Self {
        Self {
            name: name.into(),
            width,
            height,
            fps,
            start: 0,
            end: (fps * 4.0) as Frame - 1,
            background: Color::WHITE,
            layers: Vec::new(),
            next_layer_id: 1,
            guides: Vec::new(),
            camera: Camera::default(),
            action_edit: None,
            media: Default::default(),
        }
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    pub fn frame_count(&self) -> u32 {
        (self.end - self.start + 1).max(0) as u32
    }

    /// Allocate a fresh layer id.
    pub fn new_layer_id(&mut self) -> LayerId {
        let id = LayerId(self.next_layer_id);
        // the validator keeps ids far below this; saturating means a
        // damaged count can never crash the editor
        self.next_layer_id = self.next_layer_id.saturating_add(1);
        id
    }

    /// Create a layer with a fresh id (not yet inserted anywhere).
    pub fn new_layer(&mut self, name: impl Into<String>, kind: LayerKind) -> Layer {
        Layer::new(self.new_layer_id(), name, kind)
    }

    /// Visit every layer in the tree, depth first, parents before children.
    pub fn walk(&self, mut f: impl FnMut(&Layer)) {
        fn go(layers: &[Layer], f: &mut impl FnMut(&Layer)) {
            for l in layers {
                f(l);
                if let Some(children) = l.kind.children() {
                    go(children, f);
                }
            }
        }
        go(&self.layers, &mut f);
    }

    /// Mutable version of [`walk`](Self::walk).
    pub fn walk_mut(&mut self, mut f: impl FnMut(&mut Layer)) {
        fn go(layers: &mut [Layer], f: &mut impl FnMut(&mut Layer)) {
            for l in layers {
                f(l);
                if let Some(children) = l.kind.children_mut() {
                    go(children, f);
                }
            }
        }
        go(&mut self.layers, &mut f);
    }

    pub fn layer_count(&self) -> usize {
        let mut n = 0;
        self.walk(|_| n += 1);
        n
    }

    pub fn find(&self, id: LayerId) -> Option<&Layer> {
        fn go(layers: &[Layer], id: LayerId) -> Option<&Layer> {
            for l in layers {
                if l.id == id {
                    return Some(l);
                }
                if let Some(found) = l.kind.children().and_then(|c| go(c, id)) {
                    return Some(found);
                }
            }
            None
        }
        go(&self.layers, id)
    }

    pub fn find_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        fn go(layers: &mut [Layer], id: LayerId) -> Option<&mut Layer> {
            for l in layers {
                if l.id == id {
                    return Some(l);
                }
                if let Some(found) = l.kind.children_mut().and_then(|c| go(c, id)) {
                    return Some(found);
                }
            }
            None
        }
        go(&mut self.layers, id)
    }

    /// Where a layer sits: its parent (`None` for top level) and its index
    /// in that parent's list.
    pub fn locate(&self, id: LayerId) -> Option<(Option<LayerId>, usize)> {
        fn go(layers: &[Layer], parent: Option<LayerId>, id: LayerId) -> Option<(Option<LayerId>, usize)> {
            for (i, l) in layers.iter().enumerate() {
                if l.id == id {
                    return Some((parent, i));
                }
                if let Some(found) = l.kind.children().and_then(|c| go(c, Some(l.id), id)) {
                    return Some(found);
                }
            }
            None
        }
        go(&self.layers, None, id)
    }

    /// The list that holds the children of `parent` (`None`: top level).
    pub fn siblings_mut(&mut self, parent: Option<LayerId>) -> Option<&mut Vec<Layer>> {
        match parent {
            None => Some(&mut self.layers),
            Some(p) => self.find_mut(p)?.kind.children_mut(),
        }
    }

    /// Remove a layer (with its children) from the tree.
    pub fn remove(&mut self, id: LayerId) -> Option<Layer> {
        let (parent, index) = self.locate(id)?;
        Some(self.siblings_mut(parent)?.remove(index))
    }

    /// Insert a layer into `parent` at `index` (clamped to the end).
    /// Returns false if `parent` does not exist or cannot hold layers.
    pub fn insert(&mut self, parent: Option<LayerId>, index: usize, layer: Layer) -> bool {
        match self.siblings_mut(parent) {
            Some(list) => {
                let index = index.min(list.len());
                list.insert(index, layer);
                true
            }
            None => false,
        }
    }

    /// Whether `id` is `ancestor` or inside it.
    pub fn is_within(&self, id: LayerId, ancestor: LayerId) -> bool {
        let mut current = Some(id);
        while let Some(c) = current {
            if c == ancestor {
                return true;
            }
            current = self.locate(c).and_then(|(p, _)| p);
        }
        false
    }

    /// Give `layer` and everything inside it fresh ids from this document,
    /// for pasting or duplicating.
    pub fn reassign_ids(&mut self, layer: &mut Layer) {
        layer.id = self.new_layer_id();
        if let Some(children) = layer.kind.children_mut() {
            for child in children {
                self.reassign_ids(child);
            }
        }
    }
}

/// A layer's label colour, as in the Layers panel and on its timeline row,
/// so related layers are easy to find. The colours themselves come from the
/// brand tokens; only the name is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    Red,
    Orange,
    Yellow,
    Lime,
    Green,
    Teal,
    Cyan,
    Blue,
    Violet,
    Pink,
}

impl Label {
    pub const ALL: [Label; 10] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Lime,
        Self::Green,
        Self::Teal,
        Self::Cyan,
        Self::Blue,
        Self::Violet,
        Self::Pink,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Lime => "Lime",
            Self::Green => "Green",
            Self::Teal => "Teal",
            Self::Cyan => "Cyan",
            Self::Blue => "Blue",
            Self::Violet => "Violet",
            Self::Pink => "Pink",
        }
    }

    /// The label new layer `id` gets: the colours in turn, so neighbouring
    /// layers differ. Also the colour shown for layers saved without one.
    pub fn for_id(id: LayerId) -> Self {
        Self::ALL[id.0 as usize % Self::ALL.len()]
    }
}

/// Reads a label leniently: a label this version does not know (from a
/// newer Fanimate) becomes no label, rather than failing the whole load.
fn lenient_label<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Label>, D::Error> {
    let name = Option::<String>::deserialize(d)?;
    Ok(name.and_then(|n| Label::ALL.into_iter().find(|l| l.name().eq_ignore_ascii_case(&n))))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    /// Label colour. Layers saved before labels existed have none, and are
    /// shown with [`Label::for_id`].
    #[serde(
        default,
        deserialize_with = "lenient_label",
        skip_serializing_if = "Option::is_none"
    )]
    pub label: Option<Label>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub visible: bool,
    /// Version 5: a locked layer cannot be selected on the canvas, moved, drawn
    /// on or edited by the tools. It still shows, plays and can be unlocked.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    pub opacity: Channel<f32>,
    pub transform: AnimTransform,
    #[serde(flatten)]
    pub kind: LayerKind,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Layer {
    pub fn new(id: LayerId, name: impl Into<String>, kind: LayerKind) -> Self {
        Self {
            id,
            name: name.into(),
            label: None,
            visible: true,
            locked: false,
            opacity: Channel::new(1.0),
            transform: AnimTransform::default(),
            kind,
        }
    }

    /// The label colour to show: the layer's own, or the one its id picks.
    pub fn shown_label(&self) -> Label {
        self.label.unwrap_or_else(|| Label::for_id(self.id))
    }

    pub fn with_transform(mut self, transform: AnimTransform) -> Self {
        self.transform = transform;
        self
    }

    /// Every list of keys the layer holds itself: opacity, the transform,
    /// a bone layer's bone angles and a vector layer's point keys. Not its
    /// children's, which are their own layers on the timeline.
    pub fn key_lists(&self, mut f: impl FnMut(&dyn KeyList)) {
        f(&self.opacity.keys);
        f(&self.transform.position.keys);
        f(&self.transform.rotation.keys);
        f(&self.transform.scale.keys);
        match &self.kind {
            LayerKind::Vector { shapes } => shapes.iter().for_each(|s| {
                f(&s.path_keys);
                f(&s.fill_keys);
                f(&s.stroke_keys);
            }),
            LayerKind::Frames { cels } => {
                f(cels);
                cels.iter().for_each(|c| {
                    c.shapes().for_each(|(_, _, s)| {
                        f(&s.path_keys);
                        f(&s.fill_keys);
                        f(&s.stroke_keys);
                    })
                });
            }
            LayerKind::Bone { skeleton, .. } => skeleton.bones.iter().for_each(|b| f(&b.angle.keys)),
            LayerKind::Switch { switch, .. } => f(switch),
            LayerKind::Group { .. } | LayerKind::Image(_) | LayerKind::Audio(_) => {}
        }
    }

    /// Mutable version of [`key_lists`](Self::key_lists).
    pub fn key_lists_mut(&mut self, mut f: impl FnMut(&mut dyn KeyList)) {
        f(&mut self.opacity.keys);
        f(&mut self.transform.position.keys);
        f(&mut self.transform.rotation.keys);
        f(&mut self.transform.scale.keys);
        match &mut self.kind {
            LayerKind::Vector { shapes } => shapes.iter_mut().for_each(|s| {
                f(&mut s.path_keys);
                f(&mut s.fill_keys);
                f(&mut s.stroke_keys);
            }),
            LayerKind::Frames { cels } => {
                for c in cels.iter_mut() {
                    c.shapes_mut().for_each(|s| {
                        f(&mut s.path_keys);
                        f(&mut s.fill_keys);
                        f(&mut s.stroke_keys);
                    });
                }
                f(cels);
            }
            LayerKind::Bone { skeleton, .. } => skeleton.bones.iter_mut().for_each(|b| f(&mut b.angle.keys)),
            LayerKind::Switch { switch, .. } => f(switch),
            LayerKind::Group { .. } | LayerKind::Image(_) | LayerKind::Audio(_) => {}
        }
    }

    /// Frames with a key in any of the layer's own key lists, sorted.
    pub fn key_frames(&self) -> Vec<Frame> {
        let mut frames = Vec::new();
        self.key_lists(|k| k.frames_into(&mut frames));
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    /// The properties the timeline can show a row of keys for: the transform
    /// and opacity of every layer, a bone layer's bones, a switch layer's
    /// choice, and the shapes of a vector layer that have point keys.
    pub fn channels(&self) -> Vec<ChannelId> {
        let mut v = vec![
            ChannelId::Position,
            ChannelId::Rotation,
            ChannelId::Scale,
            ChannelId::Opacity,
        ];
        match &self.kind {
            LayerKind::Vector { shapes } => {
                for (i, s) in shapes.iter().enumerate() {
                    if s.is_animated() {
                        v.push(ChannelId::Shape(i));
                    }
                    if !s.fill_keys.is_empty() {
                        v.push(ChannelId::FillColor(i));
                    }
                    if !s.stroke_keys.is_empty() {
                        v.push(ChannelId::StrokeColor(i));
                    }
                }
            }
            LayerKind::Bone { skeleton, .. } => v.extend((0..skeleton.bones.len()).map(ChannelId::Bone)),
            LayerKind::Switch { .. } => v.push(ChannelId::Switch),
            LayerKind::Group { .. }
            | LayerKind::Frames { .. }
            | LayerKind::Image(_)
            | LayerKind::Audio(_) => {}
        }
        v
    }

    /// What a property row is called.
    pub fn channel_label(&self, ch: ChannelId) -> String {
        match (ch, &self.kind) {
            (ChannelId::Position, _) => "Position".into(),
            (ChannelId::Rotation, _) => "Rotation".into(),
            (ChannelId::Scale, _) => "Scale".into(),
            (ChannelId::Opacity, _) => "Opacity".into(),
            (ChannelId::Switch, _) => "Shows".into(),
            (ChannelId::Bone(i), LayerKind::Bone { skeleton, .. }) => skeleton
                .bones
                .get(i)
                .map_or_else(|| format!("Bone {}", i + 1), |b| b.name.clone()),
            (ChannelId::Shape(i), LayerKind::Vector { shapes }) => match shapes.get(i) {
                Some(s) if !s.name.is_empty() => format!("Points: {}", s.name),
                _ => format!("Points: shape {}", i + 1),
            },
            (ChannelId::FillColor(i), LayerKind::Vector { shapes }) => match shapes.get(i) {
                Some(s) if !s.name.is_empty() => format!("Fill: {}", s.name),
                _ => format!("Fill: shape {}", i + 1),
            },
            (ChannelId::StrokeColor(i), LayerKind::Vector { shapes }) => match shapes.get(i) {
                Some(s) if !s.name.is_empty() => format!("Stroke: {}", s.name),
                _ => format!("Stroke: shape {}", i + 1),
            },
            (ChannelId::CameraPosition | ChannelId::CameraZoom | ChannelId::CameraRoll, _) => {
                ch.camera_label().to_owned()
            }
            (ChannelId::FillColor(i), _) => format!("Fill: shape {}", i + 1),
            (ChannelId::StrokeColor(i), _) => format!("Stroke: shape {}", i + 1),
            (ChannelId::Bone(i), _) => format!("Bone {}", i + 1),
            (ChannelId::Shape(i), _) => format!("Points: shape {}", i + 1),
        }
    }

    /// The keys of one property, if the layer has it.
    pub fn channel_keys(&self, ch: ChannelId) -> Option<&dyn KeyList> {
        Some(match (ch, &self.kind) {
            (ChannelId::Position, _) => &self.transform.position.keys,
            (ChannelId::Rotation, _) => &self.transform.rotation.keys,
            (ChannelId::Scale, _) => &self.transform.scale.keys,
            (ChannelId::Opacity, _) => &self.opacity.keys,
            (ChannelId::Bone(i), LayerKind::Bone { skeleton, .. }) => &skeleton.bones.get(i)?.angle.keys,
            (ChannelId::Shape(i), LayerKind::Vector { shapes }) => &shapes.get(i)?.path_keys,
            (ChannelId::FillColor(i), LayerKind::Vector { shapes }) => &shapes.get(i)?.fill_keys,
            (ChannelId::StrokeColor(i), LayerKind::Vector { shapes }) => &shapes.get(i)?.stroke_keys,
            (ChannelId::Switch, LayerKind::Switch { switch, .. }) => switch,
            _ => return None,
        })
    }

    /// Mutable version of [`channel_keys`](Self::channel_keys).
    pub fn channel_keys_mut(&mut self, ch: ChannelId) -> Option<&mut dyn KeyList> {
        Some(match (ch, &mut self.kind) {
            (ChannelId::Position, _) => &mut self.transform.position.keys,
            (ChannelId::Rotation, _) => &mut self.transform.rotation.keys,
            (ChannelId::Scale, _) => &mut self.transform.scale.keys,
            (ChannelId::Opacity, _) => &mut self.opacity.keys,
            (ChannelId::Bone(i), LayerKind::Bone { skeleton, .. }) => {
                &mut skeleton.bones.get_mut(i)?.angle.keys
            }
            (ChannelId::Shape(i), LayerKind::Vector { shapes }) => &mut shapes.get_mut(i)?.path_keys,
            (ChannelId::FillColor(i), LayerKind::Vector { shapes }) => &mut shapes.get_mut(i)?.fill_keys,
            (ChannelId::StrokeColor(i), LayerKind::Vector { shapes }) => &mut shapes.get_mut(i)?.stroke_keys,
            (ChannelId::Switch, LayerKind::Switch { switch, .. }) => switch,
            _ => return None,
        })
    }
}

/// One animatable property of a layer: a row on the timeline, and a key
/// button in the Properties panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelId {
    Opacity,
    Position,
    Rotation,
    Scale,
    /// A bone's angle, by its index in the skeleton.
    Bone(usize),
    /// A vector shape's point keys, by its index.
    Shape(usize),
    /// A vector shape's fill colour, and its stroke colour, by its index.
    FillColor(usize),
    StrokeColor(usize),
    /// A switch layer's choice of child.
    Switch,
    /// The camera's position, zoom and roll (see [`Camera`]). They belong to
    /// the project, not to a layer.
    CameraPosition,
    CameraZoom,
    CameraRoll,
}

impl ChannelId {
    /// The camera's channels, in the order the timeline lists them.
    pub const CAMERA: [ChannelId; 3] = [Self::CameraPosition, Self::CameraZoom, Self::CameraRoll];

    /// Whether this is one of the camera's channels.
    pub fn is_camera(self) -> bool {
        Self::CAMERA.contains(&self)
    }

    /// What the camera's channel is called on the timeline.
    pub fn camera_label(self) -> &'static str {
        match self {
            Self::CameraPosition => "Position",
            Self::CameraZoom => "Zoom",
            _ => "Roll",
        }
    }
}

/// The child a switch layer shows on `frame`: the one named by the last
/// key at or before it (names compare without regard to case), or with no
/// such key, the top child.
pub fn switch_choice(children: &[Layer], keys: &[crate::anim::Key<String>], frame: f32) -> Option<usize> {
    let held = keys.partition_point(|k| k.frame as f32 <= frame);
    let named = held.checked_sub(1).and_then(|i| {
        children
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(&keys[i].value))
    });
    named.or(children.len().checked_sub(1))
}

/// What a layer contains. Matches Moho's layer types where they overlap.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayerKind {
    /// Vector shapes.
    Vector { shapes: Vec<Shape> },
    /// A folder of layers that move together.
    Group { children: Vec<Layer> },
    /// A skeleton; shapes in child vector layers can bind to its bones.
    Bone {
        skeleton: Skeleton,
        children: Vec<Layer>,
    },
    /// A picture, which bones can bend.
    Image(crate::media::Image),
    /// Frame-by-frame drawings (see [`crate::frames`]).
    Frames { cels: Vec<crate::frames::Cel> },
    /// Shows one of its children at a time, chosen by name on keys: mouth
    /// shapes for lip sync, hands, eyes. With no keys, the top child shows.
    Switch {
        children: Vec<Layer>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        switch: Vec<crate::anim::Key<String>>,
    },
    /// Sound on the timeline (see [`crate::media`]): a WAV file from the
    /// media store, starting on `start`.
    Audio(crate::media::Audio),
}

impl LayerKind {
    pub fn vector(shapes: Vec<Shape>) -> Self {
        Self::Vector { shapes }
    }

    pub fn children(&self) -> Option<&Vec<Layer>> {
        match self {
            Self::Vector { .. } | Self::Image(_) | Self::Frames { .. } | Self::Audio(_) => None,
            Self::Group { children } | Self::Bone { children, .. } | Self::Switch { children, .. } => {
                Some(children)
            }
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Layer>> {
        match self {
            Self::Vector { .. } | Self::Image(_) | Self::Frames { .. } | Self::Audio(_) => None,
            Self::Group { children } | Self::Bone { children, .. } | Self::Switch { children, .. } => {
                Some(children)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// top: [a, g: [b, c]]
    fn tree() -> (Document, [LayerId; 4]) {
        let mut doc = Document::new("t", 10.0, 10.0, 24.0);
        let a = doc.new_layer("a", LayerKind::vector(vec![]));
        let b = doc.new_layer("b", LayerKind::vector(vec![]));
        let c = doc.new_layer("c", LayerKind::vector(vec![]));
        let ids = [a.id, b.id, c.id];
        let g = doc.new_layer("g", LayerKind::Group { children: vec![b, c] });
        let gid = g.id;
        doc.layers = vec![a, g];
        (doc, [ids[0], gid, ids[1], ids[2]])
    }

    #[test]
    fn find_and_locate_nested_layers() {
        let (doc, [a, g, b, c]) = tree();
        assert_eq!(doc.find(c).unwrap().name, "c");
        assert_eq!(doc.locate(a), Some((None, 0)));
        assert_eq!(doc.locate(c), Some((Some(g), 1)));
        assert!(doc.is_within(b, g));
        assert!(!doc.is_within(a, g));
        assert!(doc.find(LayerId(999)).is_none());
    }

    #[test]
    fn layers_name_their_animatable_properties() {
        let mut layer = Layer::new(LayerId(1), "a", LayerKind::vector(vec![]));
        assert_eq!(
            layer.channels(),
            [
                ChannelId::Position,
                ChannelId::Rotation,
                ChannelId::Scale,
                ChannelId::Opacity
            ]
        );
        assert_eq!(layer.channel_label(ChannelId::Rotation), "Rotation");
        // a key on a property is found, edited and removed through its id
        layer
            .transform
            .rotation
            .set_key(6, 1.0, crate::Interpolation::Linear);
        let keys = layer.channel_keys(ChannelId::Rotation).unwrap();
        assert_eq!(keys.interp_at(6), Some(crate::Interpolation::Linear));
        assert_eq!(
            layer.channel_keys(ChannelId::Position).unwrap().interp_at(6),
            None
        );
        assert!(layer.channel_keys_mut(ChannelId::Rotation).unwrap().delete_key(6));
        assert!(!layer.transform.rotation.is_animated());
        // a property the layer does not have has no keys
        assert!(layer.channel_keys(ChannelId::Bone(0)).is_none());
        assert!(layer.channel_keys(ChannelId::Switch).is_none());

        // a switch layer adds its choice; the bones of a bone layer are rows
        let switch = Layer::new(
            LayerId(2),
            "mouth",
            LayerKind::Switch {
                children: vec![],
                switch: vec![],
            },
        );
        assert_eq!(switch.channels().last(), Some(&ChannelId::Switch));
        assert!(switch.channel_keys(ChannelId::Switch).is_some());
        let doc = crate::samples::hello();
        let rig = doc
            .layers
            .iter()
            .find(|l| matches!(l.kind, LayerKind::Bone { .. }))
            .expect("the sample has a bone layer");
        let bones = rig
            .channels()
            .iter()
            .filter(|c| matches!(c, ChannelId::Bone(_)))
            .count();
        assert!(bones > 0);
        assert!(rig.channel_keys(ChannelId::Bone(0)).is_some());
        assert!(!rig.channel_label(ChannelId::Bone(0)).is_empty());
    }

    #[test]
    fn remove_and_insert_move_layers() {
        let (mut doc, [a, g, _, c]) = tree();
        let layer = doc.remove(c).unwrap();
        assert!(doc.insert(None, 0, layer));
        assert_eq!(doc.locate(c), Some((None, 0)));
        assert_eq!(doc.locate(a), Some((None, 1)));
        // vector layers cannot hold children
        let layer = doc.remove(c).unwrap();
        assert!(!doc.insert(Some(a), 0, layer.clone()));
        assert!(doc.insert(Some(g), 99, layer));
        assert_eq!(doc.locate(c), Some((Some(g), 1)));
    }

    #[test]
    fn labels_round_trip_and_unknown_ones_are_dropped() {
        let mut layer = Layer::new(LayerId(3), "a", LayerKind::vector(vec![]));
        assert_eq!(layer.shown_label(), Label::for_id(LayerId(3)));
        layer.label = Some(Label::Teal);
        let json = serde_json::to_string(&layer).unwrap();
        assert!(json.contains(r#""label":"teal""#));
        assert_eq!(serde_json::from_str::<Layer>(&json).unwrap(), layer);
        let newer = json.replace("teal", "ultraviolet");
        assert_eq!(serde_json::from_str::<Layer>(&newer).unwrap().label, None);
        let none = serde_json::to_string(&Layer::new(LayerId(1), "b", LayerKind::vector(vec![]))).unwrap();
        assert!(!none.contains("label"));
        assert_ne!(Label::for_id(LayerId(1)), Label::for_id(LayerId(2)));
    }

    #[test]
    fn reassign_ids_renumbers_subtree() {
        let (mut doc, [_, g, ..]) = tree();
        let mut copy = doc.find(g).unwrap().clone();
        doc.reassign_ids(&mut copy);
        assert_ne!(copy.id, g);
        let mut ids = Vec::new();
        doc.layers.push(copy);
        doc.walk(|l| ids.push(l.id));
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }
}
