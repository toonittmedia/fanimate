//! Vector paths, styles and shapes.

use glam::{Affine2, Vec2};
use serde::{Deserialize, Serialize};

use crate::Color;
use crate::anim::Key;

/// One path command. Points are in the owning layer's space.
///
/// Serialised compactly as `{"M":[x,y]}`, `{"L":[x,y]}`,
/// `{"C":[[x1,y1],[x2,y2],[x,y]]}` and `"Z"`, after the SVG path letters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PathCmd {
    #[serde(rename = "M")]
    MoveTo(Vec2),
    #[serde(rename = "L")]
    LineTo(Vec2),
    /// Cubic Bézier: two control points, then the end point.
    #[serde(rename = "C")]
    CubicTo(Vec2, Vec2, Vec2),
    #[serde(rename = "Z")]
    Close,
}

/// A sequence of subpaths.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Path {
    pub cmds: Vec<PathCmd>,
}

/// Control-point distance for approximating a quarter circle with a cubic.
const KAPPA: f32 = 0.552_284_8;

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(mut self, p: Vec2) -> Self {
        self.cmds.push(PathCmd::MoveTo(p));
        self
    }

    pub fn line_to(mut self, p: Vec2) -> Self {
        self.cmds.push(PathCmd::LineTo(p));
        self
    }

    pub fn cubic_to(mut self, c1: Vec2, c2: Vec2, p: Vec2) -> Self {
        self.cmds.push(PathCmd::CubicTo(c1, c2, p));
        self
    }

    pub fn close(mut self) -> Self {
        self.cmds.push(PathCmd::Close);
        self
    }

    /// Axis-aligned rectangle with optional rounded corners.
    pub fn rect(min: Vec2, size: Vec2, radius: f32) -> Self {
        let r = radius.clamp(0.0, size.x.min(size.y) / 2.0);
        let max = min + size;
        if r <= 0.0 {
            return Self::new()
                .move_to(min)
                .line_to(Vec2::new(max.x, min.y))
                .line_to(max)
                .line_to(Vec2::new(min.x, max.y))
                .close();
        }
        let k = r * (1.0 - KAPPA);
        Self::new()
            .move_to(Vec2::new(min.x + r, min.y))
            .line_to(Vec2::new(max.x - r, min.y))
            .cubic_to(
                Vec2::new(max.x - k, min.y),
                Vec2::new(max.x, min.y + k),
                Vec2::new(max.x, min.y + r),
            )
            .line_to(Vec2::new(max.x, max.y - r))
            .cubic_to(
                Vec2::new(max.x, max.y - k),
                Vec2::new(max.x - k, max.y),
                Vec2::new(max.x - r, max.y),
            )
            .line_to(Vec2::new(min.x + r, max.y))
            .cubic_to(
                Vec2::new(min.x + k, max.y),
                Vec2::new(min.x, max.y - k),
                Vec2::new(min.x, max.y - r),
            )
            .line_to(Vec2::new(min.x, min.y + r))
            .cubic_to(
                Vec2::new(min.x, min.y + k),
                Vec2::new(min.x + k, min.y),
                Vec2::new(min.x + r, min.y),
            )
            .close()
    }

    /// Circle made of four cubic arcs.
    pub fn circle(center: Vec2, r: f32) -> Self {
        Self::ellipse(center, Vec2::splat(r))
    }

    /// Axis-aligned ellipse made of four cubic arcs.
    pub fn ellipse(center: Vec2, radii: Vec2) -> Self {
        let k = radii * KAPPA;
        let (cx, cy, rx, ry) = (center.x, center.y, radii.x, radii.y);
        Self::new()
            .move_to(Vec2::new(cx + rx, cy))
            .cubic_to(
                Vec2::new(cx + rx, cy + k.y),
                Vec2::new(cx + k.x, cy + ry),
                Vec2::new(cx, cy + ry),
            )
            .cubic_to(
                Vec2::new(cx - k.x, cy + ry),
                Vec2::new(cx - rx, cy + k.y),
                Vec2::new(cx - rx, cy),
            )
            .cubic_to(
                Vec2::new(cx - rx, cy - k.y),
                Vec2::new(cx - k.x, cy - ry),
                Vec2::new(cx, cy - ry),
            )
            .cubic_to(
                Vec2::new(cx + k.x, cy - ry),
                Vec2::new(cx + rx, cy - k.y),
                Vec2::new(cx + rx, cy),
            )
            .close()
    }

    /// A straight open path from `from` to `to`.
    pub fn line(from: Vec2, to: Vec2) -> Self {
        Self::new().move_to(from).line_to(to)
    }

    /// A regular polygon with `sides` sides, centred at `center`, with
    /// circumradius `radius`. The first vertex is at the top (−y axis).
    /// `sides` is clamped to at least 3.
    pub fn polygon(center: Vec2, radius: f32, sides: u32) -> Self {
        let n = sides.max(3) as f32;
        let angle0 = -std::f32::consts::FRAC_PI_2; // top
        let pts: Vec<Vec2> = (0..sides.max(3))
            .map(|i| {
                let a = angle0 + std::f32::consts::TAU * i as f32 / n;
                center + Vec2::new(a.cos(), a.sin()) * radius
            })
            .collect();
        let mut p = Self::new().move_to(pts[0]);
        for &pt in &pts[1..] {
            p = p.line_to(pt);
        }
        p.close()
    }

    /// A star with `points` points, centred at `center`. `outer` is the tip
    /// radius and `inner` the valley radius. The first tip is at the top.
    /// `points` is clamped to at least 2.
    pub fn star(center: Vec2, outer: f32, inner: f32, points: u32) -> Self {
        let n = points.max(2);
        let n2 = n * 2;
        let angle0 = -std::f32::consts::FRAC_PI_2; // top tip
        let pts: Vec<Vec2> = (0..n2)
            .map(|i| {
                let a = angle0 + std::f32::consts::TAU * i as f32 / n2 as f32;
                let r = if i % 2 == 0 { outer } else { inner };
                center + Vec2::new(a.cos(), a.sin()) * r
            })
            .collect();
        let mut p = Self::new().move_to(pts[0]);
        for &pt in &pts[1..] {
            p = p.line_to(pt);
        }
        p.close()
    }

    /// An equilateral triangle centred at `center` with circumradius `radius`.
    /// The top vertex is at the top (−y axis).
    pub fn triangle(center: Vec2, radius: f32) -> Self {
        Self::polygon(center, radius, 3)
    }

    /// The path as straight pieces: one point list per subpath, and whether
    /// it is closed. Curves are split finely enough to stay within about
    /// `tolerance` of the true curve.
    pub fn polylines(&self, tolerance: f32) -> Vec<(Vec<Vec2>, bool)> {
        let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
            tolerance
        } else {
            0.25
        };
        let mut out: Vec<(Vec<Vec2>, bool)> = Vec::new();
        let mut open = false;
        for cmd in &self.cmds {
            match *cmd {
                PathCmd::MoveTo(p) => {
                    out.push((vec![p], false));
                    open = true;
                }
                PathCmd::LineTo(p) if open => out.last_mut().expect("open subpath").0.push(p),
                PathCmd::CubicTo(c1, c2, p) if open => {
                    let line = &mut out.last_mut().expect("open subpath").0;
                    let p0 = *line.last().expect("subpaths start with a point");
                    let hull = p0.distance(c1) + c1.distance(c2) + c2.distance(p);
                    let n = (hull / tolerance).sqrt().ceil().clamp(1.0, 100.0) as usize;
                    for i in 1..=n {
                        line.push(cubic_point(p0, c1, c2, p, i as f32 / n as f32));
                    }
                }
                PathCmd::Close if open => {
                    out.last_mut().expect("open subpath").1 = true;
                    open = false;
                }
                // drawing with no open subpath is ignored, as when rendering
                _ => {}
            }
        }
        out
    }

    /// Apply `t` to every point.
    pub fn transform(&mut self, t: Affine2) {
        for cmd in &mut self.cmds {
            match cmd {
                PathCmd::MoveTo(p) | PathCmd::LineTo(p) => *p = t.transform_point2(*p),
                PathCmd::CubicTo(a, b, p) => {
                    *a = t.transform_point2(*a);
                    *b = t.transform_point2(*b);
                    *p = t.transform_point2(*p);
                }
                PathCmd::Close => {}
            }
        }
    }

    /// The number of on-curve points: one per move, line and cubic command.
    /// A stroke's width profile has one entry per on-curve point.
    pub fn on_curve_count(&self) -> usize {
        self.cmds.iter().filter(|c| !matches!(c, PathCmd::Close)).count()
    }

    /// Every point the path references, including control points.
    pub fn points(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.cmds.iter().flat_map(|c| {
            let (n, pts) = match *c {
                PathCmd::MoveTo(p) | PathCmd::LineTo(p) => (1, [p; 3]),
                PathCmd::CubicTo(a, b, p) => (3, [a, b, p]),
                PathCmd::Close => (0, [Vec2::ZERO; 3]),
            };
            pts.into_iter().take(n)
        })
    }
}

