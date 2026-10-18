//! Selecting whole shapes, and editing a rectangle's corners.
//!
//! The point tools work on points, but animators think in shapes: clicking
//! a shape's fill or stroke selects all of its points, and a selection box
//! picks every shape it touches. A rectangle drawn with the Rectangle tool
//! keeps its corner radius editable, until its points are edited by hand.

use std::collections::BTreeSet;

use fanimate_core::{Affine2, LayerId, LayerKind, RectParams, Vec2, evaluate};

use super::points::PointRef;
use super::{Editor, SelectMode, StatusKind, plural};

/// A rectangle whose corners can be rounded: where it is and what it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadiusTarget {
    pub layer: LayerId,
    /// The shape's index in the layer.
    pub index: usize,
    pub rect: RectParams,
    /// The layer's space to document space.
    pub world: Affine2,
}

impl RadiusTarget {
    /// Corner `0..4` (top left, top right, bottom right, bottom left) in the
    /// layer's space.
    pub fn corner(&self, which: usize) -> Vec2 {
        let (min, max) = (self.rect.min(), self.rect.min() + self.rect.size());
        match which % 4 {
            0 => min,
            1 => Vec2::new(max.x, min.y),
            2 => max,
            _ => Vec2::new(min.x, max.y),
        }
    }

    /// Which way is into the rectangle from corner `which`, as signs.
    fn inward(which: usize) -> Vec2 {
        match which % 4 {
            0 => Vec2::new(1.0, 1.0),
            1 => Vec2::new(-1.0, 1.0),
            2 => Vec2::new(-1.0, -1.0),
            _ => Vec2::new(1.0, -1.0),
        }
    }

    /// The radius a pointer (document space) asks for at corner `which`:
    /// how far along the diagonal from the corner it is, brought into range.
    pub fn radius_at(&self, which: usize, pointer: Vec2) -> f32 {
        let inv = self.world.inverse();
        if !inv.is_finite() || !pointer.is_finite() {
            return self.rect.radius;
        }
        let from_corner = (inv.transform_point2(pointer) - self.corner(which)) * Self::inward(which);
        ((from_corner.x + from_corner.y) / 2.0).clamp(0.0, self.rect.max_radius())
    }

    /// Where the corner's radius handle is, in document space: along the
    /// diagonal, `radius` in from the corner.
    pub fn handle(&self, which: usize, radius: f32) -> Vec2 {
        self.world
            .transform_point2(self.corner(which) + Self::inward(which) * radius)
    }
}

