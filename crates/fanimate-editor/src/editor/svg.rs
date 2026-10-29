//! Importing SVG files as vector layers.
//!
//! Each top-level group of the file (an Inkscape or Illustrator layer)
//! becomes a vector layer, inside a group named after the file, so
//! artwork drawn elsewhere arrives in pieces ready to rig. Shapes keep
//! their outlines and solid colours. What Fanimate cannot draw yet
//! (text, gradients, filters and the like) is simplified or left out, and
//! the import says so rather than dropping it silently.

use std::collections::HashMap;

use fanimate_core::{Color, Layer, LayerId, LayerKind, Path, Shape, Style, Vec2};
use usvg::roxmltree;
use usvg::tiny_skia_path::PathSegment;

use super::import::{file_name, stem};
use super::{Editor, StatusKind, labelled};

/// How far outlines refitted for the even-odd fill rule may stray, in
/// document units.
const REFIT_TOLERANCE: f32 = 0.1;

/// What an SVG file had that did not come through as drawn.
#[derive(Default, Debug, PartialEq)]
struct Notes {
    text: usize,
    gradients: usize,
    patterns: usize,
    dashes: usize,
    clips: usize,
    masks: usize,
    filters: usize,
    blends: usize,
    pictures: usize,
}

impl Notes {
    /// One phrase per kind of loss, for the status bar.
    fn phrases(&self) -> Vec<String> {
        let count = |n: usize, one: &str, many: &str| match n {
            0 => None,
            1 => Some(format!("1 {one}")),
            n => Some(format!("{n} {many}")),
        };
        let mut out = Vec::new();
        if let Some(t) = count(self.text, "text item", "text items") {
            out.push(format!("{t} left out (convert text to outlines first)"));
        }
        if let Some(p) = count(self.pictures, "picture", "pictures") {
            out.push(format!("{p} left out (import pictures on their own)"));
        }
        let fills = self.gradients + self.patterns;
        if let Some(g) = count(fills, "gradient or pattern", "gradients and patterns") {
            let colour = if fills == 1 {
                "a solid colour"
            } else {
                "solid colours"
            };
            out.push(format!("{g} became {colour}"));
        }
        if let Some(d) = count(self.dashes, "dashed line", "dashed lines") {
            out.push(format!("{d} became solid"));
        }
        let effects = self.clips + self.masks + self.filters + self.blends;
        if let Some(e) = count(effects, "clip, mask or effect", "clips, masks and effects") {
            out.push(format!("{e} left out"));
        }
        out
    }
}

impl Editor {
    /// Import an SVG file as vector layers, centred on the canvas. One
    /// undo step. Returns the new layer.
    pub fn import_svg(&mut self, name: &str, bytes: &[u8]) -> Result<LayerId, String> {
        let (pieces, size, notes) = read(name, bytes)?;

        self.checkpoint("Import SVG file");
        let offset = (self.doc.size() - size) / 2.0;
        let mut layers: Vec<Layer> = pieces
            .into_iter()
            .map(|(name, mut shapes)| {
                number_unnamed(&mut shapes);
                labelled(self.doc.new_layer(name, LayerKind::vector(shapes)))
            })
            .collect();
        let mut layer = if layers.len() == 1 {
            layers.remove(0)
        } else {
            // SVG draws first to last, as layers do
            labelled(
                self.doc
                    .new_layer(stem(name), LayerKind::Group { children: layers }),
            )
        };
        layer.transform.position.base = offset;
        let id = layer.id;
        self.place_imported(layer);
        let lost = notes.phrases();
        if lost.is_empty() {
            self.set_status(StatusKind::Success, format!("Imported {}", file_name(name)));
        } else {
            self.set_status(
                StatusKind::Warning,
                format!("Imported {}; {}", file_name(name), lost.join("; ")),
            );
        }
        Ok(id)
    }
}

/// The layers of an SVG file: a name and the shapes of each.
pub(crate) type Pieces = Vec<(String, Vec<Shape>)>;

