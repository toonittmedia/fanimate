//! Moho projects (`.moho`, `.mohoproj`, and Anime Studio's `.anime` and
//! `.animeproj`).
//!
//! Since Moho 12 a project is a JSON document, `Project.mohoproj`; a
//! `.moho` file is a ZIP archive holding it and a preview picture. What
//! this reader relies on, learnt from inspecting project files:
//!
//! - The canvas is two units high, with the origin at its centre and y
//!   pointing up; angles are radians counter-clockwise. Fanimate measures
//!   in pixels from the top-left with y down, so every position is scaled
//!   by half the project height and flipped, and every angle negated.
//! - Animated values are `{when, val, interp}` lists. Frame 0 holds the
//!   rest value (for bones, the rest pose), which is Fanimate's frame 0
//!   too; the animation starts on frame 1 in both, so keys keep their
//!   frame numbers.
//! - Layers are listed bottom first, as Fanimate draws them.
//! - A vector layer (`MeshLayer`) holds points and curves through them;
//!   each curve point has a smoothness that sets how round the curve is
//!   there. Shapes fill and outline runs of curve segments.
//! - A bone's position and angle are relative to its parent bone, with
//!   the origin at the parent's base and x along it, as in Fanimate.
//!   Parents may come after their children, so bones are reordered.

use std::collections::HashMap;
use std::sync::Arc;

use fanimate_core::anim::{Animatable, Key};
use fanimate_core::bones::{Skin, auto_weights};
use fanimate_core::{
    Bone, Channel, Color, Document, Image, Interpolation, Layer, LayerKind, MediaFile, Path, Shape, Skeleton,
    Stroke, Style, Vec2,
};
use serde_json::Value;

use crate::Imported;
use crate::notes::{Kind, Notes};
use crate::zip::{Archive, is_zip};

const ACTION: Kind = (
    "action or Smart Bone left out",
    "actions and Smart Bones left out",
);
const AUDIO: Kind = ("sound layer left out", "sound layers left out");
const BLEND: Kind = ("blend mode drawn as normal", "blend modes drawn as normal");
const BONE_MOVE: Kind = (
    "bone that moves or stretches kept at its rest length and place",
    "bones that move or stretch kept at their rest length and place",
);
const CYCLE: Kind = ("cycle key held instead", "cycle keys held instead");
const IMAGE_MISSING: Kind = (
    "picture not found beside the project",
    "pictures not found beside the project",
);
const MASK: Kind = ("mask left out", "masks left out");
const OTHER_LAYER: Kind = (
    "layer of a kind Fanimate lacks left out",
    "layers of kinds Fanimate lacks left out",
);
const PARENT_SWITCH: Kind = (
    "bone that changes parent kept on its first",
    "bones that change parent kept on their first",
);
const TARGET: Kind = ("bone target left out", "bone targets left out");
const TEXT: Kind = ("text layer left out", "text layers left out");
const VISIBILITY: Kind = (
    "layer shown and hidden over time kept visible",
    "layers shown and hidden over time kept visible",
);

/// Read a Moho project. `beside` reads a file next to the project by
/// relative name, for the pictures image layers show.
pub fn import(
    name: &str,
    bytes: &[u8],
    beside: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Imported, String> {
    let archive = if is_zip(bytes) {
        Some(Archive::read(bytes)?)
    } else {
        None
    };
    let json = match &archive {
        Some(a) => {
            let entry = a
                .names()
                .find(|n| {
                    let n = n.to_ascii_lowercase();
                    n.ends_with(".mohoproj") || n.ends_with(".animeproj")
                })
                .map(str::to_owned)
                .ok_or("the Moho file has no project inside")?;
            a.get(&entry).ok_or("the Moho file has no project inside")??
        }
        None => bytes.to_vec(),
    };
    let text = std::str::from_utf8(&json).map_err(|_| old_format())?;
    let root: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| old_format())?;
    if root.get("layers").and_then(Value::as_array).is_none() {
        return Err(old_format());
    }
    let find = |file: &str| -> Option<Vec<u8>> {
        // a picture's path is usually the author's own, so try its name
        // in the archive and beside the project, then in an images folder
        let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let names = [
            base.to_owned(),
            format!("images/{base}"),
            format!("Images/{base}"),
        ];
        names.iter().find_map(|n| {
            archive
                .as_ref()
                .and_then(|a| a.get(n))
                .and_then(Result::ok)
                .or_else(|| beside(n))
        })
    };
    Reader::new(&root, &find).read(name, &root)
}

fn old_format() -> String {
    "this is not a Moho 12 or later project; open it in Moho and save it again, then import that".into()
}