/// Whether the segment `a`-`b` touches the rectangle `lo`-`hi`.
fn segment_touches(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> bool {
    // slab clipping: the part of the segment inside each pair of sides
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    let d = b - a;
    for (p, q, l, h) in [(a.x, d.x, lo.x, hi.x), (a.y, d.y, lo.y, hi.y)] {
        if q.abs() < 1e-9 {
            if p < l || p > h {
                return false;
            }
        } else {
            let (mut u, mut v) = ((l - p) / q, (h - p) / q);
            if u > v {
                std::mem::swap(&mut u, &mut v);
            }
            t0 = t0.max(u);
            t1 = t1.min(v);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

impl Editor {
    /// A shape's outline in document space: one point list per subpath, and
    /// whether it is closed. Empty if the shape is not drawn.
    pub fn shape_outline(&self, layer: LayerId, index: usize) -> Vec<(Vec<Vec2>, bool)> {
        evaluate(&self.doc, self.frame)
            .iter()
            .find(|i| i.layer == layer && i.index == index && self.editable(i))
            .map(|item| {
                item.shape
                    .path
                    .polylines(0.25)
                    .into_iter()
                    .map(|(points, closed)| {
                        (
                            points
                                .into_iter()
                                .map(|p| item.transform.transform_point2(p))
                                .collect(),
                            closed,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The outlines, in document space, of every shape that has a selected
    /// point: how the point tools show which shapes are chosen. One
    /// evaluation for all of them.
    pub fn selected_shape_outlines(&self) -> Vec<Vec<(Vec<Vec2>, bool)>> {
        let chosen: BTreeSet<(LayerId, usize)> = self.points.iter().map(|r| (r.layer, r.shape)).collect();
        if chosen.is_empty() {
            return Vec::new();
        }
        evaluate(&self.doc, self.frame)
            .iter()
            .filter(|i| chosen.contains(&(i.layer, i.index)) && self.editable(i))
            .map(|item| {
                item.shape
                    .path
                    .polylines(0.25)
                    .into_iter()
                    .map(|(points, closed)| {
                        (
                            points
                                .into_iter()
                                .map(|p| item.transform.transform_point2(p))
                                .collect(),
                            closed,
                        )
                    })
                    .collect()
            })
            .collect()
    }

    /// The topmost shape the tools can edit under `p`, within `tolerance`.
    pub fn shape_to_select_at(&self, p: Vec2, tolerance: f32) -> Option<(LayerId, usize)> {
        self.editable_shape_at(p, tolerance)
    }

    /// Every point of one shape, as the point tools name them.
    fn shape_point_refs(&self, layer: LayerId, index: usize) -> BTreeSet<PointRef> {
        self.point_positions()
            .into_iter()
            .map(|(r, _)| r)
            .filter(|r| r.layer == layer && r.shape == index)
            .collect()
    }

    /// Whether every point of the shape is chosen.
    pub fn shape_selected(&self, layer: LayerId, index: usize) -> bool {
        let refs = self.shape_point_refs(layer, index);
        !refs.is_empty() && refs.iter().all(|r| self.points.contains(r))
    }

    /// Click on a shape: select all of its points. In another layer, that
    /// layer is selected first. With Ctrl the shape is added, or taken out
    /// if all of it was selected; with Shift it is added.
    pub fn select_shape(&mut self, layer: LayerId, index: usize, mode: SelectMode) {
        if self.primary() != Some(layer) {
            self.points.clear();
            self.select(layer, SelectMode::Replace);
        }
        let refs = self.shape_point_refs(layer, index);
        match mode {
            SelectMode::Replace => self.points = refs,
            SelectMode::Toggle if refs.iter().all(|r| self.points.contains(r)) => {
                for r in &refs {
                    self.points.remove(r);
                }
            }
            SelectMode::Toggle | SelectMode::Range => self.points.extend(refs),
        }
    }

    /// Box selection by shape: every shape of the point layer the rectangle
    /// `a`-`b` (document space) touches or lies inside, whole. With `add`,
    /// they join the current selection.
    pub fn select_shapes_touching(&mut self, a: Vec2, b: Vec2, add: bool) {
        let (lo, hi) = (a.min(b), a.max(b));
        if !add {
            self.points.clear();
        }
        let Some(layer) = self.point_layer() else { return };
        let centre = (lo + hi) / 2.0;
        let mut touched = Vec::new();
        for item in evaluate(&self.doc, self.current_frame() as f32)
            .iter()
            .filter(|i| i.layer == layer && self.editable(i))
        {
            let lines = item.shape.path.polylines(0.25);
            let touches = lines.iter().any(|(points, closed)| {
                let doc: Vec<Vec2> = points
                    .iter()
                    .map(|p| item.transform.transform_point2(*p))
                    .collect();
                doc.iter().any(|p| p.cmpge(lo).all() && p.cmple(hi).all())
                    || doc.windows(2).any(|w| segment_touches(w[0], w[1], lo, hi))
                    || (*closed && doc.len() > 1 && segment_touches(doc[doc.len() - 1], doc[0], lo, hi))
            });
            // a box drawn wholly inside a filled shape is on it too
            let inside = item.shape.style.fill.is_some()
                && item.transform.inverse().is_finite()
                && item
                    .shape
                    .contains(item.transform.inverse().transform_point2(centre), 0.0);
            if touches || inside {
                touched.push(item.index);
            }
        }
        for index in touched {
            self.points.extend(self.shape_point_refs(layer, index));
        }
    }

    /// The rectangle whose corners the radius handles and the Properties
    /// panel edit: the one shape selected, if it is still a rectangle. That
    /// is the selected points when they make up exactly one shape, else the
    /// only shape of the only selected layer.
    pub fn radius_target(&self) -> Option<RadiusTarget> {
        let layer = self.primary()?;
        let LayerKind::Vector { shapes } = &self.doc.find(layer)?.kind else {
            return None;
        };
        let index = if self.points.is_empty() {
            (self.selection.len() == 1 && shapes.len() == 1).then_some(0)?
        } else {
            let first = self.points.first()?;
            let one_shape = self
                .points
                .iter()
                .all(|r| r.layer == layer && r.shape == first.shape);
            (one_shape && self.shape_point_refs(layer, first.shape) == self.points).then_some(first.shape)?
        };
        let shape = shapes.get(index)?;
        // a shape that follows a bone is drawn somewhere else than its points
        let rect = shape.parametric_rect().filter(|_| shape.bone.is_none())?;
        Some(RadiusTarget {
            layer,
            index,
            rect,
            world: self.world(layer),
        })
    }

    /// Start changing the corner radius: one undo step. Returns false when
    /// there is no rectangle to change.
    pub fn begin_radius_edit(&mut self) -> bool {
        if self.radius_target().is_none() {
            return false;
        }
        self.checkpoint("Corner radius");
        true
    }

    /// Set the corner radius of the selected rectangle. Call
    /// [`begin_radius_edit`](Self::begin_radius_edit) once first. Returns
    /// whether there was a rectangle.
    pub fn set_corner_radius(&mut self, radius: f32) -> bool {
        let Some(target) = self.radius_target() else {
            return false;
        };
        let Some(shape) = self.point_shape_mut((target.layer, target.index)) else {
            return false;
        };
        let changed = shape.set_rect_radius(radius);
        if changed {
            // the point count changes with the corners, so points chosen
            // before are not the same points now: the whole shape stays chosen
            if !self.points.is_empty() {
                self.points = self.shape_point_refs(target.layer, target.index);
            }
            self.dirty = true;
        }
        changed
    }

    /// The selected layers and everything inside them.
    fn selected_with_children(&self) -> Vec<LayerId> {
        let roots = self.selected_roots();
        let mut ids = Vec::new();
        self.doc.walk(|l| {
            if roots.iter().any(|r| self.doc.is_within(l.id, *r)) {
                ids.push(l.id);
            }
        });
        ids
    }

    /// How many rectangles in the selected layers can be turned into plain
    /// paths.
    pub fn expandable_shapes(&self) -> usize {
        self.selected_with_children()
            .into_iter()
            .filter_map(|id| match &self.doc.find(id)?.kind {
                LayerKind::Vector { shapes } => {
                    Some(shapes.iter().filter(|s| s.parametric_rect().is_some()).count())
                }
                _ => None,
            })
            .sum()
    }

    /// Expand Shape: turn the selected layers' rectangles into plain paths,
    /// whose corners and sides are then edited point by point.
    pub(super) fn expand_shape(&mut self) {
        let n = self.expandable_shapes();
        if n == 0 {
            self.set_status(StatusKind::Info, "No rectangle is selected");
            return;
        }
        self.checkpoint("Expand shape");
        for id in self.selected_with_children() {
            if let Some(LayerKind::Vector { shapes }) = self.doc.find_mut(id).map(|l| &mut l.kind) {
                shapes.iter_mut().for_each(|s| {
                    s.expand_rect();
                });
            }
        }
        self.set_status(
            StatusKind::Info,
            format!("Expanded {n} rectangle{} into paths", plural(n)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Tool};

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 0.05
    }

    /// Two shapes in one layer: a rectangle from (0, 0) to (100, 60) and an
    /// ellipse centred at (300, 30).
    fn editor() -> (Editor, LayerId) {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::ZERO,
            Vec2::new(100.0, 60.0),
            false,
            false,
            1.0,
        );
        ed.add_drag_shape(
            Tool::Ellipse,
            Vec2::new(250.0, 0.0),
            Vec2::new(350.0, 60.0),
            false,
            false,
            1.0,
        );
        ed.tool = Tool::SelectPoints;
        (ed, id)
    }

    fn rect_target(ed: &Editor) -> RadiusTarget {
        ed.radius_target().expect("a rectangle")
    }

    #[test]
    fn clicking_a_shape_selects_all_its_points_and_nothing_else() {
        let (mut ed, id) = editor();
        let (layer, index) = ed.shape_to_select_at(Vec2::new(50.0, 30.0), 2.0).unwrap();
        assert_eq!((layer, index), (id, 0));
        ed.select_shape(layer, index, SelectMode::Replace);
        assert_eq!(ed.points.len(), 4);
        assert!(ed.points.iter().all(|r| r.shape == 0));
        // Shift adds the other shape, Ctrl takes it out again
        ed.select_shape(id, 1, SelectMode::Range);
        assert_eq!(ed.points.len(), 4 + 4);
        ed.select_shape(id, 1, SelectMode::Toggle);
        assert_eq!(ed.points.len(), 4);
        ed.select_shape(id, 1, SelectMode::Replace);
        assert!(ed.points.iter().all(|r| r.shape == 1));
    }

    #[test]
    fn a_shape_is_selected_only_when_all_its_points_are() {
        let (mut ed, id) = editor();
        assert!(!ed.shape_selected(id, 0));
        ed.select_shape(id, 0, SelectMode::Replace);
        assert!(ed.shape_selected(id, 0) && !ed.shape_selected(id, 1));
        let one = *ed.points.iter().next().unwrap();
        ed.select_point(one, SelectMode::Replace);
        assert!(!ed.shape_selected(id, 0));
    }

    #[test]
    fn clicking_a_shape_in_another_layer_selects_that_layer_first() {
        let (mut ed, first) = editor();
        ed.run(Action::NewVectorLayer);
        let second = ed.primary().unwrap();
        assert_ne!(first, second);
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::new(500.0, 0.0),
            Vec2::new(600.0, 50.0),
            false,
            false,
            1.0,
        );
        let (layer, index) = ed.shape_to_select_at(Vec2::new(50.0, 30.0), 2.0).unwrap();
        assert_eq!(layer, first);
        ed.select_shape(layer, index, SelectMode::Replace);
        assert_eq!(ed.primary(), Some(first));
        assert_eq!(ed.points.len(), 4);
    }

    #[test]
    fn a_box_picks_the_shapes_it_touches_whole() {
        let (mut ed, id) = editor();
        // across the right edge of the rectangle only
        ed.select_shapes_touching(Vec2::new(90.0, 20.0), Vec2::new(150.0, 40.0), false);
        assert_eq!(ed.points.len(), 4);
        assert!(ed.points.iter().all(|r| r.layer == id && r.shape == 0));
        // across nothing
        ed.select_shapes_touching(Vec2::new(150.0, 100.0), Vec2::new(200.0, 150.0), false);
        assert!(ed.points.is_empty());
        // across both; and adding to what is chosen
        ed.select_shapes_touching(Vec2::new(-10.0, -10.0), Vec2::new(400.0, 100.0), false);
        assert_eq!(ed.points.len(), 8);
        ed.select_shapes_touching(Vec2::new(90.0, 20.0), Vec2::new(150.0, 40.0), false);
        ed.select_shapes_touching(Vec2::new(290.0, 20.0), Vec2::new(310.0, 40.0), true);
        assert_eq!(ed.points.len(), 8);
        // a small box wholly inside a filled shape is on it
        ed.select_shapes_touching(Vec2::new(40.0, 20.0), Vec2::new(60.0, 40.0), false);
        assert_eq!(ed.points.len(), 4);
        assert!(ed.points.iter().all(|r| r.shape == 0));
    }

    #[test]
    fn a_segment_touches_a_box_by_crossing_it() {
        let (lo, hi) = (Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0));
        assert!(segment_touches(
            Vec2::new(-5.0, 5.0),
            Vec2::new(15.0, 5.0),
            lo,
            hi
        ));
        assert!(segment_touches(Vec2::new(5.0, 5.0), Vec2::new(6.0, 6.0), lo, hi));
        assert!(!segment_touches(
            Vec2::new(-5.0, 11.0),
            Vec2::new(15.0, 11.0),
            lo,
            hi
        ));
        assert!(!segment_touches(
            Vec2::new(-5.0, 5.0),
            Vec2::new(-1.0, 5.0),
            lo,
            hi
        ));
        assert!(segment_touches(
            Vec2::new(-5.0, 15.0),
            Vec2::new(15.0, -5.0),
            lo,
            hi
        ));
    }

    #[test]
    fn shapes_with_chosen_points_have_outlines_to_show() {
        let (mut ed, id) = editor();
        assert!(ed.selected_shape_outlines().is_empty());
        ed.select_shape(id, 1, SelectMode::Replace);
        assert_eq!(ed.selected_shape_outlines().len(), 1);
        ed.select_shape(id, 0, SelectMode::Range);
        assert_eq!(ed.selected_shape_outlines().len(), 2);
    }

    #[test]
    fn a_shape_has_an_outline_in_document_space() {
        let (ed, id) = editor();
        let outline = ed.shape_outline(id, 0);
        assert_eq!(outline.len(), 1);
        assert!(outline[0].1, "closed");
        assert!(outline[0].0.iter().any(|p| close(*p, Vec2::new(100.0, 60.0))));
        assert!(ed.shape_outline(id, 9).is_empty());
    }

    #[test]
    fn the_rectangle_tool_makes_a_rectangle_with_an_editable_radius() {
        let (mut ed, id) = editor();
        // nothing chosen: the layer holds two shapes, so no single rectangle
        assert!(ed.radius_target().is_none());
        ed.select_shape(id, 0, SelectMode::Replace);
        let t = rect_target(&ed);
        assert_eq!((t.index, t.rect.radius), (0, 0.0));
        assert_eq!((t.rect.width, t.rect.height), (100.0, 60.0));
        assert!(ed.begin_radius_edit());
        assert!(ed.set_corner_radius(12.0));
        assert_eq!(rect_target(&ed).rect.radius, 12.0);
        // the points chosen are the new shape's: all eight
        assert_eq!(ed.points.len(), 8);
        // a radius too large is brought down to half the shorter side
        assert!(ed.set_corner_radius(1000.0));
        assert_eq!(rect_target(&ed).rect.radius, 30.0);
        // one undo step for the whole drag
        ed.run(Action::Undo);
        // the points chosen belonged to the rounder shape: none are kept
        assert!(ed.points.is_empty());
        ed.select_shape(id, 0, SelectMode::Replace);
        assert_eq!(ed.radius_target().map(|t| t.rect.radius), Some(0.0));
        // the ellipse is no rectangle
        ed.select_shape(id, 1, SelectMode::Replace);
        assert!(ed.radius_target().is_none());
    }

    #[test]
    fn a_whole_layer_with_one_rectangle_has_a_radius_without_points_chosen() {
        let mut ed = Editor::untitled();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::ZERO,
            Vec2::new(100.0, 60.0),
            false,
            false,
            1.0,
        );
        assert!(ed.points.is_empty());
        assert!(ed.radius_target().is_some());
        // one point alone is not the rectangle
        ed.tool = Tool::SelectPoints;
        let p = ed.point_positions()[0].0;
        ed.select_point(p, SelectMode::Replace);
        assert!(ed.radius_target().is_none());
    }

    #[test]
    fn editing_a_point_by_hand_ends_the_rectangle() {
        let (mut ed, id) = editor();
        ed.select_shape(id, 0, SelectMode::Replace);
        assert!(ed.radius_target().is_some());
        let p = ed.point_positions()[0].0;
        ed.select_point(p, SelectMode::Replace);
        assert!(ed.begin_point_drag());
        ed.drag_points(Vec2::new(5.0, 3.0));
        ed.select_shape(id, 0, SelectMode::Replace);
        assert!(ed.radius_target().is_none(), "the path no longer matches");
        assert!(!ed.set_corner_radius(5.0));
        assert_eq!(ed.expandable_shapes(), 0);
    }

    #[test]
    fn a_rectangle_moved_with_its_layer_keeps_its_radius() {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::ZERO,
            Vec2::new(100.0, 60.0),
            false,
            false,
            1.0,
        );
        let mut t = ed.layer_transform(id).unwrap();
        t.position = Vec2::new(40.0, 10.0);
        t.rotation = 0.5;
        ed.set_layer_transform(id, t);
        let target = rect_target(&ed);
        // the radius handle is found along the diagonal in document space
        let h = target.handle(0, 10.0);
        assert!(close(h, ed.world(id).transform_point2(Vec2::new(10.0, 10.0))));
        assert!((target.radius_at(0, h) - 10.0).abs() < 0.05);
        // pulled beyond what fits: half the shorter side
        assert_eq!(
            target.radius_at(0, ed.world(id).transform_point2(Vec2::new(90.0, 50.0))),
            30.0
        );
    }

    #[test]
    fn expand_shape_turns_rectangles_into_plain_paths() {
        let (mut ed, id) = editor();
        ed.select(id, SelectMode::Replace);
        assert_eq!(ed.expandable_shapes(), 1);
        assert!(ed.can_run(Action::ExpandShape));
        ed.run(Action::ExpandShape);
        assert_eq!(ed.expandable_shapes(), 0);
        assert!(!ed.can_run(Action::ExpandShape));
        let LayerKind::Vector { shapes } = &ed.doc.find(id).unwrap().kind else {
            unreachable!()
        };
        assert!(shapes[0].rect.is_none() && shapes[0].path.on_curve_count() == 4);
        // the drawing is the same, and the step can be undone
        ed.run(Action::Undo);
        assert_eq!(ed.expandable_shapes(), 1);
        // nothing to expand says so instead of silently adding an undo step
        ed.run(Action::Redo);
        let steps = ed.history.undo_label().map(str::to_owned);
        ed.run(Action::ExpandShape);
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps);
    }

    #[test]
    fn rectangles_survive_copy_and_paste_and_undo_of_a_draw() {
        let mut ed = Editor::untitled();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::ZERO,
            Vec2::new(100.0, 60.0),
            false,
            false,
            1.0,
        );
        ed.begin_radius_edit();
        ed.set_corner_radius(8.0);
        let text = ed.copy().expect("copied");
        ed.paste(Some(&text));
        let pasted = ed.primary().unwrap();
        assert_eq!(
            ed.radius_target().map(|t| (t.layer, t.rect.radius)),
            Some((pasted, 8.0))
        );
    }
}
