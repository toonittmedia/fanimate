//! Point editing: the Select Points and Transform Points tools.
//!
//! As in Moho, these work on the selected vector layer. Points are shown
//! and picked where they are drawn at the current frame, including shapes
//! that follow bones, and moving them changes the shape for the whole
//! animation.

use std::collections::{BTreeMap, BTreeSet};

use fanimate_core::{Affine2, LayerId, Vec2, evaluate};

use super::{Editor, SelectMode, StatusKind, plural};

/// A point on a shape: its layer, the shape's index in the layer, and the
/// point's number (one per on-curve point, as in a width profile).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PointRef {
    pub layer: LayerId,
    pub shape: usize,
    pub point: usize,
}

impl Editor {
    /// The layer the point tools work on: the selected vector layer.
    pub fn point_layer(&self) -> Option<LayerId> {
        let id = self.primary()?;
        self.is_drawable(id).then_some(id)
    }

    /// Every point of the point layer's shapes, where it is drawn now (in
    /// document space). A closed shape whose last point lies on its first
    /// lists that point once.
    pub fn point_positions(&self) -> Vec<(PointRef, Vec2)> {
        self.shape_transforms()
            .into_iter()
            .flat_map(|(shape, t)| {
                let Some(s) = self.point_shape(shape) else {
                    return Vec::new();
                };
                let pts = s.path_at(self.current_frame() as f32).on_curve_points();
                let mut seen: Vec<Vec2> = Vec::new();
                pts.iter()
                    .enumerate()
                    .filter(|(_, p)| {
                        let dup = seen.contains(p);
                        seen.push(**p);
                        !dup
                    })
                    .map(|(point, p)| {
                        let r = PointRef {
                            layer: shape.0,
                            shape: shape.1,
                            point,
                        };
                        (r, t.transform_point2(*p))
                    })
                    .collect()
            })
            .collect()
    }

