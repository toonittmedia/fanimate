//! Line clean-up: refit a rough hand-drawn path with the fewest smooth curves
//! that stay within a tolerance of it.
//!
//! This is a classical method, not a model: each subpath is flattened to a
//! polyline, split where it turns sharply (a corner the artist drew on
//! purpose stays a corner), and every smooth run between corners is fitted
//! with cubic Béziers by least squares (Schneider's algorithm: chord-length
//! parameters, Newton re-parameterisation, splitting at the worst point until
//! every piece is within the tolerance). A closed outline with no corner is
//! fitted as a ring, so the seam is as smooth as the rest.
//!
//! The result never strays from the input by more than `tolerance` (up to
//! the flattening error, which is a tenth of it), and a path that is already
//! clean comes out with no more points than it went in with.

use glam::{DVec2, Vec2};

use crate::path::{Path, PathCmd};

/// The most points one subpath may have before it is left alone, so a
/// hostile project cannot make a clean-up run for long.
pub const MAX_POINTS: usize = 200_000;

/// Settings for [`clean_path`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanOptions {
    /// How far the result may stray from the input, in document units.
    pub tolerance: f32,
    /// A turn of more than this many degrees is a corner and is kept.
    pub corner_degrees: f32,
}

impl Default for CleanOptions {
    fn default() -> Self {
        Self {
            tolerance: 1.5,
            corner_degrees: 50.0,
        }
    }
}

/// What a clean-up did, for the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CleanReport {
    /// On-curve points before.
    pub before: usize,
    /// On-curve points after.
    pub after: usize,
}

/// The path refitted. Returns the new path and what changed. A path with
/// nothing to fit (a point, an empty path) comes back as it was.
pub fn clean_path(path: &Path, options: CleanOptions) -> (Path, CleanReport) {
    let defaults = CleanOptions::default();
    let number = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
    let tolerance = f64::from(number(options.tolerance, defaults.tolerance).clamp(0.01, 1000.0));
    let corner =
        f64::from(number(options.corner_degrees, defaults.corner_degrees).clamp(5.0, 175.0)).to_radians();
    let mut out = Path::new();
    for (points, closed) in path.polylines((tolerance / 10.0) as f32) {
        let points: Vec<DVec2> = dedupe(&points);
        if points.len() < 3 || points.len() > MAX_POINTS || !points.iter().all(|p| p.is_finite()) {
            copy_polyline(&mut out, &points, closed);
            continue;
        }
        fit_subpath(&mut out, &points, closed, tolerance, corner);
    }
    let report = CleanReport {
        before: path.on_curve_count(),
        after: out.on_curve_count(),
    };
    (out, report)
}

fn dedupe(points: &[Vec2]) -> Vec<DVec2> {
    let mut out: Vec<DVec2> = Vec::with_capacity(points.len());
    for p in points {
        let p = p.as_dvec2();
        if out.last().is_none_or(|q| q.distance_squared(p) > 1.0e-9) {
            out.push(p);
        }
    }
    if out.len() > 1 && out[0].distance_squared(out[out.len() - 1]) <= 1.0e-9 {
        out.pop();
    }
    out
}

fn copy_polyline(out: &mut Path, points: &[DVec2], closed: bool) {
    let mut it = points.iter();
    if let Some(first) = it.next() {
        out.cmds.push(PathCmd::MoveTo(first.as_vec2()));
        for p in it {
            out.cmds.push(PathCmd::LineTo(p.as_vec2()));
        }
        if closed && points.len() > 1 {
            out.cmds.push(PathCmd::Close);
        }
    }
}

/// The unit direction from `a` to `b`, or zero.
fn dir(a: DVec2, b: DVec2) -> DVec2 {
    (b - a).normalize_or_zero()
}

