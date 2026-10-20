//! Stroke exposure: drawing only part of a path's length.
//!
//! A shape's `trim` is a start and an end, each 0 to 1 of the length of every
//! subpath. [`Path::trimmed`] cuts the path (and the width profile that goes
//! with it) down to that part, for the stroke only: the fill is not trimmed.
//! [`Path::exposure_at`] goes the other way, from a point near the path to how
//! far along it is, for dragging the ends.

use glam::Vec2;

use crate::path::{Path, PathCmd};

/// Straight pieces a cubic is measured with. Enough that the length is right
/// to a small fraction of a percent for the curves a drawing has.
const SAMPLES: usize = 24;

/// One side of a subpath and the width profile's value at each end.
struct Seg {
    from: Vec2,
    /// The control points, or none for a straight line.
    ctrl: Option<(Vec2, Vec2)>,
    to: Vec2,
    w_from: f32,
    w_to: f32,
    /// Length so far at each sample (a line has the two ends).
    cum: Vec<f32>,
}

impl Seg {
    fn new(from: Vec2, ctrl: Option<(Vec2, Vec2)>, to: Vec2, w_from: f32, w_to: f32) -> Self {
        let steps = if ctrl.is_some() { SAMPLES } else { 1 };
        let mut seg = Self {
            from,
            ctrl,
            to,
            w_from,
            w_to,
            cum: Vec::with_capacity(steps + 1),
        };
        let mut length = 0.0;
        let mut last = from;
        seg.cum.push(0.0);
        for i in 1..=steps {
            let p = seg.at(i as f32 / steps as f32);
            length += last.distance(p);
            seg.cum.push(length);
            last = p;
        }
        seg
    }

    fn len(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    fn at(&self, t: f32) -> Vec2 {
        match self.ctrl {
            None => self.from.lerp(self.to, t),
            Some((c1, c2)) => {
                let u = 1.0 - t;
                self.from * (u * u * u)
                    + c1 * (3.0 * u * u * t)
                    + c2 * (3.0 * u * t * t)
                    + self.to * (t * t * t)
            }
        }
    }

    /// The curve parameter at `d` along the segment.
    fn param_at(&self, d: f32) -> f32 {
        let steps = self.cum.len() - 1;
        let d = d.clamp(0.0, self.len());
        let i = self.cum.partition_point(|c| *c < d).clamp(1, steps);
        let (a, b) = (self.cum[i - 1], self.cum[i]);
        let within = if b > a { (d - a) / (b - a) } else { 0.0 };
        ((i - 1) as f32 + within) / steps as f32
    }

    /// The part between parameters `t0` and `t1`, as the command that draws
    /// it from the point at `t0`.
    fn piece(&self, t0: f32, t1: f32) -> PathCmd {
        match self.ctrl {
            None => PathCmd::LineTo(self.at(t1)),
            Some((c1, c2)) => {
                // cut the curve at t1, then cut the start of what is left at
                // t0 (which is t0 / t1 of the way along it)
                let left = split(split_point(&[self.from, c1, c2, self.to], t1), Side::Left);
                let u = if t1 > 0.0 { (t0 / t1).clamp(0.0, 1.0) } else { 0.0 };
                let right = split(split_point(&left, u), Side::Right);
                PathCmd::CubicTo(right[1], right[2], right[3])
            }
        }
    }

    fn width_at(&self, t: f32) -> f32 {
        self.w_from + (self.w_to - self.w_from) * t
    }
}

/// De Casteljau at `t`: the points of each round, which cut a cubic in two.
fn split_point(p: &[Vec2; 4], t: f32) -> ([Vec2; 4], [Vec2; 4]) {
    let (ab, bc, cd) = (p[0].lerp(p[1], t), p[1].lerp(p[2], t), p[2].lerp(p[3], t));
    let (abc, bcd) = (ab.lerp(bc, t), bc.lerp(cd, t));
    let mid = abc.lerp(bcd, t);
    ([p[0], ab, abc, mid], [mid, bcd, cd, p[3]])
}

enum Side {
    Left,
    Right,
}

fn split(parts: ([Vec2; 4], [Vec2; 4]), side: Side) -> [Vec2; 4] {
    match side {
        Side::Left => parts.0,
        Side::Right => parts.1,
    }
}

/// A run of segments from one `MoveTo`.
struct Sub {
    segs: Vec<Seg>,
}

impl Sub {
    fn len(&self) -> f32 {
        self.segs.iter().map(Seg::len).sum()
    }
}

impl Path {
    /// The subpaths as segments, with the width at each end (1 where there
    /// is no profile). A closed subpath gets the side that closes it.
    fn segments(&self, widths: Option<&[f32]>) -> Vec<Sub> {
        let width = |n: usize| {
            widths
                .and_then(|w| w.get(n))
                .copied()
                .filter(|w| w.is_finite())
                .unwrap_or(1.0)
        };
        let mut subs: Vec<Sub> = Vec::new();
        let (mut n, mut first, mut first_w) = (0usize, Vec2::ZERO, 1.0);
        let (mut cur, mut cur_w) = (Vec2::ZERO, 1.0);
        for cmd in &self.cmds {
            match *cmd {
                PathCmd::MoveTo(p) => {
                    (first, cur) = (p, p);
                    (first_w, cur_w) = (width(n), width(n));
                    n += 1;
                    subs.push(Sub { segs: Vec::new() });
                }
                PathCmd::LineTo(p) => {
                    let w = width(n);
                    n += 1;
                    if let Some(sub) = subs.last_mut() {
                        sub.segs.push(Seg::new(cur, None, p, cur_w, w));
                    }
                    (cur, cur_w) = (p, w);
                }
                PathCmd::CubicTo(c1, c2, p) => {
                    let w = width(n);
                    n += 1;
                    if let Some(sub) = subs.last_mut() {
                        sub.segs.push(Seg::new(cur, Some((c1, c2)), p, cur_w, w));
                    }
                    (cur, cur_w) = (p, w);
                }
                PathCmd::Close => {
                    if let Some(sub) = subs.last_mut()
                        && cur.distance(first) > 1e-6
                    {
                        sub.segs.push(Seg::new(cur, None, first, cur_w, first_w));
                    }
                    (cur, cur_w) = (first, first_w);
                }
            }
        }
        subs
    }