    /// The nearest point within `tolerance` of `p` (document space).
    pub fn point_at(&self, p: Vec2, tolerance: f32) -> Option<PointRef> {
        self.point_positions()
            .into_iter()
            .map(|(r, q)| (r, q.distance(p)))
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(r, _)| r)
    }

    /// Click on a point: replace the selection, or with Ctrl or Shift add it
    /// (Ctrl-clicking a selected point removes it).
    pub fn select_point(&mut self, point: PointRef, mode: SelectMode) {
        self.points_pivot = None;
        match mode {
            SelectMode::Replace => self.points = BTreeSet::from([point]),
            SelectMode::Toggle if self.points.contains(&point) => {
                self.points.remove(&point);
            }
            SelectMode::Toggle | SelectMode::Range => {
                self.points.insert(point);
            }
        }
    }

    /// Box selection: the points inside the rectangle `a`-`b`. With `add`,
    /// they join the current selection.
    pub fn select_points_in(&mut self, a: Vec2, b: Vec2, add: bool) {
        let (lo, hi) = (a.min(b), a.max(b));
        self.points_pivot = None;
        if !add {
            self.points.clear();
        }
        for (r, p) in self.point_positions() {
            if p.cmpge(lo).all() && p.cmple(hi).all() {
                self.points.insert(r);
            }
        }
    }

    pub fn select_all_points(&mut self) {
        self.points_pivot = None;
        self.points = self.point_positions().into_iter().map(|(r, _)| r).collect();
    }

    /// Start dragging the selected points: records the undo step. Returns
    /// false when no points are selected.
    pub fn begin_point_drag(&mut self) -> bool {
        self.prune_points();
        if self.points.is_empty() {
            return false;
        }
        let n = self.points.len();
        self.checkpoint(if n == 1 { "Move point" } else { "Move points" });
        true
    }

    /// Move the selected points by `delta`, in document space. Call
    /// [`begin_point_drag`](Self::begin_point_drag) once first.
    pub fn drag_points(&mut self, delta: Vec2) {
        let transforms: BTreeMap<(LayerId, usize), Affine2> = self.shape_transforms().into_iter().collect();
        let frame = self.current_frame();
        let keying = self.keying();
        for (shape, points) in self.points_by_shape() {
            // the drag in the shape's own space
            let Some(t) = transforms.get(&shape) else { continue };
            let local = t.matrix2.inverse() * delta;
            if !local.is_finite() {
                continue;
            }
            // in an action, every change is a key in it
            let in_action = self.in_edited_action(shape.0);
            if let Some(s) = self.point_shape_mut(shape) {
                // auto-key: a shape with point keys gets a key on this frame
                let mut path = s.path_at(frame as f32).into_owned();
                path.move_points(&points, local);
                if in_action {
                    if s.path_keys.is_empty() {
                        // the action starts from the drawing as it is
                        s.key_path(0, s.path.clone(), keying.interp);
                    }
                    s.key_path(frame, path, keying.interp);
                } else {
                    s.set_path_at(frame, path, keying);
                }
            }
        }
        self.dirty = true;
    }

    /// Delete the selected points, and any shape left with too few.
    /// Returns whether anything was deleted.
    pub fn delete_points(&mut self) -> bool {
        self.remove_chosen_points("Delete")
    }

    /// Take the selected points out, as an undo step named for what the user
    /// did (`verb`: Delete or Cut).
    pub(super) fn remove_chosen_points(&mut self, verb: &str) -> bool {
        self.prune_points();
        if self.points.is_empty() {
            return false;
        }
        let n = self.points.len();
        self.checkpoint(&format!("{verb} point{}", plural(n)));
        // later shapes first, so removing one does not renumber the rest
        for (shape, points) in self.points_by_shape().into_iter().rev() {
            let Some(shapes) = self.layer_shapes_mut(shape.0) else {
                continue;
            };
            if let Some(s) = shapes.get_mut(shape.1)
                && !s.remove_points(&points)
            {
                shapes.remove(shape.1);
            }
        }
        self.points.clear();
        self.points_pivot = None;
        self.set_status(StatusKind::Info, format!("Deleted {n} point{}", plural(n)));
        true
    }

    /// Forget selected points that no longer exist (after undo, say).
    pub(super) fn prune_points(&mut self) {
        let layer = self.point_layer();
        let points = std::mem::take(&mut self.points);
        let before = points.len();
        self.points = points
            .into_iter()
            .filter(|r| {
                Some(r.layer) == layer
                    && self
                        .point_shape((r.layer, r.shape))
                        .is_some_and(|s| r.point < s.path.on_curve_count())
            })
            .collect();
        if self.points.len() != before {
            self.points_pivot = None;
        }
    }

    fn points_by_shape(&self) -> BTreeMap<(LayerId, usize), BTreeSet<usize>> {
        let mut out: BTreeMap<(LayerId, usize), BTreeSet<usize>> = BTreeMap::new();
        for r in &self.points {
            out.entry((r.layer, r.shape)).or_default().insert(r.point);
        }
        out
    }

    /// Where each shape of the point layer is drawn at the current frame:
    /// shape-space to document-space transforms.
    pub(super) fn shape_transforms(&self) -> Vec<((LayerId, usize), Affine2)> {
        let Some(layer) = self.point_layer() else {
            return Vec::new();
        };
        // hidden layers are not evaluated, so their points are not shown
        evaluate(&self.doc, self.current_frame() as f32)
            .into_iter()
            .filter(|item| item.layer == layer && self.editable(item))
            .map(|item| ((layer, item.index), item.transform))
            .collect()
    }

    pub(super) fn point_shape(&self, (layer, index): (LayerId, usize)) -> Option<&fanimate_core::Shape> {
        self.layer_shapes(layer)?.get(index)
    }

    pub(super) fn point_shape_mut(
        &mut self,
        (layer, index): (LayerId, usize),
    ) -> Option<&mut fanimate_core::Shape> {
        self.layer_shapes_mut(layer)?.get_mut(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Tool};
    use fanimate_core::LayerKind;
    use fanimate_core::{AnimTransform, Shape};

    /// An editor whose selected layer holds a 100x100 rectangle at the
    /// origin, its layer moved by (10, 0).
    fn editor() -> (Editor, LayerId) {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.doc.find_mut(id).unwrap().transform = AnimTransform::at(Vec2::new(10.0, 0.0));
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::new(10.0, 0.0),
            Vec2::new(110.0, 100.0),
            false,
            false,
            1.0,
        );
        ed.tool = Tool::SelectPoints;
        (ed, id)
    }

    fn shape(ed: &Editor, id: LayerId) -> Option<Shape> {
        match &ed.doc.find(id).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.first().cloned(),
            _ => None,
        }
    }

    #[test]
    fn points_are_found_where_they_are_drawn() {
        let (ed, id) = editor();
        assert_eq!(ed.point_positions().len(), 4);
        let r = ed.point_at(Vec2::new(111.0, 1.0), 3.0).unwrap();
        assert_eq!(
            r,
            PointRef {
                layer: id,
                shape: 0,
                point: 1
            }
        );
        assert!(ed.point_at(Vec2::new(60.0, 50.0), 3.0).is_none());
    }

    #[test]
    fn dragging_points_is_one_undo_step() {
        let (mut ed, id) = editor();
        let r = ed.point_at(Vec2::new(110.0, 0.0), 1.0).unwrap();
        ed.select_point(r, SelectMode::Replace);
        assert!(ed.begin_point_drag());
        ed.drag_points(Vec2::new(5.0, 0.0));
        ed.drag_points(Vec2::new(5.0, 5.0));
        let pts = shape(&ed, id).unwrap().path.on_curve_points();
        // in the layer's space, which is moved by (10, 0)
        assert_eq!(pts[1], Vec2::new(110.0, 5.0));
        assert_eq!(pts[0], Vec2::ZERO);
        ed.run(Action::Undo);
        assert_eq!(
            shape(&ed, id).unwrap().path.on_curve_points()[1],
            Vec2::new(100.0, 0.0)
        );
    }

    #[test]
    fn box_selection_picks_points_inside() {
        let (mut ed, _) = editor();
        ed.select_points_in(Vec2::new(0.0, -5.0), Vec2::new(200.0, 5.0), false);
        assert_eq!(ed.points.len(), 2);
        ed.select_points_in(Vec2::new(0.0, 95.0), Vec2::new(50.0, 105.0), true);
        assert_eq!(ed.points.len(), 3);
        ed.select_points_in(Vec2::new(500.0, 500.0), Vec2::new(600.0, 600.0), false);
        assert!(ed.points.is_empty());
    }

    #[test]
    fn delete_removes_points_while_a_point_tool_is_active() {
        let (mut ed, id) = editor();
        ed.select_all_points();
        let r = *ed.points.first().unwrap();
        ed.points = BTreeSet::from([r]);
        ed.run(Action::Delete);
        assert_eq!(shape(&ed, id).unwrap().path.on_curve_count(), 3);
        // the layer itself is still there
        assert!(ed.doc.find(id).is_some());
        // deleting every point deletes the shape
        ed.select_all_points();
        ed.run(Action::Delete);
        assert!(shape(&ed, id).is_none());
        ed.run(Action::Undo);
        assert_eq!(shape(&ed, id).unwrap().path.on_curve_count(), 3);
    }

    #[test]
    fn select_all_and_none_act_on_points_with_a_point_tool() {
        let (mut ed, id) = editor();
        ed.run(Action::SelectAll);
        assert_eq!(ed.points.len(), 4);
        assert_eq!(ed.selection, [id]);
        ed.run(Action::SelectNone);
        assert!(ed.points.is_empty());
        assert_eq!(ed.selection, [id]);
    }

    #[test]
    fn moving_points_on_an_animated_layer_sets_point_keys() {
        let (mut ed, id) = editor();
        ed.run(Action::AddKey);
        ed.go_to(10);
        let r = ed.point_at(Vec2::new(110.0, 0.0), 1.0).unwrap();
        ed.select_point(r, SelectMode::Replace);
        assert!(ed.begin_point_drag());
        ed.drag_points(Vec2::new(0.0, 20.0));
        let s = shape(&ed, id).unwrap();
        assert_eq!(s.path_keys.len(), 2);
        // the shape's own path, and so frame 0, is unchanged
        assert_eq!(s.path.on_curve_points()[1], Vec2::new(100.0, 0.0));
        assert_eq!(s.path_at(10.0).on_curve_points()[1], Vec2::new(100.0, 20.0));
        // points are shown where they are on the frame
        assert!(ed.point_at(Vec2::new(110.0, 20.0), 1.0).is_some());
        ed.go_to(5);
        assert!(ed.point_at(Vec2::new(110.0, 10.0), 1.0).is_some());
        assert_eq!(ed.key_frames(id), [0, 10]);
    }

    #[test]
    fn stale_points_are_dropped_after_undo() {
        let (mut ed, _) = editor();
        ed.select_all_points();
        ed.run(Action::Undo); // removes the rectangle
        assert!(!ed.begin_point_drag());
        assert!(ed.points.is_empty());
    }
}
