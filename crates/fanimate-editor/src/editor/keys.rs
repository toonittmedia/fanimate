//! Keys picked on the timeline: choosing them, moving, copying and scaling
//! them together, the key clipboard, looping, and keying a layer's place among
//! its siblings.
//!
//! A [`KeyRef`] names one key by the layer (or the camera), the property and
//! the frame. With no property it is the layer's summary key: every
//! property's key on that frame at once. Picking a summary key picks the
//! property keys under it too, so a box over the summary row and a box over
//! the property rows end up with the same keys picked.
//!
//! Moving and scaling work from how the keys were when the gesture began, so
//! a key dragged across another and back leaves it unharmed, and the whole
//! gesture is one undo step (see [`Editor::checkpoint`]).

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};

use fanimate_core::anim::{Animatable, KeyList, sample_in, set_key_in};
use fanimate_core::{
    Camera, Cel, Channel, ChannelId, Color, Document, Frame, Interpolation, Key, Layer, LayerId, LayerKind,
    LoopMode, Path, REST_FRAME, Shape, Vec2,
};

use super::{Editor, StatusKind, plural};

/// Where keys are parked while a gesture rearranges them, so a key moving
/// onto another one that has not moved yet cannot overwrite it.
const PARK: Frame = 1 << 29;

/// The latest frame a gesture puts a key on, well short of [`PARK`].
const MAX_KEY_FRAME: Frame = PARK / 2;

/// What a key belongs to: the project's camera, or a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyTarget {
    Camera,
    Layer(LayerId),
}

/// One key on the timeline, by what it belongs to, which property and which
/// frame. With no `channel` it is the layer's (or the camera's) summary key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyRef {
    /// The layer, or `None` for the camera.
    pub layer: Option<LayerId>,
    /// The property, or `None` for the summary key of all of them.
    pub channel: Option<ChannelId>,
    pub frame: Frame,
}

impl KeyRef {
    pub fn new(layer: Option<LayerId>, channel: Option<ChannelId>, frame: Frame) -> Self {
        Self {
            layer,
            channel,
            frame,
        }
    }

    pub fn target(&self) -> KeyTarget {
        self.layer.map_or(KeyTarget::Camera, KeyTarget::Layer)
    }
}

impl Ord for KeyRef {
    fn cmp(&self, other: &Self) -> Ordering {
        let key = |k: &Self| (k.layer, k.channel.map(channel_rank), k.frame);
        key(self).cmp(&key(other))
    }
}

impl PartialOrd for KeyRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A total order on properties, so picked keys sit in a `BTreeSet`.
fn channel_rank(ch: ChannelId) -> (u8, usize) {
    match ch {
        ChannelId::Opacity => (0, 0),
        ChannelId::Position => (1, 0),
        ChannelId::Rotation => (2, 0),
        ChannelId::Scale => (3, 0),
        ChannelId::Order => (4, 0),
        ChannelId::Bone(i) => (5, i),
        ChannelId::Shape(i) => (6, i),
        ChannelId::FillColor(i) => (7, i),
        ChannelId::StrokeColor(i) => (8, i),
        ChannelId::Trim(i) => (9, i),
        ChannelId::Switch => (10, 0),
        ChannelId::CameraPosition => (11, 0),
        ChannelId::CameraZoom => (12, 0),
        ChannelId::CameraRoll => (13, 0),
    }
}

/// How a click changes the picked keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySelect {
    /// Plain click: only this key.
    Replace,
    /// Shift-click: this key too.
    Add,
    /// Ctrl-click: pick it, or let go of it when it is picked.
    Toggle,
}

// ----------------------------------------------------------------------
// Finding keys

/// The properties of a layer (or the camera) that can hold keys.
fn channels_of(doc: &Document, layer: Option<LayerId>) -> Vec<ChannelId> {
    match layer {
        None => ChannelId::CAMERA.to_vec(),
        Some(id) => doc.find(id).map(Layer::channels).unwrap_or_default(),
    }
}

fn keys_in(doc: &Document, layer: Option<LayerId>, ch: ChannelId) -> Option<&dyn KeyList> {
    match layer {
        None => doc.camera.keys(ch),
        Some(id) => doc.find(id)?.channel_keys(ch),
    }
}

fn has_key(doc: &Document, layer: Option<LayerId>, ch: ChannelId, frame: Frame) -> bool {
    keys_in(doc, layer, ch).is_some_and(|k| k.interp_at(frame).is_some())
}

fn channel_frames(doc: &Document, layer: Option<LayerId>, ch: ChannelId) -> Vec<Frame> {
    let mut frames = Vec::new();
    if let Some(keys) = keys_in(doc, layer, ch) {
        keys.frames_into(&mut frames);
    }
    frames
}

/// The frames with a key in any of the layer's (or the camera's) properties.
fn summary_frames(doc: &Document, layer: Option<LayerId>) -> Vec<Frame> {
    match layer {
        None => doc.camera.key_frames(),
        Some(id) => doc.find(id).map(Layer::key_frames).unwrap_or_default(),
    }
}

/// The properties with a key on `frame`.
fn channels_at(doc: &Document, layer: Option<LayerId>, frame: Frame) -> Vec<ChannelId> {
    channels_of(doc, layer)
        .into_iter()
        .filter(|ch| has_key(doc, layer, *ch, frame))
        .collect()
}

/// The keys of a gesture grouped by the row they live on (layer or camera,
/// property or summary), each as (from frame, to frame).
type KeyGroups = HashMap<(Option<LayerId>, Option<ChannelId>), Vec<(Frame, Frame)>>;

fn key_exists(doc: &Document, k: &KeyRef) -> bool {
    match k.channel {
        Some(ch) => has_key(doc, k.layer, ch, k.frame),
        None => summary_frames(doc, k.layer).contains(&k.frame),
    }
}

// ----------------------------------------------------------------------
// Keying a value now, holding what it is

/// Key a list of keys at `frame`, holding the value it has there. Keying a
/// later frame than the rest pose puts a key on the rest pose first when
/// there is none, holding what the value was there, so frame 0 does not
/// change (the same rule as [`Channel::write`]).
fn key_list_now<T: Animatable>(base: T, keys: &mut Vec<Key<T>>, frame: Frame, interp: Interpolation) {
    let now = sample_in(base, keys, frame as f32);
    if frame > REST_FRAME && keys.binary_search_by_key(&REST_FRAME, |k| k.frame).is_err() {
        let rest = sample_in(base, keys, REST_FRAME as f32);
        set_key_in(keys, REST_FRAME, rest, interp);
    }
    set_key_in(keys, frame, now, interp);
}

/// [`key_list_now`] for a channel, which may repeat after its last key.
pub(crate) fn key_now<T: Animatable>(ch: &mut Channel<T>, frame: Frame, interp: Interpolation) {
    let now = ch.sample(frame as f32);
    if frame > REST_FRAME && ch.keys.binary_search_by_key(&REST_FRAME, |k| k.frame).is_err() {
        let rest = ch.sample(REST_FRAME as f32);
        ch.set_key(REST_FRAME, rest, interp);
    }
    ch.set_key(frame, now, interp);
}

fn key_shape_path_now(shape: &mut Shape, frame: Frame, interp: Interpolation) {
    let path = shape.path_at(frame as f32).into_owned();
    if frame > REST_FRAME && !shape.path_keys.iter().any(|k| k.frame == REST_FRAME) {
        let rest = shape.path_at(REST_FRAME as f32).into_owned();
        shape.key_path(REST_FRAME, rest, interp);
    }
    shape.key_path(frame, path, interp);
}

/// Key every transform channel and opacity of a layer at `frame`, and a
/// vector layer's drawings, holding their current values.
pub(crate) fn key_layer_now(l: &mut Layer, frame: Frame, interp: Interpolation) {
    key_now(&mut l.transform.position, frame, interp);
    key_now(&mut l.transform.rotation, frame, interp);
    key_now(&mut l.transform.scale, frame, interp);
    key_now(&mut l.opacity, frame, interp);
    // a vector layer's drawings too, so they can change shape
    if let LayerKind::Vector { shapes } = &mut l.kind {
        for shape in shapes {
            key_shape_path_now(shape, frame, interp);
        }
    }
}

/// [`key_layer_now`], except on a frame-by-frame layer, where a key is a
/// drawing: a copy of the one showing, to change (Animate's F6).
pub(crate) fn key_layer_any(l: &mut Layer, frame: Frame, interp: Interpolation) {
    if let LayerKind::Frames { cels } = &mut l.kind {
        if !cels.iter().any(|c| c.frame == frame) {
            let mut cel = fanimate_core::frames::cel_at(cels, frame as f32)
                .map(|i| cels[i].clone())
                .unwrap_or_default();
            cel.frame = frame;
            let at = cels.partition_point(|c| c.frame < frame);
            cels.insert(at, cel);
        }
        return;
    }
    key_layer_now(l, frame, interp);
}

/// Key one of the camera's properties at `frame`, holding its value.
pub(crate) fn key_camera_channel_now(
    camera: &mut Camera,
    ch: ChannelId,
    frame: Frame,
    interp: Interpolation,
) {
    match ch {
        ChannelId::CameraPosition => key_now(&mut camera.position, frame, interp),
        ChannelId::CameraZoom => key_now(&mut camera.zoom, frame, interp),
        ChannelId::CameraRoll => key_now(&mut camera.roll, frame, interp),
        _ => {}
    }
}

