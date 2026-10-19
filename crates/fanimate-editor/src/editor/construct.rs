//! Construction drawing: the Add Point and Pen tools.
//!
//! Add Point works like Moho's: click a line to put a point on it, drag
//! from the end of an open path to extend it, drag from empty space to
//! start a new line. The Pen works like Illustrator's: click for corner
//! points, drag for smooth ones, click the first point to close the shape.

use std::collections::BTreeSet;

use fanimate_core::{LayerId, Path, Shape, Stroke, Style, Vec2};

use super::{Editor, PointRef, StatusKind};

/// The Add Point drag in progress.
pub(super) struct AddPointDrag {
    /// The shape and the point being placed.
    point: PointRef,
    /// The other end of the open path being extended, to close the shape
    /// when the new point is dropped on it.
    close_to: Option<usize>,
}

/// A path being drawn with the Pen, in document space.
pub(super) struct PenPath {
    /// The layer it goes on; `None` makes a new vector layer.
    target: Option<LayerId>,
    points: Vec<PenPoint>,
    /// Set while the pointer is down on the first point, to close.
    closing: bool,
}

#[derive(Clone, Copy, Debug)]
struct PenPoint {
    at: Vec2,
    /// The outgoing curve handle, relative to `at`; the incoming handle is
    /// its mirror. Zero for a corner.
    handle: Vec2,
}

impl Editor {
    /// Add Point, click: put a point on the line under `at` (document
    /// space, within `tolerance`), without changing the shape. One undo
    /// step. Returns whether a point was added.
    pub fn insert_point_at(&mut self, at: Vec2, tolerance: f32) -> bool {
        let Some((shape, cmd, t)) = self.segment_at(at, tolerance) else {
            return false;
        };
        self.checkpoint("Add point");
        let Some(point) = self.point_shape_mut(shape).and_then(|s| s.insert_point(cmd, t)) else {
            return false;
        };
        self.points = BTreeSet::from([PointRef {
            layer: shape.0,
            shape: shape.1,
            point,
        }]);
        true
    }

    /// Add Point, drag start at `at`: extend an open path from its end, put
    /// a point on a line, or start a new line. The new point is selected
    /// and follows [`drag_points`](Self::drag_points). One undo step, until
    /// [`end_add_point`](Self::end_add_point).
    pub fn begin_add_point(&mut self, at: Vec2, tolerance: f32) -> bool {
        // from the end of an open path: extend it
        let end = self.point_at(at, tolerance).filter(|r| {
            self.point_shape((r.layer, r.shape))
                .is_some_and(|s| s.is_open_end(r.point))
        });
        if let Some(end) = end {
            self.checkpoint("Add point");
            let shape = (end.layer, end.shape);
            let Some(s) = self.point_shape_mut(shape) else {
                return false;
            };
            let close_to = s.other_end(end.point);
            let Some(point) = s.extend_from(end.point, Vec2::ZERO) else {
                return false;
            };
            // extending from the start renumbers the rest of the path
            let close_to = close_to.map(|c| if point <= c { c + 1 } else { c });
            let point = PointRef { point, ..end };
            self.points = BTreeSet::from([point]);
            self.add_point = Some(AddPointDrag { point, close_to });
            return true;
        }
        // on a line: a new point there
        if self.insert_point_at(at, tolerance) {
            self.add_point = self.points.first().map(|&point| AddPointDrag {
                point,
                close_to: None,
            });
            return true;
        }
        // in empty space: a new line, its far end following the pointer
        let Ok(target) = self.drawing_target() else {
            return false;
        };
        let shape = Shape::new(
            "Line",
            Path::new().move_to(at).line_to(at),
            Style {
                fill: None,
                stroke: Some(Stroke {
                    color: self.draw.line,
                    width: self.draw.width,
                }),
            },
        );
        let Some(index) = self.add_shape(target, shape) else {
            return false;
        };
        let Some(layer) = self.primary() else {
            return false;
        };
        let point = PointRef {
            layer,
            shape: index,
            point: 1,
        };
        self.points = BTreeSet::from([point]);
        self.add_point = Some(AddPointDrag {
            point,
            close_to: None,
        });
        true
    }

