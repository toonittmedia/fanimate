//! Editing a path point by point: the Select Points and Transform Points
//! tools.
//!
//! Points are numbered as in a width profile: one per on-curve point (the
//! end of each move, line and cubic command), in order. A point's curve
//! handles (the control points either side of it) move with it, so moving
//! a point bends only the segments it touches.
//!
//! Point keyframes live here too: a shape's `path_keys` hold its path at
//! chosen frames, every key with the same commands as the shape's own path,
//! so every edit that adds or removes points applies to all of them.

use std::borrow::Cow;
use std::collections::BTreeSet;

use glam::Vec2;

use crate::anim::{Frame, Interpolation, Key, Keying};
use crate::path::{Path, PathCmd, Shape};

impl Path {
    /// The on-curve points, in order.
    pub fn on_curve_points(&self) -> Vec<Vec2> {
        self.cmds
            .iter()
            .filter_map(|c| match *c {
                PathCmd::MoveTo(p) | PathCmd::LineTo(p) | PathCmd::CubicTo(_, _, p) => Some(p),
                PathCmd::Close => None,
            })
            .collect()
    }

    /// Index into `cmds` of each on-curve point.
    fn point_cmds(&self) -> Vec<usize> {
        (0..self.cmds.len())
            .filter(|&i| !matches!(self.cmds[i], PathCmd::Close))
            .collect()
    }

    /// Each subpath as a range of on-curve point numbers, and whether it is
    /// closed.
    fn subpaths(&self) -> Vec<(std::ops::Range<usize>, bool)> {
        let mut out: Vec<(std::ops::Range<usize>, bool)> = Vec::new();
        let mut k = 0;
        for cmd in &self.cmds {
            match cmd {
                PathCmd::MoveTo(_) => {
                    out.push((k..k + 1, false));
                    k += 1;
                }
                PathCmd::LineTo(_) | PathCmd::CubicTo(..) => {
                    if let Some(last) = out.last_mut() {
                        last.0.end = k + 1;
                    }
                    k += 1;
                }
                PathCmd::Close => {
                    if let Some(last) = out.last_mut() {
                        last.1 = true;
                    }
                }
            }
        }
        out
    }

    /// `points` plus their twins: a closed subpath whose last point lies on
    /// its first (as circles do) has two numbers for one visible point.
    fn with_twins(&self, points: &BTreeSet<usize>) -> BTreeSet<usize> {
        let pts = self.on_curve_points();
        let mut out = points.clone();
        for (range, closed) in self.subpaths() {
            let (first, last) = (range.start, range.end - 1);
            if closed && last > first && pts[first] == pts[last] {
                if points.contains(&first) {
                    out.insert(last);
                }
                if points.contains(&last) {
                    out.insert(first);
                }
            }
        }
        out
    }

    /// Whether `other` is made of the same commands, so the two can be
    /// interpolated point by point.
    pub fn same_structure(&self, other: &Path) -> bool {
        self.cmds.len() == other.cmds.len()
            && self
                .cmds
                .iter()
                .zip(&other.cmds)
                .all(|(a, b)| std::mem::discriminant(a) == std::mem::discriminant(b))
    }

    /// Point by point from `a` (`t` = 0) to `b` (`t` = 1). `None` when the
    /// paths are not made of the same commands.
    pub fn lerp(a: &Path, b: &Path, t: f32) -> Option<Path> {
        if !a.same_structure(b) {
            return None;
        }
        let cmds = a
            .cmds
            .iter()
            .zip(&b.cmds)
            .map(|(x, y)| match (*x, *y) {
                (PathCmd::MoveTo(p), PathCmd::MoveTo(q)) => PathCmd::MoveTo(p.lerp(q, t)),
                (PathCmd::LineTo(p), PathCmd::LineTo(q)) => PathCmd::LineTo(p.lerp(q, t)),
                (PathCmd::CubicTo(p1, p2, p), PathCmd::CubicTo(q1, q2, q)) => {
                    PathCmd::CubicTo(p1.lerp(q1, t), p2.lerp(q2, t), p.lerp(q, t))
                }
                _ => PathCmd::Close,
            })
            .collect();
        Some(Path { cmds })
    }

