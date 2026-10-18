//! Adobe Animate projects (`.fla`, `.xfl`).
//!
//! Since Flash CS5 a project is a folder of XML files (XFL): the main
//! timeline in `DOMDocument.xml`, each symbol in `LIBRARY/<name>.xml`, and
//! pictures in `LIBRARY` or `bin`. A `.fla` file is that folder in a ZIP
//! archive. What this reader relies on, learnt from inspecting files:
//!
//! - Coordinates are pixels with y down, as in Fanimate. Inside a shape's
//!   edge list they are twips (1/20 pixel), written in decimal or as `#`
//!   hexadecimal with an optional fraction in 256ths.
//! - A shape is a list of edges. Each edge run names the fill on its left
//!   (`fillStyle0`), the fill on its right (`fillStyle1`) and its line
//!   (`strokeStyle`); fills are rebuilt by joining every run that borders
//!   them into closed loops. Animate draws a shape's fills, then its lines.
//! - Layers are listed top first. A folder, mask or guide layer holds the
//!   layers whose `parentLayerIndex` points at it.
//! - A keyframe shows its elements, bottom first, until the next one.
//!   `tweenType="motion"` (a classic tween) moves a symbol from one
//!   keyframe's position to the next.
//!
//! A layer holding one symbol that moves between keyframes becomes a group
//! with the symbol's layers inside and the movement as transform keys, so
//! rigs of nested symbols stay rigs. A layer of drawings that change by
//! keyframe becomes a frame-by-frame layer.

use std::collections::HashMap;
use std::sync::Arc;

use fanimate_core::anim::KeyList;
use fanimate_core::{
    Affine2, Cel, Color, Document, Image, Interpolation, Layer, LayerKind, MediaFile, Path, PathCmd, Shape,
    Stroke, Style, Vec2,
};
use roxmltree::Node;

use crate::Imported;
use crate::notes::{Kind, Notes};
use crate::zip::{Archive, is_zip};

const BITMAP_FILL: Kind = ("picture fill became grey", "picture fills became grey");
const BLEND: Kind = ("blend mode drawn as normal", "blend modes drawn as normal");
const COLOR_EFFECT: Kind = (
    "tint or brightness effect left out",
    "tint and brightness effects left out",
);
const FILTER: Kind = ("filter left out", "filters left out");
const GRADIENT: Kind = ("gradient became a solid colour", "gradients became solid colours");
const LINE_STYLE: Kind = ("styled line drawn solid", "styled lines drawn solid");
const LOOP: Kind = (
    "symbol nested in itself left out",
    "symbols nested in themselves left out",
);
const MASK: Kind = ("mask left out", "masks left out");
const MISSING_SYMBOL: Kind = ("missing symbol left out", "missing symbols left out");
const MOTION_OBJECT: Kind = (
    "motion tween (the newer kind) held still",
    "motion tweens (the newer kind) held still",
);
const OTHER: Kind = (
    "element of a kind Fanimate lacks left out",
    "elements of kinds Fanimate lacks left out",
);
const PICTURE_FORMAT: Kind = (
    "picture in Animate's own format left out",
    "pictures in Animate's own format left out",
);
const PICTURE_IN_DRAWING: Kind = (
    "picture mixed into a drawing left out",
    "pictures mixed into drawings left out",
);
const SCRIPT: Kind = ("script left out", "scripts left out");
const SCENE: Kind = ("further scene left out", "further scenes left out");
const SHAPE_TWEEN: Kind = ("shape tween held still", "shape tweens held still");
const SKEW: Kind = (
    "skewed symbol drawn without its skew",
    "skewed symbols drawn without their skew",
);
const SOUND: Kind = ("sound left out", "sounds left out");
const TEXT: Kind = ("text item left out", "text items left out");

/// How deep symbols may nest before the rest is left out.
const MAX_DEPTH: usize = 24;

/// Read a `.fla` file.
pub fn import_fla(name: &str, bytes: &[u8]) -> Result<Imported, String> {
    if !is_zip(bytes) {
        return Err(
            "this .fla file was saved by Flash CS4 or earlier; open it in Animate and save it again, then import that"
                .into(),
        );
    }
    let archive = Archive::read(bytes)?;
    read(name, &|f| archive.get(f).and_then(Result::ok))
}

/// Read an XFL project: `files` reads a file of its folder by relative
/// name (`DOMDocument.xml`, `LIBRARY/...`).
pub fn import_xfl(name: &str, files: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Imported, String> {
    read(name, files)
}

fn parse(text: &str) -> Result<roxmltree::Document<'_>, String> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        ..Default::default()
    };
    roxmltree::Document::parse_with_options(text, options)
        .map_err(|e| format!("the Animate project is damaged: {e}"))
}

fn read(name: &str, files: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Imported, String> {
    let main =
        files("DOMDocument.xml").ok_or("the Animate project has no main timeline (DOMDocument.xml)")?;
    let text = String::from_utf8_lossy(&main);
    let xml = parse(&text)?;
    let root = xml.root_element();
    let num = |k: &str, d: f32| {
        root.attribute(k)
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(d)
    };
    let mut doc = Document::new(
        name,
        num("width", 550.0).max(1.0),
        num("height", 400.0).max(1.0),
        num("frameRate", 24.0).clamp(1.0, 240.0),
    );
    if let Some(c) = root
        .attribute("backgroundColor")
        .and_then(|c| Color::from_hex(c).ok())
    {
        doc.background = c;
    }
    let mut reader = Reader {
        files,
        doc,
        notes: Notes::default(),
        bitmaps: HashMap::new(),
        pictures: HashMap::new(),
        symbol_files: None,
        stack: Vec::new(),
    };
    for item in descendants(root, "DOMBitmapItem") {
        if let Some(n) = item.attribute("name") {
            reader.bitmaps.insert(
                n.to_owned(),
                (
                    item.attribute("href").unwrap_or(n).to_owned(),
                    item.attribute("bitmapDataHRef").map(str::to_owned),
                ),
            );
        }
    }
    let scenes: Vec<Node> = descendants(root, "DOMTimeline").collect();
    let Some(scene) = scenes.first() else {
        return Err("the Animate project has no timeline".into());
    };
    for _ in 1..scenes.len() {
        reader.notes.add(SCENE);
    }
    let length = timeline_length(*scene);
    // a project drawn by scripts has nothing on its timeline: it opens
    // empty, and the notes say why
    let layers = reader.timeline(*scene, 0, length);
    let mut doc = reader.doc;
    doc.layers = layers;
    doc.start = 0;
    doc.end = (length as i32 - 1).max(0);
    Ok(Imported {
        doc,
        notes: reader.notes.phrases(),
    })
}

fn elements<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(Node::is_element)
}