    /// The part of every subpath between `start` and `end` (each 0 to 1 of
    /// its length), as open paths, with the width profile cut to match. A
    /// range that holds nothing leaves an empty path. The whole range
    /// (0 to 1) returns the path itself, closed ones still closed.
    pub fn trimmed(&self, start: f32, end: f32, widths: Option<&[f32]>) -> (Path, Option<Vec<f32>>) {
        let (a, b) = if start.is_finite() && end.is_finite() {
            (start.min(end).clamp(0.0, 1.0), start.max(end).clamp(0.0, 1.0))
        } else {
            (0.0, 1.0)
        };
        if a <= 0.0 && b >= 1.0 {
            return (self.clone(), widths.map(<[f32]>::to_vec));
        }
        let mut cmds = Vec::new();
        let mut out_w = widths.map(|_| Vec::new());
        for sub in self.segments(widths) {
            let total = sub.len();
            let (s, e) = (a * total, b * total);
            if e - s <= 1e-6 {
                continue;
            }
            let mut offset = 0.0_f32;
            let mut started = false;
            for seg in &sub.segs {
                let (lo, hi) = (offset.max(s), (offset + seg.len()).min(e));
                offset += seg.len();
                if hi <= lo {
                    continue;
                }
                let (t0, t1) = (
                    seg.param_at(lo - (offset - seg.len())),
                    seg.param_at(hi - (offset - seg.len())),
                );
                if !started {
                    cmds.push(PathCmd::MoveTo(seg.at(t0)));
                    if let Some(w) = &mut out_w {
                        w.push(seg.width_at(t0));
                    }
                    started = true;
                }
                cmds.push(seg.piece(t0, t1));
                if let Some(w) = &mut out_w {
                    w.push(seg.width_at(t1));
                }
            }
        }
        (Path { cmds }, out_w)
    }