    /// Whether every point is a finite number.
    pub fn is_finite(&self) -> bool {
        self.points().all(|p| p.is_finite())
    }

    /// The segment nearest `p`: the index in `cmds` of the command that
    /// draws it (a line, a cubic, or the `Z` that closes a subpath), how far
    /// along it the nearest spot is (0 to 1), and the distance.
    pub fn nearest_segment(&self, p: Vec2) -> Option<(usize, f32, f32)> {
        let mut best: Option<(usize, f32, f32)> = None;
        let mut start: Option<Vec2> = None;
        let mut last: Option<Vec2> = None;
        let consider = |i: usize, t: f32, q: Vec2, best: &mut Option<(usize, f32, f32)>| {
            let d = q.distance(p);
            if best.is_none_or(|b| d < b.2) {
                *best = Some((i, t, d));
            }
        };
        for (i, cmd) in self.cmds.iter().enumerate() {
            match *cmd {
                PathCmd::MoveTo(q) => {
                    start = Some(q);
                    last = Some(q);
                }
                PathCmd::LineTo(b) => {
                    if let Some(a) = last {
                        let t = closest_t(p, a, b);
                        consider(i, t, a.lerp(b, t), &mut best);
                    }
                    last = Some(b);
                }
                PathCmd::CubicTo(c1, c2, b) => {
                    if let Some(a) = last {
                        // coarse search, then refine around the best sample
                        const N: usize = 32;
                        let mut t_best = 0.0;
                        let mut d_best = f32::INFINITY;
                        for k in 0..=N {
                            let t = k as f32 / N as f32;
                            let d = cubic(a, c1, c2, b, t).distance(p);
                            if d < d_best {
                                (t_best, d_best) = (t, d);
                            }
                        }
                        let mut step = 1.0 / N as f32;
                        for _ in 0..12 {
                            step /= 2.0;
                            for t in [t_best - step, t_best + step] {
                                let t = t.clamp(0.0, 1.0);
                                let d = cubic(a, c1, c2, b, t).distance(p);
                                if d < d_best {
                                    (t_best, d_best) = (t, d);
                                }
                            }
                        }
                        consider(i, t_best, cubic(a, c1, c2, b, t_best), &mut best);
                    }
                    last = Some(b);
                }
                PathCmd::Close => {
                    if let (Some(a), Some(b)) = (last, start)
                        && a != b
                    {
                        let t = closest_t(p, a, b);
                        consider(i, t, a.lerp(b, t), &mut best);
                    }
                    last = None;
                    start = None;
                }
            }
        }
        best
    }

    /// Split the segment drawn by `cmds[cmd]` at `t`, adding an on-curve
    /// point there without changing the path's shape. Returns the new
    /// point's number, or `None` if `cmd` does not draw a segment.
    fn split(&mut self, cmd: usize, t: f32) -> Option<usize> {
        let t = t.clamp(0.0, 1.0);
        let from = self.segment_start(cmd)?;
        let number = self.cmds[..cmd]
            .iter()
            .filter(|c| !matches!(c, PathCmd::Close))
            .count();
        match self.cmds[cmd] {
            PathCmd::LineTo(b) => self.cmds.insert(cmd, PathCmd::LineTo(from.lerp(b, t))),
            PathCmd::CubicTo(c1, c2, b) => {
                // de Casteljau: two cubics that together trace the original
                let (p01, p12, p23) = (from.lerp(c1, t), c1.lerp(c2, t), c2.lerp(b, t));
                let (p012, p123) = (p01.lerp(p12, t), p12.lerp(p23, t));
                let m = p012.lerp(p123, t);
                self.cmds[cmd] = PathCmd::CubicTo(p123, p23, b);
                self.cmds.insert(cmd, PathCmd::CubicTo(p01, p012, m));
            }
            PathCmd::Close => {
                let first = self.subpath_start(cmd)?;
                self.cmds.insert(cmd, PathCmd::LineTo(from.lerp(first, t)));
            }
            PathCmd::MoveTo(_) => return None,
        }
        Some(number)
    }