    /// Add Point, drag end: a point dropped on the other end of the path it
    /// extends closes the shape.
    pub fn end_add_point(&mut self, tolerance: f32) {
        let Some(drag) = self.add_point.take() else {
            return;
        };
        let Some(other) = drag.close_to else { return };
        let shape = (drag.point.layer, drag.point.shape);
        let frame = self.current_frame() as f32;
        let Some(t) = self
            .shape_transforms()
            .into_iter()
            .find(|(s, _)| *s == shape)
            .map(|(_, t)| t)
        else {
            return;
        };
        let Some(pts) = self
            .point_shape(shape)
            .map(|s| s.path_at(frame).on_curve_points())
        else {
            return;
        };
        let (Some(a), Some(b)) = (pts.get(drag.point.point), pts.get(other)) else {
            return;
        };
        let (a, b) = (t.transform_point2(*a), t.transform_point2(*b));
        if a.distance(b) > tolerance {
            return;
        }
        let Some(s) = self.point_shape_mut((drag.point.layer, drag.point.shape)) else {
            return;
        };
        // the dropped point merges into the end it was dropped on
        if s.path.on_curve_count() > 3 && s.remove_points(&BTreeSet::from([drag.point.point])) {
            let end = if other > drag.point.point {
                other - 1
            } else {
                other
            };
            s.close_subpath(end);
            self.points.clear();
            self.set_status(StatusKind::Info, "Closed the shape");
        }
    }

    pub fn is_adding_point(&self) -> bool {
        self.add_point.is_some()
    }

    /// The shape segment nearest `at` (document space) on the point layer,
    /// within `tolerance`: the shape, and where on which command.
    pub(super) fn segment_at(&self, at: Vec2, tolerance: f32) -> Option<((LayerId, usize), usize, f32)> {
        let frame = self.current_frame() as f32;
        self.shape_transforms()
            .into_iter()
            .filter_map(|(shape, t)| {
                let inv = t.inverse();
                if !inv.is_finite() {
                    return None;
                }
                let s = self.point_shape(shape)?;
                let local = inv.transform_point2(at);
                let (cmd, u, _) = s.path_at(frame).nearest_segment(local)?;
                // measure the distance on screen, not in the shape's space
                let path = s.path_at(frame);
                let on = point_on(&path, cmd, u)?;
                let d = t.transform_point2(on).distance(at);
                (d <= tolerance).then_some((shape, cmd, u, d))
            })
            .min_by(|a, b| a.3.total_cmp(&b.3))
            .map(|(shape, cmd, u, _)| (shape, cmd, u))
    }

    // ------------------------------------------------------------------
    // Pen

    /// Pen, pointer down at `at` (document space): add a point, or, on the
    /// first point of a path with two or more, close it. Returns false,
    /// with a status message, when the selection cannot be drawn on.
    pub fn pen_down(&mut self, at: Vec2, tolerance: f32) -> bool {
        if !at.is_finite() {
            return false;
        }
        if self.pen.is_none() {
            let Ok(target) = self.drawing_target() else {
                return false;
            };
            self.pen = Some(PenPath {
                target,
                points: Vec::new(),
                closing: false,
            });
        }
        let Some(pen) = &mut self.pen else { return false };
        if pen.points.len() >= 2 && pen.points[0].at.distance(at) <= tolerance {
            pen.closing = true;
        } else {
            pen.points.push(PenPoint {
                at,
                handle: Vec2::ZERO,
            });
        }
        true
    }

    /// Pen, dragging to `to`: pull out the curve handles of the point just
    /// placed (or of the first point, when closing), making it smooth.
    pub fn pen_drag(&mut self, to: Vec2) {
        let Some(pen) = &mut self.pen else { return };
        let point = if pen.closing {
            pen.points.first_mut()
        } else {
            pen.points.last_mut()
        };
        if let Some(p) = point
            && to.is_finite()
        {
            p.handle = to - p.at;
        }
    }

    /// Pen, pointer up: closing the path finishes it.
    pub fn pen_up(&mut self) {
        if self.pen.as_ref().is_some_and(|p| p.closing) {
            self.finish_path();
        }
    }