fn cubic_point(p0: Vec2, c1: Vec2, c2: Vec2, p1: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    p0 * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + p1 * (t * t * t)
}

/// Distance from `p` to the segment `a`-`b`.
fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
}

impl Style {
    pub fn fill(color: Color) -> Self {
        Self {
            fill: Some(color),
            stroke: None,
        }
    }

    pub fn with_stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some(Stroke { color, width });
        self
    }
}

/// What a rectangle was made from: its box and the radius of its corners.
/// A shape keeps these beside its path so the radius can be changed after
/// drawing; see [`Shape::parametric_rect`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RectParams {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Corner radius, from 0 (square corners) to half the shorter side.
    #[serde(default)]
    pub radius: f32,
}

impl RectParams {
    pub fn new(min: Vec2, size: Vec2, radius: f32) -> Self {
        Self {
            x: min.x,
            y: min.y,
            width: size.x,
            height: size.y,
            radius,
        }
    }

    pub fn min(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    /// The largest radius that fits: half the shorter side.
    pub fn max_radius(&self) -> f32 {
        self.width.min(self.height) / 2.0
    }

    /// Whether these can make a path: finite, with a size and a radius that fits.
    pub fn is_valid(&self) -> bool {
        [self.x, self.y, self.width, self.height, self.radius]
            .iter()
            .all(|v| v.is_finite())
            && self.width > 0.0
            && self.height > 0.0
            && (0.0..=self.max_radius() + 1e-3).contains(&self.radius)
    }

    /// The path these describe.
    pub fn path(&self) -> Path {
        Path::rect(self.min(), self.size(), self.radius)
    }
}

/// A styled path on a vector layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub name: String,
    pub path: Path,
    pub style: Style,
    /// Bone this shape is bound to, as an index into the skeleton of the
    /// nearest enclosing bone layer. The shape follows that bone rigidly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone: Option<usize>,
    /// The stroke's width at each on-curve point of `path`, as a multiple of
    /// `style.stroke.width`; the width changes smoothly between points. Set
    /// for brush strokes, where it records pen pressure and tapered ends.
    /// Without it the stroke has the same width everywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widths: Option<Vec<f32>>,
    /// Point keyframes: the path at chosen frames, so a drawing can change
    /// shape over time. Like a channel's keys, with `path` as the value
    /// when there are none. Every key has the same commands as `path`, only
    /// the points differ, so they interpolate point by point.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_keys: Vec<Key<Path>>,
    /// Smooth binding: for each on-curve point, the bones of the nearest
    /// enclosing bone layer it follows and their weights. A point's curve
    /// handles move with it. Takes the place of `bone` when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<crate::bones::Skin>,
    /// Version 4: this shape is a rectangle, so its corner radius can be
    /// changed after drawing. The path is still the shape; this only says how
    /// it was made. It counts only while the path is still the one these
    /// make (see [`parametric_rect`](Self::parametric_rect)), so editing the
    /// points, or any tool that changes the path, turns the shape into an
    /// ordinary one without anything having to remember to clear this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<RectParams>,
}