/// What an SVG file holds as layers: its top-level groups, each with its
/// shapes (named by id, or by Inkscape label), the size of its page and what
/// could not come through.
fn read(name: &str, bytes: &[u8]) -> Result<(Pieces, Vec2, Notes), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not an SVG file Fanimate can read".to_owned())?;
    let options = usvg::Options {
        // an SVG file may point at any file on disk; pictures are not
        // imported, so nothing outside the file is ever read
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree =
        usvg::Tree::from_str(text, &options).map_err(|e| format!("the SVG file could not be read: {e}"))?;
    let (labels, mut notes) = scan(text);

    // the file's layers: its top-level groups, under any wrapper usvg
    // adds for the view box
    let mut root = tree.root();
    while let [usvg::Node::Group(only)] = root.children()
        && only.id().is_empty()
    {
        root = only;
    }
    let mut pieces: Pieces = Vec::new();
    let mut loose = Vec::new();
    for node in root.children() {
        match node {
            usvg::Node::Group(g) => {
                let mut shapes = Vec::new();
                collect(g, 1.0, &mut shapes, &mut notes);
                if !shapes.is_empty() {
                    let id = g.id();
                    let name = labels
                        .get(id)
                        .cloned()
                        .filter(|l| !l.trim().is_empty())
                        .or_else(|| (!id.is_empty()).then(|| id.to_owned()))
                        .unwrap_or_else(|| format!("Layer {}", pieces.len() + 1));
                    pieces.push((name, shapes));
                }
            }
            other => collect_node(other, 1.0, &mut loose, &mut notes),
        }
    }
    if !loose.is_empty() {
        pieces.insert(0, (stem(name), loose));
    }
    if pieces.is_empty() {
        let mut why = "the SVG file has no shapes Fanimate can import".to_owned();
        if let Some(first) = notes.phrases().first() {
            why = format!("{why}: {first}");
        }
        return Err(why);
    }
    let size = tree.size();
    Ok((pieces, Vec2::new(size.width(), size.height()), notes))
}

/// The layers of an SVG file that ships with Fanimate (a template's mouths,
/// characters and sets): each top-level group with its shapes, and the size
/// of the page. Unlike an import it makes no document change and says
/// nothing; the files are checked by tests, so a file that fails to read
/// shows as missing artwork rather than stopping the template.
pub(crate) fn bundled(name: &str, svg: &str) -> (Pieces, Vec2) {
    match read(name, svg.as_bytes()) {
        Ok((layers, size, _)) => (layers, size),
        Err(_) => (Vec::new(), Vec2::ZERO),
    }
}

/// Inkscape's layer names by element id, and what usvg drops without a
/// trace (text, when built without fonts).
fn scan(text: &str) -> (HashMap<String, String>, Notes) {
    const INKSCAPE: &str = "http://www.inkscape.org/namespaces/inkscape";
    let mut labels = HashMap::new();
    let mut notes = Notes::default();
    if let Ok(doc) = roxmltree::Document::parse(text) {
        for node in doc.descendants().filter(|n| n.is_element()) {
            if node.tag_name().name() == "text" {
                notes.text += 1;
            }
            if let (Some(id), Some(label)) = (node.attribute("id"), node.attribute((INKSCAPE, "label"))) {
                labels.insert(id.to_owned(), label.to_owned());
            }
        }
    }
    (labels, notes)
}

/// Every drawable shape inside `g`, in drawing order, with transforms
/// applied and group opacity folded into the colours.
fn collect(g: &usvg::Group, opacity: f32, out: &mut Vec<Shape>, notes: &mut Notes) {
    notes.clips += usize::from(g.clip_path().is_some());
    notes.masks += usize::from(g.mask().is_some());
    notes.filters += usize::from(!g.filters().is_empty());
    notes.blends += usize::from(g.blend_mode() != usvg::BlendMode::Normal);
    let opacity = opacity * g.opacity().get();
    for node in g.children() {
        collect_node(node, opacity, out, notes);
    }
}

fn collect_node(node: &usvg::Node, opacity: f32, out: &mut Vec<Shape>, notes: &mut Notes) {
    match node {
        usvg::Node::Group(g) => collect(g, opacity, out, notes),
        usvg::Node::Path(p) => {
            if let Some(shape) = shape(p, opacity, notes) {
                out.push(shape);
            }
        }
        usvg::Node::Image(_) => notes.pictures += 1,
        // only when usvg renders text itself; counted from the source
        usvg::Node::Text(_) => {}
    }
}