struct Reader<'a> {
    /// Pixels per Moho unit: half the project height.
    scale: f32,
    notes: Notes,
    find: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    doc: Document,
    styles: HashMap<String, &'a Value>,
    /// Names of the actions found: each bone layer's layers repeat them.
    actions: std::collections::BTreeSet<String>,
}

/// The bone layer a layer is inside: its skeleton, and where each Moho
/// bone index went.
struct Rig {
    skeleton: Skeleton,
    index: Vec<Option<usize>>,
}

impl Rig {
    fn bone(&self, moho: i64) -> Option<usize> {
        usize::try_from(moho)
            .ok()
            .and_then(|i| self.index.get(i).copied().flatten())
    }
}

fn f(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32).filter(|x| x.is_finite())
}

fn vec2(v: &Value) -> Option<Vec2> {
    Some(Vec2::new(f(v.get("x")?)?, f(v.get("y")?)?))
}

fn color(v: &Value) -> Option<Color> {
    let c = |k: &str| f(v.get(k)?);
    let (r, g, b) = (c("r")?, c("g")?, c("b")?);
    let a = c("a").unwrap_or(1.0);
    // project colours are 0 to 255, style colours 0 to 1
    let max = r.max(g).max(b).max(a);
    let k = if max > 1.0 { 255.0 } else { 1.0 };
    Some(Color::rgba(r / k, g / k, b / k, a / k))
}

fn interp(im: u64, notes: &mut Notes) -> Interpolation {
    match im {
        0 | 4 => Interpolation::Linear,
        3 => Interpolation::Step,
        5 => {
            notes.add(CYCLE);
            Interpolation::Step
        }
        _ => Interpolation::Smooth,
    }
}

/// An animated value's keys as (Moho frame, value, interpolation code).
fn keys(v: Option<&Value>) -> Vec<(i64, &Value, u64)> {
    let Some(v) = v else { return Vec::new() };
    let (Some(when), Some(val)) = (
        v.get("when").and_then(Value::as_array),
        v.get("val").and_then(Value::as_array),
    ) else {
        return Vec::new();
    };
    let interp = v.get("interp").and_then(Value::as_array);
    when.iter()
        .zip(val)
        .enumerate()
        .filter_map(|(i, (w, x))| {
            let im = interp
                .and_then(|a| a.get(i))
                .and_then(|o| o.get("im"))
                .and_then(Value::as_u64)
                .unwrap_or(1);
            Some((w.as_f64()?.round() as i64, x, im))
        })
        .collect()
}

/// A value that may be animated, at rest: style colours and widths are
/// stored as animated values.
fn rest(v: &Value) -> &Value {
    keys(Some(v)).first().map_or(v, |k| k.1)
}

fn is_animated(v: Option<&Value>) -> bool {
    keys(v).iter().any(|k| k.0 > 0)
}

fn add(mut a: Channel<Vec2>, offset: Vec2) -> Channel<Vec2> {
    a.base += offset;
    for k in &mut a.keys {
        k.value += offset;
    }
    a
}

