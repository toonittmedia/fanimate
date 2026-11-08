//! Boolean shape operations: union, subtract and intersect, and Offset Path,
//! which grows or shrinks a shape's outline by a distance.
//!
//! Paths are flattened to polygons, combined with [`i_overlay`] (filled by
//! the non-zero rule, as the renderer fills), then turned back into a path
//! with few points: straight runs stay straight, sharp corners stay sharp,
//! and curved runs are fitted with smooth cubic curves, so the result can
//! be edited point by point like any other shape.

use glam::Vec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::stroke::offset::StrokeOffset;
use i_overlay::mesh::float::style::{LineCap, LineJoin, OutlineStyle, StrokeStyle};

use crate::path::Path;

/// How far flattening curves may stray from them, in document units.
const FLATTEN_TOLERANCE: f32 = 0.05;

/// A turn sharper than this (in radians) is kept as a corner.
const CORNER_ANGLE: f32 = 0.6;

type Contours = Vec<Vec<[f64; 2]>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoolOp {
    /// Everything covered by any of the shapes.
    Union,
    /// The first shape without the area of the others.
    Subtract,
    /// Only what all the shapes cover.
    Intersect,
}

/// Combine `paths` by `op`. Open subpaths are closed, as when filling.
/// `tolerance` is how far the result may stray from the exact outline, in
/// document units. Returns an empty path when nothing is left.
pub fn combine(paths: &[&Path], op: BoolOp, tolerance: f32) -> Path {
    let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.25
    };
    let mut iter = paths.iter().map(|p| contours(p));
    let Some(first) = iter.next() else {
        return Path::new();
    };
    // the first operand is normalised on its own, so later steps see a
    // clean outline with holes wound against their outer contour
    let empty: Contours = Vec::new();
    let mut acc = flat(first.overlay(&empty, OverlayRule::Subject, FillRule::NonZero));
    for next in iter {
        let rule = match op {
            BoolOp::Union => OverlayRule::Union,
            BoolOp::Subtract => OverlayRule::Difference,
            BoolOp::Intersect => OverlayRule::Intersect,
        };
        acc = flat(acc.overlay(&next, rule, FillRule::NonZero));
    }
    to_path(&acc, tolerance)
}

/// The area `path` covers when filled by the even-odd rule (as SVG files
/// may ask), as a path the renderer's non-zero rule fills the same way.
pub fn even_odd(path: &Path, tolerance: f32) -> Path {
    let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.25
    };
    let empty: Contours = Vec::new();
    let filled = contours(path).overlay(&empty, OverlayRule::Subject, FillRule::EvenOdd);
    to_path(&flat(filled), tolerance)
}

/// The largest distance [`offset`] takes, in document units; a larger one is
/// brought down to it.
pub const MAX_OFFSET: f32 = 10_000.0;

/// Points further from the origin than this are left out of an offset, as
/// the renderer leaves them out of a drawing.
const OFFSET_LIMIT: f32 = 1.0e7;

/// The most a round corner of an offset turns between two of its points, in
/// radians. Kept well under [`CORNER_ANGLE`], so the fitting turns the arc
/// back into smooth curves instead of keeping each step as a corner.
const ROUND_STEP: f64 = 0.3;

