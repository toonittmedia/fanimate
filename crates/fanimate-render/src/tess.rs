//! CPU tessellation of draw items into a triangle mesh.

use fanimate_core::{Affine2, Color, Document, DrawItem, PathCmd, Vec2};
use std::panic::AssertUnwindSafe;

use lyon::math::point;
use lyon::path::AttributeIndex;
use lyon::path::Path as LyonPath;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};

/// The path attribute holding the width profile.
const WIDTH: AttributeIndex = 0;

/// How far from the origin a point may be, in document units, and still be
/// drawn: far beyond any canvas, and small enough that the tessellator's
/// arithmetic stays finite. Damaged or wildly transformed shapes past it
/// are skipped rather than drawn as NaN or crashing.
const LIMIT: f32 = 1.0e7;

/// The range of width profile values drawn. A profile scales the stroke
/// width, so a sound one stays near 1. Zero is raised to a hairline: the
/// stroker divides by the width at joins and would make NaN geometry.
const WIDTH_SCALE: std::ops::RangeInclusive<f32> = 1.0e-3..=1.0e3;

/// `[x, y, r, g, b, a, u, v]`: document-space position, linear
/// premultiplied colour, and texture coordinate (0, 0 for flat colour). A
/// plain array keeps the vertex `Pod` without unsafe code.
pub type Vertex = [f32; 8];

/// Triangle mesh ready for upload.
pub type Mesh = VertexBuffers<Vertex, u32>;

/// From index `first` on, triangles are drawn with this picture (`None`:
/// flat colour), until the next run.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub first: u32,
    pub media: Option<String>,
}

pub struct Tessellator {
    fill: FillTessellator,
    stroke: StrokeTessellator,
    path: LyonPath,
    /// Where each picture's triangles start in the mesh, in order.
    pub runs: Vec<Run>,
    /// Draw everything in this colour instead of its own, as onion skins
    /// are shown. Pictures are tinted.
    pub tint: Option<Color>,
    /// Maximum distance between a curve and its flattened version, in
    /// document units. Lower is smoother and slower.
    pub tolerance: f32,
}

impl Default for Tessellator {
    fn default() -> Self {
        Self {
            fill: FillTessellator::new(),
            stroke: StrokeTessellator::new(),
            path: LyonPath::new(),
            runs: Vec::new(),
            tint: None,
            tolerance: 0.25,
        }
    }
}