fn shape(p: &usvg::Path, opacity: f32, notes: &mut Notes) -> Option<Shape> {
    if !p.is_visible() || (p.fill().is_none() && p.stroke().is_none()) {
        return None;
    }
    let ts = p.abs_transform();
    let mut path = convert(&p.data().clone().transform(ts)?);
    if path.cmds.is_empty() || !path.points().all(|v| v.is_finite()) {
        return None;
    }
    let fill = p
        .fill()
        .map(|f| paint(f.paint(), opacity * f.opacity().get(), notes));
    // the renderer fills by the non-zero rule; outlines that overlap
    // themselves are refitted when the file asks for even-odd
    if p.fill().is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd) && subpaths(&path) > 1 {
        path = fanimate_core::boolean::even_odd(&path, REFIT_TOLERANCE);
    }
    let stroke = p.stroke().map(|s| {
        notes.dashes += usize::from(s.dasharray().is_some());
        // a transform that scales scales the line too
        let scale = (ts.sx * ts.sy - ts.kx * ts.ky).abs().sqrt();
        fanimate_core::Stroke {
            color: paint(s.paint(), opacity * s.opacity().get(), notes),
            width: s.width().get() * scale,
        }
    });
    Some(Shape::new(p.id(), path, Style { fill, stroke }))
}

/// A paint as one colour: a gradient's average, a pattern's grey.
fn paint(paint: &usvg::Paint, opacity: f32, notes: &mut Notes) -> Color {
    let rgb = |c: usvg::Color, a: f32| {
        Color::rgba(
            f32::from(c.red) / 255.0,
            f32::from(c.green) / 255.0,
            f32::from(c.blue) / 255.0,
            a,
        )
    };
    let average = |stops: &[usvg::Stop]| {
        let n = stops.len().max(1) as f32;
        let sum = stops.iter().fold([0.0f32; 4], |acc, s| {
            let c = rgb(s.color(), s.opacity().get());
            [acc[0] + c.r, acc[1] + c.g, acc[2] + c.b, acc[3] + c.a]
        });
        Color::rgba(sum[0] / n, sum[1] / n, sum[2] / n, sum[3] / n * opacity)
    };
    match paint {
        usvg::Paint::Color(c) => rgb(*c, opacity),
        usvg::Paint::LinearGradient(g) => {
            notes.gradients += 1;
            average(g.stops())
        }
        usvg::Paint::RadialGradient(g) => {
            notes.gradients += 1;
            average(g.stops())
        }
        usvg::Paint::Pattern(_) => {
            notes.patterns += 1;
            Color::rgba(0.5, 0.5, 0.5, opacity)
        }
    }
}

/// A tiny-skia path as ours. Quadratic curves become cubic ones.
fn convert(src: &usvg::tiny_skia_path::Path) -> Path {
    let v = |p: usvg::tiny_skia_path::Point| Vec2::new(p.x, p.y);
    let mut path = Path::new();
    let mut last = Vec2::ZERO;
    for seg in src.segments() {
        path = match seg {
            PathSegment::MoveTo(p) => {
                last = v(p);
                path.move_to(last)
            }
            PathSegment::LineTo(p) => {
                last = v(p);
                path.line_to(last)
            }
            PathSegment::QuadTo(c, p) => {
                let (c, p) = (v(c), v(p));
                let path = path.cubic_to(last + (c - last) * (2.0 / 3.0), p + (c - p) * (2.0 / 3.0), p);
                last = p;
                path
            }
            PathSegment::CubicTo(c1, c2, p) => {
                last = v(p);
                path.cubic_to(v(c1), v(c2), last)
            }
            PathSegment::Close => path.close(),
        };
    }
    path
}

fn subpaths(path: &Path) -> usize {
    path.cmds
        .iter()
        .filter(|c| matches!(c, fanimate_core::PathCmd::MoveTo(_)))
        .count()
}

