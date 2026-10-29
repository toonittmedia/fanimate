//! The artwork the templates are made from, read from the SVG and PNG files
//! in `assets/templates/` (written by the scripts in `scripts/template-art`):
//! mouth shapes in five styles, a set of photo-real mouths, and helpers that
//! turn them into switch layers named the way Lip Sync looks for them.

use std::sync::Arc;

use fanimate_core::{
    Color, Document, Image, Interpolation, Key, Layer, LayerKind, MediaFile, Path, PathCmd, Shape, Stroke,
    Vec2,
};

use crate::editor::{labelled, svg};

/// The nine shapes Rhubarb Lip Sync names, in order, with the rest shape last.
pub const LETTERS: [&str; 9] = ["A", "B", "C", "D", "E", "F", "G", "H", "X"];

macro_rules! style {
    ($name:literal) => {
        [
            include_str!(concat!("../../assets/templates/mouths/", $name, "/A.svg")),
            include_str!(concat!("../../assets/templates/mouths/", $name, "/B.svg")),
            include_str!(concat!("../../assets/templates/mouths/", $name, "/C.svg")),
            include_str!(concat!("../../assets/templates/mouths/", $name, "/D.svg")),
            include_str!(concat!("../../assets/templates/mouths/", $name, "/E.svg")),
            include_str!(concat!("../../assets/templates/mouths/", $name, "/F.svg")),
            include_str!(concat!("../../assets/templates/mouths/", $name, "/G.svg")),
            include_str!(concat!("../../assets/templates/mouths/", $name, "/H.svg")),
            include_str!(concat!("../../assets/templates/mouths/", $name, "/X.svg")),
        ]
    };
}