    /// How far along the path `point` is nearest, as 0 to 1 of the length of
    /// the subpath it is on, with how far from it. None for an empty path.
    pub fn exposure_at(&self, point: Vec2) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        for sub in self.segments(None) {
            let total = sub.len();
            if total <= 0.0 {
                continue;
            }
            let mut offset = 0.0_f32;
            for seg in &sub.segs {
                let steps = seg.cum.len() - 1;
                for i in 0..steps {
                    let (p, q) = (
                        seg.at(i as f32 / steps as f32),
                        seg.at((i + 1) as f32 / steps as f32),
                    );
                    let d = q - p;
                    let along = if d.length_squared() > 0.0 {
                        ((point - p).dot(d) / d.length_squared()).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let distance = point.distance(p + d * along);
                    if best.is_none_or(|(_, b)| distance < b) {
                        let at = offset + seg.cum[i] + (seg.cum[i + 1] - seg.cum[i]) * along;
                        best = Some((at / total, distance));
                    }
                }
                offset += seg.len();
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> Path {
        Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(10.0, 0.0))
    }

    #[test]
    fn the_whole_range_is_the_path() {
        let path = Path::rect(Vec2::ZERO, Vec2::splat(10.0), 2.0);
        let (same, widths) = path.trimmed(0.0, 1.0, None);
        assert_eq!((same, widths), (path.clone(), None));
        // not a number is the whole thing too
        assert_eq!(path.trimmed(f32::NAN, 0.5, None).0, path);
    }

    #[test]
    fn a_line_is_cut_at_the_fractions() {
        let (cut, _) = line().trimmed(0.25, 0.75, None);
        assert_eq!(
            cut.cmds,
            vec![
                PathCmd::MoveTo(Vec2::new(2.5, 0.0)),
                PathCmd::LineTo(Vec2::new(7.5, 0.0))
            ]
        );
        // the ends can be dragged past each other
        assert_eq!(line().trimmed(0.75, 0.25, None).0, cut);
        // nothing between equal ends
        assert!(line().trimmed(0.5, 0.5, None).0.cmds.is_empty());
    }

    #[test]
    fn a_curve_is_cut_along_itself() {
        let path = Path::new().move_to(Vec2::ZERO).cubic_to(
            Vec2::new(0.0, 10.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 0.0),
        );
        let (cut, _) = path.trimmed(0.2, 0.6, None);
        let [PathCmd::MoveTo(a), PathCmd::CubicTo(c1, c2, b)] = cut.cmds[..] else {
            panic!("one cubic expected: {:?}", cut.cmds);
        };
        // the piece is a part of the same curve: its points and the middle of
        // it lie on the original, found by searching the original's samples
        let seg = Seg::new(
            Vec2::ZERO,
            Some((Vec2::new(0.0, 10.0), Vec2::new(10.0, 10.0))),
            Vec2::new(10.0, 0.0),
            1.0,
            1.0,
        );
        let near = |p: Vec2| {
            (0..=200)
                .map(|i| seg.at(i as f32 / 200.0).distance(p))
                .fold(f32::MAX, f32::min)
        };
        assert!(near(a) < 0.05 && near(b) < 0.05, "{a} {b}");
        let mid = Seg::new(a, Some((c1, c2)), b, 1.0, 1.0).at(0.5);
        assert!(near(mid) < 0.05, "{mid}");
        // it is about 0.4 of the whole length
        let whole = seg.len();
        let part = Seg::new(a, Some((c1, c2)), b, 1.0, 1.0).len();
        assert!((part / whole - 0.4).abs() < 0.02, "{part} of {whole}");
    }

    #[test]
    fn the_width_profile_is_cut_with_the_path() {
        let path = Path::new()
            .move_to(Vec2::ZERO)
            .line_to(Vec2::new(10.0, 0.0))
            .line_to(Vec2::new(20.0, 0.0));
        let (cut, widths) = path.trimmed(0.25, 0.75, Some(&[0.0, 1.0, 0.0]));
        assert_eq!(cut.on_curve_count(), 3);
        // a quarter along is halfway up the first side; the middle is full
        let w = widths.expect("a profile in, a profile out");
        assert_eq!(w.len(), 3);
        assert!(
            (w[0] - 0.5).abs() < 1e-4 && (w[1] - 1.0).abs() < 1e-4 && (w[2] - 0.5).abs() < 1e-4,
            "{w:?}"
        );
    }

    #[test]
    fn a_closed_path_is_trimmed_all_the_way_round() {
        let square = Path::rect(Vec2::ZERO, Vec2::splat(10.0), 0.0);
        let (cut, _) = square.trimmed(0.0, 0.5, None);
        // half the way round: the top and the right side
        assert_eq!(
            cut.cmds,
            vec![
                PathCmd::MoveTo(Vec2::ZERO),
                PathCmd::LineTo(Vec2::new(10.0, 0.0)),
                PathCmd::LineTo(Vec2::new(10.0, 10.0)),
            ]
        );
    }

    #[test]
    fn the_nearest_point_says_how_far_along_it_is() {
        let (f, d) = line().exposure_at(Vec2::new(3.0, 4.0)).unwrap();
        assert!((f - 0.3).abs() < 1e-4 && (d - 4.0).abs() < 1e-4, "{f} {d}");
        assert_eq!(line().exposure_at(Vec2::new(-5.0, 0.0)).map(|r| r.0), Some(0.0));
        assert!(Path::new().exposure_at(Vec2::ZERO).is_none());
    }
}