impl<'a> Reader<'a> {
    fn new(root: &'a Value, find: &'a dyn Fn(&str) -> Option<Vec<u8>>) -> Self {
        let data = root.get("project_data");
        let num = |k: &str, d: f32| data.and_then(|p| p.get(k)).and_then(f).unwrap_or(d);
        let (w, h) = (num("width", 1280.0).max(1.0), num("height", 720.0).max(1.0));
        let mut doc = Document::new("Imported", w, h, num("fps", 24.0).clamp(1.0, 240.0));
        // Moho counts from 1 as Fanimate does, and its frame 0 is the rest pose
        doc.start = (num("start_frame", 1.0) as i32).max(1);
        doc.end = (num("end_frame", 96.0) as i32).max(doc.start);
        if let Some(c) = data.and_then(|p| p.get("back_color")).and_then(color) {
            doc.background = c;
        }
        let styles = root
            .get("styles")
            .and_then(Value::as_array)
            .map(|s| {
                s.iter()
                    .filter_map(|st| Some((st.get("name")?.as_str()?.to_owned(), st)))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            scale: h / 2.0,
            notes: Notes::default(),
            find,
            doc,
            styles,
            actions: Default::default(),
        }
    }

    fn read(mut self, name: &str, root: &'a Value) -> Result<Imported, String> {
        self.doc.name = name.to_owned();
        let centre = self.doc.size() / 2.0;
        let mut out = Vec::new();
        for l in root.get("layers").and_then(Value::as_array).into_iter().flatten() {
            if let Some(layer) = self.layer(l, centre, None) {
                out.push(layer);
            }
        }
        if out.is_empty() {
            return Err("the Moho project has no layers Fanimate can import".into());
        }
        self.doc.layers = out;
        for _ in &self.actions {
            self.notes.add(ACTION);
        }
        Ok(Imported {
            doc: self.doc,
            notes: self.notes.phrases(),
        })
    }

    /// A position in Moho units as Fanimate pixels (no offset).
    fn px(&self, v: Vec2) -> Vec2 {
        Vec2::new(v.x * self.scale, -v.y * self.scale)
    }

    /// A Moho animated value as a channel, values converted by `conv`.
    fn channel<T: Animatable>(
        &mut self,
        v: Option<&Value>,
        fallback: T,
        conv: impl Fn(&Self, &Value) -> Option<T>,
    ) -> Channel<T> {
        let mut ch = Channel::new(fallback);
        let mut first = Interpolation::Smooth;
        for (when, value, im) in keys(v) {
            let Some(x) = conv(self, value) else { continue };
            let i = interp(im, &mut self.notes);
            if when <= 0 {
                ch.base = x;
                first = i;
            } else {
                ch.set_key(when.min(i64::from(i32::MAX)) as i32, x, i);
            }
        }
        // Moho moves from the rest value to the first key; Fanimate holds
        // the first key before it, so start the move on the rest frame, 0
        if let Some(k) = ch.keys.first()
            && k.frame > 0
            && k.value != ch.base
        {
            let base = ch.base;
            ch.set_key(0, base, first);
        }
        ch
    }

    fn layer(&mut self, l: &'a Value, shift: Vec2, rig: Option<&Rig>) -> Option<Layer> {
        let kind = l.get("type").and_then(Value::as_str).unwrap_or_default();
        let name = l
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Layer")
            .to_owned();
        let origin = self.px(l.get("origin").and_then(vec2).unwrap_or_default());
        let content = -origin;
        let mut picture = None;
        let kind = match kind {
            "MeshLayer" => LayerKind::vector(self.mesh(l, content, rig)),
            "GroupLayer" | "ParticleLayer" => LayerKind::Group {
                children: self.children(l, content, rig),
            },
            "BoneLayer" => {
                let made = self.skeleton(l, content);
                let children = self.children(l, content, Some(&made));
                LayerKind::Bone {
                    skeleton: made.skeleton,
                    children,
                }
            }
            "SwitchLayer" => {
                let children = self.children(l, content, rig);
                let switch = self.switch_keys(l);
                LayerKind::Switch { children, switch }
            }
            "ImageLayer" => {
                let (kind, size) = self.image(l, rig)?;
                picture = Some(size);
                kind
            }
            "AudioLayer" => {
                self.notes.add(AUDIO);
                return None;
            }
            "TextLayer" => {
                self.notes.add(TEXT);
                return None;
            }
            _ => {
                self.notes.add(OTHER_LAYER);
                return None;
            }
        };
        let mut layer = self.doc.new_layer(name, kind);
        layer.visible = l.get("visible").and_then(Value::as_bool).unwrap_or(true);
        // a picture is centred on its layer's origin, and an image layer's
        // picture starts at the layer's own origin, so the layer moves
        let centring = picture.map_or(Vec2::ZERO, |size| content - size / 2.0);
        self.transform(l, &mut layer, origin + shift + centring);
        self.effects(l, &mut layer);
        for a in l.get("actions").and_then(Value::as_array).into_iter().flatten() {
            let name = a.get("name").and_then(Value::as_str).unwrap_or_default();
            self.actions.insert(name.to_owned());
        }
        Some(layer)
    }

    fn children(&mut self, l: &'a Value, shift: Vec2, rig: Option<&Rig>) -> Vec<Layer> {
        l.get("layers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| self.layer(c, shift, rig))
            .collect()
    }

    fn transform(&mut self, l: &'a Value, layer: &mut Layer, offset: Vec2) {
        let t = l.get("transforms");
        let get = |k: &str| t.and_then(|t| t.get(k));
        let position = self.channel(get("translation"), Vec2::ZERO, |r, v| vec2(v).map(|p| r.px(p)));
        layer.transform.position = add(position, offset);
        layer.transform.rotation = self.channel(get("rotation_z"), 0.0, |_, v| f(v).map(|a| -a));
        let flip = |k: &str| {
            keys(get(k))
                .first()
                .and_then(|x| x.1.as_bool())
                .map_or(1.0, |b| if b { -1.0 } else { 1.0 })
        };
        let mirror = Vec2::new(flip("flip_h"), flip("flip_v"));
        layer.transform.scale = self.channel(get("scale"), Vec2::ONE, |_, v| vec2(v).map(|s| s * mirror));
    }

    fn effects(&mut self, l: &'a Value, layer: &mut Layer) {
        let fx = l.get("layer_effects");
        layer.opacity = self.channel(fx.and_then(|e| e.get("alpha")), 1.0, |_, v| {
            f(v).map(|a| a.clamp(0.0, 1.0))
        });
        if is_animated(fx.and_then(|e| e.get("visibility"))) {
            self.notes.add(VISIBILITY);
        }
        if l.get("masking").and_then(Value::as_i64).unwrap_or(0) != 0 {
            self.notes.add(MASK);
        }
        if l.get("blend_mode").and_then(Value::as_i64).unwrap_or(0) != 0 {
            self.notes.add(BLEND);
        }
    }

    fn switch_keys(&mut self, l: &'a Value) -> Vec<Key<String>> {
        // the switch channel is the layer's animated text value
        let channel = l
            .as_object()
            .into_iter()
            .flatten()
            .find(|(k, v)| k.contains("switch") && v.get("type").and_then(Value::as_str) == Some("String"))
            .map(|(_, v)| v);
        let mut out: Vec<Key<String>> = Vec::new();
        for (when, value, im) in keys(channel) {
            let Some(name) = value.as_str() else { continue };
            let frame = when.clamp(0, i64::from(i32::MAX)) as i32;
            let interp = interp(im, &mut self.notes);
            match out.iter_mut().find(|k| k.frame == frame) {
                // a later key on the same frame (a repeated or negative
                // frame number) wins
                Some(k) => k.value = name.to_owned(),
                None => out.push(Key {
                    frame,
                    value: name.to_owned(),
                    interp,
                }),
            }
        }
        out.sort_by_key(|k| k.frame);
        out
    }

    fn skeleton(&mut self, l: &'a Value, shift: Vec2) -> Rig {
        let bones = l
            .get("skeleton")
            .and_then(|s| s.get("bones"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let parent_of = |b: &Value| {
            b.get("parent")
                .and_then(Value::as_i64)
                .and_then(|p| usize::try_from(p).ok())
                .filter(|p| *p < bones.len())
        };
        // parents first: place bones whose parent is placed, until none
        // can be; a loop of parents leaves the rest as roots
        let mut index: Vec<Option<usize>> = vec![None; bones.len()];
        let mut order = Vec::new();
        while order.len() < bones.len() {
            let before = order.len();
            for (i, b) in bones.iter().enumerate() {
                if index[i].is_none() && parent_of(b).is_none_or(|p| index[p].is_some()) {
                    index[i] = Some(order.len());
                    order.push(i);
                }
            }
            if order.len() == before {
                for (i, slot) in index.iter_mut().enumerate() {
                    if slot.is_none() {
                        *slot = Some(order.len());
                        order.push(i);
                    }
                }
            }
        }
        let mut skeleton = Skeleton::default();
        for &i in &order {
            let b = &bones[i];
            let parent = parent_of(b)
                .and_then(|p| index[p])
                .filter(|p| *p < skeleton.bones.len());
            let pos = keys(b.get("anim_pos"))
                .first()
                .and_then(|k| vec2(k.1))
                .map(|p| self.px(p))
                .unwrap_or_default();
            let origin = if parent.is_some() { pos } else { pos + shift };
            let length = b.get("length").and_then(f).unwrap_or(0.0).abs() * self.scale;
            let name = b.get("name").and_then(Value::as_str).unwrap_or("Bone");
            let mut bone = Bone::new(name, parent, origin, length, 0.0);
            bone.angle = self.channel(b.get("anim_angle"), 0.0, |_, v| f(v).map(|a| -a));
            if b.get("constraints").and_then(Value::as_bool) == Some(true)
                && let (Some(lo), Some(hi)) = (
                    b.get("min_constraint").and_then(f),
                    b.get("max_constraint").and_then(f),
                )
            {
                bone.limits = Some([-hi.max(lo), -lo.min(hi)]);
            }
            if let Some(s) = b.get("strength").and_then(f).filter(|s| *s > 0.0) {
                bone.strength = Some(s * self.scale);
            }
            if is_animated(b.get("anim_pos")) || is_animated(b.get("anim_scale")) {
                self.notes.add(BONE_MOVE);
            }
            if is_animated(b.get("anim_parent")) {
                self.notes.add(PARENT_SWITCH);
            }
            if b.get("target_bone")
                .and_then(Value::as_f64)
                .is_some_and(|t| t >= 0.0)
            {
                self.notes.add(TARGET);
            }
            skeleton.bones.push(bone);
        }
        Rig { skeleton, index }
    }

    /// An image layer's picture, and its size.
    fn image(&mut self, l: &'a Value, rig: Option<&Rig>) -> Option<(LayerKind, Vec2)> {
        let path = l.get("image_path").and_then(Value::as_str).unwrap_or_default();
        let found = (!path.is_empty()).then(|| (self.find)(path)).flatten();
        let Some(bytes) = found else {
            self.notes.add(IMAGE_MISSING);
            return None;
        };
        let ext = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Png) => "png",
            Ok(image::ImageFormat::Jpeg) => "jpg",
            _ => {
                self.notes.add(IMAGE_MISSING);
                return None;
            }
        };
        let Ok(decoded) = image::load_from_memory(&bytes) else {
            self.notes.add(IMAGE_MISSING);
            return None;
        };
        let size = Vec2::new(decoded.width() as f32, decoded.height() as f32);
        let name = MediaFile::name_for(&bytes, ext);
        self.doc.media.insert(name.clone(), Arc::new(MediaFile { bytes }));
        let mut img = Image::new(name, size.x, size.y);
        img.bone = rig.and_then(|r| r.bone(l.get("parent_bone").and_then(Value::as_i64).unwrap_or(-1)));
        Some((LayerKind::Image(img), size))
    }

    fn mesh(&mut self, l: &'a Value, shift: Vec2, rig: Option<&Rig>) -> Vec<Shape> {
        let Some(mesh) = l.get("mesh") else {
            return Vec::new();
        };
        let points: Vec<&Value> = mesh
            .get("points")
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        let tracks: Vec<Channel<Vec2>> = points
            .iter()
            .map(|p| {
                let ch = self.channel(p.get("position"), Vec2::ZERO, |r, v| vec2(v).map(|q| r.px(q)));
                add(ch, shift)
            })
            .collect();
        let curves: Vec<Curve> = mesh
            .get("curves")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|c| Curve::read(c, points.len()))
            .collect();
        let layer_bone = rig.and_then(|r| r.bone(l.get("parent_bone").and_then(Value::as_i64).unwrap_or(-1)));
        let point_bone: Vec<Option<usize>> = points
            .iter()
            .map(|p| rig.and_then(|r| r.bone(p.get("parent").and_then(Value::as_i64).unwrap_or(-1))))
            .collect();
        // every frame on which any point is keyed
        let mut frames: Vec<i32> = tracks
            .iter()
            .flat_map(|t| t.keys.iter().map(|k| k.frame))
            .collect();
        frames.sort_unstable();
        frames.dedup();

        let mut shapes = Vec::new();
        for (n, s) in mesh
            .get("shapes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let fill = s.get("has_fill").and_then(Value::as_bool).unwrap_or(true);
            let outline = s.get("has_outline").and_then(Value::as_bool).unwrap_or(false);
            let edges = edge_list(s);
            let at_rest: Vec<Vec2> = tracks.iter().map(|t| t.base).collect();
            let Some((path, order)) = build_path(&curves, &at_rest, &edges, fill) else {
                continue;
            };
            let style = self.style(s, fill, outline);
            if style.fill.is_none() && style.stroke.is_none() {
                continue;
            }
            let name = s
                .get("name")
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
                .map_or_else(|| format!("Shape {}", n + 1), str::to_owned);
            let mut shape = Shape::new(name, path, style);
            for &frame in &frames {
                let at: Vec<Vec2> = tracks.iter().map(|t| t.sample(frame as f32)).collect();
                if let Some((p, _)) = build_path(&curves, &at, &edges, fill)
                    && p.cmds.len() == shape.path.cmds.len()
                {
                    shape.path_keys.push(Key {
                        frame,
                        value: p,
                        interp: Interpolation::Linear,
                    });
                }
            }
            if order.iter().any(|&p| point_bone[p].is_some()) {
                let skin: Skin = order
                    .iter()
                    .map(|&p| {
                        point_bone[p]
                            .or(layer_bone)
                            .map(|b| vec![(b, 1.0)])
                            .unwrap_or_default()
                    })
                    .collect();
                shape.skin = Some(skin);
            } else if layer_bone.is_some() {
                shape.bone = layer_bone;
            } else if let Some(r) = rig
                && !r.skeleton.bones.is_empty()
            {
                // flexible binding: points follow the bones near them
                let pts: Vec<Vec2> = order.iter().map(|&p| at_rest[p]).collect();
                shape.skin = Some(auto_weights(&r.skeleton, &pts, 2));
            }
            shapes.push(shape);
        }
        shapes
    }

    fn style(&mut self, s: &'a Value, fill: bool, outline: bool) -> Style {
        let own = s.get("style");
        let inherited = s
            .get("inherited_style_name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
            .and_then(|n| self.styles.get(n).copied());
        let pick = |key: &str, define: &str| {
            // a defining library style wins over the shape's own
            inherited
                .filter(|st| st.get(define).and_then(Value::as_bool) == Some(true))
                .and_then(|st| st.get(key))
                .or_else(|| own.and_then(|o| o.get(key)))
                .map(rest)
        };
        let fill = fill
            .then(|| pick("fill_color", "define_fill_color").and_then(color))
            .flatten()
            .filter(|c| c.a > 0.0);
        let stroke = outline
            .then(|| {
                let color = pick("line_color", "define_line_col").and_then(color)?;
                let width = pick("line_width", "define_line_width").and_then(f)? * self.scale;
                (color.a > 0.0 && width > 0.0).then_some(Stroke { color, width })
            })
            .flatten();
        Style { fill, stroke }
    }
}

/// One curve point: which mesh point it uses and how round it is there.
struct CurvePoint {
    point: usize,
    smooth: f32,
    weight: [f32; 2],
    offset: [f32; 2],
}

struct Curve {
    points: Vec<CurvePoint>,
    closed: bool,
}

/// How long a handle is, as a share of the distance to the neighbouring
/// point, at smoothness 1. Moho's default smoothness, 0.3, then draws
/// four points as a near-perfect circle, as it does.
const HANDLE: f32 = 1.3;

impl Curve {
    fn read(c: &Value, points: usize) -> Self {
        let pts = c
            .get("points")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let point = p.get("point")?.as_u64()? as usize;
                (point < points).then(|| {
                    let num = |k: &str, d: f32| p.get(k).and_then(f).unwrap_or(d);
                    CurvePoint {
                        point,
                        smooth: num("smoothness", 0.3).clamp(0.0, 2.0),
                        weight: [num("weight_in", 1.0), num("weight_out", 1.0)],
                        offset: [num("offset_in", 0.0), num("offset_out", 0.0)],
                    }
                })
            })
            .collect();
        Self {
            points: pts,
            closed: c.get("closed").and_then(Value::as_bool).unwrap_or(false),
        }
    }