/// Indices of the vertices where the line turns by more than `corner`.
/// The turn is measured across a short stretch each side, so the many
/// small steps of a flattened curve do not add up to a false corner.
fn corners(points: &[DVec2], closed: bool, corner: f64, tolerance: f64) -> Vec<usize> {
    let n = points.len();
    let reach = (tolerance * 3.0).max(1.0);
    let at = |i: isize| -> DVec2 {
        if closed {
            points[i.rem_euclid(n as isize) as usize]
        } else {
            points[i.clamp(0, n as isize - 1) as usize]
        }
    };
    // the first point `reach` away from `i`, walking by `step`
    let away = |i: usize, step: isize| -> DVec2 {
        let from = points[i];
        let mut j = i as isize;
        for _ in 0..n {
            j += step;
            if !closed && (j < 0 || j >= n as isize) {
                return at(j);
            }
            let p = at(j);
            if p.distance(from) >= reach {
                return p;
            }
        }
        at(j)
    };
    let mut found = Vec::new();
    for (i, &here) in points.iter().enumerate() {
        if !closed && (i == 0 || i == n - 1) {
            continue;
        }
        let (back, ahead) = (away(i, -1), away(i, 1));
        let (d_in, d_out) = (dir(back, here), dir(here, ahead));
        if d_in == DVec2::ZERO || d_out == DVec2::ZERO {
            continue;
        }
        let turn = d_in.dot(d_out).clamp(-1.0, 1.0).acos();
        if turn > corner {
            found.push(i);
        }
    }
    // a corner spread over several vertices is one corner: keep the sharpest
    let mut kept: Vec<usize> = Vec::new();
    for i in found {
        if let Some(&last) = kept.last()
            && points[i].distance(points[last]) < reach * 2.0
        {
            let sharp = |k: usize| {
                let (a, b) = (
                    dir(at(k as isize - 1), points[k]),
                    dir(points[k], at(k as isize + 1)),
                );
                a.dot(b)
            };
            if sharp(i) < sharp(last) {
                *kept.last_mut().unwrap_or(&mut 0) = i;
            }
            continue;
        }
        kept.push(i);
    }
    kept
}

fn fit_subpath(out: &mut Path, points: &[DVec2], closed: bool, tolerance: f64, corner: f64) {
    let n = points.len();
    let mut cuts = corners(points, closed, corner, tolerance);
    if closed {
        // a ring with no corner is cut in two, at the start and the far side
        let smooth_cuts = cuts.is_empty();
        if smooth_cuts {
            cuts = vec![0, n / 2];
        } else if cuts.len() == 1 {
            // one corner: the run goes right round to it again
            cuts.push(cuts[0]);
        }
        out.cmds.push(PathCmd::MoveTo(points[cuts[0]].as_vec2()));
        for w in 0..cuts.len() {
            let (from, to) = (cuts[w], cuts[(w + 1) % cuts.len()]);
            let run = ring_run(points, from, to);
            // a cut that is not a corner continues smoothly through it
            let t1 = if smooth_cuts {
                ring_tangent(points, from)
            } else {
                dir(run[0], run[1])
            };
            let t2 = if smooth_cuts {
                -ring_tangent(points, to)
            } else {
                dir(run[run.len() - 1], run[run.len() - 2])
            };
            fit(out, &run, t1, t2, tolerance, 0);
        }
        out.cmds.push(PathCmd::Close);
    } else {
        let mut starts = vec![0];
        starts.extend(cuts);
        starts.push(n - 1);
        out.cmds.push(PathCmd::MoveTo(points[0].as_vec2()));
        for w in starts.windows(2) {
            let run = &points[w[0]..=w[1]];
            if run.len() < 2 {
                continue;
            }
            let t1 = dir(run[0], run[1]);
            let t2 = dir(run[run.len() - 1], run[run.len() - 2]);
            fit(out, run, t1, t2, tolerance, 0);
        }
    }
}