    /// Where the segment drawn by `cmds[cmd]` starts.
    fn segment_start(&self, cmd: usize) -> Option<Vec2> {
        if cmd == 0 || cmd >= self.cmds.len() {
            return None;
        }
        match self.cmds[cmd - 1] {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) | PathCmd::CubicTo(_, _, p) => Some(p),
            PathCmd::Close => None,
        }
    }

    /// The first point of the subpath that `cmds[cmd]` belongs to.
    fn subpath_start(&self, cmd: usize) -> Option<Vec2> {
        self.cmds[..cmd].iter().rev().find_map(|c| match *c {
            PathCmd::MoveTo(p) => Some(p),
            _ => None,
        })
    }

    /// Move the given points, and their handles, by `delta`.
    pub fn move_points(&mut self, points: &BTreeSet<usize>, delta: Vec2) {
        let points = self.with_twins(points);
        let at = self.point_cmds();
        for &k in &points {
            let Some(&i) = at.get(k) else { continue };
            match &mut self.cmds[i] {
                PathCmd::MoveTo(p) | PathCmd::LineTo(p) => *p += delta,
                PathCmd::CubicTo(_, c2, p) => {
                    *c2 += delta;
                    *p += delta;
                }
                PathCmd::Close => {}
            }
            // the handle leaving this point belongs to it too
            if let Some(PathCmd::CubicTo(c1, _, _)) = self.cmds.get_mut(i + 1) {
                *c1 += delta;
            }
        }
    }
}

impl Path {
    /// The path without the points in `remove` (twins already included),
    /// and the numbers of the points kept, in order.
    fn without(&self, remove: &BTreeSet<usize>) -> (Path, Vec<usize>) {
        let at = self.point_cmds();
        let mut cmds = Vec::new();
        let mut kept_all = Vec::new();
        for (range, closed) in self.subpaths() {
            let kept: Vec<usize> = range.filter(|k| !remove.contains(k)).collect();
            if kept.len() < 2 {
                continue;
            }
            for (n, &k) in kept.iter().enumerate() {
                let cmd = self.cmds[at[k]];
                cmds.push(if n == 0 {
                    // a subpath starts with a move to its first kept point
                    match cmd {
                        PathCmd::MoveTo(p) | PathCmd::LineTo(p) | PathCmd::CubicTo(_, _, p) => {
                            PathCmd::MoveTo(p)
                        }
                        PathCmd::Close => continue,
                    }
                } else if let PathCmd::MoveTo(p) = cmd {
                    PathCmd::LineTo(p)
                } else {
                    cmd
                });
                kept_all.push(k);
            }
            if closed && kept.len() > 2 {
                cmds.push(PathCmd::Close);
            }
        }
        (Path { cmds }, kept_all)
    }
}

impl Shape {
    /// Remove the given points. Segments either side of a removed point join
    /// up; a subpath left with fewer than two points goes, and a closed one
    /// left with two opens. The width profile and point keys keep step.
    /// Returns false when nothing is left of the shape.
    pub fn remove_points(&mut self, points: &BTreeSet<usize>) -> bool {
        let remove = self.path.with_twins(points);
        let count = self.path.on_curve_count();
        let widths = self.widths.take().filter(|w| w.len() == count);
        let skin = self.skin.take().filter(|w| w.len() == count);
        let (path, kept) = self.path.without(&remove);
        for key in &mut self.path_keys {
            key.value = key.value.without(&remove).0;
        }
        self.path = path;
        self.widths = widths.map(|w| kept.iter().map(|&k| w[k]).collect());
        self.skin = skin.map(|w| kept.iter().map(|&k| w[k].clone()).collect());
        !self.path.cmds.is_empty()
    }