    /// Segment `i` (from curve point `i` to the next) as a cubic: start
    /// point index, end point index, and the four control points.
    fn segment(&self, i: usize, at: &[Vec2]) -> Option<(usize, usize, [Vec2; 4])> {
        let n = self.points.len();
        let j = if i + 1 < n {
            i + 1
        } else if self.closed && n > 2 {
            0
        } else {
            return None;
        };
        let pos = |k: usize| at[self.points[k].point];
        let neighbour = |k: usize, step: isize| -> Option<usize> {
            let m = k as isize + step;
            if (0..n as isize).contains(&m) {
                Some(m as usize)
            } else if self.closed && n > 2 {
                Some(m.rem_euclid(n as isize) as usize)
            } else {
                None
            }
        };
        let handle = |k: usize, out: bool| -> Vec2 {
            let p = &self.points[k];
            let here = pos(k);
            let prev = neighbour(k, -1).map(pos);
            let next = neighbour(k, 1).map(pos);
            let dir = match (prev, next) {
                (Some(a), Some(b)) => (b - a).normalize_or_zero(),
                (None, Some(b)) => (b - here).normalize_or_zero(),
                (Some(a), None) => (here - a).normalize_or_zero(),
                (None, None) => Vec2::ZERO,
            };
            let (side, w, o) = if out {
                (next, p.weight[1], p.offset[1])
            } else {
                (prev, p.weight[0], p.offset[0])
            };
            let reach = side.map_or(0.0, |s| (s - here).length()) * p.smooth * HANDLE / 2.0 * w;
            let dir = Vec2::from_angle(-o).rotate(dir);
            if out {
                here + dir * reach
            } else {
                here - dir * reach
            }
        };
        Some((
            self.points[i].point,
            self.points[j].point,
            [pos(i), handle(i, true), handle(j, false), pos(j)],
        ))
    }
}

