//! Freehand brush strokes: turning pen or mouse samples into a smooth,
//! editable stroke.
//!
//! A brush stroke is an ordinary [`Shape`]. Its path is the centreline, its
//! style's stroke gives the colour and the full-pressure width, and
//! [`Shape::widths`] holds the width at each point as a multiple of that.
//! Pressure and the tapered ends live in that profile, so the renderer draws
//! exactly what the file says and the points stay editable like any path.

use glam::Vec2;

use crate::Color;
use crate::path::{Path, PathCmd, Shape, Stroke, Style};

/// One pen or mouse position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSample {
    pub pos: Vec2,
    /// 0 to 1. Use 1 for a mouse, which has no pressure.
    pub pressure: f32,
}

impl BrushSample {
    pub fn new(pos: Vec2, pressure: f32) -> Self {
        Self { pos, pressure }
    }

    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            pos: a.pos.lerp(b.pos, t),
            pressure: a.pressure + (b.pressure - a.pressure) * t,
        }
    }
}

/// How a stroke is built from samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushOptions {
    pub color: Color,
    /// Width at full pressure.
    pub size: f32,
    /// Length of each tapered end; 0 for blunt ends.
    pub taper: f32,
    /// How far simplification may move the centreline. Smaller keeps more
    /// points.
    pub tolerance: f32,
}

/// Longest smoothing window, in resampled points.
const MAX_WINDOW: usize = 24;

/// Width at zero pressure, as a fraction of full width, so a light touch
/// still leaves a visible line.
const MIN_PRESSURE_WIDTH: f32 = 0.1;

/// Width at the very tip of a tapered end, as a fraction of full width.
const TIP_WIDTH: f32 = 0.05;

/// Smooths pen input as it arrives.
///
/// Samples are first resampled to an even spacing, so the smoothing feels
/// the same whether the pen moves fast or slowly, then averaged over a
/// window whose length is set by the strength. An average trails the pen,
/// so [`Stabiliser::samples`] adds a tail that catches up with it: the
/// stroke on screen always reaches the pen, and ends exactly where it
/// lifted, without the shaky detail of the raw input.
#[derive(Clone, Debug)]
pub struct Stabiliser {
    spacing: f32,
    window: usize,
    /// Evenly spaced input.
    raw: Vec<BrushSample>,
    /// Moving averages of `raw`, one per raw sample.
    smoothed: Vec<BrushSample>,
    /// The latest input, where the pen is now.
    tip: Option<BrushSample>,
}

impl Stabiliser {
    /// `strength` runs from 0 (off) to 1 (strongest). `spacing` is the
    /// distance between resampled points, in the samples' units; about two
    /// screen pixels works well at any zoom.
    pub fn new(strength: f32, spacing: f32) -> Self {
        let strength = if strength.is_finite() {
            strength.clamp(0.0, 1.0)
        } else {
            0.0
        };
        Self {
            spacing: if spacing.is_finite() && spacing > 0.0 {
                spacing
            } else {
                1.0
            },
            window: 1 + (strength * (MAX_WINDOW - 1) as f32).round() as usize,
            raw: Vec::new(),
            smoothed: Vec::new(),
            tip: None,
        }
    }

    pub fn push(&mut self, sample: BrushSample) {
        if !sample.pos.is_finite() || !sample.pressure.is_finite() {
            return;
        }
        self.tip = Some(sample);
        let Some(mut last) = self.raw.last().copied() else {
            self.add_raw(sample);
            return;
        };
        // walk from the last resampled point towards the new sample
        let mut d = sample.pos.distance(last.pos);
        while d >= self.spacing {
            last = BrushSample::lerp(last, sample, self.spacing / d);
            self.add_raw(last);
            d = sample.pos.distance(last.pos);
        }
    }

    fn add_raw(&mut self, s: BrushSample) {
        self.raw.push(s);
        let from = self.raw.len().saturating_sub(self.window);
        self.smoothed.push(average(&self.raw[from..]));
    }