impl Tessellator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empty `mesh` and the runs, to start a frame.
    pub fn clear(&mut self, mesh: &mut Mesh) {
        mesh.vertices.clear();
        mesh.indices.clear();
        self.runs.clear();
    }

    /// Start drawing with `media` (`None`: flat colour) from here on.
    fn switch(&mut self, mesh: &Mesh, media: Option<&str>) {
        if self
            .runs
            .last()
            .map_or(media.is_none(), |r| r.media.as_deref() == media)
        {
            return;
        }
        let first = mesh.indices.len() as u32;
        match self.runs.last_mut() {
            // a run that never drew anything is replaced
            Some(last) if last.first == first => last.media = media.map(str::to_owned),
            _ => self.runs.push(Run {
                first,
                media: media.map(str::to_owned),
            }),
        }
    }

    /// The document's canvas rectangle in its background colour.
    pub fn canvas(&mut self, doc: &Document, mesh: &mut Mesh) {
        self.canvas_through(doc, mesh, Affine2::IDENTITY);
    }

    /// The canvas rectangle as a camera sees it: carried through `camera`
    /// (see `fanimate_core::camera_at`), as the artwork is.
    pub fn canvas_through(&mut self, doc: &Document, mesh: &mut Mesh, camera: Affine2) {
        self.switch(mesh, None);
        let c = doc.background.to_linear_premultiplied();
        let base = mesh.vertices.len() as u32;
        for p in [
            Vec2::ZERO,
            Vec2::new(doc.width, 0.0),
            doc.size(),
            Vec2::new(0.0, doc.height),
        ] {
            let p = camera.transform_point2(p);
            mesh.vertices.push([p.x, p.y, c[0], c[1], c[2], c[3], 0.0, 0.0]);
        }
        mesh.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Append the fills and strokes of `items` to `mesh`, back to front.
    /// Shapes that fail to tessellate are skipped and logged.
    pub fn draw_items(&mut self, items: &[DrawItem<'_>], mesh: &mut Mesh) {
        for item in items {
            if let Some(image) = &item.image {
                self.switch(mesh, Some(image.media));
                let c = color(self.tint.unwrap_or(Color::WHITE), item.opacity);
                let base = mesh.vertices.len() as u32;
                for (p, uv) in image.vertices.iter() {
                    let w = item.transform.transform_point2(*p);
                    mesh.vertices.push([w.x, w.y, c[0], c[1], c[2], c[3], uv.x, uv.y]);
                }
                let [cols, rows] = image.cells;
                let at = |c: u32, r: u32| base + r * (cols + 1) + c;
                for r in 0..rows {
                    for c in 0..cols {
                        let (a, b, d, e) = (at(c, r), at(c + 1, r), at(c, r + 1), at(c + 1, r + 1));
                        mesh.indices.extend([a, b, e, a, e, d]);
                    }
                }
                continue;
            }
            self.switch(mesh, None);
            let style = item.shape.style;
            if style.fill.is_none() && style.stroke.is_none() {
                continue;
            }
            // a width profile that does not match the path is ignored (the
            // validator reports it), so bad data draws as a plain stroke
            let widths = item
                .shape
                .widths
                .as_deref()
                .filter(|w| w.len() == item.shape.path.on_curve_count());
            // approximate the transform's scale for the line width
            let scale = item.transform.matrix2.determinant().abs().sqrt();
            let line_width = style.stroke.map_or(0.0, |s| s.width * scale);
            if !self.build_path(&item.shape.path.cmds, item.transform, widths) {
                log::warn!(
                    "\"{}\" is too far away or not a number; not drawn",
                    item.shape.name
                );
                continue;
            }

            if let Some(fill) = style.fill {
                let c = color(self.tint.unwrap_or(fill).with_alpha(fill.a), item.opacity);
                let opts = FillOptions::tolerance(self.tolerance).with_fill_rule(FillRule::NonZero);
                let result = self.fill.tessellate_path(
                    &self.path,
                    &opts,
                    &mut BuffersBuilder::new(mesh, |v: FillVertex| {
                        let p = v.position();
                        [p.x, p.y, c[0], c[1], c[2], c[3], 0.0, 0.0]
                    }),
                );
                if let Err(e) = result {
                    log::warn!("fill of \"{}\" failed: {e:?}", item.shape.name);
                }
            }
            if let Some(stroke) = style.stroke {
                let c = color(
                    self.tint.unwrap_or(stroke.color).with_alpha(stroke.color.a),
                    item.opacity,
                );
                if !(line_width.is_finite() && (0.0..=LIMIT).contains(&line_width)) {
                    log::warn!("stroke of \"{}\" has no usable width", item.shape.name);
                    continue;
                }
                // hidden edges and stroke exposure: only part of the stroke is
                // drawn, so it is stroked from a cut of the path (the fill
                // used the whole one)
                let mut widths = widths;
                let visible = item
                    .shape
                    .hidden_edges()
                    .map(|flags| item.shape.path.without_hidden_edges(flags, widths));
                let mut source = &item.shape.path;
                let mut rebuild = false;
                if let Some((path, w)) = &visible {
                    if path.cmds.is_empty() {
                        continue;
                    }
                    source = path;
                    widths = w.as_deref();
                    rebuild = true;
                }
                let trimmed;
                if item.shape.trim != Vec2::new(0.0, 1.0) {
                    trimmed = source.trimmed(item.shape.trim.x, item.shape.trim.y, widths);
                    if trimmed.0.cmds.is_empty() {
                        continue;
                    }
                    widths = trimmed.1.as_deref();
                    if !self.build_path(&trimmed.0.cmds, item.transform, widths) {
                        continue;
                    }
                } else if rebuild && !self.build_path(&source.cmds, item.transform, widths) {
                    continue;
                }
                let mut opts = StrokeOptions::tolerance(self.tolerance).with_line_width(line_width);
                if widths.is_some() {
                    // brush strokes: width from the profile, rounded like ink
                    opts = opts
                        .with_variable_line_width(WIDTH)
                        .with_line_cap(LineCap::Round)
                        .with_line_join(LineJoin::Round);
                }
                if !self.stroke_checked(mesh, &opts, c) && widths.is_some() {
                    // the stroker cannot follow some sharp turns at varying
                    // widths; an even width it always can
                    let even = StrokeOptions::tolerance(self.tolerance)
                        .with_line_width(line_width)
                        .with_line_cap(LineCap::Round)
                        .with_line_join(LineJoin::Round);
                    if !self.stroke_checked(mesh, &even, c) {
                        log::warn!("stroke of \"{}\" could not be drawn", item.shape.name);
                    }
                }
            }
        }
    }

    /// Stroke the current path into `mesh` in colour `c`. Returns whether
    /// it worked; if not, `mesh` is left as it was. lyon asserts (in debug
    /// builds) or writes NaN positions (in release builds) on some
    /// variable-width strokes that turn back on themselves, so both are
    /// caught here rather than reaching the screen.
    fn stroke_checked(&mut self, mesh: &mut Mesh, opts: &StrokeOptions, c: [f32; 4]) -> bool {
        let (vertices, indices) = (mesh.vertices.len(), mesh.indices.len());
        let (stroke, path) = (&mut self.stroke, &self.path);
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            stroke.tessellate_path(
                path,
                opts,
                &mut BuffersBuilder::new(&mut *mesh, |v: StrokeVertex| {
                    let p = v.position();
                    [p.x, p.y, c[0], c[1], c[2], c[3], 0.0, 0.0]
                }),
            )
        }));
        let sound = match result {
            Ok(Ok(_)) => mesh.vertices[vertices..]
                .iter()
                .all(|v| v[0].is_finite() && v[1].is_finite()),
            Ok(Err(e)) => {
                log::warn!("stroke failed: {e:?}");
                false
            }
            Err(_) => {
                // its state after a panic is unknown: start a fresh one
                self.stroke = StrokeTessellator::new();
                false
            }
        };
        if !sound {
            mesh.vertices.truncate(vertices);
            mesh.indices.truncate(indices);
        }
        sound
    }

    /// Convert our commands to a lyon path in document space, with the
    /// width profile (1 everywhere without one) as attribute [`WIDTH`].
    /// Returns false, leaving no path, when a point lands past [`LIMIT`] or
    /// is not a number.
    fn build_path(&mut self, cmds: &[PathCmd], t: Affine2, widths: Option<&[f32]>) -> bool {
        let usable = |v: Vec2| {
            let w = t.transform_point2(v);
            w.is_finite() && w.abs().max_element() <= LIMIT
        };
        let all_usable = cmds.iter().all(|cmd| match *cmd {
            PathCmd::MoveTo(v) | PathCmd::LineTo(v) => usable(v),
            PathCmd::CubicTo(c1, c2, v) => usable(c1) && usable(c2) && usable(v),
            PathCmd::Close => true,
        });
        if !all_usable {
            self.path = LyonPath::new();
            return false;
        }
        let p = |v: Vec2| {
            let w = t.transform_point2(v);
            point(w.x, w.y)
        };
        let mut b = LyonPath::builder_with_attributes(1);
        let mut open = false;
        // index of the next on-curve point, which every command but Close has
        let mut n = 0;
        let mut width = || {
            let w = widths.and_then(|w| w.get(n)).copied().unwrap_or(1.0);
            n += 1;
            [if w.is_finite() {
                w.clamp(*WIDTH_SCALE.start(), *WIDTH_SCALE.end())
            } else {
                1.0
            }]
        };
        for cmd in cmds {
            match *cmd {
                PathCmd::MoveTo(v) => {
                    if open {
                        b.end(false);
                    }
                    b.begin(p(v), &width());
                    open = true;
                }
                PathCmd::LineTo(v) => {
                    let w = width();
                    if open {
                        b.line_to(p(v), &w);
                    }
                }
                PathCmd::CubicTo(c1, c2, v) => {
                    let w = width();
                    if open {
                        b.cubic_bezier_to(p(c1), p(c2), p(v), &w);
                    }
                }
                PathCmd::Close if open => {
                    b.end(true);
                    open = false;
                }
                // drawing before any MoveTo: ignore, as SVG renderers do
                _ => {}
            }
        }
        if open {
            b.end(false);
        }
        self.path = b.build();
        true
    }
}