    /// Finish the Pen path and add it to its layer, as one undo step. A
    /// path of one point is dropped.
    pub fn finish_path(&mut self) {
        let Some(pen) = self.pen.take() else { return };
        let Some(shape) = self.pen_shape(&pen, None) else {
            return;
        };
        // the layer may have gone (undo while drawing): draw on a new one
        let target = pen.target.filter(|id| self.doc.find(*id).is_some());
        self.add_shape(target, shape);
    }

    pub fn cancel_path(&mut self) {
        self.pen = None;
    }

    pub fn is_drawing_path(&self) -> bool {
        self.pen.is_some()
    }

    /// The Pen path as it would be if finished now, with a line on to
    /// `pointer` while the path is open, to preview.
    pub fn pen_preview(&self, pointer: Option<Vec2>) -> Option<Shape> {
        let pen = self.pen.as_ref()?;
        self.pen_shape(pen, pointer.filter(|_| !pen.closing))
    }

    /// The Pen path's points and handles (document space), to draw them.
    pub fn pen_points(&self) -> Vec<(Vec2, Vec2)> {
        self.pen
            .as_ref()
            .map(|p| p.points.iter().map(|q| (q.at, q.handle)).collect())
            .unwrap_or_default()
    }

    fn pen_shape(&self, pen: &PenPath, pointer: Option<Vec2>) -> Option<Shape> {
        let mut points = pen.points.clone();
        if let Some(p) = pointer {
            points.push(PenPoint {
                at: p,
                handle: Vec2::ZERO,
            });
        }
        if points.len() < 2 {
            return None;
        }
        let segment = |path: Path, a: &PenPoint, b: &PenPoint| {
            if a.handle == Vec2::ZERO && b.handle == Vec2::ZERO {
                path.line_to(b.at)
            } else {
                path.cubic_to(a.at + a.handle, b.at - b.handle, b.at)
            }
        };
        let mut path = Path::new().move_to(points[0].at);
        for w in points.windows(2) {
            path = segment(path, &w[0], &w[1]);
        }
        let d = &self.draw;
        let stroke = Stroke {
            color: d.line,
            width: d.width,
        };
        let closed = pen.closing && pointer.is_none();
        let style = if closed {
            let (first, last) = (points[0], points[points.len() - 1]);
            // a straight closing segment is drawn by the close itself
            if first.handle != Vec2::ZERO || last.handle != Vec2::ZERO {
                path = segment(path, &last, &first);
            }
            path = path.close();
            Style {
                fill: d.fill,
                stroke: (d.outline || d.fill.is_none()).then_some(stroke),
            }
        } else {
            Style {
                fill: None,
                stroke: Some(stroke),
            }
        };
        Some(Shape::new("Path", path, style))
    }
}