/// The points from index `from` round the ring to `to`, both included. When
/// `from == to` it is the whole ring, ending back at its start.
fn ring_run(points: &[DVec2], from: usize, to: usize) -> Vec<DVec2> {
    let n = points.len();
    let mut run = vec![points[from]];
    let mut i = from;
    loop {
        i = (i + 1) % n;
        run.push(points[i]);
        if i == to {
            break;
        }
    }
    run
}

/// The direction the ring runs in at vertex `i`, from its two neighbours.
fn ring_tangent(points: &[DVec2], i: usize) -> DVec2 {
    let n = points.len();
    dir(points[(i + n - 1) % n], points[(i + 1) % n])
}

/// Fit `run` (at least two points) with Béziers, appending the curves to
/// `out`, which already ends at the run's first point. `t1` points into the
/// run from its start and `t2` into it from its end.
fn fit(out: &mut Path, run: &[DVec2], t1: DVec2, t2: DVec2, error: f64, depth: usize) {
    let last = run[run.len() - 1];
    if run.len() == 2 {
        let dist = run[0].distance(last) / 3.0;
        push(out, run[0] + t1 * dist, last + t2 * dist, last);
        return;
    }
    let mut u = chord_parameters(run);
    let mut curve = generate(run, &u, t1, t2);
    let (mut worst, mut split) = max_error(run, &curve, &u);
    if worst < error {
        push(out, curve[1], curve[2], curve[3]);
        return;
    }
    // a near miss is worth improving the parameters before splitting
    if worst < error * 16.0 {
        for _ in 0..6 {
            u = reparameterise(run, &u, &curve);
            curve = generate(run, &u, t1, t2);
            (worst, split) = max_error(run, &curve, &u);
            if worst < error {
                push(out, curve[1], curve[2], curve[3]);
                return;
            }
        }
    }
    if depth > 40 || split == 0 || split >= run.len() - 1 {
        // cannot be split further: a straight piece is still within reach of the data
        push(
            out,
            run[0] + (last - run[0]) / 3.0,
            run[0] + (last - run[0]) * (2.0 / 3.0),
            last,
        );
        return;
    }
    let centre = dir(run[split + 1], run[split - 1]);
    fit(out, &run[..=split], t1, centre, error, depth + 1);
    fit(out, &run[split..], -centre, t2, error, depth + 1);
}

fn push(out: &mut Path, c1: DVec2, c2: DVec2, to: DVec2) {
    out.cmds
        .push(PathCmd::CubicTo(c1.as_vec2(), c2.as_vec2(), to.as_vec2()));
}

fn chord_parameters(run: &[DVec2]) -> Vec<f64> {
    let mut u = vec![0.0];
    for w in run.windows(2) {
        let last = u[u.len() - 1];
        u.push(last + w[0].distance(w[1]));
    }
    let total = u[u.len() - 1].max(f64::MIN_POSITIVE);
    u.iter().map(|v| v / total).collect()
}