fn child<'a, 'i>(n: Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> {
    elements(n).find(|c| c.tag_name().name() == tag)
}

fn descendants<'a, 'i>(n: Node<'a, 'i>, tag: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.descendants().filter(move |c| c.tag_name().name() == tag)
}

fn attr_f(n: Node, k: &str, d: f32) -> f32 {
    n.attribute(k)
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(d)
}

fn attr_u(n: Node, k: &str, d: u32) -> u32 {
    n.attribute(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// An element's `<matrix>`, or the identity.
fn matrix(n: Node) -> Affine2 {
    child(n, "matrix")
        .and_then(|m| child(m, "Matrix"))
        .map_or(Affine2::IDENTITY, |m| {
            Affine2::from_cols_array(&[
                attr_f(m, "a", 1.0),
                attr_f(m, "b", 0.0),
                attr_f(m, "c", 0.0),
                attr_f(m, "d", 1.0),
                attr_f(m, "tx", 0.0),
                attr_f(m, "ty", 0.0),
            ])
        })
}

/// Frames in a timeline: the end of its longest layer.
fn timeline_length(tl: Node) -> u32 {
    descendants(tl, "DOMFrame")
        .map(|f| attr_u(f, "index", 0).saturating_add(attr_u(f, "duration", 1)))
        .max()
        .unwrap_or(1)
        .clamp(1, 1_000_000)
}

/// A keyframe: where it starts, how long it lasts, and its node.
struct Keyframe<'a, 'i> {
    index: u32,
    duration: u32,
    node: Node<'a, 'i>,
}

impl Keyframe<'_, '_> {
    fn elements(&self) -> Vec<Node<'_, '_>> {
        child(self.node, "elements")
            .map(|e| elements(e).collect())
            .unwrap_or_default()
    }
}

struct Reader<'f> {
    files: &'f dyn Fn(&str) -> Option<Vec<u8>>,
    doc: Document,
    notes: Notes,
    /// Library pictures by name: their file in `LIBRARY`, and in `bin`.
    bitmaps: HashMap<String, (String, Option<String>)>,
    /// Pictures already stored, by library name: media name and size.
    pictures: HashMap<String, Option<(String, Vec2)>>,
    /// Symbol files by symbol name, read when a symbol's file is not
    /// where its name says.
    symbol_files: Option<HashMap<String, String>>,
    /// Symbols being read, to stop a symbol nested in itself.
    stack: Vec<String>,
}

impl Reader<'_> {
    /// A symbol's XML text.
    fn symbol_text(&mut self, name: &str) -> Option<String> {
        let read = |r: &Self, file: &str| (r.files)(file).map(|b| String::from_utf8_lossy(&b).into_owned());
        if let Some(t) = read(self, &format!("LIBRARY/{name}.xml")) {
            return Some(t);
        }
        // names with characters a file name cannot hold are stored under
        // another file name: find it through the main timeline's list
        if self.symbol_files.is_none() {
            let mut map = HashMap::new();
            if let Some(main) = (self.files)("DOMDocument.xml") {
                let text = String::from_utf8_lossy(&main);
                if let Ok(xml) = parse(&text) {
                    for inc in descendants(xml.root_element(), "Include") {
                        let Some(href) = inc.attribute("href") else {
                            continue;
                        };
                        if let Some(t) = read(self, &format!("LIBRARY/{href}"))
                            && let Ok(sym) = parse(&t)
                            && let Some(n) = sym.root_element().attribute("name")
                        {
                            map.insert(n.to_owned(), href.to_owned());
                        }
                    }
                }
            }
            self.symbol_files = Some(map);
        }
        let href = self.symbol_files.as_ref()?.get(name)?.clone();
        read(self, &format!("LIBRARY/{href}"))
    }

    /// Run `f` on a symbol's timeline, guarding against loops and depth.
    fn with_symbol<T>(&mut self, name: &str, f: impl FnOnce(&mut Self, Node) -> T) -> Option<T> {
        if self.stack.iter().any(|s| s == name) || self.stack.len() >= MAX_DEPTH {
            self.notes.add(LOOP);
            return None;
        }
        let Some(text) = self.symbol_text(name) else {
            self.notes.add(MISSING_SYMBOL);
            return None;
        };
        let xml = parse(&text).ok()?;
        let tl = descendants(xml.root_element(), "DOMTimeline").next()?;
        self.stack.push(name.to_owned());
        let out = f(self, tl);
        self.stack.pop();
        Some(out)
    }

    /// A timeline's layers, bottom first. Keys are moved `offset` frames
    /// later; `length` is how long the timeline plays.
    fn timeline(&mut self, tl: Node, offset: i32, length: u32) -> Vec<Layer> {
        let layers: Vec<Node> = child(tl, "layers")
            .map(|l| elements(l).collect())
            .unwrap_or_default();
        self.layers_under(&layers, None, offset, length)
    }

    fn layers_under(&mut self, all: &[Node], parent: Option<usize>, offset: i32, length: u32) -> Vec<Layer> {
        let mut out = Vec::new();
        for (i, l) in all.iter().enumerate().rev() {
            let p = l
                .attribute("parentLayerIndex")
                .and_then(|v| v.parse::<usize>().ok());
            // a parent index that points nowhere sensible counts as none
            let p = p.filter(|&p| p < all.len() && p != i);
            if p != parent {
                continue;
            }
            if let Some(layer) = self.layer(all, i, offset, length) {
                out.push(layer);
            }
        }
        out
    }

    fn layer(&mut self, all: &[Node], i: usize, offset: i32, length: u32) -> Option<Layer> {
        let l = all[i];
        let name = l.attribute("name").unwrap_or("Layer").to_owned();
        let kind = l.attribute("layerType").unwrap_or("normal");
        let mut layer = match kind {
            "folder" | "mask" | "guide" => {
                if kind == "mask" && has_content(l) {
                    self.notes.add(MASK);
                }
                let children = self.layers_under(all, Some(i), offset, length);
                if children.is_empty() {
                    return None;
                }
                self.doc.new_layer(name, LayerKind::Group { children })
            }
            _ => self.content_layer(l, name, offset, length)?,
        };
        layer.visible = l.attribute("visible") != Some("false");
        Some(layer)
    }

    fn content_layer(&mut self, l: Node, name: String, offset: i32, length: u32) -> Option<Layer> {
        let frames = child(l, "frames")?;
        let mut keys: Vec<Keyframe> = descendants(frames, "DOMFrame")
            .map(|node| Keyframe {
                index: attr_u(node, "index", 0),
                duration: attr_u(node, "duration", 1).max(1),
                node,
            })
            .collect();
        keys.sort_by_key(|k| k.index);
        keys.dedup_by_key(|k| k.index);
        for k in &keys {
            match k.node.attribute("tweenType") {
                Some("shape") => self.notes.add(SHAPE_TWEEN),
                Some("motion object") => self.notes.add(MOTION_OBJECT),
                _ => {}
            }
            if k.node.attribute("soundName").is_some() {
                self.notes.add(SOUND);
            }
            if child(k.node, "Actionscript").is_some() {
                self.notes.add(SCRIPT);
            }
        }
        let shown: Vec<&Keyframe> = keys.iter().filter(|k| !k.elements().is_empty()).collect();
        if shown.is_empty() {
            return None;
        }
        // one symbol or picture on every keyframe that shows anything: a
        // group (or image) that moves
        let single = |tag: &str| -> Option<String> {
            let mut item = None;
            for k in &shown {
                let els = k.elements();
                let [el] = els.as_slice() else { return None };
                if el.tag_name().name() != tag {
                    return None;
                }
                let n = el.attribute("libraryItemName")?;
                if item.get_or_insert(n) != &n {
                    return None;
                }
            }
            item.map(str::to_owned)
        };
        if let Some(symbol) = single("DOMSymbolInstance") {
            return self.moving(&keys, &name, offset, length, Moving::Symbol(symbol));
        }
        if let Some(picture) = single("DOMBitmapInstance") {
            return self.moving(&keys, &name, offset, length, Moving::Picture(picture));
        }
        // drawings: one, or one per keyframe
        if keys.len() == 1 && keys[0].index == 0 {
            let mut shapes = Vec::new();
            self.flatten(&keys[0].elements(), Affine2::IDENTITY, 1.0, &mut shapes);
            return (!shapes.is_empty()).then(|| self.doc.new_layer(name, LayerKind::vector(shapes)));
        }
        let mut cels = Vec::new();
        for k in &keys {
            let mut cel = Cel::new(k.index as i32 + offset);
            self.flatten(&k.elements(), Affine2::IDENTITY, 1.0, &mut cel.line);
            if cel.frame >= 0 {
                cels.push(cel);
            }
        }
        // a layer that ends before the timeline does shows nothing after
        if let Some(last) = keys.last() {
            let end = last.index + last.duration;
            if end < length {
                cels.push(Cel::new(end as i32 + offset));
            }
        }
        cels.dedup_by_key(|c| c.frame);
        (!cels.is_empty()).then(|| self.doc.new_layer(name, LayerKind::Frames { cels }))
    }

    /// A layer whose one symbol or picture moves from keyframe to keyframe.
    fn moving(
        &mut self,
        keys: &[Keyframe],
        name: &str,
        offset: i32,
        length: u32,
        what: Moving,
    ) -> Option<Layer> {
        let first = keys.iter().find(|k| !k.elements().is_empty())?;
        let first_el = first.elements()[0];
        let mut layer = match &what {
            Moving::Symbol(symbol) => {
                // a graphic symbol plays from its first frame where the
                // layer first shows it
                let start = attr_u(first_el, "firstFrame", 0) as i32;
                let inner_offset = offset + first.index as i32 - start;
                let symbol_len = length.saturating_sub(first.index);
                let children = self
                    .with_symbol(symbol, |r, tl| {
                        let own = timeline_length(tl).max(symbol_len);
                        r.timeline(tl, inner_offset, own)
                    })
                    .unwrap_or_default();
                if children.is_empty() {
                    return None;
                }
                self.doc.new_layer(name, LayerKind::Group { children })
            }
            Moving::Picture(picture) => {
                let (media, size) = self.picture(picture)?;
                self.doc
                    .new_layer(name, LayerKind::Image(Image::new(media, size.x, size.y)))
            }
        };
        let mut angle_before: Option<f32> = None;
        let mut turns = 0.0;
        let mut alpha_keys = Vec::new();
        let animated = keys.len() > 1;
        for (n, k) in keys.iter().enumerate() {
            let frame = k.index as i32 + offset;
            let interp = match k.node.attribute("tweenType") {
                Some("motion") if attr_f(k.node, "acceleration", 0.0) != 0.0 => Interpolation::Smooth,
                Some("motion") => Interpolation::Linear,
                _ => Interpolation::Step,
            };
            let els = k.elements();
            let Some(el) = els.first() else {
                alpha_keys.push((frame, 0.0, Interpolation::Step));
                continue;
            };
            self.element_effects(*el);
            let m = matrix(*el);
            let (pos, mut angle, scale, skewed) = decompose(m);
            if skewed {
                self.notes.add(SKEW);
            }
            // turn the short way from the last key, plus any whole turns the
            // tween asks for
            if let Some(before) = angle_before {
                angle = before + wrap(angle - before) + turns;
            }
            turns = match k.node.attribute("motionTweenRotate") {
                Some("cw") => std::f32::consts::TAU * attr_f(k.node, "motionTweenRotateTimes", 0.0),
                Some("ccw") => -std::f32::consts::TAU * attr_f(k.node, "motionTweenRotateTimes", 0.0),
                _ => 0.0,
            };
            angle_before = Some(angle);
            alpha_keys.push((frame, alpha(*el), interp));
            let t = &mut layer.transform;
            if n == 0 || !animated || frame < 0 {
                t.position.base = pos;
                t.rotation.base = angle;
                t.scale.base = scale;
            }
            if animated && frame >= 0 {
                t.position.set_key(frame, pos, interp);
                t.rotation.set_key(frame, angle, interp);
                t.scale.set_key(frame, scale, interp);
            }
        }
        if let Some(last) = keys.last() {
            let end = last.index + last.duration;
            if end < length {
                alpha_keys.push((end as i32 + offset, 0.0, Interpolation::Step));
            }
        }
        let varies = alpha_keys.windows(2).any(|w| w[0].1 != w[1].1);
        let (_, first_alpha, _) = alpha_keys
            .first()
            .copied()
            .unwrap_or((0, 1.0, Interpolation::Step));
        layer.opacity.base = first_alpha;
        if varies || keys.first().is_some_and(|k| k.index > 0) {
            if keys.first().is_some_and(|k| k.index as i32 + offset > 0) {
                layer.opacity.set_key(0, 0.0, Interpolation::Step);
            }
            for (frame, a, interp) in alpha_keys {
                if frame >= 0 {
                    layer.opacity.set_key(frame, a, interp);
                }
            }
        }
        // a static transform needs no keys
        if !layer
            .transform
            .position
            .keys
            .windows(2)
            .any(|w| w[0].value != w[1].value)
            && !layer
                .transform
                .rotation
                .keys
                .windows(2)
                .any(|w| w[0].value != w[1].value)
            && !layer
                .transform
                .scale
                .keys
                .windows(2)
                .any(|w| w[0].value != w[1].value)
        {
            for list in [
                &mut layer.transform.position.keys as &mut dyn KeyList,
                &mut layer.transform.rotation.keys,
                &mut layer.transform.scale.keys,
            ] {
                let mut frames = Vec::new();
                list.frames_into(&mut frames);
                for f in frames {
                    list.delete_key(f);
                }
            }
        }
        Some(layer)
    }

    /// Note what an element's colour effect, filters and blending lose.
    fn element_effects(&mut self, el: Node) {
        if let Some(c) = child(el, "color").and_then(|c| child(c, "Color")) {
            let tint = [
                "tintMultiplier",
                "brightness",
                "redMultiplier",
                "greenMultiplier",
                "blueMultiplier",
            ]
            .iter()
            .any(|k| c.attribute(*k).is_some());
            if tint {
                self.notes.add(COLOR_EFFECT);
            }
        }
        if child(el, "filters").is_some_and(|f| elements(f).next().is_some()) {
            self.notes.add(FILTER);
        }
        if el.attribute("blendMode").is_some_and(|b| b != "normal") {
            self.notes.add(BLEND);
        }
    }

    /// A library picture: its media name and size, stored on first use.
    fn picture(&mut self, name: &str) -> Option<(String, Vec2)> {
        if let Some(found) = self.pictures.get(name) {
            return found.clone();
        }
        let found = self.load_picture(name);
        if found.is_none() {
            self.notes.add(PICTURE_FORMAT);
        }
        self.pictures.insert(name.to_owned(), found.clone());
        found
    }

    fn load_picture(&mut self, name: &str) -> Option<(String, Vec2)> {
        let (href, data) = self.bitmaps.get(name).cloned().unwrap_or((name.to_owned(), None));
        // the original file when the project keeps it, else Animate's
        // copy when that is a plain PNG or JPEG
        let candidates = [Some(format!("LIBRARY/{href}")), data.map(|d| format!("bin/{d}"))];
        for file in candidates.into_iter().flatten() {
            let Some(bytes) = (self.files)(&file) else {
                continue;
            };
            let ext = match image::guess_format(&bytes) {
                Ok(image::ImageFormat::Png) => "png",
                Ok(image::ImageFormat::Jpeg) => "jpg",
                _ => continue,
            };
            let Ok(img) = image::load_from_memory(&bytes) else {
                continue;
            };
            let size = Vec2::new(img.width() as f32, img.height() as f32);
            let media = MediaFile::name_for(&bytes, ext);
            self.doc
                .media
                .insert(media.clone(), Arc::new(MediaFile { bytes }));
            return Some((media, size));
        }
        None
    }

    /// Every drawable shape among `els`, transformed by `m`, with `alpha`
    /// folded into the colours.
    fn flatten(&mut self, els: &[Node], m: Affine2, alpha: f32, out: &mut Vec<Shape>) {
        for &el in els {
            match el.tag_name().name() {
                "DOMShape" => self.shape(el, m * matrix(el), alpha, out),
                "DOMGroup" => {
                    let members: Vec<Node> = child(el, "members")
                        .map(|c| elements(c).collect())
                        .unwrap_or_default();
                    self.flatten(&members, m, alpha, out);
                }
                "DOMSymbolInstance" => {
                    self.element_effects(el);
                    let Some(symbol) = el.attribute("libraryItemName") else {
                        continue;
                    };
                    let frame = attr_u(el, "firstFrame", 0);
                    let (m, alpha) = (m * matrix(el), alpha * self::alpha(el));
                    let mut shapes = Vec::new();
                    self.with_symbol(symbol, |r, tl| r.frame_shapes(tl, frame, m, alpha, &mut shapes));
                    out.extend(shapes);
                }
                "DOMRectangleObject" | "DOMOvalObject" => self.primitive(el, m * matrix(el), alpha, out),
                "DOMBitmapInstance" => self.notes.add(PICTURE_IN_DRAWING),
                "DOMStaticText" | "DOMDynamicText" | "DOMInputText" | "DOMTLFText" => self.notes.add(TEXT),
                _ => self.notes.add(OTHER),
            }
        }
    }

    /// The shapes a symbol's timeline shows on `frame`.
    fn frame_shapes(&mut self, tl: Node, frame: u32, m: Affine2, alpha: f32, out: &mut Vec<Shape>) {
        let layers: Vec<Node> = child(tl, "layers")
            .map(|l| elements(l).collect())
            .unwrap_or_default();
        for l in layers.iter().rev() {
            if matches!(l.attribute("layerType"), Some("folder" | "mask" | "guide")) {
                continue;
            }
            let Some(frames) = child(*l, "frames") else {
                continue;
            };
            let shown = descendants(frames, "DOMFrame").find(|f| {
                let i = attr_u(*f, "index", 0);
                frame >= i && frame < i.saturating_add(attr_u(*f, "duration", 1).max(1))
            });
            if let Some(k) = shown {
                let els: Vec<Node> = child(k, "elements")
                    .map(|e| elements(e).collect())
                    .unwrap_or_default();
                self.flatten(&els, m, alpha, out);
            }
        }
    }

    fn primitive(&mut self, el: Node, m: Affine2, alpha: f32, out: &mut Vec<Shape>) {
        let (x, y) = (attr_f(el, "x", 0.0), attr_f(el, "y", 0.0));
        let size = Vec2::new(attr_f(el, "objectWidth", 0.0), attr_f(el, "objectHeight", 0.0));
        let path = if el.tag_name().name() == "DOMRectangleObject" {
            let radius = attr_f(el, "topLeftRadius", 0.0);
            Path::rect(Vec2::new(x, y), size, radius)
        } else {
            Path::ellipse(Vec2::new(x, y) + size / 2.0, size / 2.0)
        };
        let fill = child(el, "fill")
            .and_then(|f| elements(f).next())
            .map(|f| self.fill(f));
        let stroke = child(el, "stroke")
            .and_then(|s| elements(s).next())
            .and_then(|s| self.stroke(s));
        let style = Style {
            fill: fill.map(|c| fade(c, alpha)),
            stroke: stroke.map(|s| Stroke {
                color: fade(s.color, alpha),
                width: s.width * scale_of(m),
            }),
        };
        if style.fill.is_some() || style.stroke.is_some() {
            out.push(Shape::new("Shape", transform(&path, m), style));
        }
    }

    /// A fill style's colour.
    fn fill(&mut self, f: Node) -> Color {
        match f.tag_name().name() {
            "SolidColor" => solid(f),
            "LinearGradient" | "RadialGradient" => {
                self.notes.add(GRADIENT);
                let entries: Vec<Color> = descendants(f, "GradientEntry").map(solid).collect();
                average(&entries)
            }
            "BitmapFill" => {
                self.notes.add(BITMAP_FILL);
                Color::rgb(0.5, 0.5, 0.5)
            }
            _ => Color::BLACK,
        }
    }

    fn stroke(&mut self, s: Node) -> Option<Stroke> {
        if s.tag_name().name() != "SolidStroke" {
            self.notes.add(LINE_STYLE);
        }
        let color = child(s, "fill")
            .and_then(|f| elements(f).next())
            .map_or(Color::BLACK, |f| self.fill(f));
        // a hairline is one pixel whatever the zoom
        let width = attr_f(s, "weight", 1.0).max(0.1);
        Some(Stroke { color, width })
    }

    fn shape(&mut self, el: Node, m: Affine2, alpha: f32, out: &mut Vec<Shape>) {
        let mut fills: Vec<(u32, Color)> = Vec::new();
        for f in child(el, "fills").into_iter().flat_map(elements) {
            if let Some(style) = elements(f).next() {
                let c = self.fill(style);
                fills.push((attr_u(f, "index", 0), c));
            }
        }
        let mut strokes: Vec<(u32, Stroke)> = Vec::new();
        for s in child(el, "strokes").into_iter().flat_map(elements) {
            if let Some(style) = elements(s).next().and_then(|st| self.stroke(st)) {
                strokes.push((attr_u(s, "index", 0), style));
            }
        }
        let mut fill_segs: HashMap<u32, Vec<Seg>> = HashMap::new();
        let mut stroke_runs: HashMap<u32, Vec<Vec<Seg>>> = HashMap::new();
        for edge in child(el, "edges").into_iter().flat_map(elements) {
            let Some(text) = edge.attribute("edges") else {
                continue;
            };
            let runs = parse_edges(text);
            let style = |k: &str| {
                edge.attribute(k)
                    .and_then(|v| v.parse::<u32>().ok())
                    .filter(|v| *v > 0)
            };
            let (left, right, line) = (style("fillStyle0"), style("fillStyle1"), style("strokeStyle"));
            for run in runs {
                if let Some(f) = right {
                    fill_segs.entry(f).or_default().extend(run.iter().copied());
                }
                if let Some(f) = left {
                    fill_segs
                        .entry(f)
                        .or_default()
                        .extend(run.iter().map(Seg::reversed));
                }
                if let Some(s) = line {
                    stroke_runs.entry(s).or_default().push(run);
                }
            }
        }
        for (index, color) in &fills {
            let Some(segs) = fill_segs.remove(index) else {
                continue;
            };
            let path = loops(segs);
            if !path.cmds.is_empty() {
                let style = Style::fill(fade(*color, alpha));
                out.push(Shape::new(format!("Fill {index}"), transform(&path, m), style));
            }
        }
        for (index, stroke) in &strokes {
            let Some(runs) = stroke_runs.remove(index) else {
                continue;
            };
            let path = lines(runs);
            if !path.cmds.is_empty() {
                let style = Style {
                    fill: None,
                    stroke: Some(Stroke {
                        color: fade(stroke.color, alpha),
                        width: stroke.width * scale_of(m),
                    }),
                };
                out.push(Shape::new(format!("Line {index}"), transform(&path, m), style));
            }
        }
    }
}

enum Moving {
    Symbol(String),
    Picture(String),
}

fn has_content(l: Node) -> bool {
    descendants(l, "elements").any(|e| elements(e).next().is_some())
}

fn solid(n: Node) -> Color {
    let c = n
        .attribute("color")
        .and_then(|c| Color::from_hex(c).ok())
        .unwrap_or(Color::BLACK);
    c.with_alpha(attr_f(n, "alpha", 1.0).clamp(0.0, 1.0))
}

fn average(colors: &[Color]) -> Color {
    if colors.is_empty() {
        return Color::BLACK;
    }
    let n = colors.len() as f32;
    let sum = colors
        .iter()
        .fold([0.0; 4], |s, c| [s[0] + c.r, s[1] + c.g, s[2] + c.b, s[3] + c.a]);
    Color::rgba(sum[0] / n, sum[1] / n, sum[2] / n, sum[3] / n)
}

fn fade(c: Color, alpha: f32) -> Color {
    c.with_alpha(c.a * alpha)
}

/// An element's opacity from its colour effect.
fn alpha(el: Node) -> f32 {
    child(el, "color")
        .and_then(|c| child(c, "Color"))
        .map_or(1.0, |c| attr_f(c, "alphaMultiplier", 1.0).clamp(0.0, 1.0))
}

/// How much `m` scales lengths, for line widths.
fn scale_of(m: Affine2) -> f32 {
    m.matrix2.determinant().abs().sqrt()
}

/// A matrix as position, angle and scale, and whether it also skews.
fn decompose(m: Affine2) -> (Vec2, f32, Vec2, bool) {
    let (x, y) = (m.matrix2.x_axis, m.matrix2.y_axis);
    let sx = x.length();
    let angle = x.y.atan2(x.x);
    let det = m.matrix2.determinant();
    let sy = if sx > 0.0 { det / sx } else { y.length() };
    let skewed = sx > 0.0 && y.length() > 0.0 && (x.dot(y) / (sx * y.length())).abs() > 0.01;
    (m.translation, angle, Vec2::new(sx, sy), skewed)
}

/// An angle difference brought within half a turn.
fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

fn transform(path: &Path, m: Affine2) -> Path {
    let t = |p: Vec2| m.transform_point2(p);
    Path {
        cmds: path
            .cmds
            .iter()
            .map(|c| match *c {
                PathCmd::MoveTo(p) => PathCmd::MoveTo(t(p)),
                PathCmd::LineTo(p) => PathCmd::LineTo(t(p)),
                PathCmd::CubicTo(a, b, p) => PathCmd::CubicTo(t(a), t(b), t(p)),
                PathCmd::Close => PathCmd::Close,
            })
            .collect(),
    }
}

/// One edge piece, in pixels: a line, or a quadratic curve through
/// `control`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seg {
    from: Vec2,
    control: Option<Vec2>,
    to: Vec2,
}