/// Key one property of a layer at `frame`, holding the value it has there (a
/// bone's angle, a shape's points, the drawing a switch layer shows).
pub(crate) fn key_channel_now(l: &mut Layer, ch: ChannelId, frame: Frame, interp: Interpolation) {
    match ch {
        ChannelId::Position => key_now(&mut l.transform.position, frame, interp),
        ChannelId::Rotation => key_now(&mut l.transform.rotation, frame, interp),
        ChannelId::Scale => key_now(&mut l.transform.scale, frame, interp),
        ChannelId::Opacity => key_now(&mut l.opacity, frame, interp),
        ChannelId::Order => key_now(&mut l.order, frame, interp),
        ChannelId::Bone(i) => {
            if let LayerKind::Bone { skeleton, .. } = &mut l.kind
                && let Some(bone) = skeleton.bones.get_mut(i)
            {
                key_now(&mut bone.angle, frame, interp);
            }
        }
        ChannelId::Shape(i) => {
            if let LayerKind::Vector { shapes } = &mut l.kind
                && let Some(shape) = shapes.get_mut(i)
            {
                key_shape_path_now(shape, frame, interp);
            }
        }
        ChannelId::FillColor(i) => {
            // a key holds the colour as it is now, so only a shape with that colour has one
            if let LayerKind::Vector { shapes } = &mut l.kind
                && let Some(shape) = shapes.get_mut(i)
                && let Some(fill) = shape.style.fill
            {
                key_list_now(fill, &mut shape.fill_keys, frame, interp);
            }
        }
        ChannelId::StrokeColor(i) => {
            if let LayerKind::Vector { shapes } = &mut l.kind
                && let Some(shape) = shapes.get_mut(i)
                && let Some(stroke) = shape.style.stroke
            {
                key_list_now(stroke.color, &mut shape.stroke_keys, frame, interp);
            }
        }
        ChannelId::Trim(i) => {
            if let LayerKind::Vector { shapes } = &mut l.kind
                && let Some(shape) = shapes.get_mut(i)
            {
                key_list_now(shape.trim, &mut shape.trim_keys, frame, interp);
            }
        }
        ChannelId::Switch => {
            if let LayerKind::Switch { children, switch } = &mut l.kind
                && let Some(shown) = fanimate_core::scene::switch_choice(children, switch, frame as f32)
            {
                let name = children[shown].name.clone();
                if frame > REST_FRAME
                    && switch.binary_search_by_key(&REST_FRAME, |k| k.frame).is_err()
                    && let Some(rest) =
                        fanimate_core::scene::switch_choice(children, switch, REST_FRAME as f32)
                {
                    set_key_in(switch, REST_FRAME, children[rest].name.clone(), interp);
                }
                set_key_in(switch, frame, name, interp);
            }
        }
        // the camera's belong to the project, not a layer
        ChannelId::CameraPosition | ChannelId::CameraZoom | ChannelId::CameraRoll => {}
    }
}

// ----------------------------------------------------------------------
// A key's value, whatever its type

/// One key with its value, the type erased enough to carry it between
/// properties of the same kind (and between layers).
#[derive(Clone, Debug, PartialEq)]
enum KeyValue {
    Float(Key<f32>),
    Vec2(Key<Vec2>),
    Colour(Key<Color>),
    Path(Key<Path>),
    Text(Key<String>),
}

fn key_at<T: Clone>(keys: &[Key<T>], frame: Frame) -> Option<Key<T>> {
    keys.binary_search_by_key(&frame, |k| k.frame)
        .ok()
        .map(|i| keys[i].clone())
}

/// The key on `frame` of one property. `None` when there is none, or the
/// property is not one that has keys of a copyable kind.
fn read_key(doc: &Document, layer: Option<LayerId>, ch: ChannelId, frame: Frame) -> Option<KeyValue> {
    use KeyValue as V;
    let Some(id) = layer else {
        let c = &doc.camera;
        return match ch {
            ChannelId::CameraPosition => key_at(&c.position.keys, frame).map(V::Vec2),
            ChannelId::CameraZoom => key_at(&c.zoom.keys, frame).map(V::Float),
            ChannelId::CameraRoll => key_at(&c.roll.keys, frame).map(V::Float),
            _ => None,
        };
    };
    let l = doc.find(id)?;
    match (ch, &l.kind) {
        (ChannelId::Position, _) => key_at(&l.transform.position.keys, frame).map(V::Vec2),
        (ChannelId::Rotation, _) => key_at(&l.transform.rotation.keys, frame).map(V::Float),
        (ChannelId::Scale, _) => key_at(&l.transform.scale.keys, frame).map(V::Vec2),
        (ChannelId::Opacity, _) => key_at(&l.opacity.keys, frame).map(V::Float),
        (ChannelId::Order, _) => key_at(&l.order.keys, frame).map(V::Float),
        (ChannelId::Bone(i), LayerKind::Bone { skeleton, .. }) => {
            key_at(&skeleton.bones.get(i)?.angle.keys, frame).map(V::Float)
        }
        (ChannelId::Shape(i), LayerKind::Vector { shapes }) => {
            key_at(&shapes.get(i)?.path_keys, frame).map(V::Path)
        }
        (ChannelId::FillColor(i), LayerKind::Vector { shapes }) => {
            key_at(&shapes.get(i)?.fill_keys, frame).map(V::Colour)
        }
        (ChannelId::StrokeColor(i), LayerKind::Vector { shapes }) => {
            key_at(&shapes.get(i)?.stroke_keys, frame).map(V::Colour)
        }
        (ChannelId::Trim(i), LayerKind::Vector { shapes }) => {
            key_at(&shapes.get(i)?.trim_keys, frame).map(V::Vec2)
        }
        (ChannelId::Switch, LayerKind::Switch { switch, .. }) => key_at(switch, frame).map(V::Text),
        _ => None,
    }
}

/// Put `kv` on `frame` of one property, replacing the key there. Returns
/// whether the property takes it: the layer and property must exist and be of
/// the kind the key came from (a shape's points need the same structure). With
/// `dry` nothing is written, so it can be asked first.
fn place_key(
    doc: &mut Document,
    layer: Option<LayerId>,
    ch: ChannelId,
    frame: Frame,
    kv: KeyValue,
    dry: bool,
) -> bool {
    use KeyValue as V;
    fn put<T>(keys: &mut Vec<Key<T>>, frame: Frame, k: Key<T>, dry: bool) -> bool {
        if !dry {
            set_key_in(keys, frame, k.value, k.interp);
        }
        true
    }
    let Some(id) = layer else {
        let c = &mut doc.camera;
        return match (ch, kv) {
            (ChannelId::CameraPosition, V::Vec2(k)) => put(&mut c.position.keys, frame, k, dry),
            (ChannelId::CameraZoom, V::Float(k)) => put(&mut c.zoom.keys, frame, k, dry),
            (ChannelId::CameraRoll, V::Float(k)) => put(&mut c.roll.keys, frame, k, dry),
            _ => false,
        };
    };
    let Some(l) = doc.find_mut(id) else {
        return false;
    };
    match (ch, &mut l.kind, kv) {
        (ChannelId::Position, _, V::Vec2(k)) => put(&mut l.transform.position.keys, frame, k, dry),
        (ChannelId::Rotation, _, V::Float(k)) => put(&mut l.transform.rotation.keys, frame, k, dry),
        (ChannelId::Scale, _, V::Vec2(k)) => put(&mut l.transform.scale.keys, frame, k, dry),
        (ChannelId::Opacity, _, V::Float(k)) => put(&mut l.opacity.keys, frame, k, dry),
        (ChannelId::Order, _, V::Float(k)) => put(&mut l.order.keys, frame, k, dry),
        (ChannelId::Bone(i), LayerKind::Bone { skeleton, .. }, V::Float(k)) => {
            match skeleton.bones.get_mut(i) {
                Some(b) => put(&mut b.angle.keys, frame, k, dry),
                None => false,
            }
        }
        (ChannelId::Shape(i), LayerKind::Vector { shapes }, V::Path(k)) => match shapes.get_mut(i) {
            Some(s) if k.value.same_structure(&s.path) => put(&mut s.path_keys, frame, k, dry),
            _ => false,
        },
        (ChannelId::FillColor(i), LayerKind::Vector { shapes }, V::Colour(k)) => match shapes.get_mut(i) {
            Some(s) if s.style.fill.is_some() => put(&mut s.fill_keys, frame, k, dry),
            _ => false,
        },
        (ChannelId::StrokeColor(i), LayerKind::Vector { shapes }, V::Colour(k)) => match shapes.get_mut(i) {
            Some(s) if s.style.stroke.is_some() => put(&mut s.stroke_keys, frame, k, dry),
            _ => false,
        },
        (ChannelId::Trim(i), LayerKind::Vector { shapes }, V::Vec2(k)) => match shapes.get_mut(i) {
            Some(s) => put(&mut s.trim_keys, frame, k, dry),
            None => false,
        },
        (ChannelId::Switch, LayerKind::Switch { switch, .. }, V::Text(k)) => put(switch, frame, k, dry),
        _ => false,
    }
}

// ----------------------------------------------------------------------
// Gestures and the clipboard

/// Moving, copying or scaling the picked keys. The layers (and camera) they
/// are on are kept as they were when the gesture began, and each step starts
/// from those.
pub(crate) struct KeyGesture {
    layers: Vec<Layer>,
    camera: Option<Camera>,
    /// Everything that was picked, to pick the moved keys afterwards.
    picked: Vec<KeyRef>,
    /// The keys that move: a summary key stands for the property keys under it.
    operands: Vec<KeyRef>,
    copy: bool,
    /// Whether the history has its step yet (it gets one at the first change).
    recorded: bool,
    /// The earliest frame of the keys, so a move cannot take one below frame 0.
    earliest: Frame,
    /// The frame a scale stretches about.
    anchor: Frame,
}

/// Keys on the key clipboard.
#[derive(Clone)]
pub(crate) struct KeyClip {
    keys: Vec<ClipKey>,
}

#[derive(Clone)]
struct ClipKey {
    layer: Option<LayerId>,
    channel: ChannelId,
    /// Frames after the earliest copied key.
    rel: Frame,
    key: KeyValue,
}