/// A shape's edges as (curve, segment) pairs.
fn edge_list(s: &Value) -> Vec<(usize, usize)> {
    let e = s.get("edges");
    let list = |k: &str| -> Vec<usize> {
        e.and_then(|e| e.get(k))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_u64().map(|x| x as usize))
            .collect()
    };
    list("curve").into_iter().zip(list("segment")).collect()
}

/// The path through a shape's edges, joined end to end into closed loops
/// (or open runs for a line), and the mesh point behind each on-curve
/// point, for binding.
fn build_path(
    curves: &[Curve],
    at: &[Vec2],
    edges: &[(usize, usize)],
    fill: bool,
) -> Option<(Path, Vec<usize>)> {
    let mut segs: Vec<(usize, usize, [Vec2; 4])> = edges
        .iter()
        .filter_map(|&(c, s)| curves.get(c)?.segment(s, at))
        .collect();
    if segs.is_empty() {
        return None;
    }
    let mut path = Path::new();
    let mut order = Vec::new();
    while let Some(first) = (!segs.is_empty()).then(|| segs.remove(0)) {
        let start = first.0;
        path.cmds.push(fanimate_core::PathCmd::MoveTo(first.2[0]));
        order.push(first.0);
        let mut end = first.1;
        let push = |s: [Vec2; 4], path: &mut Path| {
            path.cmds.push(fanimate_core::PathCmd::CubicTo(s[1], s[2], s[3]));
        };
        push(first.2, &mut path);
        loop {
            if end == start {
                path.cmds.push(fanimate_core::PathCmd::Close);
                break;
            }
            order.push(end);
            let next = segs
                .iter()
                .position(|s| s.0 == end)
                .map(|i| (i, false))
                .or_else(|| segs.iter().position(|s| s.1 == end).map(|i| (i, true)));
            let Some((i, reversed)) = next else {
                if fill {
                    path.cmds.push(fanimate_core::PathCmd::Close);
                }
                break;
            };
            let (a, b, mut c) = segs.remove(i);
            if reversed {
                c.reverse();
                end = a;
            } else {
                end = b;
            }
            push(c, &mut path);
        }
    }
    Some((path, order))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn animated(vals: &[(i64, Value)]) -> Value {
        serde_json::json!({
            "when": vals.iter().map(|v| v.0).collect::<Vec<_>>(),
            "val": vals.iter().map(|v| v.1.clone()).collect::<Vec<_>>(),
            "interp": vals.iter().map(|_| serde_json::json!({"im": 1})).collect::<Vec<_>>(),
        })
    }

    fn xy(x: f64, y: f64) -> Value {
        serde_json::json!({"x": x, "y": y})
    }

    /// A 200x100 project with one bone layer holding a two-bone arm, the
    /// child listed before its parent, and a square bound to the upper arm.
    fn project() -> Value {
        let point = |x: f64, y: f64| serde_json::json!({"type": "Point", "position": animated(&[(0, xy(x, y)), (10, xy(x + 0.5, y))]), "parent": -2});
        let curve_point = |i: usize| serde_json::json!({"point": i, "smoothness": 0.0});
        serde_json::json!({
            "project_data": {"width": 200, "height": 100, "fps": 24.0, "start_frame": 1, "end_frame": 24,
                "back_color": {"r": 255, "g": 0, "b": 0, "a": 255}},
            "layers": [{
                "type": "BoneLayer", "name": "Rig", "origin": xy(0.0, 0.0),
                "skeleton": {"bones": [
                    {"name": "Lower", "parent": 1, "length": 0.5, "anim_pos": animated(&[(0, xy(0.5, 0.0))]),
                     "anim_angle": animated(&[(0, Value::from(0.0)), (5, Value::from(1.0))])},
                    {"name": "Upper", "parent": -1, "length": 0.5, "anim_pos": animated(&[(0, xy(0.0, 0.0))]),
                     "anim_angle": animated(&[(0, Value::from(0.5))])}
                ]},
                "layers": [{
                    "type": "MeshLayer", "name": "Square", "parent_bone": 1,
                    "transforms": {"translation": animated(&[(0, serde_json::json!({"x": 0.1, "y": 0.2, "z": 0.0}))])},
                    "mesh": {
                        "points": [point(0.0, 0.0), point(0.2, 0.0), point(0.2, 0.2), point(0.0, 0.2)],
                        "curves": [{"closed": true, "points": [curve_point(0), curve_point(1), curve_point(2), curve_point(3)]}],
                        "shapes": [{"has_fill": true, "has_outline": true,
                            "edges": {"curve": [0, 0, 0, 0], "segment": [0, 1, 2, 3]},
                            "style": {"fill_color": {"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0},
                                      "line_color": {"r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0}, "line_width": animated(&[(0, Value::from(0.02))])}}]
                    }
                }, {"type": "TextLayer", "name": "Words"}],
                "actions": [{"name": "Smile"}]
            }]
        })
    }

    fn read(v: &Value) -> Imported {
        import("Test", v.to_string().as_bytes(), &|_| None).unwrap()
    }

    #[test]
    fn project_settings_come_through() {
        let im = read(&project());
        let d = &im.doc;
        assert_eq!((d.width, d.height, d.fps), (200.0, 100.0, 24.0));
        assert_eq!((d.start, d.end), (1, 24));
        assert_eq!(d.background, Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(d.name, "Test");
    }

    #[test]
    fn bones_are_reordered_flipped_and_scaled() {
        let im = read(&project());
        let LayerKind::Bone { skeleton, children } = &im.doc.layers[0].kind else {
            panic!("not a bone layer")
        };
        let names: Vec<_> = skeleton.bones.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Upper", "Lower"]);
        assert_eq!(skeleton.bones[1].parent, Some(0));
        // half the height (50 px) per unit
        assert_eq!(skeleton.bones[1].origin, Vec2::new(25.0, 0.0));
        assert_eq!(skeleton.bones[0].length, 25.0);
        // counter-clockwise becomes clockwise
        assert_eq!(skeleton.bones[0].angle.base, -0.5);
        // keys keep Moho's frame numbers; the move starts on the rest frame, 0
        let keys: Vec<_> = skeleton.bones[1]
            .angle
            .keys
            .iter()
            .map(|k| (k.frame, k.value))
            .collect();
        assert_eq!(keys, [(0, 0.0), (5, -1.0)]);
        // the square follows the upper arm, renumbered
        let LayerKind::Vector { shapes } = &children[0].kind else {
            panic!()
        };
        assert_eq!(shapes[0].bone, Some(0));
    }

    #[test]
    fn shapes_and_point_keys_come_through() {
        let im = read(&project());
        let LayerKind::Bone { children, .. } = &im.doc.layers[0].kind else {
            panic!()
        };
        let square = &children[0];
        // translation (0.1, 0.2) is (5, -10) px
        assert_eq!(square.transform.position.base, Vec2::new(5.0, -10.0));
        let LayerKind::Vector { shapes } = &square.kind else {
            panic!()
        };
        let s = &shapes[0];
        assert_eq!(s.style.fill, Some(Color::rgb(0.0, 0.0, 1.0)));
        assert_eq!(s.style.stroke.map(|st| st.width), Some(1.0));
        // move, four sides, close
        assert_eq!(s.path.cmds.len(), 6);
        assert_eq!(s.path.cmds[0], fanimate_core::PathCmd::MoveTo(Vec2::ZERO));
        // the points move on Moho frame 10: a key on the rest frame and frame 10
        let frames: Vec<_> = s.path_keys.iter().map(|k| k.frame).collect();
        assert_eq!(frames, [0, 10]);
        assert!(
            s.path_keys
                .iter()
                .all(|k| k.value.cmds.len() == s.path.cmds.len())
        );
    }

    #[test]
    fn what_is_left_out_is_reported() {
        let im = read(&project());
        assert!(
            im.notes.iter().any(|n| n == "1 text layer left out"),
            "{:?}",
            im.notes
        );
        assert!(
            im.notes.iter().any(|n| n == "1 action or Smart Bone left out"),
            "{:?}",
            im.notes
        );
        // the root origin is the canvas centre
        assert_eq!(im.doc.layers[0].transform.position.base, Vec2::new(100.0, 50.0));
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        for bad in [
            &b""[..],
            b"{",
            b"[]",
            b"{\"layers\": 3}",
            b"PK\x03\x04junk",
            b"\xff\xfe",
        ] {
            assert!(import("x", bad, &|_| None).is_err());
        }
        // wrong types everywhere still read, or fail cleanly
        let odd = serde_json::json!({"layers": [{"type": "MeshLayer", "mesh": {"points": 5, "curves": [{"points": [{"point": 99}]}],
            "shapes": [{"edges": {"curve": [7], "segment": [3]}}]}},
            {"type": "BoneLayer", "skeleton": {"bones": [{"parent": 0}, {"parent": 1}]}}]});
        let _ = import("x", odd.to_string().as_bytes(), &|_| None);
    }

    #[test]
    fn a_moho_archive_is_read() {
        let json = project().to_string();
        let zip = crate::zip::write(&[("Project.mohoproj", json.as_bytes()), ("preview.jpg", b"")]).unwrap();
        let im = import("Zipped", &zip, &|_| None).unwrap();
        assert_eq!(im.doc.layers.len(), 1);
    }

    #[test]
    fn an_origin_pivots_the_layer() {
        // a layer's origin pivots it: content moves by minus the origin
        let v = serde_json::json!({"project_data": {"width": 100, "height": 100},
            "layers": [{"type": "GroupLayer", "name": "G", "origin": xy(0.2, 0.0), "layers": []}]});
        let im = read(&v);
        assert_eq!(im.doc.layers[0].transform.position.base, Vec2::new(60.0, 50.0));
    }
}