    pub fn is_empty(&self) -> bool {
        self.tip.is_none()
    }

    /// The stroke so far: the smoothed points, then a tail that catches up
    /// with the pen. Use it both to preview the stroke and to finish it.
    pub fn samples(&self) -> Vec<BrushSample> {
        let mut out = self.smoothed.clone();
        // shrink the window from the old end so the average reaches the
        // newest raw point
        let n = self.raw.len();
        for keep in (1..self.window.min(n)).rev() {
            out.push(average(&self.raw[n - keep..]));
        }
        if let Some(tip) = self.tip
            && out.last().is_none_or(|l| l.pos != tip.pos)
        {
            out.push(tip);
        }
        out
    }
}

fn average(samples: &[BrushSample]) -> BrushSample {
    let n = samples.len().max(1) as f32;
    let (pos, pressure) = samples
        .iter()
        .fold((Vec2::ZERO, 0.0), |(p, q), s| (p + s.pos, q + s.pressure));
    BrushSample::new(pos / n, pressure / n)
}

/// Build a brush stroke from samples (usually [`Stabiliser::samples`]).
///
/// The centreline is simplified to the fewest points that stay within
/// `tolerance`, then joined with smooth curves that pass through every
/// point. A stroke too short to have a direction becomes a round dot.
/// Returns `None` when there are no usable samples.
pub fn build_stroke(samples: &[BrushSample], opts: &BrushOptions) -> Option<Shape> {
    let samples: Vec<BrushSample> = samples
        .iter()
        .copied()
        .filter(|s| s.pos.is_finite() && s.pressure.is_finite())
        .collect();
    let first = *samples.first()?;
    let size = if opts.size.is_finite() {
        opts.size.max(0.0)
    } else {
        1.0
    };

    let mut along = Vec::with_capacity(samples.len());
    let mut total = 0.0;
    for (i, s) in samples.iter().enumerate() {
        if i > 0 {
            total += s.pos.distance(samples[i - 1].pos);
        }
        along.push(total);
    }

    if total < size * 0.25 {
        let pressure = samples.iter().map(|s| s.pressure).fold(0.0, f32::max);
        let r = (size * pressure_width(pressure) / 2.0).max(0.05);
        return Some(Shape::new(
            "Dot",
            Path::circle(first.pos, r),
            Style::fill(opts.color),
        ));
    }

    let taper = opts.taper.max(0.0).min(total / 2.0);
    let widths: Vec<f32> = samples
        .iter()
        .zip(&along)
        .map(|(s, &d)| pressure_width(s.pressure) * taper_width(d.min(total - d), taper))
        .collect();

    let positions: Vec<Vec2> = samples.iter().map(|s| s.pos).collect();
    let kept = simplify(&positions, &widths, size / 2.0, opts.tolerance.max(0.0));
    let points: Vec<Vec2> = kept.iter().map(|&i| positions[i]).collect();
    let mut shape = Shape::new(
        "Stroke",
        smooth_path(&points),
        Style {
            fill: None,
            stroke: Some(Stroke {
                color: opts.color,
                width: size,
            }),
        },
    );
    shape.widths = Some(kept.iter().map(|&i| widths[i]).collect());
    Some(shape)
}