/// Slots in the order layers are drawn, by index into the list, bottom
/// first: a layer with Order keys sits at the value they hold on `frame`, any
/// other at its place in the list; ties keep the list order. The same rule the
/// renderer uses, so what is keyed here is what is drawn.
fn draw_order(layers: &[Layer], frame: f32) -> Vec<usize> {
    let mut slots: Vec<(f32, usize)> = layers
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let slot = if l.order.is_animated() {
                l.order.sample(frame.floor())
            } else {
                i as f32
            };
            (slot, i)
        })
        .collect();
    slots.sort_by(|a, b| a.0.total_cmp(&b.0));
    slots.into_iter().map(|(_, i)| i).collect()
}

impl Editor {
    // ------------------------------------------------------------------
    // Picking keys

    /// The keys picked on the timeline.
    pub fn selected_keys(&self) -> &BTreeSet<KeyRef> {
        &self.selected_keys
    }

    pub fn has_selected_keys(&self) -> bool {
        !self.selected_keys.is_empty()
    }

    pub fn is_key_selected(&self, k: &KeyRef) -> bool {
        self.selected_keys.contains(k)
    }

    pub fn clear_key_selection(&mut self) {
        self.selected_keys.clear();
    }

    /// `k` with, for a summary key, the property keys under it.
    /// The picked keys of properties (not the summary keys over them).
    fn picked_property_keys(&self) -> Vec<KeyRef> {
        self.selected_keys
            .iter()
            .filter(|k| k.channel.is_some())
            .copied()
            .collect()
    }

    /// The interpolation of the picked keys, if any are picked and they all
    /// agree; `Some(None)` when they differ. `None` when no property key is
    /// picked, so the keys on the current frame are the ones meant.
    pub(super) fn picked_interpolation(&self) -> Option<Option<Interpolation>> {
        let mut found: Option<Option<Interpolation>> = None;
        for k in self.picked_property_keys() {
            let Some(i) = k
                .channel
                .and_then(|ch| keys_in(&self.doc, k.layer, ch))
                .and_then(|list| list.interp_at(k.frame))
            else {
                continue;
            };
            found = Some(match found {
                None => Some(i),
                Some(Some(j)) if j == i => Some(i),
                Some(_) => None,
            });
        }
        found
    }

    /// Set how the picked keys move on to the next key, as one undo step.
    pub(super) fn set_picked_interpolation(&mut self, interp: Interpolation) {
        self.checkpoint("Change interpolation");
        for k in self.picked_property_keys() {
            self.with_lists(k.layer, k.channel, &mut |list| {
                list.set_interp(k.frame, interp);
            });
        }
    }

    fn with_keys_under(&self, k: KeyRef) -> Vec<KeyRef> {
        let mut out = vec![k];
        if k.channel.is_none() {
            out.extend(
                channels_at(&self.doc, k.layer, k.frame)
                    .into_iter()
                    .map(|ch| KeyRef::new(k.layer, Some(ch), k.frame)),
            );
        }
        out
    }

    /// Pick (or let go of) a key. A key that is not there picks nothing, and
    /// with `Replace` clears the pick.
    pub fn select_key(&mut self, k: KeyRef, mode: KeySelect) {
        if !key_exists(&self.doc, &k) {
            if mode == KeySelect::Replace {
                self.selected_keys.clear();
            }
            return;
        }
        let refs = self.with_keys_under(k);
        match mode {
            KeySelect::Replace => {
                self.selected_keys.clear();
                self.selected_keys.extend(refs);
            }
            KeySelect::Add => self.selected_keys.extend(refs),
            KeySelect::Toggle => {
                if self.selected_keys.contains(&k) {
                    for r in &refs {
                        self.selected_keys.remove(r);
                    }
                } else {
                    self.selected_keys.extend(refs);
                }
            }
        }
    }

    /// Pick every key of `rows` from frame `from` to `to`, both included (a
    /// box dragged over the timeline). A row is a layer (`None` for the
    /// camera) and a property, or no property for its summary row, which
    /// covers all its properties. With `add` the keys already picked stay.
    pub fn select_keys_in(
        &mut self,
        rows: &[(Option<LayerId>, Option<ChannelId>)],
        from: Frame,
        to: Frame,
        add: bool,
    ) {
        let (lo, hi) = (from.min(to), from.max(to));
        let within = |f: &Frame| (lo..=hi).contains(f);
        if !add {
            self.selected_keys.clear();
        }
        for &(layer, channel) in rows {
            match channel {
                Some(ch) => {
                    for f in channel_frames(&self.doc, layer, ch).into_iter().filter(within) {
                        self.selected_keys.insert(KeyRef::new(layer, Some(ch), f));
                    }
                }
                None => {
                    for f in summary_frames(&self.doc, layer).into_iter().filter(within) {
                        self.selected_keys.insert(KeyRef::new(layer, None, f));
                    }
                    for ch in channels_of(&self.doc, layer) {
                        for f in channel_frames(&self.doc, layer, ch).into_iter().filter(within) {
                            self.selected_keys.insert(KeyRef::new(layer, Some(ch), f));
                        }
                    }
                }
            }
        }
    }

    /// Pick every key of every layer and of the camera.
    pub fn select_all_keys(&mut self) {
        let mut rows: Vec<(Option<LayerId>, Option<ChannelId>)> = vec![(None, None)];
        self.doc.walk(|l| rows.push((Some(l.id), None)));
        self.select_keys_in(&rows, REST_FRAME, Frame::MAX, false);
    }

    /// Let go of picked keys that are gone: their layer was deleted, or an
    /// undo took the key away.
    pub fn prune_key_selection(&mut self) {
        self.selected_keys.retain(|k| key_exists(&self.doc, k));
    }

    /// The picked keys the commands act on: a summary key stands for the
    /// property keys under it, so those are left out.
    fn operands(&self) -> Vec<KeyRef> {
        let summaries: HashSet<(Option<LayerId>, Frame)> = self
            .selected_keys
            .iter()
            .filter(|k| k.channel.is_none())
            .map(|k| (k.layer, k.frame))
            .collect();
        self.selected_keys
            .iter()
            .filter(|k| k.channel.is_none() || !summaries.contains(&(k.layer, k.frame)))
            .filter(|k| key_exists(&self.doc, k))
            .copied()
            .collect()
    }