/// Offset Path: the outline `distance` outside `path`, or inside it when
/// `distance` is negative, with round corners where it grows.
///
/// Closed subpaths count as the area they fill (by the non-zero rule, as
/// the renderer fills), so overlaps and loops drawn over themselves are
/// resolved first, and parts that shrink to nothing disappear instead of
/// turning inside out. An open subpath has no inside: a positive distance
/// makes the closed band that far around it, with round ends, and a negative
/// one leaves nothing of it. `tolerance` is how far the result may stray
/// from the exact outline. Returns an empty path when nothing is left, or
/// when the distance is zero or not a number; points that are not finite
/// are left out.
pub fn offset(path: &Path, distance: f32, tolerance: f32) -> Path {
    let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        0.25
    };
    if !distance.is_finite() || distance == 0.0 {
        return Path::new();
    }
    let distance = distance.clamp(-MAX_OFFSET, MAX_OFFSET);
    let (mut closed, mut open): (Contours, Contours) = (Vec::new(), Vec::new());
    for (pts, is_closed) in path.polylines(FLATTEN_TOLERANCE) {
        if !pts
            .iter()
            .all(|p| p.is_finite() && p.abs().max_element() < OFFSET_LIMIT)
        {
            continue;
        }
        let contour: Vec<[f64; 2]> = pts.iter().map(|p| [p.x as f64, p.y as f64]).collect();
        if is_closed && pts.len() >= 3 {
            closed.push(contour);
        } else if pts.len() >= 2 {
            // a closed subpath of two points covers no area: a line
            open.push(contour);
        }
    }
    // the angle a round corner turns per step for the curve to stay within
    // the tolerance (a chord of a circle of radius `distance`)
    let fits = 2.0 * (1.0 - (tolerance / distance.abs()).min(1.0) as f64).acos();
    let step = fits.min(ROUND_STEP);
    let empty: Contours = Vec::new();
    let mut pieces: Contours = Vec::new();
    if !closed.is_empty() {
        // resolve overlaps and self-intersections into clean outer contours
        // and holes, which the outline offsets the right way round
        let shapes = closed.overlay(&empty, OverlayRule::Subject, FillRule::NonZero);
        let style = OutlineStyle::new(distance as f64).line_join(LineJoin::Round(step));
        pieces = flat(shapes.outline(&style));
    }
    if distance > 0.0 && !open.is_empty() {
        let style = StrokeStyle::<[f64; 2]>::new(2.0 * distance as f64)
            .line_join(LineJoin::Round(step))
            .start_cap(LineCap::Round(step))
            .end_cap(LineCap::Round(step));
        let band = flat(open.stroke(style, false));
        pieces = flat(pieces.overlay(&band, OverlayRule::Union, FillRule::NonZero));
    }
    to_path(&pieces, tolerance)
}

/// The path's subpaths as closed polygons, dropping any too small to fill.
fn contours(path: &Path) -> Contours {
    path.polylines(FLATTEN_TOLERANCE)
        .into_iter()
        .filter(|(pts, _)| pts.len() >= 3)
        .filter(|(pts, _)| pts.iter().all(|p| p.is_finite()))
        .map(|(pts, _)| pts.iter().map(|p| [p.x as f64, p.y as f64]).collect())
        .collect()
}