    /// Add a point on the segment drawn by `path.cmds[cmd]`, `t` of the way
    /// along it, in the path and every point key, without changing the
    /// shape. Returns the new point's number.
    pub fn insert_point(&mut self, cmd: usize, t: f32) -> Option<usize> {
        let count = self.path.on_curve_count();
        let mut path = self.path.clone();
        let number = path.split(cmd, t)?;
        for key in &mut self.path_keys {
            key.value.split(cmd, t);
        }
        self.path = path;
        if let Some(w) = self.widths.as_mut().filter(|w| w.len() == count) {
            // the new point sits between its neighbours, so is its width
            let before = w[number - 1];
            let after = w.get(number).copied().unwrap_or(before);
            w.insert(number, before + (after - before) * t.clamp(0.0, 1.0));
        }
        if let Some(skin) = self.skin.as_mut().filter(|s| s.len() == count) {
            // the nearer neighbour's bones
            let from = if t < 0.5 {
                number - 1
            } else {
                number.min(skin.len() - 1)
            };
            skin.insert(number, skin[from].clone());
        }
        Some(number)
    }

    /// Whether `point` is the first or last point of an open subpath, so
    /// the path can be extended from it.
    pub fn is_open_end(&self, point: usize) -> bool {
        self.path
            .subpaths()
            .iter()
            .any(|(r, closed)| !closed && (point == r.start || point + 1 == r.end))
    }

    /// The other end of the open subpath that `point` ends.
    pub fn other_end(&self, point: usize) -> Option<usize> {
        self.path.subpaths().iter().find_map(|(r, closed)| {
            if *closed || r.len() < 2 {
                None
            } else if point == r.start {
                Some(r.end - 1)
            } else if point + 1 == r.end {
                Some(r.start)
            } else {
                None
            }
        })
    }

    /// Extend an open subpath from its end `point` with a straight segment
    /// to a new point `offset` away from it, in the path and in every point
    /// key. Returns the new point's number.
    pub fn extend_from(&mut self, point: usize, offset: Vec2) -> Option<usize> {
        let (range, _) = self
            .path
            .subpaths()
            .into_iter()
            .find(|(r, closed)| !closed && (point == r.start || point + 1 == r.end))?;
        // a lone point is extended forwards
        let at_start = point == range.start && range.len() > 1;
        let count = self.path.on_curve_count();
        let extend = |path: &mut Path| {
            let at = path.point_cmds();
            let pts = path.on_curve_points();
            let new = pts[point] + offset;
            if at_start {
                // the new point starts the subpath; the old start joins it
                path.cmds[at[point]] = PathCmd::LineTo(pts[point]);
                path.cmds.insert(at[point], PathCmd::MoveTo(new));
            } else {
                path.cmds.insert(at[point] + 1, PathCmd::LineTo(new));
            }
        };
        extend(&mut self.path);
        for key in &mut self.path_keys {
            extend(&mut key.value);
        }
        let number = if at_start { point } else { point + 1 };
        if let Some(w) = self.widths.as_mut().filter(|w| w.len() == count) {
            w.insert(number, w[point]);
        }
        if let Some(skin) = self.skin.as_mut().filter(|s| s.len() == count) {
            skin.insert(number, skin[point].clone());
        }
        Some(number)
    }

    /// Close the open subpath that ends at `point`.
    pub fn close_subpath(&mut self, point: usize) -> bool {
        let Some((range, _)) = self
            .path
            .subpaths()
            .into_iter()
            .find(|(r, closed)| !closed && r.len() > 2 && (point == r.start || point + 1 == r.end))
        else {
            return false;
        };
        let close = |path: &mut Path| {
            let at = path.point_cmds();
            path.cmds.insert(at[range.end - 1] + 1, PathCmd::Close);
        };
        close(&mut self.path);
        for key in &mut self.path_keys {
            close(&mut key.value);
        }
        true
    }