impl Shape {
    pub fn new(name: impl Into<String>, path: Path, style: Style) -> Self {
        Self {
            name: name.into(),
            path,
            style,
            bone: None,
            widths: None,
            path_keys: Vec::new(),
            skin: None,
            rect: None,
        }
    }

    /// A rectangle whose corner radius stays editable.
    pub fn rectangle(name: impl Into<String>, min: Vec2, size: Vec2, radius: f32, style: Style) -> Self {
        let rect = RectParams::new(min, size, radius.clamp(0.0, size.x.min(size.y) / 2.0));
        let mut shape = Self::new(name, rect.path(), style);
        shape.rect = Some(rect);
        shape
    }

    /// The rectangle this shape is, if it still is one: it was made as a
    /// rectangle, its path is still the one that rectangle makes, and
    /// nothing that depends on the points (a width profile, point keys, a
    /// smooth binding) would be left wrong by changing them.
    pub fn parametric_rect(&self) -> Option<RectParams> {
        let rect = self.rect.filter(RectParams::is_valid)?;
        let plain = self.widths.is_none() && self.path_keys.is_empty() && self.skin.is_none();
        (plain && paths_close(&self.path, &rect.path(), 0.01)).then_some(rect)
    }

    /// Change the corner radius of a rectangle. Returns false (changing
    /// nothing) if this is not one any more.
    pub fn set_rect_radius(&mut self, radius: f32) -> bool {
        let Some(mut rect) = self.parametric_rect() else {
            return false;
        };
        if !radius.is_finite() {
            return false;
        }
        rect.radius = radius.clamp(0.0, rect.max_radius());
        self.path = rect.path();
        self.rect = Some(rect);
        true
    }

