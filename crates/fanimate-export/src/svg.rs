//! One frame as an SVG picture. The frame is evaluated like a render (bones,
//! Smart Bones, keys, text, the camera), so the SVG shows what the canvas
//! shows at that frame, and every shape becomes a path with its colours.
//!
//! What SVG cannot say is simplified and counted in the [`Report`]: a brush
//! stroke's width profile is drawn as an even stroke, pictures are left out
//! (they are pixels, and a warped one has no SVG form).

use std::fmt::Write;

use fanimate_core::eval::{DrawItem, Evaluator};
use fanimate_core::{Affine2, Color, Document, Path, PathCmd, Vec2, camera_at};

use crate::report::Report;

/// The most elements one frame may write, so a damaged project cannot ask for
/// a file of unbounded size.
const MAX_SHAPES: usize = 200_000;

/// The frame `frame` of `doc` as an SVG document, and what it could not carry.
pub fn frame(doc: &Document, frame: f32) -> (String, Report) {
    let mut report = Report::default();
    let mut items = Vec::new();
    Evaluator::for_render().evaluate_into(doc, frame, &mut items);
    let (w, h) = (doc.width.max(1.0), doc.height.max(1.0));
    let mut out = String::with_capacity(4096);
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        num(w),
        num(h),
        num(w),
        num(h)
    );
    // the canvas in its background colour, carried through the camera
    if doc.background.a > 0.0 {
        let camera = camera_at(doc, frame).unwrap_or(Affine2::IDENTITY);
        let corners = [
            Vec2::ZERO,
            Vec2::new(doc.width, 0.0),
            doc.size(),
            Vec2::new(0.0, doc.height),
        ];
        let mut d = String::new();
        for (i, c) in corners.iter().enumerate() {
            let p = camera.transform_point2(*c);
            let _ = write!(d, "{}{} {}", if i == 0 { "M" } else { "L" }, num(p.x), num(p.y));
        }
        d.push('Z');
        let _ = writeln!(out, "<path d=\"{d}\"{}/>", paint("fill", doc.background, 1.0));
    }
    let mut written = 0;
    for item in &items {
        if item.image.is_some() {
            report.add("pictures left out");
            continue;
        }
        if written >= MAX_SHAPES {
            report.add("shapes past the limit left out");
            continue;
        }
        if shape(&mut out, item, &mut report) {
            written += 1;
        }
    }
    out.push_str("</svg>\n");
    (out, report)
}

/// Write one shape. Returns whether it wrote anything.
fn shape(out: &mut String, item: &DrawItem<'_>, report: &mut Report) -> bool {
    let style = item.shape.style;
    if style.fill.is_none() && style.stroke.is_none() {
        return false;
    }
    let t = item.transform;
    if !t.is_finite() || item.shape.path.points().any(|p| !p.is_finite()) {
        report.add("shapes with unusable points left out");
        return false;
    }
    // the canvas blends in linear light and SVG in sRGB, so a see-through
    // shape looks a little different in other programs
    let translucent = |c: Color| c.a * item.opacity < 0.999;
    if style.fill.is_some_and(translucent) || style.stroke.is_some_and(|s| translucent(s.color)) {
        report.add("translucent shapes blended in sRGB, so they can look different from the canvas");
    }
    let mut wrote = false;
    if let Some(fill) = style.fill {
        let d = path_data(&item.shape.path, t);
        if !d.is_empty() {
            let _ = writeln!(
                out,
                "<path d=\"{d}\"{} fill-rule=\"nonzero\"/>",
                paint("fill", fill, item.opacity)
            );
            wrote = true;
        }
    }
    if let Some(stroke) = style.stroke {
        let widths = item
            .shape
            .widths
            .as_deref()
            .filter(|w| w.len() == item.shape.path.on_curve_count());
        if widths.is_some() {
            report.add("width profiles drawn as an even stroke");
        }
        // hidden edges and stroke exposure cut the path the stroke follows
        let mut source = std::borrow::Cow::Borrowed(&item.shape.path);
        if let Some(flags) = item.shape.hidden_edges() {
            source = std::borrow::Cow::Owned(item.shape.path.without_hidden_edges(flags, None).0);
        }
        if item.shape.trim != Vec2::new(0.0, 1.0) {
            source = std::borrow::Cow::Owned(source.trimmed(item.shape.trim.x, item.shape.trim.y, None).0);
        }
        let d = path_data(&source, t);
        let width = stroke.width * t.matrix2.determinant().abs().sqrt();
        if !d.is_empty() && width.is_finite() && width > 0.0 {
            // brush strokes are round, like ink; plain ones keep the renderer's butt caps
            let round = if widths.is_some() {
                " stroke-linecap=\"round\" stroke-linejoin=\"round\""
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "<path d=\"{d}\" fill=\"none\"{} stroke-width=\"{}\"{round}/>",
                paint("stroke", stroke.color, item.opacity),
                num(width)
            );
            wrote = true;
        }
    }
    wrote
}