/// Shapes without an id in the file get "Shape 1", "Shape 2"…
fn number_unnamed(shapes: &mut [Shape]) {
    let mut n = 0;
    for s in shapes.iter_mut().filter(|s| s.name.is_empty()) {
        n += 1;
        s.name = format!("Shape {n}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;

    const LAYERED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg"
        xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
        width="200" height="100" viewBox="0 0 100 50">
      <g id="layer1" inkscape:label="Body">
        <rect id="torso" x="10" y="10" width="20" height="30" fill="#ff0000"/>
        <g transform="translate(40 0)" opacity="0.5">
          <circle cx="10" cy="10" r="5" fill="#0000ff" stroke="#000" stroke-width="2"/>
        </g>
      </g>
      <g id="arm">
        <path d="M 0 0 Q 10 10 20 0" fill="none" stroke="#00ff00" stroke-dasharray="2 2"/>
      </g>
      <text x="0" y="40">Hello</text>
    </svg>"##;

    fn vector(layer: &Layer) -> &[Shape] {
        match &layer.kind {
            LayerKind::Vector { shapes } => shapes,
            other => panic!("not a vector layer: {other:?}"),
        }
    }

    #[test]
    fn top_level_groups_become_layers_in_a_group() {
        let mut ed = Editor::untitled();
        let id = ed.import_svg("C:\\art\\hero.svg", LAYERED.as_bytes()).unwrap();
        let root = ed.doc.find(id).unwrap();
        assert_eq!(root.name, "hero");
        let layers = root.kind.children().unwrap();
        let names: Vec<&str> = layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Body", "arm"]);
        // the view box scales 2x; the nested transform and opacity apply
        let body = vector(&layers[0]);
        assert_eq!(body[0].name, "torso");
        assert_eq!(body[0].style.fill, Some(Color::rgb(1.0, 0.0, 0.0)));
        assert!(body[0].path.points().any(|p| p == Vec2::new(20.0, 20.0)));
        assert_eq!(body[1].name, "Shape 1");
        let circle = &body[1];
        assert!((circle.style.fill.unwrap().a - 0.5).abs() < 1e-3);
        assert!((circle.style.stroke.unwrap().width - 4.0).abs() < 1e-3);
        assert!(circle.path.points().all(|p| p.x >= 89.0 && p.x <= 111.0));
        // the quadratic curve came through as a cubic one
        let arm = vector(&layers[1]);
        assert!(
            arm[0]
                .path
                .cmds
                .iter()
                .any(|c| matches!(c, fanimate_core::PathCmd::CubicTo(..)))
        );
        // centred: the 200x100 drawing in the middle of the canvas
        assert_eq!(root.transform.position.base, Vec2::new(540.0, 310.0));
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
        let status = &ed.status.as_ref().unwrap().1;
        assert!(status.contains("1 text item left out"), "{status}");
        assert!(status.contains("1 dashed line became solid"), "{status}");
        ed.run(Action::Undo);
        assert!(ed.doc.find(id).is_none());
    }

    #[test]
    fn a_file_without_groups_is_one_layer() {
        let mut ed = Editor::untitled();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
            <rect width="10" height="10" fill="url(#g)"/>
            <defs><linearGradient id="g"><stop offset="0" stop-color="#000"/>
            <stop offset="1" stop-color="#fff"/></linearGradient></defs></svg>"##;
        let id = ed.import_svg("icon.svg", svg.as_bytes()).unwrap();
        let layer = ed.doc.find(id).unwrap();
        assert_eq!(layer.name, "icon");
        let fill = vector(layer)[0].style.fill.unwrap();
        assert!((fill.r - 0.5).abs() < 0.01, "gradient averaged: {fill:?}");
        assert!(ed.status.as_ref().unwrap().1.contains("became a solid colour"));
    }

    #[test]
    fn even_odd_holes_stay_holes() {
        let mut ed = Editor::untitled();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30">
            <path fill-rule="evenodd" d="M0 0H30V30H0Z M10 10H20V20H10Z"/></svg>"##;
        let id = ed.import_svg("ring.svg", svg.as_bytes()).unwrap();
        let shape = &vector(ed.doc.find(id).unwrap())[0];
        assert!(shape.contains(Vec2::new(5.0, 5.0), 0.0));
        assert!(!shape.contains(Vec2::new(15.0, 15.0), 0.0));
    }

    #[test]
    fn unreadable_or_empty_files_are_refused_without_a_trace() {
        let mut ed = Editor::untitled();
        assert!(ed.import_svg("x.svg", b"<svg").is_err());
        assert!(ed.import_svg("x.svg", &[0xff, 0xfe]).is_err());
        let only_text = r#"<svg xmlns="http://www.w3.org/2000/svg"><text>Hi</text></svg>"#;
        let err = ed.import_svg("t.svg", only_text.as_bytes()).unwrap_err();
        assert!(err.contains("text"), "{err}");
        assert!(ed.history.undo_label().is_none());
    }

    #[test]
    fn files_outside_the_svg_are_never_read() {
        let mut ed = Editor::untitled();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
            <image xlink:href="/etc/passwd" width="10" height="10"/>
            <rect width="5" height="5"/></svg>"#;
        assert!(ed.import_svg("p.svg", svg.as_bytes()).is_ok());
        assert!(ed.doc.media.is_empty());
    }
}