fn flat(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Contours {
    shapes.into_iter().flatten().collect()
}

fn to_path(contours: &Contours, tolerance: f32) -> Path {
    let mut path = Path::new();
    for contour in contours {
        let pts: Vec<Vec2> = contour
            .iter()
            .map(|p| Vec2::new(p[0] as f32, p[1] as f32))
            .collect();
        // curves fitted through few points follow the outline closely, so
        // start coarse and keep more points only where that strays
        let mut coarse = tolerance * 16.0;
        let fitted = loop {
            let kept = simplify_closed(&pts, coarse);
            let fitted = fit_closed(Path::new(), &kept);
            if kept.len() < 3 || coarse <= tolerance || deviation(&pts, &fitted) <= tolerance {
                break (kept.len() >= 3).then_some(fitted);
            }
            coarse /= 2.0;
        };
        if let Some(fitted) = fitted {
            path.cmds.extend(fitted.cmds);
        }
    }
    path
}

/// How far the furthest of `pts` is from `path`'s outline.
fn deviation(pts: &[Vec2], path: &Path) -> f32 {
    let lines = path.polylines(FLATTEN_TOLERANCE);
    pts.iter()
        .map(|&p| {
            lines
                .iter()
                .flat_map(|(q, _)| (0..q.len()).map(move |i| (q[i], q[(i + 1) % q.len()])))
                .map(|(a, b)| segment_distance(p, a, b))
                .fold(f32::INFINITY, f32::min)
        })
        .fold(0.0, f32::max)
}

/// The points of a closed polygon worth keeping: every corner, and enough
/// of the rest that the outline stays within `tolerance` of the original.
fn simplify_closed(pts: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let corner = |i: usize| {
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        turn(a, b, c) > CORNER_ANGLE
    };
    let mut anchors: Vec<usize> = (0..n).filter(|&i| corner(i)).collect();
    if anchors.is_empty() {
        // a smooth loop: start from two opposite points
        anchors = vec![0, n / 2];
    } else if anchors.len() == 1 {
        anchors.push((anchors[0] + n / 2) % n);
        anchors.sort_unstable();
    }
    let mut keep = vec![false; n];
    for (k, &a) in anchors.iter().enumerate() {
        let b = anchors[(k + 1) % anchors.len()];
        keep[a] = true;
        // the run from one anchor to the next, wrapping round
        let run: Vec<usize> = std::iter::successors(Some(a), |&i| Some((i + 1) % n))
            .take(((b + n - a) % n).max(1) + 1)
            .collect();
        douglas_peucker(pts, &run, tolerance, &mut keep);
    }
    (0..n).filter(|&i| keep[i]).map(|i| pts[i]).collect()
}

fn douglas_peucker(pts: &[Vec2], run: &[usize], tolerance: f32, keep: &mut [bool]) {
    if run.len() < 3 {
        return;
    }
    let (a, b) = (pts[run[0]], pts[run[run.len() - 1]]);
    let (far, d) = run[1..run.len() - 1]
        .iter()
        .enumerate()
        .map(|(k, &i)| (k + 1, segment_distance(pts[i], a, b)))
        .fold((0, 0.0), |best, x| if x.1 > best.1 { x } else { best });
    if d > tolerance {
        keep[run[far]] = true;
        douglas_peucker(pts, &run[..=far], tolerance, keep);
        douglas_peucker(pts, &run[far..], tolerance, keep);
    }
}

/// A closed subpath through `pts`: straight where the outline turns
/// sharply or runs straight, smooth cubic curves elsewhere.
fn fit_closed(mut path: Path, pts: &[Vec2]) -> Path {
    let n = pts.len();
    let corner: Vec<bool> = (0..n)
        .map(|i| turn(pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]) > CORNER_ANGLE)
        .collect();
    // the tangent at each smooth point (Catmull-Rom); none at corners
    let tangent = |i: usize| -> Vec2 {
        if corner[i] {
            Vec2::ZERO
        } else {
            (pts[(i + 1) % n] - pts[(i + n - 1) % n]) / 6.0
        }
    };
    path = path.move_to(pts[0]);
    for i in 0..n {
        let j = (i + 1) % n;
        let (t0, t1) = (tangent(i), tangent(j));
        if t0 == Vec2::ZERO && t1 == Vec2::ZERO {
            if j != 0 {
                path = path.line_to(pts[j]);
            }
        } else {
            path = path.cubic_to(pts[i] + t0, pts[j] - t1, pts[j]);
        }
    }
    path.close()
}