/// The path data of `path` carried through `t`.
fn path_data(path: &Path, t: Affine2) -> String {
    let mut d = String::new();
    let p = |v: Vec2| {
        let v = t.transform_point2(v);
        format!("{} {}", num(v.x), num(v.y))
    };
    for cmd in &path.cmds {
        match cmd {
            PathCmd::MoveTo(a) => {
                let _ = write!(d, "M{}", p(*a));
            }
            PathCmd::LineTo(a) => {
                let _ = write!(d, "L{}", p(*a));
            }
            PathCmd::CubicTo(a, b, c) => {
                let _ = write!(d, "C{} {} {}", p(*a), p(*b), p(*c));
            }
            PathCmd::Close => d.push('Z'),
        }
    }
    d
}

/// ` fill="#rrggbb" fill-opacity="0.5"` for `color` at `opacity`; the
/// opacity is left out when it is 1.
fn paint(attr: &str, color: Color, opacity: f32) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut s = format!(
        " {attr}=\"#{:02x}{:02x}{:02x}\"",
        q(color.r),
        q(color.g),
        q(color.b)
    );
    let a = (color.a * opacity).clamp(0.0, 1.0);
    if a < 0.999 {
        let _ = write!(s, " {attr}-opacity=\"{}\"", num(a));
    }
    s
}

/// A number with at most three decimals and no trailing zeros; a number that
/// is not finite is 0, so the file is always valid.
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{LayerKind, Shape, Style};

    fn doc_with(shapes: Vec<Shape>) -> Document {
        let mut doc = Document::new("t", 200.0, 100.0, 24.0);
        let mut layer = doc.new_layer("art", LayerKind::Vector { shapes });
        layer.transform.position.base = Vec2::new(10.0, 20.0);
        doc.layers.push(layer);
        doc
    }

    fn square(style: Style) -> Shape {
        Shape::new("sq", Path::rect(Vec2::ZERO, Vec2::new(40.0, 30.0), 0.0), style)
    }

    #[test]
    fn a_frame_is_a_document_with_the_background_and_each_shape_where_it_is_drawn() {
        let doc = doc_with(vec![square(Style::fill(Color::rgb(1.0, 0.0, 0.0)))]);
        let (svg, report) = frame(&doc, 1.0);
        assert!(report.is_empty());
        assert!(svg.starts_with(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\" viewBox=\"0 0 200 100\">"
        ));
        assert!(svg.contains("fill=\"#ffffff\""), "the white background: {svg}");
        // the layer sits at (10, 20), so the square's corner is there
        assert!(svg.contains("M10 20L50 20L50 50L10 50Z"), "{svg}");
        assert!(svg.contains("fill=\"#ff0000\""));
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn strokes_opacity_and_even_strokes_are_written_and_the_loss_is_reported() {
        let mut brush = square(Style {
            fill: None,
            stroke: Some(fanimate_core::Stroke {
                color: Color::rgba(0.0, 0.0, 1.0, 0.5),
                width: 4.0,
            }),
        });
        let n = brush.path.on_curve_count();
        brush.widths = Some(vec![1.0; n]);
        let doc = doc_with(vec![brush]);
        let (svg, report) = frame(&doc, 1.0);
        assert!(
            svg.contains("stroke=\"#0000ff\" stroke-opacity=\"0.5\" stroke-width=\"4\""),
            "{svg}"
        );
        assert_eq!(
            report.count("translucent shapes blended in sRGB, so they can look different from the canvas"),
            1
        );
        assert!(svg.contains("stroke-linecap=\"round\""));
        assert_eq!(report.count("width profiles drawn as an even stroke"), 1);
    }

    #[test]
    fn a_shape_with_no_paint_or_a_damaged_point_writes_nothing_and_never_breaks_the_file() {
        let none = square(Style::default());
        let mut bad = square(Style::fill(Color::BLACK));
        bad.path.cmds.push(PathCmd::LineTo(Vec2::new(f32::NAN, 0.0)));
        let doc = doc_with(vec![none, bad]);
        let (svg, report) = frame(&doc, 1.0);
        assert!(!svg.contains("NaN") && !svg.contains("inf"));
        assert_eq!(report.count("shapes with unusable points left out"), 1);
        assert_eq!(svg.matches("<path").count(), 1, "only the background");
    }

    #[test]
    fn numbers_are_short_and_always_finite() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.23456), "1.235");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(f32::NAN), "0");
        assert_eq!(num(f32::INFINITY), "0");
    }
}