/// Build a closed shape from a sketched outline: the outline simplified to
/// within `tolerance`, joined with smooth curves that pass through every kept
/// point, and closed from the last point back to the first.
///
/// Returns `None` when the sketch has no inside: fewer than three distinct
/// points are left, or the points lie along one line. The pen need not return
/// to where it started.
pub fn build_outline(samples: &[BrushSample], tolerance: f32, style: Style) -> Option<Shape> {
    let mut positions: Vec<Vec2> = Vec::with_capacity(samples.len());
    for s in samples.iter().filter(|s| s.pos.is_finite()) {
        if positions.last().is_none_or(|p| p.distance(s.pos) > 1e-4) {
            positions.push(s.pos);
        }
    }
    // a pen that ended on its start does not repeat it
    while positions.len() > 1
        && positions
            .first()
            .zip(positions.last())
            .is_some_and(|(a, b)| a.distance(*b) <= tolerance)
    {
        positions.pop();
    }
    if positions.len() < 3 {
        return None;
    }
    let widths = vec![1.0; positions.len()];
    let kept: Vec<Vec2> = simplify(&positions, &widths, 0.0, tolerance.max(0.0))
        .into_iter()
        .map(|i| positions[i])
        .collect();
    if kept.len() < 3 {
        return None;
    }
    // all on one line: nothing inside
    let (a, b) = (kept[0], kept[kept.len() / 2]);
    let wide = kept
        .iter()
        .any(|p| ((*p - a).perp_dot(b - a)).abs() > tolerance.max(1e-3) * a.distance(b).max(1e-3));
    if !wide {
        return None;
    }
    Some(Shape::new("Shape", smooth_closed_path(&kept), style))
}

/// A closed path through `points` with smooth (Catmull-Rom) curves, going
/// round from the last point to the first.
fn smooth_closed_path(points: &[Vec2]) -> Path {
    let n = points.len() as isize;
    let at = |i: isize| points[i.rem_euclid(n) as usize];
    let mut path = Path::new().move_to(points[0]);
    for i in 0..n {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let limit = p1.distance(p2) / 2.0;
        let c1 = p1 + ((p2 - p0) / 6.0).clamp_length_max(limit);
        let c2 = p2 - ((p3 - p1) / 6.0).clamp_length_max(limit);
        path = path.cubic_to(c1, c2, p2);
    }
    path.close()
}

fn pressure_width(pressure: f32) -> f32 {
    MIN_PRESSURE_WIDTH + (1.0 - MIN_PRESSURE_WIDTH) * pressure.clamp(0.0, 1.0)
}

/// Width factor at `d` from the nearer end of the stroke: thin at the tip,
/// easing out to full width over `taper`.
fn taper_width(d: f32, taper: f32) -> f32 {
    if taper <= 0.0 {
        return 1.0;
    }
    let t = (d / taper).clamp(0.0, 1.0);
    TIP_WIDTH + (1.0 - TIP_WIDTH) * t * (2.0 - t)
}

/// Erase along `eraser`, a line through points in the shape's own space,
/// everything within `radius` of it.
///
/// Lines (shapes with a stroke and no fill) are cut where the eraser
/// crosses them; each remaining piece keeps its width profile and is
/// smoothed again to within `tolerance`. A shape lying wholly under the
/// eraser, such as a dot, is removed. Filled shapes the eraser only partly
/// covers are left alone. Returns `None` when the shape is untouched, else
/// what is left of it, which may be nothing.
pub fn erase(shape: &Shape, eraser: &[Vec2], radius: f32, tolerance: f32) -> Option<Vec<Shape>> {
    if eraser.is_empty() || !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let covered = |p: Vec2| eraser_distance(eraser, p) <= radius;
    // points close enough together that the eraser cannot slip between them
    let runs = dense_runs(shape, radius / 2.0);
    let points = || runs.iter().flat_map(|run| run.iter().map(|(p, _)| *p));
    if !points().any(covered) {
        return None;
    }
    if points().all(covered) {
        return Some(Vec::new());
    }
    let stroke = shape.style.stroke?;
    if shape.style.fill.is_some() {
        return None;
    }
    let mut pieces = Vec::new();
    for run in &runs {
        let mut piece: Vec<(Vec2, f32)> = Vec::new();
        for &(p, w) in run.iter().chain([&(Vec2::NAN, 0.0)]) {
            // NaN ends the last piece
            if p.is_finite() && !covered(p) {
                piece.push((p, w));
                continue;
            }
            if piece.len() >= 2 {
                let (pts, widths): (Vec<Vec2>, Vec<f32>) = piece.iter().copied().unzip();
                let kept = simplify(&pts, &widths, stroke.width / 2.0, tolerance.max(0.0));
                let kept_pts: Vec<Vec2> = kept.iter().map(|&i| pts[i]).collect();
                let mut cut = shape.clone();
                cut.path = smooth_path(&kept_pts);
                cut.widths = Some(kept.iter().map(|&i| widths[i]).collect());
                pieces.push(cut);
            }
            piece.clear();
        }
    }
    Some(pieces)
}