/// The least-squares cubic through the ends of `run` whose end tangents
/// point along `t1` and `t2`; returns its four control points.
fn generate(run: &[DVec2], u: &[f64], t1: DVec2, t2: DVec2) -> [DVec2; 4] {
    let (first, last) = (run[0], run[run.len() - 1]);
    let (mut c00, mut c01, mut c11) = (0.0, 0.0, 0.0);
    let (mut x0, mut x1) = (0.0, 0.0);
    for (p, &t) in run.iter().zip(u) {
        let (b0, b1, b2, b3) = bernstein(t);
        let a1 = t1 * b1;
        let a2 = t2 * b2;
        c00 += a1.dot(a1);
        c01 += a1.dot(a2);
        c11 += a2.dot(a2);
        let tmp = *p - (first * (b0 + b1) + last * (b2 + b3));
        x0 += a1.dot(tmp);
        x1 += a2.dot(tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let chord = first.distance(last);
    let eps = 1.0e-6 * chord;
    let (mut alpha1, mut alpha2) = if det.abs() > 1.0e-12 {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    } else {
        (0.0, 0.0)
    };
    // a handle that is tiny or points backwards makes a kink: fall back to a third of the chord
    if alpha1 < eps || alpha2 < eps || !alpha1.is_finite() || !alpha2.is_finite() {
        alpha1 = chord / 3.0;
        alpha2 = chord / 3.0;
    }
    [first, first + t1 * alpha1, last + t2 * alpha2, last]
}

fn bernstein(t: f64) -> (f64, f64, f64, f64) {
    let s = 1.0 - t;
    (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t)
}

fn at(curve: &[DVec2; 4], t: f64) -> DVec2 {
    let (b0, b1, b2, b3) = bernstein(t);
    curve[0] * b0 + curve[1] * b1 + curve[2] * b2 + curve[3] * b3
}

/// The largest distance from a point of `run` to the curve at its
/// parameter, and which point it is (never an end).
fn max_error(run: &[DVec2], curve: &[DVec2; 4], u: &[f64]) -> (f64, usize) {
    let (mut worst, mut index) = (0.0, run.len() / 2);
    for i in 1..run.len() - 1 {
        let d = at(curve, u[i]).distance(run[i]);
        if d > worst {
            worst = d;
            index = i;
        }
    }
    (worst, index)
}

/// One Newton step toward the parameter of each point's nearest point on the curve.
fn reparameterise(run: &[DVec2], u: &[f64], curve: &[DVec2; 4]) -> Vec<f64> {
    run.iter()
        .zip(u)
        .map(|(p, &t)| {
            let q = at(curve, t);
            // first and second derivatives of the cubic
            let d1 = ((curve[1] - curve[0]) * (1.0 - t) * (1.0 - t)
                + (curve[2] - curve[1]) * (2.0 * t * (1.0 - t))
                + (curve[3] - curve[2]) * (t * t))
                * 3.0;
            let d2 = ((curve[2] - curve[1] * 2.0 + curve[0]) * (1.0 - t)
                + (curve[3] - curve[2] * 2.0 + curve[1]) * t)
                * 6.0;
            let diff = q - *p;
            let top = diff.dot(d1);
            let bottom = d1.dot(d1) + diff.dot(d2);
            if bottom.abs() < 1.0e-12 {
                t
            } else {
                (t - top / bottom).clamp(0.0, 1.0)
            }
        })
        .collect()
}

/// A brush stroke's width profile (one width per on-curve point of `old`)
/// carried over to `new`: each new point takes the width at the place along
/// the old line that it lies nearest to. Only for a path of one subpath;
/// `None` when the widths do not fit `old`, or there is more than one.
pub fn resample_widths(old: &Path, widths: &[f32], new: &Path) -> Option<Vec<f32>> {
    let single = |p: &Path| p.cmds.iter().filter(|c| matches!(c, PathCmd::MoveTo(_))).count() == 1;
    let closed = old.cmds.iter().any(|c| matches!(c, PathCmd::Close));
    let mut line = old.on_curve_points();
    if widths.len() != line.len() || line.len() < 2 || !single(old) || !single(new) {
        return None;
    }
    let mut w = widths.to_vec();
    if closed {
        line.push(line[0]);
        w.push(w[0]);
    }
    let mut out = Vec::new();
    for p in new.on_curve_points() {
        let (mut best, mut at) = (f32::MAX, 0.0f32);
        for (i, pair) in line.windows(2).enumerate() {
            let ab = pair[1] - pair[0];
            let len2 = ab.length_squared();
            let t = if len2 > 0.0 {
                ((p - pair[0]).dot(ab) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = p.distance(pair[0] + ab * t);
            if d < best {
                best = d;
                at = i as f32 + t;
            }
        }
        let (i, t) = (at.floor() as usize, at.fract());
        let (i, t) = if i + 1 >= w.len() {
            (w.len() - 2, 1.0)
        } else {
            (i, t)
        };
        out.push(w[i] + (w[i + 1] - w[i]) * t);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far `a` strays from `b`, sampled finely: the larger of the two
    /// one-way distances.
    fn hausdorff(a: &Path, b: &Path) -> f32 {
        let ring = |path: &Path| -> Vec<Vec2> {
            path.polylines(0.05)
                .into_iter()
                .flat_map(|(mut p, closed)| {
                    if closed && let Some(first) = p.first().copied() {
                        p.push(first);
                    }
                    p
                })
                .collect()
        };
        let (pa, pb) = (ring(a), ring(b));
        let one_way = |from: &[Vec2], to: &[Vec2]| {
            from.iter()
                .map(|p| {
                    to.windows(2)
                        .map(|s| seg_distance(*p, s[0], s[1]))
                        .fold(f32::MAX, f32::min)
                })
                .fold(0.0, f32::max)
        };
        one_way(&pa, &pb).max(one_way(&pb, &pa))
    }

    fn seg_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
        let ab = b - a;
        let t = if ab.length_squared() == 0.0 {
            0.0
        } else {
            ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        };
        p.distance(a + ab * t)
    }

    /// A deterministic wobble in [-1, 1].
    fn wobble(i: usize) -> f32 {
        let x = (i as f32 * 12.9898).sin() * 43758.547;
        (x - x.floor()) * 2.0 - 1.0
    }

    fn rough_circle(points: usize, jitter: f32) -> Path {
        let mut p = Path::new();
        for i in 0..points {
            let a = i as f32 / points as f32 * std::f32::consts::TAU;
            let r = 100.0 + wobble(i) * jitter;
            let v = Vec2::new(200.0 + r * a.cos(), 200.0 + r * a.sin());
            p = if i == 0 { p.move_to(v) } else { p.line_to(v) };
        }
        p.close()
    }

    #[test]
    fn a_rough_hand_drawn_circle_becomes_a_few_curves_close_to_it() {
        let rough = rough_circle(300, 0.6);
        let (clean, report) = clean_path(&rough, CleanOptions::default());
        assert_eq!(report.before, 300);
        assert!(report.after <= 16, "{} points left", report.after);
        assert!(report.after >= 3);
        let d = hausdorff(&clean, &rough);
        assert!(d < 2.2, "strayed {d}");
        assert!(matches!(clean.cmds.last(), Some(PathCmd::Close)));
    }

    #[test]
    fn a_corner_the_artist_drew_stays_a_corner() {
        // an L: two straight strokes meeting at a right angle, drawn as many small steps
        let mut p = Path::new().move_to(Vec2::new(0.0, 0.0));
        for i in 1..=40 {
            p = p.line_to(Vec2::new(i as f32 * 5.0, wobble(i) * 0.3));
        }
        for i in 1..=40 {
            p = p.line_to(Vec2::new(200.0 + wobble(i + 99) * 0.3, i as f32 * 5.0));
        }
        let (clean, report) = clean_path(&p, CleanOptions::default());
        assert!(report.after <= 6, "{} points", report.after);
        // the corner point is among the kept points
        assert!(clean.points().any(|q| q.distance(Vec2::new(200.0, 0.0)) < 3.0));
        assert!(hausdorff(&clean, &p) < 2.0);
    }

    #[test]
    fn a_clean_path_does_not_grow() {
        let circle = Path::circle(Vec2::new(50.0, 50.0), 40.0);
        let (clean, report) = clean_path(&circle, CleanOptions::default());
        assert!(report.after <= report.before.max(4) + 2, "{report:?}");
        assert!(hausdorff(&clean, &circle) < 1.0);
        let rect = Path::rect(Vec2::ZERO, Vec2::new(80.0, 40.0), 0.0);
        let (clean, _) = clean_path(&rect, CleanOptions::default());
        assert!(hausdorff(&clean, &rect) < 1.0);
        assert!(clean.on_curve_count() <= 8);
    }

    #[test]
    fn a_smaller_tolerance_keeps_more_of_the_wobble() {
        let rough = rough_circle(300, 2.0);
        let loose = clean_path(
            &rough,
            CleanOptions {
                tolerance: 4.0,
                ..CleanOptions::default()
            },
        )
        .1;
        let tight = clean_path(
            &rough,
            CleanOptions {
                tolerance: 0.5,
                ..CleanOptions::default()
            },
        )
        .1;
        assert!(tight.after > loose.after, "{} vs {}", tight.after, loose.after);
    }

    #[test]
    fn hostile_paths_come_back_without_a_panic() {
        let nan = Path::new()
            .move_to(Vec2::new(f32::NAN, 0.0))
            .line_to(Vec2::new(1.0, 1.0))
            .line_to(Vec2::new(2.0, 0.0));
        let _ = clean_path(&nan, CleanOptions::default());
        let single = Path::new().move_to(Vec2::new(1.0, 1.0));
        assert_eq!(clean_path(&single, CleanOptions::default()).0.cmds.len(), 1);
        let empty = Path::new();
        assert!(clean_path(&empty, CleanOptions::default()).0.cmds.is_empty());
        // every point the same
        let same = Path::new()
            .move_to(Vec2::ONE)
            .line_to(Vec2::ONE)
            .line_to(Vec2::ONE)
            .close();
        let _ = clean_path(&same, CleanOptions::default());
        // absurd options are clamped
        let rough = rough_circle(50, 1.0);
        let _ = clean_path(
            &rough,
            CleanOptions {
                tolerance: -5.0,
                corner_degrees: 1000.0,
            },
        );
        let _ = clean_path(
            &rough,
            CleanOptions {
                tolerance: f32::NAN,
                corner_degrees: f32::NAN,
            },
        );
    }

    #[test]
    fn widths_follow_their_place_along_the_stroke() {
        // a straight stroke of 11 points that thickens from 1 to 11
        let mut p = Path::new().move_to(Vec2::new(0.0, 0.0));
        for i in 1..=10 {
            p = p.line_to(Vec2::new(i as f32 * 10.0, 0.0));
        }
        let widths: Vec<f32> = (0..=10).map(|i| 1.0 + i as f32).collect();
        let (clean, _) = clean_path(&p, CleanOptions::default());
        assert!(clean.on_curve_count() < 11);
        let new = resample_widths(&p, &widths, &clean).unwrap();
        assert_eq!(new.len(), clean.on_curve_count());
        for (w, at) in new.iter().zip(clean.on_curve_points()) {
            assert!((w - (1.0 + at.x / 10.0)).abs() < 0.05, "{w} at {at:?}");
        }
        // widths that do not fit the path are refused, as is more than one subpath
        assert!(resample_widths(&p, &widths[1..], &clean).is_none());
        let two = p.clone().move_to(Vec2::ONE).line_to(Vec2::new(5.0, 5.0));
        assert!(resample_widths(&two, &widths, &clean).is_none());
    }

    #[test]
    fn an_open_stroke_keeps_its_ends() {
        let mut p = Path::new().move_to(Vec2::new(0.0, 0.0));
        for i in 1..=100 {
            let x = i as f32 * 3.0;
            p = p.line_to(Vec2::new(x, 40.0 * (x / 60.0).sin() + wobble(i) * 0.4));
        }
        let (clean, report) = clean_path(&p, CleanOptions::default());
        assert!(report.after < 20);
        let first = clean.points().next().unwrap();
        let last = clean.points().last().unwrap();
        assert!(first.distance(Vec2::ZERO) < 0.01);
        assert!(last.distance(Vec2::new(300.0, 40.0 * 5.0f32.sin() + wobble(100) * 0.4)) < 0.01);
        assert!(!matches!(clean.cmds.last(), Some(PathCmd::Close)));
    }
}