    /// Whether the shape has point keyframes.
    pub fn is_animated(&self) -> bool {
        !self.path_keys.is_empty()
    }

    /// Whether the shape's fill or stroke colour has keyframes.
    pub fn colors_animated(&self) -> bool {
        !self.fill_keys.is_empty() || !self.stroke_keys.is_empty()
    }

    /// The style at `frame`: the colours as their keys make them, the rest
    /// (stroke width) as it is.
    pub fn style_at(&self, frame: f32) -> crate::Style {
        let mut style = self.style;
        if let Some(fill) = &mut style.fill {
            *fill = crate::anim::sample_in(*fill, &self.fill_keys, frame);
        }
        if let Some(stroke) = &mut style.stroke {
            stroke.color = crate::anim::sample_in(stroke.color, &self.stroke_keys, frame);
        }
        style
    }

    /// Change the fill's colour at `frame` the way an edit is recorded (see
    /// [`Keying`]). A shape with no fill has no colour to change.
    pub fn set_fill_at(&mut self, frame: Frame, color: crate::Color, keying: Keying) {
        if let Some(fill) = &mut self.style.fill {
            crate::anim::write_in(fill, &mut self.fill_keys, frame, color, keying);
        }
    }

    /// Change the stroke's colour at `frame`, the same way.
    pub fn set_stroke_color_at(&mut self, frame: Frame, color: crate::Color, keying: Keying) {
        if let Some(stroke) = &mut self.style.stroke {
            crate::anim::write_in(&mut stroke.color, &mut self.stroke_keys, frame, color, keying);
        }
    }

    /// Whether the stroke's exposure is anything but the whole stroke, or
    /// animated.
    pub fn is_trimmed(&self) -> bool {
        !crate::path::is_full_trim(&self.trim) || !self.trim_keys.is_empty()
    }

    /// The stroke's start and end (each 0 to 1) at `frame`.
    pub fn trim_at(&self, frame: f32) -> Vec2 {
        crate::anim::sample_in(self.trim, &self.trim_keys, frame)
    }

    /// Change the stroke's exposure at `frame` the way an edit is recorded
    /// (see [`Keying`]). Both ends are kept within 0 to 1.
    pub fn set_trim_at(&mut self, frame: Frame, trim: Vec2, keying: Keying) {
        let clamp = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let trim = Vec2::new(clamp(trim.x), clamp(trim.y));
        crate::anim::write_in(&mut self.trim, &mut self.trim_keys, frame, trim, keying);
    }

    /// Without a fill there is nothing for fill keys to colour: drop them.
    pub fn drop_orphan_color_keys(&mut self) {
        if self.style.fill.is_none() {
            self.fill_keys.clear();
        }
        if self.style.stroke.is_none() {
            self.stroke_keys.clear();
        }
    }