/// Distance from `p` to the eraser's line.
fn eraser_distance(eraser: &[Vec2], p: Vec2) -> f32 {
    if eraser.len() == 1 {
        return p.distance(eraser[0]);
    }
    eraser
        .windows(2)
        .map(|w| {
            let ab = w[1] - w[0];
            let len2 = ab.length_squared();
            let t = if len2 > 0.0 {
                ((p - w[0]).dot(ab) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            p.distance(w[0] + ab * t)
        })
        .fold(f32::INFINITY, f32::min)
}

/// The shape's outline as runs of points no more than `step` apart, each
/// with the stroke width factor there (from the profile, or 1). A closed
/// subpath ends back at its start.
fn dense_runs(shape: &Shape, step: f32) -> Vec<Vec<(Vec2, f32)>> {
    let widths = shape
        .widths
        .as_deref()
        .filter(|w| w.len() == shape.path.on_curve_count());
    let width_at = |k: usize| widths.and_then(|w| w.get(k)).copied().unwrap_or(1.0);
    let step = if step.is_finite() && step > 0.0 { step } else { 1.0 };
    let mut runs: Vec<Vec<(Vec2, f32)>> = Vec::new();
    let mut k = 0; // index of the next on-curve point
    let mut start = None;
    let add = |runs: &mut Vec<Vec<(Vec2, f32)>>, to: Vec2, w1: f32, curve: Option<(Vec2, Vec2)>| {
        let Some(run) = runs.last_mut() else { return };
        let &(from, w0) = run.last().expect("runs start with a point");
        let (c1, c2) = curve.unwrap_or((from, to));
        let hull = from.distance(c1) + c1.distance(c2) + c2.distance(to);
        let n = (hull / step).ceil().clamp(1.0, 10_000.0) as usize;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let p = from * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + to * (t * t * t);
            run.push((p, w0 + (w1 - w0) * t));
        }
    };
    for cmd in &shape.path.cmds {
        match *cmd {
            PathCmd::MoveTo(p) => {
                runs.push(vec![(p, width_at(k))]);
                start = Some((p, width_at(k)));
                k += 1;
            }
            PathCmd::LineTo(p) => {
                add(&mut runs, p, width_at(k), None);
                k += 1;
            }
            PathCmd::CubicTo(c1, c2, p) => {
                add(&mut runs, p, width_at(k), Some((c1, c2)));
                k += 1;
            }
            PathCmd::Close => {
                if let Some((p, w)) = start.take() {
                    add(&mut runs, p, w, None);
                }
            }
        }
    }
    runs
}

/// Indices of the samples to keep (Ramer-Douglas-Peucker), treating the
/// half-width as a third coordinate so changes in width are kept too.
fn simplify(points: &[Vec2], widths: &[f32], half_size: f32, tolerance: f32) -> Vec<usize> {
    let point = |i: usize| (points[i], widths[i] * half_size);
    let last = points.len() - 1;
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[last] = true;
    let mut stack = vec![(0, last)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let ((pa, wa), (pb, wb)) = (point(a), point(b));
        let seg = pb - pa;
        let len2 = seg.length_squared();
        let (mut worst, mut worst_d) = (a, 0.0);
        for i in a + 1..b {
            let (p, w) = point(i);
            let t = if len2 > 0.0 {
                ((p - pa).dot(seg) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let off_line = p.distance(pa + seg * t);
            let off_width = (w - (wa + (wb - wa) * t)).abs();
            let d = off_line.max(off_width);
            if d > worst_d {
                (worst, worst_d) = (i, d);
            }
        }
        if worst_d > tolerance {
            keep[worst] = true;
            stack.push((a, worst));
            stack.push((worst, b));
        }
    }
    (0..points.len()).filter(|&i| keep[i]).collect()
}

/// A path through `points` with smooth (Catmull-Rom) curves. Control
/// handles are kept within half of their segment so uneven spacing cannot
/// make the curve loop.
fn smooth_path(points: &[Vec2]) -> Path {
    let mut path = Path::new().move_to(points[0]);
    if points.len() == 2 {
        return path.line_to(points[1]);
    }
    let at = |i: isize| points[i.clamp(0, points.len() as isize - 1) as usize];
    for i in 0..points.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let limit = p1.distance(p2) / 2.0;
        let c1 = p1 + ((p2 - p0) / 6.0).clamp_length_max(limit);
        let c2 = p2 - ((p3 - p1) / 6.0).clamp_length_max(limit);
        path = path.cubic_to(c1, c2, p2);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PathCmd;

    fn opts() -> BrushOptions {
        BrushOptions {
            color: Color::BLACK,
            size: 10.0,
            taper: 20.0,
            tolerance: 0.5,
        }
    }

    fn line(from: Vec2, to: Vec2, n: usize) -> Vec<BrushSample> {
        (0..n)
            .map(|i| BrushSample::new(from.lerp(to, i as f32 / (n - 1) as f32), 1.0))
            .collect()
    }

    fn circle_samples(n: usize, radius: f32) -> Vec<BrushSample> {
        (0..n)
            .map(|i| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                BrushSample::new(Vec2::new(a.cos(), a.sin()) * radius, 1.0)
            })
            .collect()
    }

    #[test]
    fn a_sketched_loop_becomes_a_closed_smooth_shape() {
        let style = Style::fill(Color::WHITE);
        let shape = build_outline(&circle_samples(120, 100.0), 0.5, style).expect("a loop has an inside");
        assert_eq!(shape.style, style);
        assert!(
            shape.path.cmds.iter().any(|c| matches!(c, PathCmd::Close)),
            "closed"
        );
        // smooth: curves, and far fewer points than were sketched
        assert!(shape.path.cmds.iter().any(|c| matches!(c, PathCmd::CubicTo(..))));
        assert!(
            shape.path.on_curve_count() < 60,
            "{}",
            shape.path.on_curve_count()
        );
        // and it still is the loop that was drawn: every kept point on the circle
        for p in shape.path.on_curve_points() {
            assert!((p.length() - 100.0).abs() < 1.0, "{p}");
        }
        assert!(shape.widths.is_none(), "an outline has an even stroke");
    }

    #[test]
    fn a_pen_that_stops_short_of_its_start_still_closes() {
        // three quarters of a circle: the shape closes across the gap
        let mut samples = circle_samples(120, 50.0);
        samples.truncate(90);
        let shape =
            build_outline(&samples, 0.5, Style::fill(Color::BLACK)).expect("three quarters has an inside");
        assert!(shape.path.cmds.iter().any(|c| matches!(c, PathCmd::Close)));
    }

    #[test]
    fn a_sketch_with_no_inside_makes_no_shape() {
        let style = Style::fill(Color::BLACK);
        // a dab, a short line and a straight line all enclose nothing
        assert!(build_outline(&[BrushSample::new(Vec2::ZERO, 1.0)], 0.5, style).is_none());
        assert!(build_outline(&line(Vec2::ZERO, Vec2::new(100.0, 0.0), 30), 0.5, style).is_none());
        assert!(build_outline(&[], 0.5, style).is_none());
        // a sketch that goes nowhere finite is not a shape either
        let bad = [BrushSample::new(Vec2::new(f32::NAN, 0.0), 1.0); 5];
        assert!(build_outline(&bad, 0.5, style).is_none());
    }

    fn end_points(shape: &Shape) -> (Vec2, Vec2) {
        let pts: Vec<Vec2> = shape.path.points().collect();
        (pts[0], *pts.last().unwrap())
    }

    #[test]
    fn straight_line_simplifies_but_keeps_its_ends_and_taper() {
        let samples = line(Vec2::ZERO, Vec2::new(200.0, 0.0), 101);
        let shape = build_stroke(&samples, &opts()).unwrap();
        assert_eq!(end_points(&shape), (Vec2::ZERO, Vec2::new(200.0, 0.0)));
        let widths = shape.widths.as_ref().unwrap();
        assert_eq!(widths.len(), shape.path.on_curve_count());
        // the tapers need a few points; the straight middle needs none
        assert!(widths.len() < 20, "{} points kept", widths.len());
        assert!((widths[0] - TIP_WIDTH).abs() < 1e-6);
        assert!((widths[widths.len() - 1] - TIP_WIDTH).abs() < 1e-6);
        assert!(widths.iter().any(|w| (w - 1.0).abs() < 1e-6));
        assert_eq!(shape.style.stroke.unwrap().width, 10.0);
        assert!(shape.style.fill.is_none());
    }

    #[test]
    fn pressure_sets_the_width_profile() {
        let mut samples = line(Vec2::ZERO, Vec2::new(100.0, 0.0), 51);
        for (i, s) in samples.iter_mut().enumerate() {
            s.pressure = i as f32 / 50.0;
        }
        let shape = build_stroke(&samples, &BrushOptions { taper: 0.0, ..opts() }).unwrap();
        let widths = shape.widths.unwrap();
        assert!((widths[0] - MIN_PRESSURE_WIDTH).abs() < 1e-6);
        assert!((widths[widths.len() - 1] - 1.0).abs() < 1e-6);
        assert!(widths.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn a_tap_makes_a_round_dot() {
        let shape = build_stroke(&[BrushSample::new(Vec2::new(5.0, 5.0), 1.0)], &opts()).unwrap();
        assert_eq!(shape.style.fill, Some(Color::BLACK));
        assert!(shape.widths.is_none());
        let (lo, hi) = shape
            .path
            .points()
            .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)));
        assert!(((hi - lo) - Vec2::splat(10.0)).abs().max_element() < 1e-4);
    }

    #[test]
    fn curve_passes_through_kept_points_without_overshoot() {
        // a quarter circle drawn as many samples
        let samples: Vec<BrushSample> = (0..=90)
            .map(|deg| {
                let a = (deg as f32).to_radians();
                BrushSample::new(Vec2::new(a.cos(), a.sin()) * 100.0, 1.0)
            })
            .collect();
        let shape = build_stroke(&samples, &opts()).unwrap();
        for cmd in &shape.path.cmds {
            if let PathCmd::CubicTo(_, _, p) | PathCmd::LineTo(p) | PathCmd::MoveTo(p) = cmd {
                assert!((p.length() - 100.0).abs() < 0.6, "{p:?} is off the arc");
            }
        }
    }

    #[test]
    fn bad_samples_are_ignored() {
        assert!(build_stroke(&[], &opts()).is_none());
        let nan = BrushSample::new(Vec2::NAN, 1.0);
        assert!(build_stroke(&[nan], &opts()).is_none());
    }

    fn drawn_line() -> Shape {
        let samples = line(Vec2::ZERO, Vec2::new(200.0, 0.0), 101);
        build_stroke(&samples, &opts()).unwrap()
    }

    #[test]
    fn eraser_cuts_a_line_in_two() {
        let shape = drawn_line();
        let eraser = [Vec2::new(100.0, -50.0), Vec2::new(100.0, 50.0)];
        let pieces = erase(&shape, &eraser, 10.0, 0.5).unwrap();
        assert_eq!(pieces.len(), 2);
        let (a0, a1) = end_points(&pieces[0]);
        let (b0, b1) = end_points(&pieces[1]);
        assert_eq!(a0, Vec2::ZERO);
        assert!(a1.x < 90.5 && a1.x > 85.0, "left piece ends at {a1:?}");
        assert!(b0.x > 109.5 && b0.x < 115.0, "right piece starts at {b0:?}");
        assert_eq!(b1, Vec2::new(200.0, 0.0));
        for p in &pieces {
            assert_eq!(p.widths.as_ref().unwrap().len(), p.path.on_curve_count());
            assert_eq!(p.style, shape.style);
        }
        // the tapered start is still there
        assert!((pieces[0].widths.as_ref().unwrap()[0] - TIP_WIDTH).abs() < 1e-6);
    }

    #[test]
    fn eraser_missing_the_shape_changes_nothing() {
        let eraser = [Vec2::new(0.0, 50.0), Vec2::new(200.0, 50.0)];
        assert!(erase(&drawn_line(), &eraser, 10.0, 0.5).is_none());
    }

    #[test]
    fn eraser_removes_what_it_covers_and_spares_filled_shapes() {
        let dot = build_stroke(&[BrushSample::new(Vec2::ZERO, 1.0)], &opts()).unwrap();
        assert_eq!(erase(&dot, &[Vec2::ZERO], 10.0, 0.5), Some(vec![]));
        let square = Shape::new(
            "sq",
            Path::rect(Vec2::ZERO, Vec2::splat(100.0), 0.0),
            Style::fill(Color::BLACK),
        );
        assert!(erase(&square, &[Vec2::ZERO], 10.0, 0.5).is_none());
    }

    #[test]
    fn eraser_cuts_straight_lines_between_their_points() {
        // a two-point line: the eraser must find its middle
        let shape = Shape::new(
            "l",
            Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(100.0, 0.0)),
            Style::default().with_stroke(Color::BLACK, 2.0),
        );
        let pieces = erase(&shape, &[Vec2::new(50.0, 0.0)], 5.0, 0.5).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(
            pieces[0].widths.as_ref().unwrap(),
            &vec![1.0; pieces[0].path.on_curve_count()]
        );
    }

    #[test]
    fn stabiliser_off_keeps_the_input_path() {
        let mut st = Stabiliser::new(0.0, 1.0);
        for s in line(Vec2::ZERO, Vec2::new(10.0, 0.0), 3) {
            st.push(s);
        }
        let out = st.samples();
        assert_eq!(out.first().unwrap().pos, Vec2::ZERO);
        assert_eq!(out.last().unwrap().pos, Vec2::new(10.0, 0.0));
        assert!(out.iter().all(|s| s.pos.y == 0.0));
        // resampled every unit
        assert_eq!(out.len(), 11);
    }

    #[test]
    fn stabiliser_smooths_jitter_and_ends_at_the_pen() {
        let jitter = |strength: f32| {
            let mut st = Stabiliser::new(strength, 1.0);
            for i in 0..=100 {
                let y = if i % 2 == 0 { 3.0 } else { -3.0 };
                st.push(BrushSample::new(Vec2::new(i as f32 * 2.0, y), 1.0));
            }
            let out = st.samples();
            assert_eq!(out.last().unwrap().pos, Vec2::new(200.0, 3.0));
            // wobble in the middle of the stroke
            out[40..out.len() - 40]
                .iter()
                .map(|s| s.pos.y.abs())
                .fold(0.0, f32::max)
        };
        let (raw, smooth) = (jitter(0.0), jitter(0.6));
        assert!(smooth < raw / 3.0, "jitter {raw} became {smooth}");
    }

    #[test]
    fn stabiliser_ignores_non_finite_input() {
        let mut st = Stabiliser::new(0.5, 1.0);
        st.push(BrushSample::new(Vec2::new(f32::INFINITY, 0.0), 1.0));
        assert!(st.is_empty());
        assert!(st.samples().is_empty());
    }
}