/// A style of mouth shapes: one SVG for each of [`LETTERS`].
pub struct MouthStyle {
    pub name: &'static str,
    pub svgs: [&'static str; 9],
}

/// The five styles of vector mouths, in the order the Mouth styles template
/// shows them.
pub const STYLES: [MouthStyle; 5] = [
    MouthStyle {
        name: "Classic",
        svgs: style!("classic"),
    },
    MouthStyle {
        name: "Flat",
        svgs: style!("flat"),
    },
    MouthStyle {
        name: "Ink",
        svgs: style!("ink"),
    },
    MouthStyle {
        name: "Line",
        svgs: style!("line"),
    },
    MouthStyle {
        name: "Cute",
        svgs: style!("cute"),
    },
];

/// The centre of every mouth SVG.
const SVG_CENTRE: Vec2 = Vec2::new(120.0, 90.0);

/// The photo-real mouth set: its pictures and the name of each. A to H and X
/// are the ones Lip Sync keys; the rest are expressions to key by hand.
const PHOTO: [(&str, &[u8]); 14] = [
    ("A", include_bytes!("../../assets/templates/mouths/frontal/M.png")),
    ("B", include_bytes!("../../assets/templates/mouths/frontal/S.png")),
    (
        "C",
        include_bytes!("../../assets/templates/mouths/frontal/Uh.png"),
    ),
    (
        "D",
        include_bytes!("../../assets/templates/mouths/frontal/Ah.png"),
    ),
    (
        "E",
        include_bytes!("../../assets/templates/mouths/frontal/Oh.png"),
    ),
    (
        "F",
        include_bytes!("../../assets/templates/mouths/frontal/W-Oo.png"),
    ),
    ("G", include_bytes!("../../assets/templates/mouths/frontal/F.png")),
    ("H", include_bytes!("../../assets/templates/mouths/frontal/L.png")),
    (
        "X",
        include_bytes!("../../assets/templates/mouths/frontal/Neutral.png"),
    ),
    (
        "Ee",
        include_bytes!("../../assets/templates/mouths/frontal/Ee.png"),
    ),
    (
        "Closed teeth",
        include_bytes!("../../assets/templates/mouths/frontal/D.png"),
    ),
    ("R", include_bytes!("../../assets/templates/mouths/frontal/R.png")),
    (
        "Smile",
        include_bytes!("../../assets/templates/mouths/frontal/Smile.png"),
    ),
    (
        "Surprised",
        include_bytes!("../../assets/templates/mouths/frontal/Surprised.png"),
    ),
];

/// The widest picture of the photo set, in pixels: the scale every picture
/// shares, so a narrow mouth stays narrow.
const PHOTO_WIDEST: f32 = 507.0;

/// Width and height from a PNG's header.
fn png_size(bytes: &[u8]) -> Option<(f32, f32)> {
    let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some((w as f32, h as f32))
}

/// Move and scale every point (and stroke width) of a shape.
fn place(shape: &mut Shape, scale: f32, shift: impl Fn(Vec2) -> Vec2) {
    let map = |p: Vec2| shift(p);
    shape.path.cmds = shape
        .path
        .cmds
        .iter()
        .map(|c| match *c {
            PathCmd::MoveTo(p) => PathCmd::MoveTo(map(p)),
            PathCmd::LineTo(p) => PathCmd::LineTo(map(p)),
            PathCmd::CubicTo(a, b, p) => PathCmd::CubicTo(map(a), map(b), map(p)),
            PathCmd::Close => PathCmd::Close,
        })
        .collect();
    if let Some(Stroke { width, .. }) = shape.style.stroke.as_mut() {
        *width *= scale;
    }
}

/// The shapes of an SVG file, all its layers in one list, in drawing order.
pub fn shapes_of(name: &str, svg_text: &str) -> Vec<Shape> {
    svg::bundled(name, svg_text)
        .0
        .into_iter()
        .flat_map(|(_, shapes)| shapes)
        .collect()
}

/// A switch layer of vector mouths for lip sync. The mouth is centred on
/// `centre`; `scale` is how many document units one unit of the 240 x 180
/// artwork becomes. With a `bone`, the shapes follow it.
pub fn vector_mouths(
    doc: &mut Document,
    style: &MouthStyle,
    centre: Vec2,
    scale: f32,
    bone: Option<usize>,
) -> Layer {
    let mut children = Vec::new();
    for (letter, svg_text) in LETTERS.iter().zip(style.svgs) {
        let mut shapes = shapes_of(letter, svg_text);
        for shape in &mut shapes {
            place(shape, scale, |p| centre + (p - SVG_CENTRE) * scale);
            if let Some(b) = bone {
                shape.bone = Some(b);
            }
        }
        children.push(labelled(doc.new_layer(*letter, LayerKind::vector(shapes))));
    }
    resting_switch(doc, "Mouth", children)
}

/// A switch layer of the photo-real mouths: `width` is the document width of
/// the widest picture, and the pictures sit on a common chin line `chin`
/// units below `centre`. With a `bone`, they follow it.
pub fn photo_mouths(doc: &mut Document, centre: Vec2, width: f32, bone: Option<usize>) -> Layer {
    let scale = width / PHOTO_WIDEST;
    let chin = centre.y + width * 0.2;
    let mut children = Vec::new();
    for (name, bytes) in PHOTO {
        let Some((w, h)) = png_size(bytes) else {
            continue;
        };
        let media = MediaFile::name_for(bytes, "png");
        doc.media.entry(media.clone()).or_insert_with(|| {
            Arc::new(MediaFile {
                bytes: bytes.to_vec(),
            })
        });
        let mut image = Image::new(media, w * scale, h * scale);
        image.bone = bone;
        let mut layer = labelled(doc.new_layer(name, LayerKind::Image(image)));
        layer.transform.position.base = Vec2::new(centre.x - w * scale / 2.0, chin - h * scale);
        children.push(layer);
    }
    resting_switch(doc, "Mouth", children)
}

/// A switch layer that starts on its rest shape, X.
pub fn resting_switch(doc: &mut Document, name: &str, children: Vec<Layer>) -> Layer {
    let rest = children.iter().find(|c| c.name == "X").or(children.first());
    let first = rest.map(|c| c.name.clone()).unwrap_or_default();
    labelled(doc.new_layer(
        name,
        LayerKind::Switch {
            children,
            switch: vec![Key {
                frame: 1,
                value: first,
                interp: Interpolation::Step,
            }],
        },
    ))
}

/// The layers of a scene's SVG (a back or front file), as vector layers.
pub fn set_layers(doc: &mut Document, name: &str, svg_text: &str) -> Vec<Layer> {
    svg::bundled(name, svg_text)
        .0
        .into_iter()
        .map(|(layer_name, shapes)| labelled(doc.new_layer(layer_name, LayerKind::vector(shapes))))
        .collect()
}

/// A flat rectangle as a one-shape vector layer.
pub fn backdrop(doc: &mut Document, name: &str, min: Vec2, size: Vec2, color: Color) -> Layer {
    labelled(doc.new_layer(
        name,
        LayerKind::vector(vec![Shape::new(
            name,
            Path::rect(min, size, 0.0),
            fanimate_core::Style::fill(color),
        )]),
    ))
}