    /// The path at `frame`, which may fall between whole frames: as a
    /// channel samples its keys, holding the first and last key's path
    /// outside them.
    pub fn path_at(&self, frame: f32) -> Cow<'_, Path> {
        let keys = &self.path_keys;
        let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
            return Cow::Borrowed(&self.path);
        };
        if frame <= first.frame as f32 {
            return Cow::Borrowed(&first.value);
        }
        if frame >= last.frame as f32 {
            return Cow::Borrowed(&last.value);
        }
        let next = keys.partition_point(|k| k.frame as f32 <= frame);
        let (a, b) = (&keys[next - 1], &keys[next]);
        let t = a
            .interp
            .apply((frame - a.frame as f32) / (b.frame - a.frame) as f32);
        if t <= 0.0 {
            return Cow::Borrowed(&a.value);
        }
        match Path::lerp(&a.value, &b.value, t) {
            Some(p) => Cow::Owned(p),
            None => Cow::Borrowed(&a.value),
        }
    }

    /// Change the path at `frame` the way an edit is recorded (see
    /// [`Keying`]): a shape with point keys gets a key on `frame`; one
    /// without has its path changed outright on the base frame, and on a
    /// later frame starts animating from a key on the base frame holding
    /// the drawing as it was. `path` must have the shape's commands; one
    /// that does not is ignored.
    pub fn set_path_at(&mut self, frame: Frame, path: Path, keying: Keying) {
        if !path.same_structure(&self.path) {
            return;
        }
        if self.is_animated() {
            self.key_path(frame, path, keying.interp);
        } else if frame > keying.base {
            self.key_path(keying.base, self.path.clone(), keying.interp);
            self.key_path(frame, path, keying.interp);
        } else {
            self.path = path;
        }
    }

    /// Key the path at `frame`, replacing any key there.
    pub fn key_path(&mut self, frame: Frame, path: Path, interp: Interpolation) {
        let key = Key {
            frame,
            value: path,
            interp,
        };
        match self.path_keys.binary_search_by_key(&frame, |k| k.frame) {
            Ok(i) => self.path_keys[i] = key,
            Err(i) => self.path_keys.insert(i, key),
        }
    }
}