/// The point `t` of the way along the segment drawn by `path.cmds[cmd]`.
fn point_on(path: &Path, cmd: usize, t: f32) -> Option<Vec2> {
    use fanimate_core::PathCmd;
    let start = match path.cmds.get(cmd.checked_sub(1)?)? {
        PathCmd::MoveTo(p) | PathCmd::LineTo(p) | PathCmd::CubicTo(_, _, p) => *p,
        PathCmd::Close => return None,
    };
    Some(match *path.cmds.get(cmd)? {
        PathCmd::LineTo(b) => start.lerp(b, t),
        PathCmd::CubicTo(c1, c2, b) => {
            let u = 1.0 - t;
            start * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + b * (t * t * t)
        }
        PathCmd::Close => {
            let first = path.cmds[..cmd].iter().rev().find_map(|c| match *c {
                PathCmd::MoveTo(p) => Some(p),
                _ => None,
            })?;
            start.lerp(first, t)
        }
        PathCmd::MoveTo(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Tool};
    use fanimate_core::{LayerKind, PathCmd};

    fn shapes(ed: &Editor) -> Vec<Shape> {
        let id = ed.primary().unwrap();
        match &ed.doc.find(id).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.clone(),
            _ => panic!("not a vector layer"),
        }
    }

    fn editor_with_line() -> Editor {
        let mut ed = Editor::untitled();
        ed.tool = Tool::AddPoint;
        assert!(ed.begin_add_point(Vec2::new(0.0, 0.0), 2.0));
        ed.drag_points(Vec2::new(100.0, 0.0));
        ed.end_add_point(2.0);
        ed
    }

    #[test]
    fn dragging_in_empty_space_starts_a_line() {
        let mut ed = editor_with_line();
        let s = shapes(&ed);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Line 1");
        assert_eq!(s[0].path.on_curve_points(), [Vec2::ZERO, Vec2::new(100.0, 0.0)]);
        assert!(s[0].style.fill.is_none());
        ed.run(Action::Undo);
        assert!(shapes(&ed).is_empty());
    }

    #[test]
    fn clicking_a_line_adds_a_point_on_it() {
        let mut ed = editor_with_line();
        assert!(!ed.insert_point_at(Vec2::new(50.0, 30.0), 2.0));
        assert!(ed.insert_point_at(Vec2::new(40.0, 1.0), 2.0));
        let s = &shapes(&ed)[0];
        assert_eq!(s.path.on_curve_points()[1], Vec2::new(40.0, 0.0));
        assert_eq!(ed.points.len(), 1);
        assert_eq!(ed.history.undo_label(), Some("Add point"));
    }

    #[test]
    fn dragging_from_an_end_extends_and_dropping_on_the_other_end_closes() {
        let mut ed = editor_with_line();
        assert!(ed.begin_add_point(Vec2::new(100.0, 0.0), 2.0));
        ed.drag_points(Vec2::new(0.0, 100.0));
        ed.end_add_point(2.0);
        assert_eq!(shapes(&ed)[0].path.on_curve_count(), 3);
        // from the far end, back to the start: a closed triangle
        assert!(ed.begin_add_point(Vec2::new(100.0, 100.0), 2.0));
        ed.drag_points(Vec2::new(-50.0, 0.0));
        ed.end_add_point(2.0);
        assert!(ed.begin_add_point(Vec2::new(50.0, 100.0), 2.0));
        ed.drag_points(Vec2::new(-50.0, -100.0));
        ed.end_add_point(2.0);
        let s = &shapes(&ed)[0];
        assert_eq!(s.path.cmds.last(), Some(&PathCmd::Close));
        assert_eq!(s.path.on_curve_count(), 4);
    }

    #[test]
    fn pen_clicks_make_corners_and_drags_make_curves() {
        let mut ed = Editor::untitled();
        assert!(ed.pen_down(Vec2::ZERO, 2.0));
        ed.pen_up();
        assert!(ed.pen_preview(None).is_none());
        assert!(ed.pen_down(Vec2::new(100.0, 0.0), 2.0));
        ed.pen_drag(Vec2::new(120.0, 20.0));
        ed.pen_up();
        let preview = ed.pen_preview(Some(Vec2::new(100.0, 100.0))).unwrap();
        assert_eq!(preview.path.on_curve_count(), 3);
        ed.run(Action::FinishPath);
        assert!(!ed.is_drawing_path());
        let s = &shapes(&ed)[0];
        assert_eq!(s.name, "Path 1");
        assert!(matches!(s.path.cmds[1], PathCmd::CubicTo(_, _, _)));
        assert!(s.style.fill.is_none());
        // one undo step
        ed.run(Action::Undo);
        assert!(shapes(&ed).is_empty());
    }

    #[test]
    fn pen_closes_on_the_first_point() {
        let mut ed = Editor::untitled();
        for p in [Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(50.0, 80.0)] {
            assert!(ed.pen_down(p, 3.0));
            ed.pen_up();
        }
        assert!(ed.pen_down(Vec2::new(1.0, 1.0), 3.0));
        ed.pen_up();
        assert!(!ed.is_drawing_path());
        let s = &shapes(&ed)[0];
        assert_eq!(s.path.cmds.last(), Some(&PathCmd::Close));
        assert_eq!(s.path.on_curve_count(), 3);
        assert_eq!(s.style.fill, ed.draw.fill);
    }

    #[test]
    fn esc_finishes_and_a_lone_point_is_dropped() {
        let mut ed = Editor::untitled();
        assert!(ed.pen_down(Vec2::ZERO, 2.0));
        ed.run(Action::SelectNone);
        assert!(!ed.is_drawing_path());
        assert!(shapes(&ed).is_empty());
        assert!(ed.history.undo_label().is_none());
    }
}
