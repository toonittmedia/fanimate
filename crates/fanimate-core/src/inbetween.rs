//! Inbetween assist: a drawing between two drawings, worked out by
//! geometry alone.
//!
//! This is a classical method, not a model. Shapes of the two drawings are
//! paired by position and size, each pair's outlines are resampled to the
//! same number of points (a closed outline is rotated and, if need be,
//! reversed to the alignment that moves its points least), and the points
//! are blended along **arcs**: the best similarity transform (turn, scale
//! and move) from one drawing to the other is found, applied in part, and
//! only what the transform does not explain is blended in a straight line.
//! A forearm that swings 90 degrees therefore passes through 45 degrees at
//! the half, at its own length, where blending the points straight would
//! shrink it to 70 percent in the middle.
//!
//! The result is a suggestion: ordinary shapes the artist edits, never
//! anything hidden. A shape with no partner is held as it was in the first
//! drawing and counted, so the caller can say so.

use glam::Vec2;

use crate::cleanup::{CleanOptions, clean_path};
use crate::path::{Path, PathCmd, Shape};

/// The most subpaths one shape may have to be blended.
const MAX_SUBPATHS: usize = 16;
/// Points per blended outline are at least this many and at most that many.
const MIN_SAMPLES: usize = 16;
const MAX_SAMPLES: usize = 300;

/// How the blended outline is smoothed back into curves, in document units.
const SMOOTHING: f32 = 0.3;

/// What blending two drawings did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Shapes of the first drawing that found a partner and were blended.
    pub blended: usize,
    /// Shapes of the first drawing with no partner, held as they were.
    pub held: usize,
    /// Shapes of the second drawing that no shape of the first matched.
    pub left_out: usize,
}

fn rotate(v: Vec2, angle: f32, scale: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c) * scale
}

fn centroid(points: &[Vec2]) -> Vec2 {
    points.iter().copied().sum::<Vec2>() / points.len().max(1) as f32
}