fn closest_t(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 > 0.0 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn cubic(p0: Vec2, c1: Vec2, c2: Vec2, p1: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    p0 * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + p1 * (t * t * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;
    use crate::path::Style;

    fn set(points: &[usize]) -> BTreeSet<usize> {
        points.iter().copied().collect()
    }

    #[test]
    fn moving_a_point_moves_its_handles() {
        let mut path = Path::new()
            .move_to(Vec2::ZERO)
            .cubic_to(Vec2::new(1.0, 1.0), Vec2::new(2.0, 1.0), Vec2::new(3.0, 0.0))
            .cubic_to(Vec2::new(4.0, -1.0), Vec2::new(5.0, -1.0), Vec2::new(6.0, 0.0));
        path.move_points(&set(&[1]), Vec2::new(0.0, 10.0));
        assert_eq!(
            path.cmds[1],
            PathCmd::CubicTo(Vec2::new(1.0, 1.0), Vec2::new(2.0, 11.0), Vec2::new(3.0, 10.0))
        );
        assert_eq!(
            path.cmds[2],
            PathCmd::CubicTo(Vec2::new(4.0, 9.0), Vec2::new(5.0, -1.0), Vec2::new(6.0, 0.0))
        );
    }

    #[test]
    fn a_circle_moves_its_start_and_end_together() {
        let mut path = Path::circle(Vec2::ZERO, 10.0);
        assert_eq!(path.on_curve_points().len(), 5);
        path.move_points(&set(&[0]), Vec2::new(5.0, 0.0));
        let pts = path.on_curve_points();
        assert_eq!(pts[0], Vec2::new(15.0, 0.0));
        assert_eq!(pts[4], Vec2::new(15.0, 0.0));
    }

    #[test]
    fn removing_points_joins_segments_and_keeps_widths_in_step() {
        let mut shape = Shape::new(
            "s",
            Path::new()
                .move_to(Vec2::ZERO)
                .line_to(Vec2::new(1.0, 0.0))
                .line_to(Vec2::new(2.0, 0.0))
                .line_to(Vec2::new(3.0, 0.0)),
            Style::default().with_stroke(Color::BLACK, 1.0),
        );
        shape.widths = Some(vec![0.1, 0.2, 0.3, 0.4]);
        assert!(shape.remove_points(&set(&[0, 2])));
        assert_eq!(
            shape.path.on_curve_points(),
            [Vec2::new(1.0, 0.0), Vec2::new(3.0, 0.0)]
        );
        assert!(matches!(shape.path.cmds[0], PathCmd::MoveTo(_)));
        assert_eq!(shape.widths, Some(vec![0.2, 0.4]));
        assert!(!shape.remove_points(&set(&[0])));
    }

    #[test]
    fn a_closed_shape_cut_to_two_points_opens() {
        let mut shape = Shape::new(
            "r",
            Path::rect(Vec2::ZERO, Vec2::ONE, 0.0),
            Style::fill(Color::BLACK),
        );
        assert!(shape.remove_points(&set(&[1])));
        assert_eq!(shape.path.on_curve_points().len(), 3);
        assert_eq!(shape.path.cmds.last(), Some(&PathCmd::Close));
        assert!(shape.remove_points(&set(&[1])));
        assert_ne!(shape.path.cmds.last(), Some(&PathCmd::Close));
    }

    fn line3() -> Shape {
        Shape::new(
            "l",
            Path::new()
                .move_to(Vec2::ZERO)
                .line_to(Vec2::new(10.0, 0.0))
                .line_to(Vec2::new(20.0, 0.0)),
            Style::default().with_stroke(Color::BLACK, 1.0),
        )
    }

    #[test]
    fn point_keys_interpolate_and_hold_outside() {
        let mut s = line3();
        let mut moved = s.path.clone();
        moved.move_points(&set(&[1]), Vec2::new(0.0, 10.0));
        s.key_path(0, s.path.clone(), Interpolation::Linear);
        s.key_path(10, moved.clone(), Interpolation::Linear);
        assert!(s.is_animated());
        assert_eq!(s.path_at(5.0).on_curve_points()[1], Vec2::new(10.0, 5.0));
        assert_eq!(*s.path_at(-3.0), s.path);
        assert_eq!(*s.path_at(99.0), moved);
        // the keying rule: keyed shapes get a key; others change outright
        // on the base frame, and start animating on a later one
        let keying = Keying::default();
        let mut plain = line3();
        plain.set_path_at(0, moved.clone(), keying);
        assert!(!plain.is_animated());
        assert_eq!(plain.path, moved);
        let mut later = line3();
        let original = later.path.clone();
        later.set_path_at(7, moved.clone(), keying);
        assert_eq!(
            later.path_keys.iter().map(|k| k.frame).collect::<Vec<_>>(),
            [0, 7]
        );
        assert_eq!(*later.path_at(0.0), original, "the base frame keeps the drawing");
        assert_eq!(*later.path_at(7.0), moved);
        s.set_path_at(7, moved.clone(), keying);
        assert_eq!(s.path_keys.len(), 3);
        // a path with other commands is refused
        s.set_path_at(8, Path::circle(Vec2::ZERO, 1.0), keying);
        assert_eq!(s.path_keys.len(), 3);
    }

    #[test]
    fn paths_with_different_commands_do_not_interpolate() {
        let a = Path::rect(Vec2::ZERO, Vec2::ONE, 0.0);
        assert!(Path::lerp(&a, &Path::circle(Vec2::ZERO, 1.0), 0.5).is_none());
        let mut s = line3();
        s.path_keys = vec![
            Key {
                frame: 0,
                value: s.path.clone(),
                interp: Interpolation::Linear,
            },
            Key {
                frame: 10,
                value: a,
                interp: Interpolation::Linear,
            },
        ];
        // holds the earlier key instead
        assert_eq!(*s.path_at(5.0), s.path);
    }

    #[test]
    fn removing_points_keeps_point_keys_in_step() {
        let mut s = line3();
        s.key_path(4, s.path.clone(), Interpolation::Smooth);
        assert!(s.remove_points(&set(&[1])));
        assert_eq!(s.path_keys[0].value, s.path);
        assert_eq!(s.path.on_curve_count(), 2);
    }

    #[test]
    fn inserting_a_point_keeps_the_shape() {
        let mut s = Shape::new("c", Path::circle(Vec2::ZERO, 10.0), Style::fill(Color::BLACK));
        let before = s.path.polylines(0.01);
        let (cmd, t, d) = s.path.nearest_segment(Vec2::new(8.0, 8.0)).unwrap();
        assert!(d < 2.0);
        assert_eq!(cmd, 1);
        let n = s.insert_point(cmd, t).unwrap();
        assert_eq!(n, 1);
        assert_eq!(s.path.on_curve_count(), 6);
        let p = s.path.on_curve_points()[1];
        assert!((p.length() - 10.0).abs() < 0.05, "{p:?}");
        // the outline is unchanged: every old sample still lies on the circle
        let after = s.path.polylines(0.01);
        for q in after.iter().flat_map(|(pts, _)| pts) {
            assert!((q.length() - 10.0).abs() < 0.05);
        }
        assert!(!before.is_empty());
    }

    #[test]
    fn inserting_on_a_line_and_the_closing_segment() {
        let mut s = line3();
        s.widths = Some(vec![0.0, 1.0, 0.5]);
        s.key_path(3, s.path.clone(), Interpolation::Smooth);
        let n = s.insert_point(2, 0.5).unwrap();
        assert_eq!(n, 2);
        assert_eq!(s.path.on_curve_points()[2], Vec2::new(15.0, 0.0));
        assert_eq!(s.widths, Some(vec![0.0, 1.0, 0.75, 0.5]));
        assert_eq!(s.path_keys[0].value, s.path);
        let mut r = Shape::new(
            "r",
            Path::rect(Vec2::ZERO, Vec2::splat(10.0), 0.0),
            Style::fill(Color::BLACK),
        );
        let (cmd, t, _) = r.path.nearest_segment(Vec2::new(-1.0, 5.0)).unwrap();
        assert!(matches!(r.path.cmds[cmd], PathCmd::Close));
        assert_eq!(r.insert_point(cmd, t), Some(4));
        assert_eq!(r.path.on_curve_points()[4], Vec2::new(0.0, 5.0));
    }

    #[test]
    fn point_edits_keep_the_skin_in_step() {
        let mut s = line3();
        s.skin = Some(vec![vec![(0, 1.0)], vec![(1, 1.0)], vec![(2, 1.0)]]);
        s.insert_point(2, 0.8);
        assert_eq!(s.skin.as_ref().unwrap()[2], vec![(2, 1.0)]);
        s.extend_from(0, Vec2::X);
        assert_eq!(s.skin.as_ref().unwrap()[0], vec![(0, 1.0)]);
        s.remove_points(&set(&[1]));
        assert_eq!(s.skin.as_ref().unwrap().len(), s.path.on_curve_count());
    }

    #[test]
    fn extending_and_closing_an_open_path() {
        let mut s = line3();
        assert!(s.is_open_end(0) && s.is_open_end(2) && !s.is_open_end(1));
        assert_eq!(s.other_end(0), Some(2));
        assert_eq!(s.extend_from(2, Vec2::new(0.0, 10.0)), Some(3));
        assert_eq!(s.path.on_curve_points()[3], Vec2::new(20.0, 10.0));
        assert_eq!(s.extend_from(0, Vec2::new(-5.0, 0.0)), Some(0));
        let pts = s.path.on_curve_points();
        assert_eq!(pts[0], Vec2::new(-5.0, 0.0));
        assert_eq!(pts[1], Vec2::ZERO);
        assert!(matches!(s.path.cmds[0], PathCmd::MoveTo(_)));
        assert!(matches!(s.path.cmds[1], PathCmd::LineTo(_)));
        assert!(s.close_subpath(4));
        assert_eq!(s.path.cmds.last(), Some(&PathCmd::Close));
        assert!(!s.is_open_end(0));
    }

    #[test]
    fn removing_a_circles_start_removes_its_twin() {
        let mut shape = Shape::new("c", Path::circle(Vec2::ZERO, 1.0), Style::fill(Color::BLACK));
        assert!(shape.remove_points(&set(&[0])));
        assert_eq!(shape.path.on_curve_points().len(), 3);
    }
}