impl Seg {
    fn reversed(&self) -> Seg {
        Seg {
            from: self.to,
            control: self.control,
            to: self.from,
        }
    }

    fn cmd(&self) -> PathCmd {
        match self.control {
            None => PathCmd::LineTo(self.to),
            Some(c) => PathCmd::CubicTo(
                self.from + (c - self.from) * (2.0 / 3.0),
                self.to + (c - self.to) * (2.0 / 3.0),
                self.to,
            ),
        }
    }
}

/// A point as a key for joining edges: edges meet exactly, in twips.
fn at(p: Vec2) -> (i64, i64) {
    (
        (p.x * 20.0 * 256.0).round() as i64,
        (p.y * 20.0 * 256.0).round() as i64,
    )
}

/// Parse an edge list into runs of connected pieces. A malformed number
/// ends the list there.
fn parse_edges(text: &str) -> Vec<Vec<Seg>> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut runs: Vec<Vec<Seg>> = Vec::new();
    let mut run: Vec<Seg> = Vec::new();
    let mut pen: Option<Vec2> = None;
    let number = |i: &mut usize| -> Option<f32> {
        while *i < b.len() && b[*i] == b' ' {
            *i += 1;
        }
        let start = *i;
        if b.get(*i) == Some(&b'#') {
            *i += 1;
            let s = *i;
            while *i < b.len() && (b[*i].is_ascii_hexdigit() || b[*i] == b'-') {
                *i += 1;
            }
            let whole = i64::from_str_radix(std::str::from_utf8(&b[s..*i]).ok()?, 16).ok()?;
            // 24 bits, signed
            let whole = if whole >= 0x80_0000 {
                whole - 0x100_0000
            } else {
                whole
            };
            let mut v = whole as f32;
            if b.get(*i) == Some(&b'.') {
                *i += 1;
                let s = *i;
                while *i < b.len() && b[*i].is_ascii_hexdigit() {
                    *i += 1;
                }
                let frac = u32::from_str_radix(std::str::from_utf8(&b[s..*i]).ok()?, 16).unwrap_or(0);
                let digits = (*i - s) as i32;
                let f = frac as f32 / 16f32.powi(digits);
                v += if whole < 0 { -f } else { f };
            }
            return Some(v / 20.0);
        }
        while *i < b.len() && (b[*i].is_ascii_digit() || matches!(b[*i], b'-' | b'.' | b'e' | b'E' | b'+')) {
            *i += 1;
        }
        let v: f32 = std::str::from_utf8(&b[start..*i]).ok()?.parse().ok()?;
        v.is_finite().then_some(v / 20.0)
    };
    let point = |i: &mut usize| -> Option<Vec2> { Some(Vec2::new(number(i)?, number(i)?)) };
    while i < b.len() {
        let c = b[i];
        i += 1;
        let ok = match c {
            b'!' => point(&mut i).map(|p| {
                if pen != Some(p) && !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                }
                pen = Some(p);
            }),
            b'|' | b'/' => point(&mut i).map(|p| {
                if let Some(from) = pen {
                    run.push(Seg {
                        from,
                        control: None,
                        to: p,
                    });
                }
                pen = Some(p);
            }),
            b'[' | b']' => point(&mut i)
                .and_then(|c| point(&mut i).map(|p| (c, p)))
                .map(|(c, p)| {
                    if let Some(from) = pen {
                        run.push(Seg {
                            from,
                            control: Some(c),
                            to: p,
                        });
                    }
                    pen = Some(p);
                }),
            b'S' => {
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                Some(())
            }
            _ => Some(()),
        };
        if ok.is_none() {
            break;
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

/// A fill's pieces joined end to end into closed loops.
fn loops(segs: Vec<Seg>) -> Path {
    let mut by_start: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        by_start.entry(at(s.from)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut path = Path::new();
    for first in 0..segs.len() {
        if used[first] || segs[first].from == segs[first].to && segs[first].control.is_none() {
            continue;
        }
        let start = at(segs[first].from);
        path.cmds.push(PathCmd::MoveTo(segs[first].from));
        let mut cur = first;
        loop {
            used[cur] = true;
            path.cmds.push(segs[cur].cmd());
            let end = at(segs[cur].to);
            if end == start {
                break;
            }
            let next = by_start
                .get(&end)
                .and_then(|c| c.iter().copied().find(|&j| !used[j]));
            match next {
                Some(j) => cur = j,
                None => break,
            }
        }
        path.cmds.push(PathCmd::Close);
    }
    path
}

/// A line's runs joined where one ends and the next begins.
fn lines(runs: Vec<Vec<Seg>>) -> Path {
    let mut path = Path::new();
    let mut pen: Option<Vec2> = None;
    for run in runs {
        for s in run {
            if pen.map(at) != Some(at(s.from)) {
                path.cmds.push(PathCmd::MoveTo(s.from));
            }
            path.cmds.push(s.cmd());
            pen = Some(s.to);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns="http://ns.adobe.com/xfl/2008/""#;

    fn doc(layers: &str) -> String {
        format!(
            r##"<DOMDocument {NS} width="400" height="300" frameRate="12" backgroundColor="#336699">
            <media><DOMBitmapItem name="pic.png" href="pic.png" bitmapDataHRef="M 1 1.dat"/></media>
            <timelines><DOMTimeline name="Scene 1"><layers>{layers}</layers></DOMTimeline>
            <DOMTimeline name="Scene 2"/></timelines></DOMDocument>"##
        )
    }

    fn files(main: String, extra: Vec<(&'static str, Vec<u8>)>) -> impl Fn(&str) -> Option<Vec<u8>> {
        move |f: &str| {
            if f == "DOMDocument.xml" {
                return Some(main.clone().into_bytes());
            }
            extra.iter().find(|(n, _)| *n == f).map(|(_, b)| b.clone())
        }
    }

    /// A 100 px red square (2000 twips), outlined in 2 px blue.
    const SQUARE: &str = r##"<DOMShape><fills><FillStyle index="1"><SolidColor color="#FF0000" alpha="0.5"/></FillStyle></fills>
        <strokes><StrokeStyle index="1"><SolidStroke weight="2"><fill><SolidColor color="#0000FF"/></fill></SolidStroke></StrokeStyle></strokes>
        <edges><Edge fillStyle1="1" strokeStyle="1" edges="!0 0S2|2000 0!2000 0|2000 2000|0 2000!0 2000[#3E8 #9C4.80 0 0"/>
        <Edge cubics="!0 0(;0,0 1,1 2,2q0 0 2 2);"/></edges></DOMShape>"##;

    #[test]
    fn edges_parse_in_decimal_and_hex() {
        let runs = parse_edges("!0 0S2|20 0!20 0[#28 #14.80 40 20 !99 99|#FFFFEC 0");
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0][0],
            Seg {
                from: Vec2::ZERO,
                control: None,
                to: Vec2::new(1.0, 0.0)
            }
        );
        // #14.80 is 20.5 twips
        assert_eq!(runs[0][1].control, Some(Vec2::new(2.0, 20.5 / 20.0)));
        // 24-bit hex is signed: #FFFFEC is -20 twips
        assert_eq!(runs[1][0].to, Vec2::new(-1.0, 0.0));
        // malformed input stops cleanly
        assert!(parse_edges("!1 x|2").is_empty());
        let _ = parse_edges("![[[#.#--|!!");
    }

    #[test]
    fn a_drawing_becomes_a_vector_layer() {
        let main = doc(&format!(
            r#"<DOMLayer name="Art"><frames><DOMFrame index="0" duration="10"><elements>{SQUARE}</elements></DOMFrame></frames></DOMLayer>"#
        ));
        let im = import_xfl("Test", &files(main, vec![])).unwrap();
        let d = &im.doc;
        assert_eq!((d.width, d.height, d.fps, d.end), (400.0, 300.0, 12.0, 9));
        assert_eq!(d.background, Color::from_hex("#336699").unwrap());
        let LayerKind::Vector { shapes } = &d.layers[0].kind else {
            panic!("not vector")
        };
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[0].style.fill, Some(Color::rgba(1.0, 0.0, 0.0, 0.5)));
        // the fill is one closed loop: move, four sides, close
        assert_eq!(shapes[0].path.cmds.len(), 6);
        assert_eq!(shapes[0].path.cmds[2], PathCmd::LineTo(Vec2::new(100.0, 100.0)));
        assert_eq!(shapes[1].style.stroke.map(|s| s.width), Some(2.0));
        assert_eq!(im.notes, ["1 further scene left out"]);
    }

    #[test]
    fn keyframed_drawings_become_frame_by_frame() {
        let main = doc(&format!(
            r#"<DOMLayer name="Ball"><frames>
              <DOMFrame index="0" duration="2"><elements>{SQUARE}</elements></DOMFrame>
              <DOMFrame index="2" duration="2" tweenType="shape"><elements/></DOMFrame>
              <DOMFrame index="4" duration="2"><elements>{SQUARE}</elements></DOMFrame>
            </frames></DOMLayer>
            <DOMLayer name="Long"><frames><DOMFrame index="0" duration="8"><elements>{SQUARE}</elements></DOMFrame></frames></DOMLayer>"#
        ));
        let im = import_xfl("Test", &files(main, vec![])).unwrap();
        // bottom first
        assert_eq!(im.doc.layers[0].name, "Long");
        let LayerKind::Frames { cels } = &im.doc.layers[1].kind else {
            panic!("not frames")
        };
        let frames: Vec<_> = cels.iter().map(|c| (c.frame, c.line.len())).collect();
        // the layer ends on frame 6, before the timeline's 8
        assert_eq!(frames, [(0, 2), (2, 0), (4, 2), (6, 0)]);
        assert!(im.notes.contains(&"1 shape tween held still".to_owned()));
    }

    #[test]
    fn a_tweened_symbol_becomes_a_moving_group() {
        let symbol = format!(
            r#"<DOMSymbolItem {NS} name="Box" symbolType="graphic"><timeline><DOMTimeline name="Box"><layers>
            <DOMLayer name="Inside"><frames><DOMFrame index="0"><elements>{SQUARE}</elements></DOMFrame></frames></DOMLayer>
            </layers></DOMTimeline></timeline></DOMSymbolItem>"#
        );
        let main = doc(r#"<DOMLayer name="Folder" layerType="folder"/>
            <DOMLayer name="Mover" parentLayerIndex="0"><frames>
              <DOMFrame index="0" duration="10" tweenType="motion"><elements>
                <DOMSymbolInstance libraryItemName="Box"><matrix><Matrix tx="10" ty="20"/></matrix>
                <color><Color alphaMultiplier="0.5"/></color></DOMSymbolInstance></elements></DOMFrame>
              <DOMFrame index="10" duration="5"><elements>
                <DOMSymbolInstance libraryItemName="Box"><matrix><Matrix a="0" b="2" c="-2" d="0" tx="50" ty="20"/></matrix></DOMSymbolInstance>
              </elements></DOMFrame>
            </frames></DOMLayer>"#);
        let im = import_xfl(
            "Test",
            &files(main, vec![("LIBRARY/Box.xml", symbol.into_bytes())]),
        )
        .unwrap();
        let LayerKind::Group { children } = &im.doc.layers[0].kind else {
            panic!("folder")
        };
        let mover = &children[0];
        let LayerKind::Group { children: inside } = &mover.kind else {
            panic!("symbol group")
        };
        assert_eq!(inside[0].name, "Inside");
        let t = &mover.transform;
        let keys: Vec<_> = t
            .position
            .keys
            .iter()
            .map(|k| (k.frame, k.value, k.interp))
            .collect();
        assert_eq!(
            keys,
            [
                (0, Vec2::new(10.0, 20.0), Interpolation::Linear),
                (10, Vec2::new(50.0, 20.0), Interpolation::Step)
            ]
        );
        assert!((t.rotation.keys[1].value - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(t.scale.keys[1].value, Vec2::new(2.0, 2.0));
        let alpha: Vec<_> = mover.opacity.keys.iter().map(|k| (k.frame, k.value)).collect();
        assert_eq!(alpha, [(0, 0.5), (10, 1.0)]);
    }

    #[test]
    fn symbols_nested_in_themselves_stop() {
        let symbol = format!(
            r#"<DOMSymbolItem {NS} name="Loop"><timeline><DOMTimeline name="Loop"><layers>
            <DOMLayer name="L"><frames><DOMFrame index="0"><elements>{SQUARE}<DOMSymbolInstance libraryItemName="Loop"/></elements></DOMFrame></frames></DOMLayer>
            </layers></DOMTimeline></timeline></DOMSymbolItem>"#
        );
        let main = doc(&format!(
            r#"<DOMLayer name="A"><frames><DOMFrame index="0"><elements>{SQUARE}<DOMSymbolInstance libraryItemName="Loop"/>
            <DOMSymbolInstance libraryItemName="Gone"/><DOMStaticText/></elements></DOMFrame></frames></DOMLayer>"#
        ));
        let im = import_xfl(
            "Test",
            &files(main, vec![("LIBRARY/Loop.xml", symbol.into_bytes())]),
        )
        .unwrap();
        let LayerKind::Vector { shapes } = &im.doc.layers[0].kind else {
            panic!()
        };
        // its own square, plus the symbol's once
        assert_eq!(shapes.len(), 4);
        for note in [
            "1 symbol nested in itself left out",
            "1 missing symbol left out",
            "1 text item left out",
        ] {
            assert!(im.notes.contains(&note.to_owned()), "{:?}", im.notes);
        }
    }

    #[test]
    fn pictures_come_from_the_library() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(4, 3)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let main = doc(r#"<DOMLayer name="Photo"><frames><DOMFrame index="0"><elements>
            <DOMBitmapInstance libraryItemName="pic.png"><matrix><Matrix tx="5"/></matrix></DOMBitmapInstance>
            </elements></DOMFrame></frames></DOMLayer>"#);
        let im = import_xfl("Test", &files(main, vec![("LIBRARY/pic.png", png)])).unwrap();
        let layer = &im.doc.layers[0];
        let LayerKind::Image(img) = &layer.kind else {
            panic!("not an image")
        };
        assert_eq!((img.width, img.height), (4.0, 3.0));
        assert!(im.doc.media.contains_key(&img.media));
        assert_eq!(layer.transform.position.base, Vec2::new(5.0, 0.0));
    }

    #[test]
    fn a_fla_archive_is_read_and_bad_files_fail_cleanly() {
        let main = doc(&format!(
            r#"<DOMLayer name="Art"><frames><DOMFrame index="0"><elements>{SQUARE}</elements></DOMFrame></frames></DOMLayer>"#
        ));
        let zip = crate::zip::write(&[("DOMDocument.xml", main.as_bytes())]).unwrap();
        assert_eq!(import_fla("A", &zip).unwrap().doc.layers.len(), 1);
        assert!(
            import_fla("A", b"\xd0\xcf\x11\xe0 old flash")
                .unwrap_err()
                .contains("CS4")
        );
        let empty = crate::zip::write(&[("other.xml", b"<x/>")]).unwrap();
        assert!(import_fla("A", &empty).is_err());
        let broken = crate::zip::write(&[("DOMDocument.xml", b"<DOMDocument><timelines>")]).unwrap();
        assert!(import_fla("A", &broken).is_err());
    }

    #[test]
    fn a_project_drawn_by_scripts_opens_empty() {
        let main = doc(
            r#"<DOMLayer name="Code"><frames><DOMFrame index="0" duration="5"><Actionscript><script>draw();</script></Actionscript>
            <elements/></DOMFrame></frames></DOMLayer>"#,
        );
        let im = import_xfl("Test", &files(main, vec![])).unwrap();
        assert!(im.doc.layers.is_empty());
        assert_eq!((im.doc.width, im.doc.end), (400.0, 4));
        assert!(
            im.notes.contains(&"1 script left out".to_owned()),
            "{:?}",
            im.notes
        );
    }
}