    /// Resolve a picked key's lists and run `f` on each (a summary key has
    /// many).
    fn with_lists(
        &mut self,
        layer: Option<LayerId>,
        channel: Option<ChannelId>,
        f: &mut dyn FnMut(&mut dyn KeyList),
    ) {
        match (layer, channel) {
            (Some(id), Some(ch)) => {
                if let Some(list) = self.doc.find_mut(id).and_then(|l| l.channel_keys_mut(ch)) {
                    f(list);
                }
            }
            (Some(id), None) => {
                if let Some(l) = self.doc.find_mut(id) {
                    l.key_lists_mut(|list| f(list));
                }
            }
            (None, Some(ch)) => {
                if let Some(list) = self.doc.camera.keys_mut(ch) {
                    f(list);
                }
            }
            (None, None) => {
                for ch in ChannelId::CAMERA {
                    if let Some(list) = self.doc.camera.keys_mut(ch) {
                        f(list);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Add and delete

    /// Add Key with keys picked: key the properties of the picked keys on the
    /// playhead frame, holding their values there. A layer's summary key keys
    /// the layer as Add Key without a pick does.
    pub(crate) fn add_key_to_selected_keys(&mut self) {
        self.prune_key_selection();
        let operands = self.operands();
        if operands.is_empty() {
            return;
        }
        self.checkpoint("Add keyframe");
        let (frame, interp) = (self.current_frame(), self.key_interp);
        let mut done = HashSet::new();
        for k in operands {
            if !done.insert((k.layer, k.channel)) {
                continue;
            }
            match (k.layer, k.channel) {
                (None, None) => {
                    for ch in ChannelId::CAMERA {
                        key_camera_channel_now(&mut self.doc.camera, ch, frame, interp);
                    }
                }
                (None, Some(ch)) => key_camera_channel_now(&mut self.doc.camera, ch, frame, interp),
                (Some(id), None) => {
                    if let Some(l) = self.doc.find_mut(id) {
                        key_layer_any(l, frame, interp);
                    }
                }
                (Some(id), Some(ch)) => {
                    if let Some(l) = self.doc.find_mut(id) {
                        key_channel_now(l, ch, frame, interp);
                    }
                }
            }
        }
    }

    /// Delete Key with keys picked: remove them, as one undo step.
    pub(crate) fn delete_selected_keys(&mut self) {
        let n = self.remove_selected_keys("Delete keyframes");
        if n > 0 {
            self.set_status(StatusKind::Info, format!("Deleted {n} keyframe{}", plural(n)));
        }
    }

    /// Remove the picked keys under a history step called `label`. Returns how
    /// many went.
    fn remove_selected_keys(&mut self, label: &str) -> usize {
        self.prune_key_selection();
        let operands = self.operands();
        if operands.is_empty() {
            self.set_status(StatusKind::Info, "No keyframes picked");
            return 0;
        }
        self.checkpoint(label);
        for k in &operands {
            let frame = k.frame;
            self.with_lists(k.layer, k.channel, &mut |list| {
                list.delete_key(frame);
            });
        }
        self.selected_keys.clear();
        operands.len()
    }

    // ------------------------------------------------------------------
    // Moving, copying and scaling the picked keys

    fn begin_gesture(&mut self, copy: bool) -> Option<KeyGesture> {
        self.prune_key_selection();
        let operands = self.operands();
        let earliest = operands.iter().map(|k| k.frame).min()?;
        let ids: BTreeSet<LayerId> = operands.iter().filter_map(|k| k.layer).collect();
        let layers = ids.iter().filter_map(|id| self.doc.find(*id)).cloned().collect();
        let camera = operands
            .iter()
            .any(|k| k.layer.is_none())
            .then(|| self.doc.camera.clone());
        Some(KeyGesture {
            layers,
            camera,
            picked: self.selected_keys.iter().copied().collect(),
            operands,
            copy,
            recorded: false,
            earliest,
            anchor: earliest,
        })
    }

    /// Show the gesture with every operand's frame mapped through `map`.
    fn apply_gesture(&mut self, g: &KeyGesture, map: &dyn Fn(Frame) -> Frame) {
        // back to how the keys were when the gesture began
        for original in &g.layers {
            if let Some(l) = self.doc.find_mut(original.id) {
                *l = original.clone();
            }
        }
        if let Some(camera) = &g.camera {
            self.doc.camera = camera.clone();
        }

        // operands come in order, so each group's frames ascend
        let mut groups = KeyGroups::new();
        for k in &g.operands {
            groups
                .entry((k.layer, k.channel))
                .or_default()
                .push((k.frame, map(k.frame)));
        }

        if g.copy {
            self.copy_groups(&groups);
        } else {
            // park every moving key first, then set each down on its frame in
            // order: a key moving onto one that has not moved yet cannot
            // overwrite it, and where two arrive on one frame the later wins
            for ((layer, channel), pairs) in &groups {
                self.with_lists(*layer, *channel, &mut |list| {
                    for (from, _) in pairs {
                        if let Some(parked) = from.checked_add(PARK) {
                            list.move_key(*from, parked);
                        }
                    }
                });
            }
            for ((layer, channel), pairs) in &groups {
                self.with_lists(*layer, *channel, &mut |list| {
                    for (from, to) in pairs {
                        if let Some(parked) = from.checked_add(PARK) {
                            list.move_key(parked, *to);
                        }
                    }
                });
            }
        }

        self.selected_keys = g
            .picked
            .iter()
            .map(|k| KeyRef {
                frame: map(k.frame),
                ..*k
            })
            .collect();
        self.prune_key_selection();
        self.dirty = true;
    }

    /// Put copies of the keys on their new frames, leaving the originals.
    fn copy_groups(&mut self, groups: &KeyGroups) {
        // read everything first, so a copy cannot be read as an original
        let mut keys: Vec<(Option<LayerId>, ChannelId, Frame, KeyValue)> = Vec::new();
        let mut cels: Vec<(LayerId, Cel)> = Vec::new();
        for ((layer, channel), pairs) in groups {
            let channels = match channel {
                Some(ch) => vec![*ch],
                None => channels_of(&self.doc, *layer),
            };
            for (from, to) in pairs {
                for ch in &channels {
                    if let Some(kv) = read_key(&self.doc, *layer, *ch, *from) {
                        keys.push((*layer, *ch, *to, kv));
                    }
                }
                // a frame-by-frame layer's summary key is a drawing
                if channel.is_none()
                    && let Some(id) = layer
                    && let Some(LayerKind::Frames { cels: list }) = self.doc.find(*id).map(|l| &l.kind)
                    && let Some(cel) = list.iter().find(|c| c.frame == *from)
                {
                    let mut cel = cel.clone();
                    cel.frame = *to;
                    cels.push((*id, cel));
                }
            }
        }
        for (layer, ch, frame, kv) in keys {
            place_key(&mut self.doc, layer, ch, frame, kv, false);
        }
        for (id, cel) in cels {
            if let Some(l) = self.doc.find_mut(id)
                && let LayerKind::Frames { cels: list } = &mut l.kind
            {
                match list.binary_search_by_key(&cel.frame, |c| c.frame) {
                    Ok(i) => list[i] = cel,
                    Err(i) => list.insert(i, cel),
                }
            }
        }
    }

    /// Start moving the picked keys along the timeline, or copying them with
    /// `copy`. Returns false when no key is picked.
    pub fn begin_selected_key_drag(&mut self, copy: bool) -> bool {
        if self.playing {
            return false;
        }
        match self.begin_gesture(copy) {
            Some(g) => {
                self.sel_drag = Some(g);
                true
            }
            None => false,
        }
    }

    /// Show the picked keys `delta` frames from where they were when the drag
    /// began (negative is earlier). No key goes before frame 0, and a key
    /// landing on another one replaces it. The picked keys follow. The whole
    /// drag is one undo step.
    pub fn drag_selected_keys_by(&mut self, delta: Frame) {
        let Some(mut g) = self.sel_drag.take() else {
            return;
        };
        let delta = delta.max(g.earliest.saturating_neg()).min(MAX_KEY_FRAME);
        if g.recorded || delta != 0 {
            if !g.recorded {
                self.checkpoint(if g.copy {
                    "Copy keyframes"
                } else {
                    "Move keyframes"
                });
                g.recorded = true;
            }
            self.apply_gesture(&g, &|f| f.saturating_add(delta).clamp(0, MAX_KEY_FRAME));
        }
        self.sel_drag = Some(g);
    }

    pub fn end_selected_key_drag(&mut self) {
        self.sel_drag = None;
    }

    /// Start stretching or squeezing the picked keys about the frame `anchor`.
    /// Returns false when no key is picked.
    pub fn begin_key_scale(&mut self, anchor: Frame) -> bool {
        if self.playing {
            return false;
        }
        match self.begin_gesture(false) {
            Some(mut g) => {
                g.anchor = anchor;
                self.scale_drag = Some(g);
                true
            }
            None => false,
        }
    }

    /// The frame the scale in progress stretches about.
    pub fn key_scale_anchor(&self) -> Option<Frame> {
        self.scale_drag.as_ref().map(|g| g.anchor)
    }

    /// Scale the picked keys, from how they were when the scale began, about
    /// `anchor`, by how far `to_edge` is from it compared with `from_edge`
    /// (the edge being dragged, where it began). Key times are whole frames; a
    /// key never goes before frame 0, and where two land on one frame the later
    /// wins. The whole scale is one undo step.
    pub fn scale_selected_keys(&mut self, anchor: Frame, from_edge: Frame, to_edge: Frame) {
        let Some(mut g) = self.scale_drag.take() else {
            return;
        };
        if from_edge != anchor {
            // past the anchor the keys fold onto it rather than turning round
            let factor =
                ((to_edge as f64 - anchor as f64) / (from_edge as f64 - anchor as f64)).clamp(0.0, 1.0e6);
            let map = move |f: Frame| -> Frame {
                let t = anchor as f64 + (f as f64 - anchor as f64) * factor;
                t.round().clamp(0.0, MAX_KEY_FRAME as f64) as Frame
            };
            if g.recorded || g.operands.iter().any(|k| map(k.frame) != k.frame) {
                if !g.recorded {
                    self.checkpoint("Scale keyframes");
                    g.recorded = true;
                }
                self.apply_gesture(&g, &map);
            }
        }
        self.scale_drag = Some(g);
    }

    pub fn end_key_scale(&mut self) {
        self.scale_drag = None;
    }

    /// Whether picked keys are being moved, copied or scaled.
    pub fn is_dragging_selected_keys(&self) -> bool {
        self.sel_drag.is_some() || self.scale_drag.is_some()
    }

    // ------------------------------------------------------------------
    // The key clipboard

    pub fn has_key_clipboard(&self) -> bool {
        self.key_clip.is_some()
    }

    /// Copy the picked keys. Returns whether there was anything to copy.
    pub fn copy_keys(&mut self) -> bool {
        self.prune_key_selection();
        let mut found: Vec<(Option<LayerId>, ChannelId, Frame, KeyValue)> = Vec::new();
        let mut seen = HashSet::new();
        for k in &self.selected_keys {
            let channels = match k.channel {
                Some(ch) => vec![ch],
                None => channels_at(&self.doc, k.layer, k.frame),
            };
            for ch in channels {
                if seen.insert((k.layer, ch, k.frame))
                    && let Some(kv) = read_key(&self.doc, k.layer, ch, k.frame)
                {
                    found.push((k.layer, ch, k.frame, kv));
                }
            }
        }
        let Some(first) = found.iter().map(|f| f.2).min() else {
            self.set_status(StatusKind::Info, "Pick keyframes to copy");
            return false;
        };
        let n = found.len();
        self.key_clip = Some(KeyClip {
            keys: found
                .into_iter()
                .map(|(layer, channel, frame, key)| ClipKey {
                    layer,
                    channel,
                    rel: frame - first,
                    key,
                })
                .collect(),
        });
        self.set_status(StatusKind::Info, format!("Copied {n} keyframe{}", plural(n)));
        true
    }

    /// Copy the picked keys and remove them.
    pub fn cut_keys(&mut self) -> bool {
        if !self.copy_keys() {
            return false;
        }
        let n = self.remove_selected_keys("Cut keyframes");
        if n > 0 {
            self.set_status(StatusKind::Info, format!("Cut {n} keyframe{}", plural(n)));
        }
        n > 0
    }

    /// Paste the copied keys with the earliest at the playhead, each on the
    /// layer and property it came from (replacing the keys beneath). A layer
    /// that is gone, as when pasting in another document, gives way to the
    /// selected layer's matching properties. The pasted keys are picked.
    pub fn paste_keys(&mut self) -> bool {
        let Some(clip) = self.key_clip.clone() else {
            return false;
        };
        let at = self.current_frame();
        let fallback = self.primary().filter(|id| self.doc.find(*id).is_some());
        let mut jobs: Vec<(Option<LayerId>, ChannelId, Frame, KeyValue)> = Vec::new();
        for ck in &clip.keys {
            let layer = match ck.layer {
                None => None,
                Some(id) if self.doc.find(id).is_some() => Some(id),
                Some(_) => match fallback {
                    Some(id) => Some(id),
                    None => continue,
                },
            };
            let frame = at.saturating_add(ck.rel).min(MAX_KEY_FRAME);
            if place_key(&mut self.doc, layer, ck.channel, frame, ck.key.clone(), true) {
                jobs.push((layer, ck.channel, frame, ck.key.clone()));
            }
        }
        if jobs.is_empty() {
            self.set_status(StatusKind::Info, "The copied keyframes don't fit here");
            return false;
        }
        self.checkpoint("Paste keyframes");
        let n = jobs.len();
        self.selected_keys.clear();
        for (layer, ch, frame, kv) in jobs {
            if place_key(&mut self.doc, layer, ch, frame, kv, false) {
                self.selected_keys.insert(KeyRef::new(layer, Some(ch), frame));
            }
        }
        self.set_status(StatusKind::Info, format!("Pasted {n} keyframe{}", plural(n)));
        true
    }

    // ------------------------------------------------------------------
    // Looping

    /// What a property does after its last key (the camera's when `layer` is
    /// `None`).
    pub fn channel_loop(&self, layer: Option<LayerId>, channel: ChannelId) -> LoopMode {
        match layer {
            None => self.doc.camera.channel_loop(channel),
            Some(id) => self
                .doc
                .find(id)
                .map_or(LoopMode::Off, |l| l.channel_loop(channel)),
        }
    }

    /// Whether the property exists and is one that can repeat: numbers and
    /// vectors do, point, colour, exposure and switch keys do not.
    pub fn can_loop(&self, layer: Option<LayerId>, channel: ChannelId) -> bool {
        matches!(
            channel,
            ChannelId::Position
                | ChannelId::Rotation
                | ChannelId::Scale
                | ChannelId::Opacity
                | ChannelId::Order
                | ChannelId::Bone(_)
                | ChannelId::CameraPosition
                | ChannelId::CameraZoom
                | ChannelId::CameraRoll
        ) && keys_in(&self.doc, layer, channel).is_some()
    }

    fn write_loop(&mut self, layer: Option<LayerId>, channel: ChannelId, mode: LoopMode) {
        match layer {
            None => {
                self.doc.camera.set_channel_loop(channel, mode);
            }
            Some(id) => {
                if let Some(l) = self.doc.find_mut(id) {
                    l.set_channel_loop(channel, mode);
                }
            }
        }
    }

    /// Set what one property does after its last key, as one undo step.
    /// Returns whether anything changed.
    pub fn set_channel_loop(&mut self, layer: Option<LayerId>, channel: ChannelId, mode: LoopMode) -> bool {
        if !self.can_loop(layer, channel) {
            self.set_status(StatusKind::Info, "This property can't repeat");
            return false;
        }
        if self.channel_loop(layer, channel) == mode {
            return false;
        }
        self.checkpoint("Loop");
        self.write_loop(layer, channel, mode);
        self.set_status(StatusKind::Info, format!("Loop: {}", mode.label()));
        true
    }

    /// Set what the properties of the picked keys do after their last key.
    pub(crate) fn set_loop_on_selected_keys(&mut self, mode: LoopMode) {
        self.prune_key_selection();
        let mut targets: Vec<(Option<LayerId>, ChannelId)> = Vec::new();
        for k in &self.selected_keys {
            let channels = match k.channel {
                Some(ch) => vec![ch],
                None => channels_at(&self.doc, k.layer, k.frame),
            };
            for ch in channels {
                if self.can_loop(k.layer, ch) && !targets.contains(&(k.layer, ch)) {
                    targets.push((k.layer, ch));
                }
            }
        }
        if targets.is_empty() {
            self.set_status(StatusKind::Info, "Pick keyframes that can repeat first");
            return;
        }
        targets.retain(|(layer, ch)| self.channel_loop(*layer, *ch) != mode);
        if targets.is_empty() {
            return;
        }
        self.checkpoint("Loop");
        for (layer, ch) in targets {
            self.write_loop(layer, ch, mode);
        }
        self.set_status(StatusKind::Info, format!("Loop: {}", mode.label()));
    }

    // ------------------------------------------------------------------
    // Layer order

    /// The layers sharing a list with a layer: the document's, or a group's,
    /// bone layer's or switch layer's children.
    fn sibling_layers(&self, parent: Option<LayerId>) -> Option<&[Layer]> {
        match parent {
            None => Some(&self.doc.layers),
            Some(p) => self.doc.find(p)?.kind.children().map(|c| c.as_slice()),
        }
    }

    /// The siblings' ids in the order they are drawn on `frame`, bottom first.
    fn draw_order_ids(&self, parent: Option<LayerId>, frame: Frame) -> Vec<LayerId> {
        let Some(list) = self.sibling_layers(parent) else {
            return Vec::new();
        };
        draw_order(list, frame as f32)
            .into_iter()
            .map(|i| list[i].id)
            .collect()
    }

    /// Key the drawing order of a list of siblings: every one gets a Step key
    /// on the playhead frame holding its new slot (`new_order` is bottom
    /// first), and one without a key on the rest pose gets one holding its
    /// slot there, so frame 0 looks as it did.
    fn write_order_keys(&mut self, parent: Option<LayerId>, new_order: &[LayerId]) {
        let frame = self.current_frame();
        let rest: HashMap<LayerId, f32> = self
            .draw_order_ids(parent, REST_FRAME)
            .into_iter()
            .enumerate()
            .map(|(slot, id)| (id, slot as f32))
            .collect();
        for (slot, id) in new_order.iter().enumerate() {
            let Some(l) = self.doc.find_mut(*id) else {
                continue;
            };
            if !l.order.keys.iter().any(|k| k.frame == REST_FRAME) {
                let seed = rest.get(id).copied().unwrap_or(slot as f32);
                l.order.set_key(REST_FRAME, seed, Interpolation::Step);
            }
            l.order.set_key(frame, slot as f32, Interpolation::Step);
        }
        self.dirty = true;
    }

    /// Move Layer Up and Down after the rest pose: the layers' places are
    /// keyed on the playhead frame and the list keeps its order. `roots` are
    /// the selected layers, top of the stack first.
    pub(crate) fn move_selected_keyed(&mut self, roots: &[LayerId], up: bool) {
        let frame = self.current_frame();
        let mut parents: Vec<Option<LayerId>> = Vec::new();
        for id in roots {
            if let Some((parent, _)) = self.doc.locate(*id)
                && !parents.contains(&parent)
            {
                parents.push(parent);
            }
        }
        let mut changes: Vec<(Option<LayerId>, Vec<LayerId>)> = Vec::new();
        for parent in parents {
            let mut order = self.draw_order_ids(parent, frame);
            let mut chosen: Vec<LayerId> = roots
                .iter()
                .copied()
                .filter(|id| self.doc.locate(*id).is_some_and(|(p, _)| p == parent))
                .collect();
            // the leading layer first, so neighbours do not block each other
            if !up {
                chosen.reverse();
            }
            let mut moved = false;
            for id in chosen {
                let Some(i) = order.iter().position(|x| *x == id) else {
                    continue;
                };
                let target = if up { i + 1 } else { i.wrapping_sub(1) };
                if target >= order.len() || self.is_selected(order[target]) {
                    continue;
                }
                order.swap(i, target);
                moved = true;
            }
            if moved {
                changes.push((parent, order));
            }
        }
        if changes.is_empty() {
            self.set_status(StatusKind::Info, "Already at the end of its group");
            return;
        }
        self.checkpoint(if up { "Move layer up" } else { "Move layer down" });
        for (parent, order) in changes {
            self.write_order_keys(parent, &order);
        }
        self.set_status(StatusKind::Info, format!("Layer order keyed on frame {frame}"));
    }

    /// Drag and drop of layers next to `target` after the rest pose: keyed
    /// like [`move_selected_keyed`](Self::move_selected_keyed). `None` when the
    /// layers do not all share `target`'s list, since moving to another list
    /// is a change of structure, not of order over time; the caller then moves
    /// them as it does on the rest pose. `roots` are the dragged layers
    /// without any inside another.
    pub(crate) fn move_relative_keyed(
        &mut self,
        roots: &[LayerId],
        target: LayerId,
        above: bool,
    ) -> Option<bool> {
        let (parent, _) = self.doc.locate(target)?;
        if roots
            .iter()
            .any(|r| self.doc.locate(*r).map(|(p, _)| p) != Some(parent))
        {
            return None;
        }
        let frame = self.current_frame();
        let current = self.draw_order_ids(parent, frame);
        let mut order: Vec<LayerId> = current.iter().copied().filter(|id| !roots.contains(id)).collect();
        let moving: Vec<LayerId> = current.iter().copied().filter(|id| roots.contains(id)).collect();
        let at = order.iter().position(|id| *id == target)? + usize::from(above);
        for (offset, id) in moving.into_iter().enumerate() {
            order.insert(at + offset, id);
        }
        if order == current {
            return Some(false);
        }
        self.checkpoint("Move layers");
        self.write_order_keys(parent, &order);
        self.selection = roots.to_vec();
        self.set_status(StatusKind::Info, format!("Layer order keyed on frame {frame}"));
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Placement, SelectMode};
    use crate::{Action, Chord, Keymap, KeymapKind};

    /// Layers A and B (bottom to top), with the playhead on the rest pose.
    fn two() -> (Editor, LayerId, LayerId) {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let ids = ["A", "B"].map(|n| {
            let l = doc.new_layer(n, LayerKind::vector(vec![]));
            let id = l.id;
            doc.layers.push(l);
            id
        });
        (Editor::new(doc), ids[0], ids[1])
    }

    fn key_rotation(ed: &mut Editor, id: LayerId, frames: &[Frame]) {
        let l = ed.doc.find_mut(id).unwrap();
        for f in frames {
            l.transform.rotation.set_key(*f, *f as f32, Interpolation::Linear);
        }
    }

    fn rotation(ed: &Editor, id: LayerId) -> Vec<(Frame, f32)> {
        let l = ed.doc.find(id).unwrap();
        l.transform
            .rotation
            .keys
            .iter()
            .map(|k| (k.frame, k.value))
            .collect()
    }

    fn frames(ed: &Editor, id: LayerId, ch: ChannelId) -> Vec<Frame> {
        ed.channel_key_frames(id, ch)
    }

    fn pick(ed: &mut Editor, id: LayerId, ch: ChannelId, fs: &[Frame]) {
        for (i, f) in fs.iter().enumerate() {
            let mode = if i == 0 {
                KeySelect::Replace
            } else {
                KeySelect::Add
            };
            ed.select_key(KeyRef::new(Some(id), Some(ch), *f), mode);
        }
    }

    fn name_order(ed: &Editor) -> Vec<String> {
        ed.doc.layers.iter().map(|l| l.name.clone()).collect()
    }

    // --- the frame model

    #[test]
    fn a_new_file_counts_from_one_and_the_playhead_can_stand_on_the_rest_pose() {
        let ed = Editor::untitled();
        assert_eq!((ed.doc.start, ed.doc.end), (1, 120));
        assert_eq!(ed.current_frame(), REST_FRAME);
        let mut ed = ed;
        ed.go_to(-5);
        assert_eq!(ed.current_frame(), 0);
        ed.go_to(40);
        ed.run(Action::GoToRestPose);
        assert_eq!(ed.current_frame(), 0);
        ed.run(Action::FirstFrame);
        assert_eq!(
            ed.current_frame(),
            1,
            "First Frame is the first frame of the animation"
        );
        ed.run(Action::LastFrame);
        assert_eq!(ed.current_frame(), 120);
        assert_eq!(ed.keying().base, REST_FRAME);
    }

    #[test]
    fn playing_from_the_rest_pose_starts_on_the_first_frame() {
        let (mut ed, ..) = two();
        ed.doc.end = 24;
        ed.playing = true;
        ed.tick(0.25); // six frames
        assert_eq!(ed.current_frame(), 7);
        // playback loops over 1 to 24, never back onto the rest pose
        ed.frame = 24.0;
        ed.tick(0.09); // a little over two frames
        assert_eq!(ed.current_frame(), 2);
    }

    #[test]
    fn the_rest_pose_has_a_shortcut_in_the_fanimate_keymap_only() {
        let fanimate = Keymap::new(KeymapKind::Fanimate);
        let key = Chord::new("1", false, false, false);
        assert_eq!(fanimate.lookup(key), Some(Action::GoToRestPose));
        assert_eq!(Keymap::new(KeymapKind::Moho).lookup(key), None);
        assert_eq!(Action::GoToRestPose.label(), "Go to Rest Pose");
    }

    #[test]
    fn add_key_on_a_later_frame_seeds_the_rest_pose() {
        let (mut ed, a, _) = two();
        ed.select(a, SelectMode::Replace);
        ed.doc.find_mut(a).unwrap().transform.rotation.base = 0.5;
        ed.go_to(12);
        ed.run(Action::AddKey);
        assert_eq!(rotation(&ed, a), [(0, 0.5), (12, 0.5)]);
        // a channel that already has keys, but none on the rest pose, is seeded too
        key_rotation(&mut ed, a, &[6, 20]);
        ed.doc.find_mut(a).unwrap().transform.rotation.remove_key(0);
        ed.go_to(30);
        ed.run(Action::AddChannelKey(ChannelId::Rotation));
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [0, 6, 12, 20, 30]);
        // on the rest pose itself it just keys that frame (opacity was keyed
        // on 0 and 12 by the Add Key above, so nothing else appears)
        ed.go_to(0);
        ed.run(Action::AddChannelKey(ChannelId::Opacity));
        assert_eq!(frames(&ed, a, ChannelId::Opacity), [0, 12]);
    }

    #[test]
    fn interpolation_applies_to_the_picked_keys_wherever_the_playhead_is() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[3, 6, 9]);
        let interps = |ed: &Editor| -> Vec<(Frame, Interpolation)> {
            ed.doc
                .find(a)
                .unwrap()
                .transform
                .rotation
                .keys
                .iter()
                .map(|k| (k.frame, k.interp))
                .collect()
        };
        pick(&mut ed, a, ChannelId::Rotation, &[3, 9]);
        // the playhead is on a frame with no key at all
        ed.go_to(20);
        assert_eq!(ed.key_interpolation(), Some(Some(Interpolation::Linear)));
        assert!(ed.can_run(Action::SetInterpolation(Interpolation::Step)));
        ed.run(Action::SetInterpolation(Interpolation::Step));
        assert_eq!(
            interps(&ed),
            [
                (3, Interpolation::Step),
                (6, Interpolation::Linear),
                (9, Interpolation::Step)
            ]
        );
        // keys that disagree report no single answer; one undo step puts them back
        pick(&mut ed, a, ChannelId::Rotation, &[3, 6]);
        assert_eq!(ed.key_interpolation(), Some(None));
        ed.select_key(
            KeyRef::new(Some(a), Some(ChannelId::Rotation), 99),
            KeySelect::Replace,
        );
        ed.clear_key_selection();
        ed.run(Action::Undo);
        assert_eq!(interps(&ed)[0].1, Interpolation::Linear);
        assert_eq!(interps(&ed)[2].1, Interpolation::Linear);
    }

    #[test]
    fn clipboards_follow_the_user_from_one_editor_to_another() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[3, 6]);
        pick(&mut ed, a, ChannelId::Rotation, &[3, 6]);
        assert!(ed.copy_keys());
        // another tab starts with nothing to paste and takes over the clipboards
        let (mut other, a2, _) = two();
        assert!(!other.has_key_clipboard());
        other.adopt_clipboards(ed.clipboards());
        assert!(other.has_key_clipboard());
        other.select(a2, super::super::SelectMode::Replace);
        other.go_to(10);
        assert!(other.paste_keys());
        assert_eq!(
            other
                .doc
                .find(a2)
                .unwrap()
                .transform
                .rotation
                .keys
                .iter()
                .map(|k| k.frame)
                .collect::<Vec<_>>(),
            [10, 13]
        );
    }