fn color(c: Color, opacity: f32) -> [f32; 4] {
    c.with_alpha(c.a * opacity).to_linear_premultiplied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{evaluate, samples};

    #[test]
    fn benchmark_scene_tessellates() {
        let doc = samples::benchmark(40);
        let items = evaluate(&doc, 30.0);
        let mut mesh = Mesh::new();
        let mut t = Tessellator::new();
        t.canvas(&doc, &mut mesh);
        t.draw_items(&items, &mut mesh);
        assert!(mesh.indices.len() > 1000);
        assert_eq!(mesh.indices.len() % 3, 0);
        let n = mesh.vertices.len() as u32;
        assert!(mesh.indices.iter().all(|&i| i < n));
    }

    /// Half the mesh's height among vertices with `x` in `range`.
    fn half_height(mesh: &Mesh, range: std::ops::RangeInclusive<f32>) -> f32 {
        mesh.vertices
            .iter()
            .filter(|v| range.contains(&v[0]))
            .map(|v| v[1].abs())
            .fold(0.0, f32::max)
    }

    fn stroke_mesh(widths: Option<Vec<f32>>) -> Mesh {
        use fanimate_core::{Path, Shape, Style};
        let path = Path::new()
            .move_to(Vec2::new(0.0, 0.0))
            .line_to(Vec2::new(50.0, 0.0))
            .line_to(Vec2::new(100.0, 0.0));
        let mut shape = Shape::new("s", path, Style::default().with_stroke(Color::BLACK, 10.0));
        shape.widths = widths;
        let item = DrawItem {
            layer: fanimate_core::LayerId(1),
            index: 0,
            shape: std::borrow::Cow::Borrowed(&shape),
            part: None,
            image: None,
            transform: Affine2::IDENTITY,
            opacity: 1.0,
        };
        let mut mesh = Mesh::new();
        Tessellator::new().draw_items(&[item], &mut mesh);
        mesh
    }

    #[test]
    fn width_profile_varies_the_stroke_width() {
        let mesh = stroke_mesh(Some(vec![0.1, 1.0, 0.2]));
        assert!((half_height(&mesh, 49.0..=51.0) - 5.0).abs() < 0.3);
        assert!(half_height(&mesh, -2.0..=1.0) < 1.0);
        assert!(half_height(&mesh, 99.0..=102.0) < 1.5);
    }

    #[test]
    fn stroke_exposure_draws_only_that_part_of_the_stroke_and_all_of_the_fill() {
        use fanimate_core::{Path, Shape, Style};
        let path = Path::rect(Vec2::ZERO, Vec2::new(100.0, 40.0), 0.0);
        let mut shape = Shape::new(
            "s",
            path,
            Style::fill(Color::WHITE).with_stroke(Color::BLACK, 2.0),
        );
        let reach = |mesh: &Mesh, colour: [f32; 4]| {
            mesh.vertices
                .iter()
                .filter(|v| v[2..6] == colour)
                .map(|v| v[0].max(v[1]))
                .fold(f32::MIN, f32::max)
        };
        let whole = mesh_of(&shape, Affine2::IDENTITY);
        // the stroke goes all the way round: past the right edge and the bottom
        assert!(reach(&whole, [0.0, 0.0, 0.0, 1.0]) > 40.5);
        // a tenth of the way round is a short bit of the top edge
        shape.trim = Vec2::new(0.0, 0.1);
        let part = mesh_of(&shape, Affine2::IDENTITY);
        let stroke_y = part
            .vertices
            .iter()
            .filter(|v| v[2..6] == [0.0, 0.0, 0.0, 1.0])
            .map(|v| v[1])
            .fold(f32::MIN, f32::max);
        assert!(
            stroke_y < 2.0,
            "only the top edge is drawn, not the sides: {stroke_y}"
        );
        // the fill is still the whole rectangle
        assert!(reach(&part, [1.0, 1.0, 1.0, 1.0]) >= 99.9);
        // nothing exposed draws no stroke at all
        shape.trim = Vec2::new(0.3, 0.3);
        let none = mesh_of(&shape, Affine2::IDENTITY);
        assert!(none.vertices.iter().all(|v| v[2..6] != [0.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn a_hidden_edge_loses_its_stroke_but_not_its_fill() {
        use fanimate_core::{Path, Shape, Style};
        // commands: move, top, right, bottom, close (the left side)
        let path = Path::rect(Vec2::ZERO, Vec2::new(100.0, 40.0), 0.0);
        let mut shape = Shape::new(
            "s",
            path,
            Style::fill(Color::WHITE).with_stroke(Color::BLACK, 2.0),
        );
        let ink = [0.0, 0.0, 0.0, 1.0];
        let right_most = |mesh: &Mesh, colour: [f32; 4]| {
            mesh.vertices
                .iter()
                .filter(|v| v[2..6] == colour)
                .map(|v| v[0])
                .fold(f32::MIN, f32::max)
        };
        let whole = mesh_of(&shape, Affine2::IDENTITY);
        assert!(right_most(&whole, ink) > 100.5, "the right edge is stroked");
        assert!(shape.set_edge_hidden(2, true));
        let part = mesh_of(&shape, Affine2::IDENTITY);
        assert!(
            right_most(&part, ink) <= 100.01,
            "the right edge is not: {}",
            right_most(&part, ink)
        );
        // the other sides and the fill are still there
        assert!(part.vertices.iter().any(|v| v[2..6] == ink));
        assert!(right_most(&part, [1.0, 1.0, 1.0, 1.0]) >= 99.9);
        // flags that no longer match the path hide nothing
        shape.path.cmds.pop();
        let stale = mesh_of(&shape, Affine2::IDENTITY);
        assert!(right_most(&stale, ink) > 100.5);
    }

    #[test]
    fn mismatched_width_profile_draws_a_plain_stroke() {
        let mesh = stroke_mesh(Some(vec![0.1]));
        assert!((half_height(&mesh, -1.0..=1.0) - 5.0).abs() < 0.3);
    }

    fn mesh_of(shape: &fanimate_core::Shape, transform: Affine2) -> Mesh {
        let item = DrawItem {
            layer: fanimate_core::LayerId(1),
            index: 0,
            shape: std::borrow::Cow::Borrowed(shape),
            part: None,
            image: None,
            transform,
            opacity: 1.0,
        };
        let mut mesh = Mesh::new();
        Tessellator::new().draw_items(&[item], &mut mesh);
        mesh
    }

    /// A brush stroke that turns straight back on itself, which lyon's
    /// variable-width stroker cannot follow (found by the damaged-project
    /// test): drawn at an even width instead, never as NaN or a panic.
    #[test]
    fn a_stroke_that_doubles_back_still_draws() {
        use fanimate_core::{Path, Shape, Style};
        let path = Path::new().move_to(Vec2::new(200.0, 500.0)).cubic_to(
            Vec2::new(200.83, 500.82),
            Vec2::new(203.33, 0.0),
            Vec2::new(205.0, 504.95),
        );
        let mut shape = Shape::new("spike", path, Style::default().with_stroke(Color::BLACK, 8.0));
        shape.widths = Some(vec![0.0365, 0.534]);
        let mesh = mesh_of(&shape, Affine2::IDENTITY);
        assert!(!mesh.indices.is_empty(), "drawn");
        assert!(mesh.vertices.iter().all(|v| v[0].is_finite() && v[1].is_finite()));
        let n = mesh.vertices.len() as u32;
        assert!(mesh.indices.iter().all(|&i| i < n));
    }

    /// Points that are not numbers, or too far out for the tessellator's
    /// arithmetic, skip the shape instead of crashing.
    #[test]
    fn unusable_points_skip_the_shape() {
        use fanimate_core::{Path, Shape, Style};
        let style = Style::fill(Color::WHITE).with_stroke(Color::BLACK, 2.0);
        for far in [f32::INFINITY, f32::NAN, 1.0e30] {
            let path = Path::new()
                .move_to(Vec2::ZERO)
                .line_to(Vec2::new(far, 10.0))
                .line_to(Vec2::new(10.0, 10.0))
                .close();
            let mesh = mesh_of(&Shape::new("far", path, style), Affine2::IDENTITY);
            assert!(mesh.indices.is_empty(), "{far} is not drawn");
        }
        // a sound shape moved out of range by its transform is skipped too
        let path = Path::new()
            .move_to(Vec2::ZERO)
            .line_to(Vec2::new(10.0, 0.0))
            .line_to(Vec2::new(10.0, 10.0))
            .close();
        let shape = Shape::new("near", path, style);
        assert!(
            mesh_of(&shape, Affine2::from_scale(Vec2::splat(1.0e30)))
                .indices
                .is_empty()
        );
        assert!(!mesh_of(&shape, Affine2::IDENTITY).indices.is_empty());
    }

    #[test]
    fn opacity_premultiplies_colour() {
        let c = color(Color::WHITE, 0.5);
        assert_eq!(c, [0.5, 0.5, 0.5, 0.5]);
    }
}