/// `n` points evenly spaced along the polyline by length. A closed polyline
/// does not repeat its first point.
fn resample(points: &[Vec2], closed: bool, n: usize) -> Vec<Vec2> {
    let mut pts = points.to_vec();
    if closed && let Some(first) = pts.first().copied() {
        pts.push(first);
    }
    let mut cumulative = vec![0.0f32];
    for w in pts.windows(2) {
        let last = cumulative[cumulative.len() - 1];
        cumulative.push(last + w[0].distance(w[1]));
    }
    let total = cumulative[cumulative.len() - 1];
    if total <= f32::EPSILON || pts.len() < 2 {
        return vec![pts.first().copied().unwrap_or(Vec2::ZERO); n];
    }
    let steps = if closed { n } else { n - 1 } as f32;
    let mut out = Vec::with_capacity(n);
    let mut seg = 0;
    for k in 0..n {
        let s = total * k as f32 / steps;
        while seg + 2 < cumulative.len() && cumulative[seg + 1] < s {
            seg += 1;
        }
        let (c0, c1) = (cumulative[seg], cumulative[seg + 1]);
        let t = if c1 > c0 {
            ((s - c0) / (c1 - c0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(pts[seg].lerp(pts[seg + 1], t));
    }
    out
}

/// `b` rotated (and reversed when that helps) so that its points sit
/// nearest to the matching points of `a`.
fn align_ring(a: &[Vec2], b: &[Vec2]) -> Vec<Vec2> {
    let n = a.len();
    let (mut best, mut best_cost) = (b.to_vec(), f32::MAX);
    for reversed in [false, true] {
        for shift in 0..n {
            let mut cost = 0.0;
            for (i, p) in a.iter().enumerate() {
                let j = if reversed {
                    (n + shift - i) % n
                } else {
                    (i + shift) % n
                };
                cost += p.distance_squared(b[j]);
                if cost >= best_cost {
                    break;
                }
            }
            if cost < best_cost {
                best_cost = cost;
                best = (0..n)
                    .map(|i| {
                        b[if reversed {
                            (n + shift - i) % n
                        } else {
                            (i + shift) % n
                        }]
                    })
                    .collect();
            }
        }
    }
    best
}

/// The points between `a` and `b` at `t` (0 is `a`, 1 is `b`), blended along arcs.
fn blend_points(a: &[Vec2], b: &[Vec2], t: f32) -> Vec<Vec2> {
    let (ca, cb) = (centroid(a), centroid(b));
    // best similarity a -> b about the centroids, as the complex number z = s e^(i theta)
    let (mut re, mut im, mut norm) = (0.0f64, 0.0f64, 0.0f64);
    for (p, q) in a.iter().zip(b) {
        let (p, q) = ((*p - ca).as_dvec2(), (*q - cb).as_dvec2());
        re += p.x * q.x + p.y * q.y;
        im += p.x * q.y - p.y * q.x;
        norm += p.x * p.x + p.y * p.y;
    }
    let lerp = |p: Vec2, q: Vec2| p.lerp(q, t);
    if norm < 1.0e-9 || !(re.is_finite() && im.is_finite()) {
        return a.iter().zip(b).map(|(p, q)| lerp(*p, *q)).collect();
    }
    let (zr, zi) = (re / norm, im / norm);
    let scale = (zr * zr + zi * zi).sqrt();
    if scale < 1.0e-6 {
        return a.iter().zip(b).map(|(p, q)| lerp(*p, *q)).collect();
    }
    let angle = zi.atan2(zr) as f32;
    let scale = scale as f32;
    let (zr, zi) = (zr as f32, zi as f32);
    let denom = zr * zr + zi * zi;
    let centre = ca.lerp(cb, t);
    let (turn, grow) = (angle * t, scale.powf(t));
    a.iter()
        .zip(b)
        .map(|(p, q)| {
            let (p, q) = (*p - ca, *q - cb);
            // what the transform leaves over: the target taken back through it, minus the start
            let back = Vec2::new((q.x * zr + q.y * zi) / denom, (q.y * zr - q.x * zi) / denom);
            let residual = back - p;
            centre + rotate(p + residual * t, turn, grow)
        })
        .collect()
}

fn outline(points: &[Vec2], closed: bool) -> Path {
    let mut path = Path::new();
    for (i, p) in points.iter().enumerate() {
        path.cmds.push(if i == 0 {
            PathCmd::MoveTo(*p)
        } else {
            PathCmd::LineTo(*p)
        });
    }
    if closed {
        path.cmds.push(PathCmd::Close);
    }
    path
}

/// The path between `a` and `b` at `t`. `None` when they cannot be blended:
/// they differ in how many subpaths they have, or in which are closed,
/// have too many, or hold points that are not numbers.
pub fn blend_paths(a: &Path, b: &Path, t: f32) -> Option<Path> {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.5 };
    let (pa, pb) = (a.polylines(0.25), b.polylines(0.25));
    if pa.is_empty() || pa.len() != pb.len() || pa.len() > MAX_SUBPATHS {
        return None;
    }
    let mut out = Path::new();
    for ((la, closed_a), (lb, closed_b)) in pa.iter().zip(&pb) {
        if closed_a != closed_b || la.len() < 2 || lb.len() < 2 {
            return None;
        }
        if !la.iter().chain(lb).all(|p| p.is_finite()) {
            return None;
        }
        let n = la.len().max(lb.len()).clamp(MIN_SAMPLES, MAX_SAMPLES);
        let sa = resample(la, *closed_a, n);
        let mut sb = resample(lb, *closed_b, n);
        if *closed_a {
            sb = align_ring(&sa, &sb);
        }
        let blended = blend_points(&sa, &sb, t);
        let rough = outline(&blended, *closed_a);
        let smooth = clean_path(
            &rough,
            CleanOptions {
                tolerance: SMOOTHING,
                ..CleanOptions::default()
            },
        )
        .0;
        out.cmds.extend(smooth.cmds);
    }
    Some(out)
}

fn bounds(path: &Path) -> Option<(Vec2, Vec2)> {
    let mut it = path.polylines(0.5).into_iter().flat_map(|(p, _)| p);
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}

/// Which shape of `b` each shape of `a` goes with: the nearest by centre
/// and size, each used once, closest pairs first. A pair must have the same
/// number of subpaths, and a shape far from every other one (further than
/// the size of the drawing) stays unpaired.
pub fn pair_shapes(a: &[Shape], b: &[Shape]) -> Vec<Option<usize>> {
    let boxes = |shapes: &[Shape]| -> Vec<Option<(Vec2, f32, usize)>> {
        shapes
            .iter()
            .map(|s| {
                let (lo, hi) = bounds(&s.path)?;
                Some(((lo + hi) / 2.0, (hi - lo).length(), s.path.polylines(0.5).len()))
            })
            .collect()
    };
    let (ba, bb) = (boxes(a), boxes(b));
    let reach = ba
        .iter()
        .chain(&bb)
        .flatten()
        .map(|(_, size, _)| *size)
        .fold(0.0f32, f32::max)
        * 1.5;
    let mut costs: Vec<(f32, usize, usize)> = Vec::new();
    for (i, x) in ba.iter().enumerate() {
        for (j, y) in bb.iter().enumerate() {
            if let (Some((ca, sa, na)), Some((cb, sb, nb))) = (x, y)
                && na == nb
            {
                let cost = ca.distance(*cb) + (sa - sb).abs();
                if cost <= reach {
                    costs.push((cost, i, j));
                }
            }
        }
    }
    costs.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut pairs = vec![None; a.len()];
    let mut taken = vec![false; b.len()];
    for (_, i, j) in costs {
        if pairs[i].is_none() && !taken[j] {
            pairs[i] = Some(j);
            taken[j] = true;
        }
    }
    pairs
}

/// The shapes between drawing `a` and drawing `b` at `t`: every shape of
/// `a` blended with its partner in `b`, or held when it has none (or cannot
/// be blended with it). Style is the first drawing's, with a stroke width
/// that moves between the two.
pub fn blend_shapes(a: &[Shape], b: &[Shape], t: f32) -> (Vec<Shape>, Stats) {
    let pairs = pair_shapes(a, b);
    let mut stats = Stats::default();
    let mut out = Vec::with_capacity(a.len());
    let mut used = vec![false; b.len()];
    for (shape, partner) in a.iter().zip(pairs) {
        let blended = partner.and_then(|j| {
            let other = &b[j];
            let path = blend_paths(&shape.path, &other.path, t)?;
            used[j] = true;
            let mut made = shape.clone();
            made.path = path;
            made.widths = None;
            made.path_keys.clear();
            made.hidden_edge_flags.clear();
            made.rect = None;
            made.skin = None;
            if let (Some(x), Some(y)) = (shape.style.stroke, other.style.stroke) {
                let mut stroke = x;
                let t = t.clamp(0.0, 1.0);
                stroke.width = x.width + (y.width - x.width) * t;
                stroke.color = crate::Color::lerp(x.color, y.color, t);
                made.style.stroke = Some(stroke);
            }
            Some(made)
        });
        match blended {
            Some(shape) => {
                stats.blended += 1;
                out.push(shape);
            }
            None => {
                stats.held += 1;
                out.push(shape.clone());
            }
        }
    }
    stats.left_out = used.iter().filter(|u| !**u).count();
    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::Style;

    fn line(a: Vec2, b: Vec2) -> Path {
        Path::line(a, b)
    }

    fn points_of(path: &Path) -> Vec<Vec2> {
        path.polylines(0.1).into_iter().flat_map(|(p, _)| p).collect()
    }

    fn style() -> Style {
        Style::fill(crate::Color::BLACK)
    }

    #[test]
    fn a_swinging_line_keeps_its_length_on_an_arc() {
        // a 100 long line turning 90 degrees about its start
        let a = line(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0));
        let b = line(Vec2::new(0.0, 0.0), Vec2::new(0.0, 100.0));
        let half = blend_paths(&a, &b, 0.5).unwrap();
        let p = points_of(&half);
        let (first, last) = (p[0], p[p.len() - 1]);
        let length = first.distance(last);
        assert!(length > 98.0, "shrank to {length}");
        // straight blending would give 70.7: the arc keeps it at the same length
        let direction = (last - first).normalize();
        assert!(
            (direction.y.atan2(direction.x).to_degrees() - 45.0).abs() < 2.0,
            "{direction:?}"
        );
    }

    #[test]
    fn ends_are_the_drawings_and_a_slide_is_halfway() {
        let a = Path::rect(Vec2::new(0.0, 0.0), Vec2::new(40.0, 20.0), 0.0);
        let b = Path::rect(Vec2::new(100.0, 50.0), Vec2::new(40.0, 20.0), 0.0);
        let at0 = blend_paths(&a, &b, 0.0).unwrap();
        let mid = blend_paths(&a, &b, 0.5).unwrap();
        let at1 = blend_paths(&a, &b, 1.0).unwrap();
        let centre = |p: &Path| centroid(&points_of(p));
        assert!(centre(&at0).distance(Vec2::new(20.0, 10.0)) < 1.0);
        assert!(
            centre(&mid).distance(Vec2::new(70.0, 35.0)) < 1.0,
            "{:?}",
            centre(&mid)
        );
        assert!(centre(&at1).distance(Vec2::new(120.0, 60.0)) < 1.0);
    }

    #[test]
    fn a_turning_square_passes_through_the_turn_with_points_in_another_order() {
        // the same square, drawn starting from another corner and turned 90 degrees
        let a = Path::rect(Vec2::new(-10.0, -10.0), Vec2::new(20.0, 20.0), 0.0);
        let b = Path::new()
            .move_to(Vec2::new(10.0, -10.0))
            .line_to(Vec2::new(10.0, 10.0))
            .line_to(Vec2::new(-10.0, 10.0))
            .line_to(Vec2::new(-10.0, -10.0))
            .close();
        let mid = blend_paths(&a, &b, 0.5).unwrap();
        // a square turned 45 degrees about its centre has its corners 14.1 from the centre
        let far = points_of(&mid).iter().map(|p| p.length()).fold(0.0, f32::max);
        assert!((far - 14.14).abs() < 1.0, "corners at {far}");
    }

    #[test]
    fn paths_that_cannot_be_blended_say_so() {
        let one = line(Vec2::ZERO, Vec2::new(10.0, 0.0));
        let two = one
            .clone()
            .move_to(Vec2::new(0.0, 5.0))
            .line_to(Vec2::new(10.0, 5.0));
        assert!(blend_paths(&one, &two, 0.5).is_none());
        let open = line(Vec2::ZERO, Vec2::new(10.0, 0.0));
        let closed = Path::rect(Vec2::ZERO, Vec2::new(10.0, 10.0), 0.0);
        assert!(blend_paths(&open, &closed, 0.5).is_none());
        assert!(blend_paths(&Path::new(), &Path::new(), 0.5).is_none());
        let nan = line(Vec2::new(f32::NAN, 0.0), Vec2::new(1.0, 1.0));
        assert!(blend_paths(&nan, &open, 0.5).is_none());
        // a nonsense t is held to the range
        assert!(blend_paths(&open, &open, f32::NAN).is_some());
    }

    #[test]
    fn shapes_pair_by_nearness_each_used_once() {
        let at = |x: f32, y: f32| {
            Shape::new(
                "s",
                Path::rect(Vec2::new(x, y), Vec2::new(20.0, 20.0), 0.0),
                style(),
            )
        };
        let a = vec![at(0.0, 0.0), at(200.0, 0.0), at(400.0, 0.0)];
        // the second drawing lists them in another order and has one fewer
        let b = vec![at(205.0, 3.0), at(3.0, 2.0)];
        let pairs = pair_shapes(&a, &b);
        assert_eq!(pairs, vec![Some(1), Some(0), None]);
        let (shapes, stats) = blend_shapes(&a, &b, 0.5);
        assert_eq!(shapes.len(), 3);
        assert_eq!(
            stats,
            Stats {
                blended: 2,
                held: 1,
                left_out: 0
            }
        );
        // a shape of the second drawing nothing matched is counted
        let (_, stats) = blend_shapes(&b, &a, 0.5);
        assert_eq!(stats.left_out, 1);
    }

    #[test]
    fn a_stroke_width_moves_between_the_drawings() {
        let thin = Shape::new(
            "a",
            line(Vec2::ZERO, Vec2::new(50.0, 0.0)),
            Style::default().with_stroke(crate::Color::BLACK, 2.0),
        );
        let thick = Shape::new(
            "b",
            line(Vec2::ZERO, Vec2::new(50.0, 10.0)),
            Style::default().with_stroke(crate::Color::BLACK, 6.0),
        );
        let (shapes, _) = blend_shapes(&[thin], &[thick], 0.5);
        assert!((shapes[0].style.stroke.unwrap().width - 4.0).abs() < 1.0e-4);
    }
}