    // --- picking keys

    #[test]
    fn keys_are_picked_replaced_added_and_toggled() {
        let (mut ed, a, b) = two();
        key_rotation(&mut ed, a, &[3, 6]);
        key_rotation(&mut ed, b, &[3]);
        let rot = ChannelId::Rotation;
        let k = |l, f| KeyRef::new(Some(l), Some(rot), f);
        ed.select_key(k(a, 3), KeySelect::Replace);
        ed.select_key(k(a, 6), KeySelect::Add);
        assert_eq!(ed.selected_keys().len(), 2);
        ed.select_key(k(a, 6), KeySelect::Toggle);
        assert!(ed.is_key_selected(&k(a, 3)) && !ed.is_key_selected(&k(a, 6)));
        ed.select_key(k(b, 3), KeySelect::Replace);
        assert_eq!(ed.selected_keys().iter().collect::<Vec<_>>(), [&k(b, 3)]);
        // a key that is not there picks nothing, and a plain click clears
        ed.select_key(k(b, 9), KeySelect::Add);
        assert_eq!(ed.selected_keys().len(), 1);
        ed.select_key(k(b, 9), KeySelect::Replace);
        assert!(!ed.has_selected_keys());
    }

    #[test]
    fn a_summary_key_picks_the_property_keys_under_it() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[4]);
        ed.doc
            .find_mut(a)
            .unwrap()
            .opacity
            .set_key(4, 0.5, Interpolation::Linear);
        ed.select_key(KeyRef::new(Some(a), None, 4), KeySelect::Replace);
        assert_eq!(ed.selected_keys().len(), 3, "the summary, rotation and opacity");
        assert!(ed.is_key_selected(&KeyRef::new(Some(a), Some(ChannelId::Opacity), 4)));
        ed.select_key(KeyRef::new(Some(a), None, 4), KeySelect::Toggle);
        assert!(!ed.has_selected_keys());
    }

    #[test]
    fn a_box_picks_the_keys_of_its_rows_within_its_frames() {
        let (mut ed, a, b) = two();
        key_rotation(&mut ed, a, &[2, 5, 9]);
        key_rotation(&mut ed, b, &[5]);
        // dragged right to left, over A's property row only
        ed.select_keys_in(&[(Some(a), Some(ChannelId::Rotation))], 6, 2, false);
        let picked: Vec<Frame> = ed.selected_keys().iter().map(|k| k.frame).collect();
        assert_eq!(picked, [2, 5]);
        // over B's summary row, adding: its summary key and its property key
        ed.select_keys_in(&[(Some(b), None)], 0, 10, true);
        assert_eq!(ed.selected_keys().len(), 4);
        assert!(ed.is_key_selected(&KeyRef::new(Some(b), None, 5)));
        ed.select_all_keys();
        assert!(ed.is_key_selected(&KeyRef::new(Some(a), Some(ChannelId::Rotation), 9)));
        ed.clear_key_selection();
        assert!(!ed.has_selected_keys());
    }

    #[test]
    fn the_pick_follows_undo_and_layer_deletion() {
        let (mut ed, a, b) = two();
        key_rotation(&mut ed, a, &[3]);
        key_rotation(&mut ed, b, &[3]);
        ed.select_all_keys();
        assert!(ed.selected_keys().iter().any(|k| k.layer == Some(b)));
        ed.select(b, SelectMode::Replace);
        ed.run(Action::DeleteLayers);
        assert!(ed.selected_keys().iter().all(|k| k.layer == Some(a)));
        assert!(ed.has_selected_keys());
        // deleting the picked key unpicks it, and undo brings the key back
        pick(&mut ed, a, ChannelId::Rotation, &[3]);
        ed.run(Action::DeleteKey);
        assert!(!ed.has_selected_keys());
        ed.run(Action::Undo);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [3]);
        // an undo that takes a picked key away unpicks it
        ed.select_key(
            KeyRef::new(Some(a), Some(ChannelId::Rotation), 3),
            KeySelect::Replace,
        );
        ed.run(Action::Undo);
        assert!(ed.selected_keys().iter().all(|k| key_exists(&ed.doc, k)));
    }

    // --- moving, copying and scaling

    #[test]
    fn picked_keys_move_together_in_one_undo_step() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[3, 6]);
        ed.doc
            .find_mut(a)
            .unwrap()
            .transform
            .position
            .set_key(6, Vec2::X, Interpolation::Linear);
        ed.select_key(
            KeyRef::new(Some(a), Some(ChannelId::Rotation), 3),
            KeySelect::Replace,
        );
        ed.select_key(KeyRef::new(Some(a), Some(ChannelId::Position), 6), KeySelect::Add);
        assert!(ed.begin_selected_key_drag(false));
        ed.drag_selected_keys_by(2);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [5, 6]);
        assert_eq!(frames(&ed, a, ChannelId::Position), [8]);
        // onto the key on 6: it is replaced, and the pick follows
        ed.drag_selected_keys_by(3);
        assert_eq!(rotation(&ed, a), [(6, 3.0)]);
        assert!(ed.is_key_selected(&KeyRef::new(Some(a), Some(ChannelId::Rotation), 6)));
        // never before frame 0
        ed.drag_selected_keys_by(-40);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [0, 6]);
        assert_eq!(frames(&ed, a, ChannelId::Position), [3]);
        // back where it began, the key it had replaced comes back
        ed.drag_selected_keys_by(0);
        assert_eq!(rotation(&ed, a), [(3, 3.0), (6, 6.0)]);
        ed.drag_selected_keys_by(4);
        ed.end_selected_key_drag();
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [6, 7]);
        ed.run(Action::Undo);
        assert_eq!(rotation(&ed, a), [(3, 3.0), (6, 6.0)]);
        assert_eq!(frames(&ed, a, ChannelId::Position), [6]);
    }

    #[test]
    fn a_drag_that_goes_nowhere_records_nothing() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[3]);
        pick(&mut ed, a, ChannelId::Rotation, &[3]);
        let before = ed.history.undo_label().map(str::to_owned);
        assert!(ed.begin_selected_key_drag(false));
        ed.drag_selected_keys_by(0);
        ed.end_selected_key_drag();
        assert_eq!(ed.history.undo_label().map(str::to_owned), before);
        ed.clear_key_selection();
        assert!(!ed.begin_selected_key_drag(false), "nothing is picked");
    }

    #[test]
    fn keys_move_in_either_direction_without_overwriting_each_other() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[10, 11, 12]);
        pick(&mut ed, a, ChannelId::Rotation, &[10, 11]);
        assert!(ed.begin_selected_key_drag(false));
        ed.drag_selected_keys_by(1);
        assert_eq!(
            rotation(&ed, a),
            [(11, 10.0), (12, 11.0)],
            "the unpicked 12 was in the way"
        );
        ed.drag_selected_keys_by(-1);
        assert_eq!(rotation(&ed, a), [(9, 10.0), (10, 11.0), (12, 12.0)]);
        ed.end_selected_key_drag();
    }

    #[test]
    fn a_copy_drag_leaves_the_originals() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[3, 6]);
        ed.select_key(KeyRef::new(Some(a), None, 3), KeySelect::Replace);
        assert!(ed.begin_selected_key_drag(true));
        ed.drag_selected_keys_by(10);
        ed.end_selected_key_drag();
        assert_eq!(rotation(&ed, a), [(3, 3.0), (6, 6.0), (13, 3.0)]);
        assert!(ed.is_key_selected(&KeyRef::new(Some(a), Some(ChannelId::Rotation), 13)));
        assert!(!ed.is_key_selected(&KeyRef::new(Some(a), Some(ChannelId::Rotation), 3)));
        ed.run(Action::Undo);
        assert_eq!(rotation(&ed, a), [(3, 3.0), (6, 6.0)]);
    }

    #[test]
    fn the_camera_keys_move_with_the_pick() {
        let (mut ed, ..) = two();
        ed.doc.camera.zoom.set_key(2, 1.0, Interpolation::Linear);
        ed.doc.camera.zoom.set_key(8, 2.0, Interpolation::Linear);
        ed.select_key(KeyRef::new(None, None, 8), KeySelect::Replace);
        assert!(ed.begin_selected_key_drag(false));
        ed.drag_selected_keys_by(5);
        ed.end_selected_key_drag();
        assert_eq!(ed.doc.camera.key_frames(), [2, 13]);
        assert_eq!(
            KeyRef::new(None, None, 1).target(),
            KeyTarget::Camera,
            "no layer is the camera"
        );
    }

    #[test]
    fn scaling_stretches_key_times_about_the_anchor_in_one_undo_step() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[10, 20, 30]);
        pick(&mut ed, a, ChannelId::Rotation, &[10, 20, 30]);
        assert!(ed.begin_key_scale(10));
        assert_eq!(ed.key_scale_anchor(), Some(10));
        ed.scale_selected_keys(10, 30, 50);
        assert_eq!(rotation(&ed, a), [(10, 10.0), (30, 20.0), (50, 30.0)]);
        // each call works from the original positions
        ed.scale_selected_keys(10, 30, 20);
        assert_eq!(rotation(&ed, a), [(10, 10.0), (15, 20.0), (20, 30.0)]);
        // squeezed onto the anchor, the later key wins
        ed.scale_selected_keys(10, 30, 10);
        assert_eq!(rotation(&ed, a), [(10, 30.0)]);
        ed.scale_selected_keys(10, 30, 40);
        ed.end_key_scale();
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [10, 25, 40]);
        ed.run(Action::Undo);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [10, 20, 30]);
    }

    #[test]
    fn scaling_never_takes_a_key_below_frame_zero() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[5, 10]);
        pick(&mut ed, a, ChannelId::Rotation, &[5, 10]);
        assert!(ed.begin_key_scale(10));
        // the key on 5 would land far below 0
        ed.scale_selected_keys(10, 5, -90);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [0, 10]);
        ed.end_key_scale();
    }

    // --- add and delete on the pick

    #[test]
    fn add_and_delete_key_act_on_the_picked_keys() {
        let (mut ed, a, b) = two();
        key_rotation(&mut ed, a, &[4]);
        ed.select(b, SelectMode::Replace);
        // with nothing picked they act on the selected layer, as they always did
        ed.go_to(6);
        ed.run(Action::AddKey);
        assert!(ed.doc.find(b).unwrap().transform.position.is_animated());
        assert!(!ed.doc.find(a).unwrap().transform.position.is_animated());
        // with a key picked, they act on its property
        pick(&mut ed, a, ChannelId::Rotation, &[4]);
        ed.go_to(9);
        assert!(ed.can_run(Action::AddKey));
        ed.run(Action::AddKey);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [0, 4, 9]);
        assert!(
            !ed.doc.find(a).unwrap().transform.position.is_animated(),
            "only the picked property"
        );
        ed.select_keys_in(&[(Some(a), Some(ChannelId::Rotation))], 4, 9, false);
        ed.run(Action::DeleteKey);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [0]);
        assert!(!ed.has_selected_keys());
        ed.run(Action::Undo);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [0, 4, 9]);
    }

    // --- the key clipboard

    #[test]
    fn copied_keys_paste_at_the_playhead_in_one_undo_step() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[5, 8]);
        ed.doc
            .find_mut(a)
            .unwrap()
            .opacity
            .set_key(8, 0.25, Interpolation::Step);
        assert!(!ed.copy_keys(), "nothing picked");
        assert!(!ed.has_key_clipboard());
        ed.select_keys_in(&[(Some(a), None)], 5, 8, false);
        assert!(ed.copy_keys());
        assert!(ed.has_key_clipboard());
        ed.go_to(20);
        let revision = ed.revision();
        assert!(ed.paste_keys());
        assert_eq!(ed.revision(), revision + 1, "one step");
        assert_eq!(rotation(&ed, a), [(5, 5.0), (8, 8.0), (20, 5.0), (23, 8.0)]);
        let l = ed.doc.find(a).unwrap();
        assert_eq!(
            l.opacity.keys.len(),
            2,
            "the opacity key came too, three frames after the first"
        );
        assert_eq!(
            (l.opacity.keys[1].frame, l.opacity.keys[1].interp),
            (23, Interpolation::Step)
        );
        // the pasted keys are the pick
        assert!(ed.is_key_selected(&KeyRef::new(Some(a), Some(ChannelId::Rotation), 23)));
        assert!(!ed.is_key_selected(&KeyRef::new(Some(a), Some(ChannelId::Rotation), 8)));
        // pasting again over them replaces what is beneath
        ed.go_to(21);
        assert!(ed.paste_keys());
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [5, 8, 20, 21, 23, 24]);
        ed.run(Action::Undo);
        ed.run(Action::Undo);
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [5, 8]);
    }

    #[test]
    fn cut_keys_removes_them_and_paste_brings_them_back() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[5, 8]);
        pick(&mut ed, a, ChannelId::Rotation, &[8]);
        assert!(ed.cut_keys());
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [5]);
        ed.go_to(2);
        assert!(ed.paste_keys());
        assert_eq!(frames(&ed, a, ChannelId::Rotation), [2, 5]);
    }

    #[test]
    fn keys_paste_onto_the_selected_layer_when_theirs_is_gone() {
        let (mut ed, a, b) = two();
        key_rotation(&mut ed, a, &[5, 8]);
        pick(&mut ed, a, ChannelId::Rotation, &[5, 8]);
        assert!(ed.copy_keys());
        // another document: the layer is not there
        let mut other = Editor::new(Document::new("o", 100.0, 100.0, 24.0));
        // ids that are not the copied layer's
        other.doc.new_layer_id();
        other.doc.new_layer_id();
        let id = other.doc.new_layer_id();
        other
            .doc
            .layers
            .push(Layer::new(id, "X", LayerKind::vector(vec![])));
        other.key_clip = ed.key_clip.clone();
        assert!(!other.paste_keys(), "no layer selected there to take them");
        other.select(id, SelectMode::Replace);
        other.go_to(10);
        assert!(other.paste_keys());
        assert_eq!(frames(&other, id, ChannelId::Rotation), [10, 13]);
        // and the camera's keys stay the camera's
        ed.doc.camera.zoom.set_key(3, 2.0, Interpolation::Linear);
        ed.select_key(
            KeyRef::new(None, Some(ChannelId::CameraZoom), 3),
            KeySelect::Replace,
        );
        assert!(ed.copy_keys());
        ed.select(b, SelectMode::Replace);
        ed.go_to(7);
        assert!(ed.paste_keys());
        assert_eq!(frames(&ed, b, ChannelId::Rotation), Vec::<Frame>::new());
        assert_eq!(ed.doc.camera.key_frames(), [3, 7]);
    }

    #[test]
    fn a_shape_that_does_not_fit_takes_no_point_keys() {
        use fanimate_core::Style;
        let (mut ed, a, b) = two();
        let rect =
            |name| Shape::rectangle(name, Vec2::ZERO, Vec2::splat(5.0), 0.0, Style::fill(Color::BLACK));
        let mut round = rect("c");
        round.path = Path::circle(Vec2::ZERO, 5.0);
        let path = round.path.clone();
        round.key_path(3, path, Interpolation::Linear);
        if let Some(LayerKind::Vector { shapes }) = ed.doc.find_mut(a).map(|l| &mut l.kind) {
            shapes.push(round);
        }
        if let Some(LayerKind::Vector { shapes }) = ed.doc.find_mut(b).map(|l| &mut l.kind) {
            shapes.push(rect("s"));
        }
        let key_count = |ed: &Editor, id| match &ed.doc.find(id).unwrap().kind {
            LayerKind::Vector { shapes } => shapes[0].path_keys.len(),
            _ => 0,
        };
        pick(&mut ed, a, ChannelId::Shape(0), &[3]);
        assert!(ed.copy_keys());
        // onto its own shape it fits
        ed.go_to(9);
        assert!(ed.paste_keys());
        assert_eq!(key_count(&ed, a), 2);
        // onto a rectangle, whose path has other commands, it does not
        ed.key_clip = ed.key_clip.take().map(|mut clip| {
            clip.keys[0].layer = Some(b);
            clip
        });
        assert!(!ed.paste_keys());
        assert_eq!(key_count(&ed, b), 0);
    }

    // --- looping

    #[test]
    fn loop_is_set_on_one_property_and_on_the_picked_keys_properties() {
        let (mut ed, a, _) = two();
        key_rotation(&mut ed, a, &[2, 8]);
        let rot = ChannelId::Rotation;
        assert!(ed.set_channel_loop(Some(a), rot, LoopMode::Absolute));
        assert_eq!(ed.channel_loop(Some(a), rot), LoopMode::Absolute);
        assert_eq!(ed.status.as_ref().map(|s| s.1.as_str()), Some("Loop: Repeat"));
        assert!(
            !ed.set_channel_loop(Some(a), rot, LoopMode::Absolute),
            "already so"
        );
        ed.run(Action::Undo);
        assert_eq!(ed.channel_loop(Some(a), rot), LoopMode::Off);
        // point keys cannot repeat
        assert!(!ed.can_loop(Some(a), ChannelId::Shape(0)));
        assert!(!ed.set_channel_loop(Some(a), ChannelId::Shape(0), LoopMode::Absolute));

        // the action follows the pick
        assert!(!ed.can_run(Action::SetLoop(LoopMode::Relative)));
        ed.doc
            .find_mut(a)
            .unwrap()
            .opacity
            .set_key(2, 1.0, Interpolation::Linear);
        ed.select_key(KeyRef::new(Some(a), None, 2), KeySelect::Replace);
        assert!(ed.can_run(Action::SetLoop(LoopMode::Relative)));
        ed.run(Action::SetLoop(LoopMode::Relative));
        let l = ed.doc.find(a).unwrap();
        assert_eq!(l.transform.rotation.loop_mode, LoopMode::Relative);
        assert_eq!(l.opacity.loop_mode, LoopMode::Relative);
        assert_eq!(
            l.transform.position.loop_mode,
            LoopMode::Off,
            "no key of it was picked"
        );
        ed.run(Action::Undo);
        assert_eq!(
            ed.doc.find(a).unwrap().transform.rotation.loop_mode,
            LoopMode::Off
        );
        // the camera repeats too
        ed.doc.camera.zoom.set_key(1, 1.0, Interpolation::Linear);
        ed.doc.camera.zoom.set_key(5, 2.0, Interpolation::Linear);
        assert!(ed.set_channel_loop(None, ChannelId::CameraZoom, LoopMode::Absolute));
        assert_eq!(ed.doc.camera.zoom.loop_mode, LoopMode::Absolute);
        assert_eq!(ed.doc.camera.zoom.sample(7.0), 1.5);
    }

    // --- layer order

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

    #[test]
    fn on_the_rest_pose_moving_a_layer_changes_the_list() {
        let (mut ed, [_, b, _]) = three();
        ed.select(b, SelectMode::Replace);
        ed.run(Action::MoveLayerUp);
        assert_eq!(name_order(&ed), ["A", "C", "B"]);
        assert!(ed.doc.layers.iter().all(|l| !l.order.is_animated()));
    }

    #[test]
    fn after_the_rest_pose_moving_a_layer_keys_the_order_and_keeps_the_list() {
        let (mut ed, [a, b, c]) = three();
        ed.select(b, SelectMode::Replace);
        ed.go_to(5);
        ed.run(Action::MoveLayerUp);
        assert_eq!(name_order(&ed), ["A", "B", "C"], "the list is as it was");
        assert_eq!(
            ed.status.as_ref().map(|s| s.1.as_str()),
            Some("Layer order keyed on frame 5")
        );
        let slot = |ed: &Editor, id: LayerId, f: f32| ed.doc.find(id).unwrap().order.sample(f);
        // frame 0 is as it was; from frame 5 C sits below B
        assert_eq!(
            [slot(&ed, a, 0.0), slot(&ed, b, 0.0), slot(&ed, c, 0.0)],
            [0.0, 1.0, 2.0]
        );
        assert_eq!(
            [slot(&ed, a, 5.0), slot(&ed, c, 5.0), slot(&ed, b, 5.0)],
            [0.0, 1.0, 2.0]
        );
        assert!(
            ed.doc
                .find(b)
                .unwrap()
                .order
                .keys
                .iter()
                .all(|k| k.interp == Interpolation::Step)
        );
        // one undo step
        ed.run(Action::Undo);
        assert!(ed.doc.layers.iter().all(|l| !l.order.is_animated()));
        // the top layer cannot go higher, and nothing is keyed
        ed.select(c, SelectMode::Replace);
        ed.run(Action::MoveLayerUp);
        assert!(ed.doc.layers.iter().all(|l| !l.order.is_animated()));
        // moving again on a later frame starts from the order as drawn there
        ed.select(b, SelectMode::Replace);
        ed.run(Action::MoveLayerUp);
        ed.go_to(9);
        ed.select(a, SelectMode::Replace);
        ed.run(Action::MoveLayerUp);
        // at 9: B was above C already? A swaps with its neighbour in the drawn order
        let drawn = ed.draw_order_ids(None, 9);
        assert_eq!(drawn, [c, a, b]);
        assert_eq!(ed.draw_order_ids(None, 5), [a, c, b]);
        assert_eq!(ed.draw_order_ids(None, 0), [a, b, c]);
        assert_eq!(ed.draw_order_ids(None, 3), [a, b, c]);
    }

    #[test]
    fn dropping_a_layer_after_the_rest_pose_keys_the_order() {
        let (mut ed, [a, b, c]) = three();
        ed.go_to(4);
        assert!(ed.move_relative(&[a], c, Placement::Above));
        assert_eq!(name_order(&ed), ["A", "B", "C"]);
        assert_eq!(ed.draw_order_ids(None, 4), [b, c, a]);
        assert_eq!(ed.draw_order_ids(None, 2), [a, b, c]);
        assert_eq!(ed.selection, vec![a]);
        // dropping where it already is changes nothing and records nothing
        let before = ed.history.undo_label().map(str::to_owned);
        assert!(!ed.move_relative(&[a], c, Placement::Above));
        assert_eq!(ed.history.undo_label().map(str::to_owned), before);
        ed.run(Action::Undo);
        assert!(ed.doc.layers.iter().all(|l| !l.order.is_animated()));
        // moving into a group is a change of structure, not of order over time
        ed.select(b, SelectMode::Replace);
        ed.run(Action::NewGroupLayer);
        let g = ed.primary().unwrap();
        assert!(ed.move_relative(&[b], g, Placement::Into));
        assert!(ed.doc.layers.iter().all(|l| l.id != b));
    }

    #[test]
    fn layers_inside_a_group_are_keyed_among_themselves() {
        let (mut ed, [a, b, c]) = three();
        ed.select(a, SelectMode::Replace);
        ed.select(b, SelectMode::Toggle);
        ed.run(Action::GroupLayers);
        let g = ed.primary().unwrap();
        ed.go_to(6);
        ed.select(a, SelectMode::Replace);
        ed.run(Action::MoveLayerUp);
        assert_eq!(ed.draw_order_ids(Some(g), 6), [b, a]);
        assert_eq!(ed.draw_order_ids(Some(g), 0), [a, b]);
        assert!(
            !ed.doc.find(c).unwrap().order.is_animated(),
            "C is in another list"
        );
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
    }
}