    /// Forget that this is a rectangle: it is a path from now on. Returns
    /// whether it was one.
    pub fn expand_rect(&mut self) -> bool {
        let was = self.parametric_rect().is_some();
        self.rect = None;
        was
    }

    /// The smooth binding, if it has one weight list per on-curve point.
    pub fn valid_skin(&self) -> Option<&[Vec<(usize, f32)>]> {
        self.skin
            .as_deref()
            .filter(|s| s.len() == self.path.on_curve_count())
    }

    /// Move the shape by `t`, scaling its stroke width by `t`'s average
    /// scale, as the renderer does.
    pub fn transform(&mut self, t: Affine2) {
        self.path.transform(t);
        for key in &mut self.path_keys {
            key.value.transform(t);
        }
        if let Some(stroke) = &mut self.style.stroke {
            stroke.width *= t.matrix2.determinant().abs().sqrt();
        }
        // a rectangle stays one when moved or scaled evenly; turned,
        // stretched or flipped it is an ordinary path
        if let Some(rect) = &mut self.rect {
            let (x, y) = (t.matrix2.x_axis, t.matrix2.y_axis);
            let even = x.y.abs() < 1e-6 && y.x.abs() < 1e-6 && x.x > 0.0 && (x.x - y.y).abs() <= 1e-4 * x.x;
            if even {
                let min = t.transform_point2(rect.min());
                (rect.x, rect.y) = (min.x, min.y);
                rect.width *= x.x;
                rect.height *= x.x;
                rect.radius *= x.x;
            } else {
                self.rect = None;
            }
        }
    }

    /// Whether `p` (in the shape's own space) is on the shape: inside its
    /// fill (non-zero rule, open subpaths closed as when filling) or within
    /// `tolerance` of its stroke. Shapes with neither are never hit.
    pub fn contains(&self, p: Vec2, tolerance: f32) -> bool {
        if !p.is_finite() {
            return false;
        }
        let lines = self.path.polylines((tolerance / 4.0).max(0.01));
        if let Some(stroke) = self.style.stroke {
            let widest = self
                .widths
                .as_deref()
                .and_then(|w| w.iter().copied().filter(|w| w.is_finite()).reduce(f32::max))
                .unwrap_or(1.0);
            let reach = stroke.width * widest / 2.0 + tolerance;
            let near = lines.iter().any(|(pts, closed)| {
                let closing = closed.then(|| (pts[pts.len() - 1], pts[0]));
                let dot = (pts.len() == 1).then(|| (pts[0], pts[0]));
                pts.windows(2)
                    .map(|w| (w[0], w[1]))
                    .chain(closing)
                    .chain(dot)
                    .any(|(a, b)| segment_distance(p, a, b) <= reach)
            });
            if near {
                return true;
            }
        }
        // a fill is hit inside, or within `tolerance` of its edge, which
        // closes every subpath
        self.style.fill.is_some()
            && (winding(&lines, p) != 0
                || lines.iter().any(|(pts, _)| {
                    (0..pts.len()).any(|i| segment_distance(p, pts[i], pts[(i + 1) % pts.len()]) <= tolerance)
                }))
    }

    pub fn bound_to(mut self, bone: usize) -> Self {
        self.bone = Some(bone);
        self
    }
}

/// Whether two paths have the same commands, with every point within `eps`.
fn paths_close(a: &Path, b: &Path, eps: f32) -> bool {
    let near = |p: Vec2, q: Vec2| p.distance(q) <= eps;
    a.cmds.len() == b.cmds.len()
        && a.cmds.iter().zip(&b.cmds).all(|pair| match pair {
            (PathCmd::MoveTo(p), PathCmd::MoveTo(q)) | (PathCmd::LineTo(p), PathCmd::LineTo(q)) => {
                near(*p, *q)
            }
            (PathCmd::CubicTo(a1, a2, a3), PathCmd::CubicTo(b1, b2, b3)) => {
                near(*a1, *b1) && near(*a2, *b2) && near(*a3, *b3)
            }
            (PathCmd::Close, PathCmd::Close) => true,
            _ => false,
        })
}