/// The angle the outline turns through at `b`, from 0 (straight on) to π.
fn turn(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let (u, v) = (b - a, c - b);
    if u.length_squared() == 0.0 || v.length_squared() == 0.0 {
        return 0.0;
    }
    u.angle_to(v).abs()
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::{Shape, Style};
    use crate::{Color, PathCmd};

    fn area(path: &Path) -> f32 {
        // the shoelace formula over the flattened outline, non-zero filled
        path.polylines(0.01)
            .iter()
            .map(|(pts, _)| {
                (0..pts.len())
                    .map(|i| pts[i].perp_dot(pts[(i + 1) % pts.len()]))
                    .sum::<f32>()
                    / 2.0
            })
            .sum::<f32>()
            .abs()
    }

    fn square(x: f32, y: f32, s: f32) -> Path {
        Path::rect(Vec2::new(x, y), Vec2::splat(s), 0.0)
    }

    #[test]
    fn union_of_overlapping_squares() {
        let (a, b) = (square(0.0, 0.0, 10.0), square(5.0, 5.0, 10.0));
        let u = combine(&[&a, &b], BoolOp::Union, 0.1);
        assert!((area(&u) - 175.0).abs() < 0.5, "{}", area(&u));
        // straight edges and sharp corners: lines only, eight corners
        assert!(u.cmds.iter().all(|c| !matches!(c, PathCmd::CubicTo(..))));
        assert_eq!(u.on_curve_count(), 8);
    }

    #[test]
    fn subtract_and_intersect() {
        let (a, b) = (square(0.0, 0.0, 10.0), square(5.0, 5.0, 10.0));
        let s = combine(&[&a, &b], BoolOp::Subtract, 0.1);
        assert!((area(&s) - 75.0).abs() < 0.5);
        let i = combine(&[&a, &b], BoolOp::Intersect, 0.1);
        assert!((area(&i) - 25.0).abs() < 0.5);
        let apart = square(50.0, 50.0, 5.0);
        assert!(combine(&[&a, &apart], BoolOp::Intersect, 0.1).cmds.is_empty());
    }

    #[test]
    fn a_hole_punched_through_stays_a_hole() {
        let ring = combine(
            &[&Path::circle(Vec2::ZERO, 20.0), &Path::circle(Vec2::ZERO, 10.0)],
            BoolOp::Subtract,
            0.1,
        );
        let shape = Shape::new("ring", ring.clone(), Style::fill(Color::BLACK));
        assert!(shape.contains(Vec2::new(15.0, 0.0), 0.0));
        assert!(!shape.contains(Vec2::ZERO, 0.0));
        let expected = std::f32::consts::PI * (400.0 - 100.0);
        assert!((area(&ring) - expected).abs() / expected < 0.01);
    }

    #[test]
    fn even_odd_overlaps_become_holes() {
        // two squares wound the same way: non-zero fills both, even-odd
        // leaves the inner one empty
        let mut both = square(0.0, 0.0, 30.0);
        both.cmds.extend(square(10.0, 10.0, 10.0).cmds);
        let ring = even_odd(&both, 0.1);
        let shape = Shape::new("ring", ring.clone(), Style::fill(Color::BLACK));
        assert!(shape.contains(Vec2::new(5.0, 5.0), 0.0));
        assert!(!shape.contains(Vec2::new(15.0, 15.0), 0.0));
        assert!((area(&ring) - 800.0).abs() < 0.5);
    }

    #[test]
    fn curves_come_back_as_few_smooth_points() {
        let a = Path::circle(Vec2::ZERO, 40.0);
        let b = Path::circle(Vec2::new(50.0, 0.0), 40.0);
        let u = combine(&[&a, &b], BoolOp::Union, 0.25);
        assert!(u.on_curve_count() < 40, "{} points", u.on_curve_count());
        assert!(u.cmds.iter().any(|c| matches!(c, PathCmd::CubicTo(..))));
        // still close to the true outline
        let r = 40.0_f32;
        for (pts, _) in u.polylines(0.05) {
            for p in pts {
                let d = (p.length() - r)
                    .abs()
                    .min((p.distance(Vec2::new(50.0, 0.0)) - r).abs());
                assert!(d < 1.0, "{p:?} is {d} off");
            }
        }
    }

    #[test]
    fn offset_grows_a_circle_into_a_bigger_circle() {
        let grown = offset(&Path::circle(Vec2::ZERO, 40.0), 10.0, 0.1);
        let expected = std::f32::consts::PI * 50.0 * 50.0;
        assert!(
            (area(&grown) - expected).abs() / expected < 0.01,
            "{}",
            area(&grown)
        );
        for (pts, closed) in grown.polylines(0.05) {
            assert!(closed);
            for p in pts {
                assert!((p.length() - 50.0).abs() < 0.5, "{p:?}");
            }
        }
        // fitted back into a few smooth points, not hundreds of corners
        assert!(grown.on_curve_count() < 40, "{}", grown.on_curve_count());
        let shrunk = offset(&Path::circle(Vec2::ZERO, 40.0), -15.0, 0.1);
        let expected = std::f32::consts::PI * 25.0 * 25.0;
        assert!((area(&shrunk) - expected).abs() / expected < 0.01);
    }

    #[test]
    fn offset_rounds_a_rectangle_outside_and_keeps_its_corners_inside() {
        let rect = Path::rect(Vec2::ZERO, Vec2::new(100.0, 60.0), 0.0);
        let grown = offset(&rect, 10.0, 0.1);
        let expected = 6000.0 + 2.0 * 10.0 * 160.0 + std::f32::consts::PI * 100.0;
        // the fitted curves of the corners may bulge by a fraction of the tolerance
        assert!(
            (area(&grown) - expected).abs() / expected < 0.005,
            "{}",
            area(&grown)
        );
        let shape = Shape::new("r", grown.clone(), Style::fill(Color::BLACK));
        assert!(shape.contains(Vec2::new(-9.0, 30.0), 0.0));
        // the corner is round: its far diagonal point is not covered
        assert!(!shape.contains(Vec2::new(-9.0, -9.0), 0.0));
        let shrunk = offset(&rect, -10.0, 0.1);
        assert!((area(&shrunk) - 80.0 * 40.0).abs() < 2.0, "{}", area(&shrunk));
        // shrinking past half the shorter side leaves nothing
        assert!(offset(&rect, -31.0, 0.1).cmds.is_empty());
    }

    #[test]
    fn offset_makes_a_band_round_an_open_line() {
        let line = Path::new().move_to(Vec2::ZERO).cubic_to(
            Vec2::new(30.0, -20.0),
            Vec2::new(70.0, 20.0),
            Vec2::new(100.0, 0.0),
        );
        let band = offset(&line, 5.0, 0.1);
        assert!(!band.cmds.is_empty());
        assert!(
            band.cmds.iter().any(|c| matches!(c, PathCmd::Close)),
            "a closed outline"
        );
        let shape = Shape::new("b", band.clone(), Style::fill(Color::BLACK));
        assert!(shape.contains(Vec2::new(-4.0, 0.0), 0.0), "round end");
        assert!(!shape.contains(Vec2::new(50.0, 20.0), 0.0));
        let straight = offset(&Path::line(Vec2::ZERO, Vec2::new(100.0, 0.0)), 5.0, 0.1);
        let expected = 100.0 * 10.0 + std::f32::consts::PI * 25.0;
        assert!(
            (area(&straight) - expected).abs() / expected < 0.01,
            "{}",
            area(&straight)
        );
        // a line has no inside to shrink
        assert!(offset(&line, -5.0, 0.1).cmds.is_empty());
    }

    #[test]
    fn offset_resolves_self_intersections_and_splits_what_shrinks_apart() {
        // a bow tie drawn as one crossing loop: two triangles
        let bow = Path::new()
            .move_to(Vec2::ZERO)
            .line_to(Vec2::new(100.0, 100.0))
            .line_to(Vec2::new(100.0, 0.0))
            .line_to(Vec2::new(0.0, 100.0))
            .close();
        let grown = offset(&bow, 4.0, 0.1);
        assert!(area(&grown) > 5000.0, "{}", area(&grown));
        let shrunk = offset(&bow, -4.0, 0.1);
        assert!(
            area(&shrunk) < 5000.0 && area(&shrunk) > 1000.0,
            "{}",
            area(&shrunk)
        );
        // two squares joined by a thin bar come apart when shrunk past it
        let dumbbell = combine(
            &[
                &square(0.0, 0.0, 40.0),
                &Path::rect(Vec2::new(40.0, 18.0), Vec2::new(40.0, 4.0), 0.0),
                &square(80.0, 0.0, 40.0),
            ],
            BoolOp::Union,
            0.1,
        );
        let apart = offset(&dumbbell, -5.0, 0.1);
        assert_eq!(apart.polylines(0.1).len(), 2);
        assert!((area(&apart) - 2.0 * 30.0 * 30.0).abs() < 5.0, "{}", area(&apart));
    }

    #[test]
    fn offset_moves_a_hole_the_other_way() {
        let ring = combine(
            &[&Path::circle(Vec2::ZERO, 40.0), &Path::circle(Vec2::ZERO, 20.0)],
            BoolOp::Subtract,
            0.1,
        );
        let grown = offset(&ring, 5.0, 0.1);
        let expected = std::f32::consts::PI * (45.0 * 45.0 - 15.0 * 15.0);
        assert!(
            (area(&grown) - expected).abs() / expected < 0.01,
            "{}",
            area(&grown)
        );
        let shape = Shape::new("ring", grown, Style::fill(Color::BLACK));
        assert!(!shape.contains(Vec2::ZERO, 0.0));
        assert!(shape.contains(Vec2::new(17.0, 0.0), 0.0));
    }

    #[test]
    fn offset_of_nothing_or_nonsense_is_nothing_and_never_panics() {
        let circle = Path::circle(Vec2::ZERO, 10.0);
        assert!(offset(&Path::new(), 5.0, 0.1).cmds.is_empty());
        for d in [0.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(offset(&circle, d, 0.1).cmds.is_empty(), "{d}");
        }
        // a silly tolerance falls back to the default; a huge distance is capped
        assert!(!offset(&circle, 5.0, f32::NAN).cmds.is_empty());
        let huge = offset(&circle, 1.0e30, 0.1);
        assert!(huge.points().all(|p| p.length() < MAX_OFFSET + 20.0));
        assert!(offset(&circle, -1.0e30, 0.1).cmds.is_empty());
        // points that are not finite or far out are left out, the rest offset
        let mut bad = Path::new()
            .move_to(Vec2::new(f32::NAN, 0.0))
            .line_to(Vec2::X)
            .line_to(Vec2::Y)
            .close();
        bad.cmds.extend(square(0.0, 0.0, 10.0).cmds);
        bad.cmds.extend(square(1.0e9, 0.0, 10.0).cmds);
        let a = area(&offset(&bad, 1.0, 0.1));
        // (the 10 by 10 square grown by 1, with its small round corners fitted)
        assert!((a - (140.0 + std::f32::consts::PI)).abs() < 3.0, "{a}");
        // a single point, a zero-length line, a two-point closed loop
        let dot = Path::new().move_to(Vec2::ZERO);
        assert!(offset(&dot, 5.0, 0.1).cmds.is_empty());
        let _ = offset(&Path::line(Vec2::ONE, Vec2::ONE), 5.0, 0.1);
        let flat_loop = Path::new().move_to(Vec2::ZERO).line_to(Vec2::X * 10.0).close();
        assert!(!offset(&flat_loop, 2.0, 0.1).cmds.is_empty());
        // tiny distances and tiny shapes
        let _ = offset(&circle, 1.0e-6, 0.1);
        let _ = offset(&Path::circle(Vec2::ZERO, 1.0e-4), 3.0, 0.1);
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(combine(&[], BoolOp::Union, 0.1).cmds.is_empty());
        let line = Path::new().move_to(Vec2::ZERO).line_to(Vec2::X);
        assert!(combine(&[&line], BoolOp::Union, 0.1).cmds.is_empty());
    }
}