/// Winding number of the polylines, each closed, around `p`.
fn winding(lines: &[(Vec<Vec2>, bool)], p: Vec2) -> i32 {
    let mut w = 0;
    for (pts, _) in lines {
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let side = (b - a).perp_dot(p - a);
            if a.y <= p.y {
                if b.y > p.y && side > 0.0 {
                    w += 1;
                }
            } else if b.y <= p.y && side < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_serialises_compactly() {
        let p = Path::new()
            .move_to(Vec2::new(0.0, 0.0))
            .line_to(Vec2::new(10.0, 0.0))
            .close();
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, r#"[{"M":[0.0,0.0]},{"L":[10.0,0.0]},"Z"]"#);
        assert_eq!(serde_json::from_str::<Path>(&json).unwrap(), p);
    }

    #[test]
    fn on_curve_count_skips_control_points_and_closes() {
        assert_eq!(Path::circle(Vec2::ZERO, 1.0).on_curve_count(), 5);
        assert_eq!(Path::rect(Vec2::ZERO, Vec2::ONE, 0.0).on_curve_count(), 4);
    }

    #[test]
    fn transform_moves_points_and_scales_stroke() {
        let mut shape = Shape::new(
            "s",
            Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(1.0, 0.0)),
            Style::default().with_stroke(Color::BLACK, 4.0),
        );
        shape.transform(Affine2::from_scale_angle_translation(
            Vec2::splat(0.5),
            0.0,
            Vec2::new(10.0, 0.0),
        ));
        let pts: Vec<Vec2> = shape.path.points().collect();
        assert_eq!(pts, [Vec2::new(10.0, 0.0), Vec2::new(10.5, 0.0)]);
        assert_eq!(shape.style.stroke.unwrap().width, 2.0);
    }

    #[test]
    fn filled_shapes_are_hit_inside_and_not_outside() {
        let ring = Shape::new("o", Path::circle(Vec2::ZERO, 10.0), Style::fill(Color::BLACK));
        assert!(ring.contains(Vec2::new(0.0, 0.0), 0.0));
        assert!(ring.contains(Vec2::new(9.5, 0.0), 0.0));
        assert!(!ring.contains(Vec2::new(8.0, 8.0), 0.0));
        assert!(!ring.contains(Vec2::new(10.5, 0.0), 0.0));
        assert!(ring.contains(Vec2::new(10.5, 0.0), 1.0));
    }

    #[test]
    fn strokes_are_hit_near_the_line_only() {
        let line = Shape::new(
            "l",
            Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(100.0, 0.0)),
            Style::default().with_stroke(Color::BLACK, 4.0),
        );
        assert!(line.contains(Vec2::new(50.0, 1.9), 0.0));
        assert!(!line.contains(Vec2::new(50.0, 3.0), 0.0));
        assert!(line.contains(Vec2::new(50.0, 3.0), 2.0));
        // an open path with no fill is not hit across its middle
        let arc = Shape::new(
            "a",
            Path::new()
                .move_to(Vec2::ZERO)
                .line_to(Vec2::new(10.0, 10.0))
                .line_to(Vec2::new(20.0, 0.0)),
            Style::default().with_stroke(Color::BLACK, 1.0),
        );
        assert!(!arc.contains(Vec2::new(10.0, 2.0), 0.0));
    }

    #[test]
    fn ellipse_spans_its_radii() {
        let e = Path::ellipse(Vec2::new(5.0, 5.0), Vec2::new(10.0, 4.0));
        let (lo, hi) = e
            .polylines(0.01)
            .iter()
            .flat_map(|(pts, _)| pts.iter().copied())
            .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)));
        assert!(lo.abs_diff_eq(Vec2::new(-5.0, 1.0), 0.01), "{lo:?}");
        assert!(hi.abs_diff_eq(Vec2::new(15.0, 9.0), 0.01), "{hi:?}");
    }

    #[test]
    fn points_includes_control_points() {
        let p = Path::circle(Vec2::ZERO, 1.0);
        assert_eq!(p.points().count(), 1 + 4 * 3);
    }

    #[test]
    fn a_rectangle_keeps_its_radius_editable() {
        let mut shape = Shape::rectangle(
            "R",
            Vec2::new(10.0, 20.0),
            Vec2::new(100.0, 60.0),
            0.0,
            Style::fill(Color::BLACK),
        );
        assert_eq!(shape.path.on_curve_count(), 4);
        assert!(shape.set_rect_radius(12.0));
        assert_eq!(
            shape.path,
            Path::rect(Vec2::new(10.0, 20.0), Vec2::new(100.0, 60.0), 12.0)
        );
        assert_eq!(shape.parametric_rect().unwrap().radius, 12.0);
        // a radius larger than fits is brought down to half the shorter side
        assert!(shape.set_rect_radius(500.0));
        assert_eq!(shape.parametric_rect().unwrap().radius, 30.0);
        // back to square
        assert!(shape.set_rect_radius(0.0));
        assert_eq!(shape.path.on_curve_count(), 4);
        assert!(!shape.set_rect_radius(f32::NAN));
    }

    #[test]
    fn moving_or_scaling_evenly_keeps_a_rectangle_and_anything_else_ends_it() {
        let make = || {
            Shape::rectangle(
                "R",
                Vec2::new(10.0, 20.0),
                Vec2::new(100.0, 60.0),
                8.0,
                Style::fill(Color::BLACK),
            )
        };
        let mut moved = make();
        moved.transform(Affine2::from_translation(Vec2::new(5.0, -7.0)));
        let r = moved.parametric_rect().unwrap();
        assert_eq!((r.x, r.y, r.width, r.radius), (15.0, 13.0, 100.0, 8.0));
        let mut scaled = make();
        scaled.transform(Affine2::from_scale(Vec2::splat(2.0)));
        let r = scaled.parametric_rect().unwrap();
        assert_eq!((r.x, r.width, r.height, r.radius), (20.0, 200.0, 120.0, 16.0));
        for t in [
            Affine2::from_scale(Vec2::new(2.0, 1.0)),
            Affine2::from_angle(0.3),
            Affine2::from_scale(Vec2::new(-1.0, 1.0)),
        ] {
            let mut other = make();
            other.transform(t);
            assert!(other.parametric_rect().is_none(), "{t:?}");
        }
    }

    #[test]
    fn editing_the_path_makes_a_rectangle_an_ordinary_shape() {
        let mut shape = Shape::rectangle(
            "R",
            Vec2::ZERO,
            Vec2::new(100.0, 100.0),
            10.0,
            Style::fill(Color::BLACK),
        );
        assert!(shape.parametric_rect().is_some());
        // a point dragged by hand
        shape
            .path
            .move_points(&[0].into_iter().collect(), Vec2::new(4.0, 0.0));
        assert!(shape.parametric_rect().is_none());
        assert!(!shape.set_rect_radius(20.0), "must not rebuild over the edit");
        // things that depend on the points
        let mut keyed = Shape::rectangle("R", Vec2::ZERO, Vec2::new(100.0, 100.0), 0.0, Style::default());
        keyed.widths = Some(vec![1.0; 4]);
        assert!(keyed.parametric_rect().is_none());
        // expanding is explicit, and says whether there was anything to expand
        let mut plain = Shape::rectangle("R", Vec2::ZERO, Vec2::new(100.0, 100.0), 0.0, Style::default());
        assert!(plain.expand_rect());
        assert!(plain.rect.is_none() && !plain.expand_rect());
    }

    #[test]
    fn rectangles_round_trip_and_old_shapes_have_none() {
        let shape = Shape::rectangle(
            "R",
            Vec2::new(1.0, 2.0),
            Vec2::new(30.0, 40.0),
            5.0,
            Style::fill(Color::BLACK),
        );
        let text = serde_json::to_string(&shape).unwrap();
        assert_eq!(serde_json::from_str::<Shape>(&text).unwrap(), shape);
        let plain = Shape::new("P", Path::circle(Vec2::ZERO, 5.0), Style::default());
        assert!(!serde_json::to_string(&plain).unwrap().contains("rect"));
        // nonsense sizes are not a rectangle
        let mut bad = shape.clone();
        bad.rect = Some(RectParams::new(Vec2::ZERO, Vec2::new(-1.0, 5.0), 0.0));
        assert!(bad.parametric_rect().is_none());
    }
}
