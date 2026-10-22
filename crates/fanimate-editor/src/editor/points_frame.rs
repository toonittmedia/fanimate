//! Turning and scaling chosen points as a group, and the pivot picker.
//!
//! With the Transform tool and two or more points chosen, the selection
//! frame is the box around those points (as in Moho) instead of the layer's.
//! It has the same handles as a layer's: scale squares, the rotate button and
//! zone, and a pivot the points turn about. The pivot is the points'
//! centroid until it is moved; it is a document-space point kept only while
//! the choice stays the same, never saved.
//!
//! The maths runs in document space, so a layer that is itself turned or
//! scaled gives the same result as a plain one: each shape's chosen points
//! are mapped through the shape's own transform and back, and written with
//! [`Shape::set_path_at`](fanimate_core::Shape::set_path_at), so the keying
//! rule holds (a later frame keys the shape and seeds the first frame).
//!
//! One gesture is one undo step ([`Editor::begin_handle`] checkpoints once).
//! Moving the pivot is not an edit of the document and records nothing.

use std::collections::{BTreeMap, BTreeSet};

use fanimate_core::{Affine2, LayerId, Path, PathCmd, Vec2};

use super::handles::{axis, keep_open, wrap_pi};
use super::{Editor, Handle, HandleMods, SelectionFrame, StatusKind, plural};
use crate::Tool;

/// One shape holding chosen points, as it was when the drag began.
struct Start {
    key: (LayerId, usize),
    /// The shape's path at the current frame (interpolated between point keys).
    path: Path,
    /// Shape space to document space.
    world: Affine2,
    /// The numbers of the chosen points.
    points: BTreeSet<usize>,
}

pub(super) struct PointsDrag {
    handle: Handle,
    shapes: Vec<Start>,
    /// The box around the points, in document space.
    lo: Vec2,
    hi: Vec2,
    /// Rotate: the pivot, and where the pointer started round it.
    centre: Vec2,
    start_angle: f32,
    /// The pivot as it was, if it had been moved, so a scale carries it along.
    pivot: Option<Vec2>,
}

/// The anchor (corners, middles of the sides, centre) of the box `lo`-`hi`
/// that lies nearest to `at` once the box is mapped to document space by
/// `to_doc`, within `within`. Returned in the box's own space.
fn nearest_anchor(lo: Vec2, hi: Vec2, to_doc: Affine2, at: Vec2, within: f32) -> Option<Vec2> {
    let mut best: Option<(f32, Vec2)> = None;
    for fy in [0.0, 0.5, 1.0] {
        for fx in [0.0, 0.5, 1.0] {
            let a = lo + (hi - lo) * Vec2::new(fx, fy);
            let d = to_doc.transform_point2(a).distance(at);
            if d <= within && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, a));
            }
        }
    }
    best.map(|(_, a)| a)
}

/// `points` plus their twins: a closed subpath whose last point lies on its
/// first (as circles do) has two numbers for one visible point.
fn with_twins(path: &Path, points: &BTreeSet<usize>) -> BTreeSet<usize> {
    let pts = path.on_curve_points();
    let mut out = points.clone();
    let mut k = 0;
    let mut first = None;
    for cmd in &path.cmds {
        match cmd {
            PathCmd::MoveTo(_) => {
                first = Some(k);
                k += 1;
            }
            PathCmd::LineTo(_) | PathCmd::CubicTo(..) => k += 1,
            PathCmd::Close => {
                if let Some(first) = first.take()
                    && k > 0
                {
                    let last = k - 1;
                    if last > first && pts[first] == pts[last] {
                        if points.contains(&first) {
                            out.insert(last);
                        }
                        if points.contains(&last) {
                            out.insert(first);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Map the given points through `t`, and their curve handles with them (the
/// one arriving at the point and the one leaving it), so only the segments
/// they touch bend.
fn transform_chosen(path: &mut Path, points: &BTreeSet<usize>, t: Affine2) {
    let points = with_twins(path, points);
    let at: Vec<usize> = (0..path.cmds.len())
        .filter(|&i| !matches!(path.cmds[i], PathCmd::Close))
        .collect();
    for &k in &points {
        let Some(&i) = at.get(k) else { continue };
        match &mut path.cmds[i] {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => *p = t.transform_point2(*p),
            PathCmd::CubicTo(_, c2, p) => {
                *c2 = t.transform_point2(*c2);
                *p = t.transform_point2(*p);
            }
            PathCmd::Close => {}
        }
        // the handle leaving this point belongs to it too
        if let Some(PathCmd::CubicTo(c1, _, _)) = path.cmds.get_mut(i + 1) {
            *c1 = t.transform_point2(*c1);
        }
    }
}

impl Editor {
    /// Whether the Transform tool works on chosen points now: it does when
    /// two or more are chosen, and otherwise on layers.
    pub(super) fn points_mode(&self) -> bool {
        self.tool == Tool::TransformLayer && self.points.len() > 1
    }

    /// Where the chosen points are drawn now, in document space.
    fn chosen_positions(&self) -> Vec<Vec2> {
        self.point_positions()
            .into_iter()
            .filter(|(r, p)| self.points.contains(r) && p.is_finite())
            .map(|(_, p)| p)
            .collect()
    }

    /// The pivot the points turn about, when it has been moved and the
    /// choice is still the one it was placed for.
    fn live_points_pivot(&self) -> Option<Vec2> {
        self.points_pivot
            .filter(|p| p.is_finite() && self.points_pivot_for == self.points)
    }

    /// Put the pivot of the chosen points at `at` (document space).
    pub fn set_points_pivot(&mut self, at: Vec2) {
        if at.is_finite() {
            self.points_pivot = Some(at);
            self.points_pivot_for = self.points.clone();
        }
    }

    /// The frame around the chosen points, or `None` with fewer than two.
    /// Its pivot is the one moved to, else the points' centroid. The box is
    /// axis-aligned in document space.
    pub fn points_frame(&self) -> Option<SelectionFrame> {
        let pts = self.chosen_positions();
        if pts.len() < 2 {
            return None;
        }
        let layer = self.point_layer()?;
        let (lo, hi) = pts
            .iter()
            .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let centroid = pts.iter().copied().fold(Vec2::ZERO, |a, b| a + b) / pts.len() as f32;
        let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
        Some(SelectionFrame {
            corners,
            members: vec![corners],
            layers: vec![layer],
            pivot: Some(self.live_points_pivot().unwrap_or(centroid)),
            primary: layer,
            free_scale: true,
        })
    }

    /// Each shape with chosen points: where it is, and its path as it is now.
    fn points_start(&self) -> Vec<Start> {
        let transforms: BTreeMap<(LayerId, usize), Affine2> = self.shape_transforms().into_iter().collect();
        let at = self.current_frame() as f32;
        let mut by_shape: BTreeMap<(LayerId, usize), BTreeSet<usize>> = BTreeMap::new();
        for r in &self.points {
            by_shape.entry((r.layer, r.shape)).or_default().insert(r.point);
        }
        by_shape
            .into_iter()
            .filter_map(|(key, points)| {
                let world = *transforms.get(&key)?;
                if !world.inverse().is_finite() {
                    return None;
                }
                let shape = self.point_shape(key)?;
                let count = shape.path.on_curve_count();
                let points: BTreeSet<usize> = points.into_iter().filter(|p| *p < count).collect();
                if points.is_empty() {
                    return None;
                }
                Some(Start {
                    key,
                    path: shape.path_at(at).into_owned(),
                    world,
                    points,
                })
            })
            .collect()
    }

    /// Start dragging a handle of the points frame. One undo step, except for
    /// the pivot, which is not part of the document.
    pub(super) fn begin_points_handle(
        &mut self,
        handle: Handle,
        pointer: Vec2,
        frame: &SelectionFrame,
    ) -> bool {
        let shapes = self.points_start();
        if shapes.is_empty() {
            return false;
        }
        let n: usize = shapes.iter().map(|s| s.points.len()).sum();
        match handle {
            Handle::Scale { .. } => self.checkpoint("Scale points"),
            Handle::Rotate => self.checkpoint("Rotate points"),
            Handle::Pivot => {}
        }
        let centre = frame.pivot.unwrap_or_else(|| frame.center());
        self.handle_drag = None;
        self.points_drag = Some(PointsDrag {
            handle,
            shapes,
            lo: frame.corners[0],
            hi: frame.corners[2],
            centre,
            start_angle: (pointer - centre).to_angle(),
            pivot: self.live_points_pivot(),
        });
        self.handle_readout = None;
        self.set_status(StatusKind::Info, format!("Transforming {n} point{}", plural(n)));
        true
    }

    /// Move the handle to `pointer` (document space).
    pub(super) fn drag_points_handle(&mut self, pointer: Vec2, mods: HandleMods) {
        let Some(drag) = self.points_drag.take() else {
            return;
        };
        match drag.handle {
            Handle::Scale { x, y } => self.drag_points_scale(&drag, x, y, pointer, mods),
            Handle::Rotate => self.drag_points_rotate(&drag, pointer, mods),
            Handle::Pivot => {
                // an anchor of the box wins; else guides and shapes, else the pointer
                let at = nearest_anchor(drag.lo, drag.hi, Affine2::IDENTITY, pointer, mods.snap)
                    .unwrap_or_else(|| mods.snapped.unwrap_or(pointer));
                self.set_points_pivot(at);
            }
        }
        self.points_drag = Some(drag);
    }

    /// Write `map` (a change in document space) onto the chosen points.
    fn apply_points_map(&mut self, shapes: &[Start], map: Affine2) {
        let frame = self.current_frame();
        let keying = self.keying();
        for s in shapes {
            // the same change in the shape's own space
            let local = s.world.inverse() * map * s.world;
            if !local.is_finite() {
                continue;
            }
            let mut path = s.path.clone();
            transform_chosen(&mut path, &s.points, local);
            if !path.is_finite() {
                continue;
            }
            // in an action, every change is a key in it
            let in_action = self.in_edited_action(s.key.0);
            let Some(shape) = self.point_shape_mut(s.key) else {
                continue;
            };
            if in_action {
                if shape.path_keys.is_empty() {
                    // the action starts from the drawing as it is
                    shape.key_path(0, shape.path.clone(), keying.interp);
                }
                shape.key_path(frame, path, keying.interp);
            } else {
                shape.set_path_at(frame, path, keying);
            }
        }
        self.dirty = true;
    }

    fn drag_points_scale(&mut self, drag: &PointsDrag, x: i8, y: i8, pointer: Vec2, mods: HandleMods) {
        let (ax, rx) = axis(x, drag.lo.x, drag.hi.x, pointer.x, mods.alt);
        let (ay, ry) = axis(y, drag.lo.y, drag.hi.y, pointer.y, mods.alt);
        let (mut anchor, mut ratio) = (Vec2::new(ax, ay), Vec2::new(rx, ry));
        if mods.shift {
            // an edge sets the ratio; a corner takes the bigger of the two
            let r = match (x, y) {
                (0, _) => ratio.y,
                (_, 0) => ratio.x,
                _ if (ratio.x.abs() - 1.0).abs() >= (ratio.y.abs() - 1.0).abs() => ratio.x,
                _ => ratio.y,
            };
            ratio = Vec2::splat(r);
            // the side left alone keeps its middle
            if x == 0 {
                anchor.x = (drag.lo.x + drag.hi.x) / 2.0;
            }
            if y == 0 {
                anchor.y = (drag.lo.y + drag.hi.y) / 2.0;
            }
        }
        ratio = Vec2::new(keep_open(ratio.x), keep_open(ratio.y));
        let map = Affine2::from_translation(anchor)
            * Affine2::from_scale(ratio)
            * Affine2::from_translation(-anchor);
        self.apply_points_map(&drag.shapes, map);
        // a pivot that was moved keeps its place among the points
        if let Some(p) = drag.pivot {
            let carried = map.transform_point2(p);
            self.set_points_pivot(carried);
        }
        let size = (drag.hi - drag.lo) * ratio.abs();
        self.handle_readout = Some(format!("{:.0} × {:.0}", size.x, size.y));
    }

    fn drag_points_rotate(&mut self, drag: &PointsDrag, pointer: Vec2, mods: HandleMods) {
        let mut delta = wrap_pi((pointer - drag.centre).to_angle() - drag.start_angle);
        if mods.shift {
            let step = 15.0_f32.to_radians();
            delta = (delta / step).round() * step;
        }
        let map = Affine2::from_translation(drag.centre)
            * Affine2::from_angle(delta)
            * Affine2::from_translation(-drag.centre);
        self.apply_points_map(&drag.shapes, map);
        self.handle_readout = Some(format!("{:.0}°", delta.to_degrees()));
    }

    // ------------------------------------------------------------------
    // The pivot picker

    /// Arm the picker: the next click on the canvas places the pivot of the
    /// selected layer, or of the chosen points. It is one shot; Esc cancels.
    pub fn arm_pivot_pick(&mut self) {
        if self.primary().is_none() {
            return;
        }
        // the Transform tool is the one that shows pivots
        self.set_tool(Tool::TransformLayer);
        self.pivot_pick = true;
        self.set_status(StatusKind::Info, "Click to place the pivot, Esc to cancel");
    }

    pub fn cancel_pivot_pick(&mut self) {
        if self.pivot_pick {
            self.pivot_pick = false;
            self.status = None;
        }
    }

    /// The click of the pivot picker, at `doc_point` (document space). With
    /// `snap` the pivot snaps to the nearest of the box's nine anchors within
    /// [`pivot_snap`](Editor::pivot_snap); `free` turns that off. Chosen points
    /// get a pivot of their own (not an edit); otherwise the selected layer's
    /// pivot moves without moving the artwork, one undo step. The picker is
    /// disarmed either way. Returns whether a pivot was placed.
    pub fn set_pivot_at(&mut self, doc_point: Vec2, snap: bool, free: bool) -> bool {
        if !self.pivot_pick {
            return false;
        }
        self.pivot_pick = false;
        if !doc_point.is_finite() {
            return false;
        }
        let anchors = snap && !free;
        let within = self.pivot_snap;

        if self.points_mode()
            && let Some(frame) = self.points_frame()
        {
            let at = if anchors {
                nearest_anchor(
                    frame.corners[0],
                    frame.corners[2],
                    Affine2::IDENTITY,
                    doc_point,
                    within,
                )
            } else {
                None
            };
            self.set_points_pivot(at.unwrap_or(doc_point));
            self.status = None;
            return true;
        }

        let roots = self.selected_roots();
        let &[id] = roots.as_slice() else {
            self.set_status(StatusKind::Info, "Select one layer to place its pivot");
            return false;
        };
        let world = self.world(id);
        let inverse = world.inverse();
        if !inverse.is_finite() {
            return false;
        }
        let mut local = inverse.transform_point2(doc_point);
        if anchors
            && let Some(Some((lo, hi))) = self.layer_bounds(&[id]).first().copied()
            && let Some(a) = nearest_anchor(lo, hi, world, doc_point, within)
        {
            local = a;
        }
        if !local.is_finite() {
            return false;
        }
        self.checkpoint("Move pivot");
        let frame = self.frame;
        let clean = self
            .doc
            .find_mut(id)
            .is_some_and(|l| l.transform.set_pivot_keeping_look(local, frame));
        if clean {
            self.status = None;
        } else {
            self.set_status(
                StatusKind::Warning,
                "The rotation or scale is animated, so the layer only stays put on this frame",
            );
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, SelectMode};
    use fanimate_core::Shape;

    use super::super::PointRef;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 0.05
    }

    /// An editor on the Transform tool whose layer holds a 100 x 100
    /// rectangle from the origin. Its points are 0 top left, 1 top right,
    /// 2 bottom right and 3 bottom left.
    fn editor() -> (Editor, LayerId) {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 100.0),
            false,
            false,
            1.0,
        );
        ed.points.clear();
        ed.tool = Tool::TransformLayer;
        (ed, id)
    }

    fn choose(ed: &mut Editor, id: LayerId, which: &[usize]) {
        for (n, &point) in which.iter().enumerate() {
            let r = PointRef {
                layer: id,
                shape: 0,
                point,
            };
            ed.select_point(
                r,
                if n == 0 {
                    SelectMode::Replace
                } else {
                    SelectMode::Toggle
                },
            );
        }
    }

    /// Where point `k` is drawn, in document space.
    fn pos(ed: &Editor, k: usize) -> Vec2 {
        ed.point_positions()
            .into_iter()
            .find(|(r, _)| r.point == k)
            .unwrap()
            .1
    }

    fn shape(ed: &Editor, id: LayerId) -> Shape {
        match &ed.doc.find(id).unwrap().kind {
            fanimate_core::LayerKind::Vector { shapes } => shapes[0].clone(),
            _ => panic!("not a vector layer"),
        }
    }

    fn centroid(points: &[Vec2]) -> Vec2 {
        points.iter().copied().fold(Vec2::ZERO, |a, b| a + b) / points.len() as f32
    }

    /// A quarter turn clockwise about `c`, as the rotate handle makes it.
    fn quarter_turn(c: Vec2, p: Vec2) -> Vec2 {
        let d = p - c;
        c + Vec2::new(-d.y, d.x)
    }

    fn quarter_turn_drag(ed: &mut Editor, c: Vec2) {
        assert!(ed.begin_handle(Handle::Rotate, c + Vec2::new(0.0, 100.0)));
        ed.drag_handle(c + Vec2::new(-100.0, 0.0), HandleMods::default());
        ed.end_handle();
    }

    #[test]
    fn two_or_more_points_give_the_transform_tool_a_frame_of_their_own() {
        let (mut ed, id) = editor();
        // none, then one point: the layer's frame
        assert!(close(
            ed.selection_frame().unwrap().corners[2],
            Vec2::new(100.0, 100.0)
        ));
        choose(&mut ed, id, &[1]);
        assert!(close(ed.selection_frame().unwrap().corners[0], Vec2::ZERO));
        // two points: the box around them, turning about their middle
        choose(&mut ed, id, &[1, 2]);
        let f = ed.selection_frame().unwrap();
        assert!(close(f.corners[0], Vec2::new(100.0, 0.0)), "{:?}", f.corners);
        assert!(close(f.corners[2], Vec2::new(100.0, 100.0)), "{:?}", f.corners);
        assert!(close(f.pivot.unwrap(), Vec2::new(100.0, 50.0)));
        assert!(f.free_scale && !f.is_multiple());
        assert_eq!(f.primary, id);
        // other tools keep the layer frame
        ed.tool = Tool::SelectPoints;
        assert!(close(ed.selection_frame().unwrap().corners[0], Vec2::ZERO));
        ed.tool = Tool::TransformLayer;
        // points that are gone leave the layer frame behind again
        ed.points.clear();
        assert!(close(ed.selection_frame().unwrap().corners[0], Vec2::ZERO));
    }

    #[test]
    fn three_points_turn_a_quarter_about_their_centroid() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2]);
        let before: Vec<Vec2> = (0..4).map(|k| pos(&ed, k)).collect();
        let c = centroid(&before[..3]);
        assert!(close(ed.points_frame().unwrap().pivot.unwrap(), c));
        assert!(ed.begin_handle(Handle::Rotate, c + Vec2::new(0.0, 100.0)));
        ed.drag_handle(c + Vec2::new(-100.0, 0.0), HandleMods::default());
        assert_eq!(ed.handle_readout(), Some("90°"));
        ed.end_handle();
        for (k, was) in before.iter().enumerate().take(3) {
            assert!(close(pos(&ed, k), quarter_turn(c, *was)), "{k}: {}", pos(&ed, k));
        }
        // the point that was not chosen stayed
        assert!(close(pos(&ed, 3), before[3]));
        // the pivot stayed where it was
        assert!(close(ed.points_frame().unwrap().pivot.unwrap(), c));
    }

    #[test]
    fn shift_turns_in_steps_of_fifteen_degrees() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2, 3]);
        let c = Vec2::new(50.0, 50.0);
        assert!(ed.begin_handle(Handle::Rotate, c + Vec2::new(0.0, 100.0)));
        // 40 degrees round from straight below
        let a = std::f32::consts::FRAC_PI_2 + 40.0_f32.to_radians();
        ed.drag_handle(
            c + Vec2::from_angle(a) * 100.0,
            HandleMods {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(ed.handle_readout(), Some("45°"));
        ed.end_handle();
        let d = pos(&ed, 0) - c;
        assert!((d.to_angle() - (-135.0_f32).to_radians() - 45.0_f32.to_radians()).abs() < 1e-3);
    }

    #[test]
    fn scaling_goes_about_the_opposite_corner_and_carries_a_moved_pivot() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2, 3]);
        // the pivot is moved first: scaling does not depend on it
        ed.set_points_pivot(Vec2::new(25.0, 25.0));
        assert!(close(
            ed.points_frame().unwrap().pivot.unwrap(),
            Vec2::new(25.0, 25.0)
        ));
        assert!(ed.begin_handle(Handle::Scale { x: 1, y: 1 }, Vec2::new(100.0, 100.0)));
        ed.drag_handle(Vec2::new(200.0, 200.0), HandleMods::default());
        assert_eq!(ed.handle_readout(), Some("200 × 200"));
        ed.end_handle();
        assert!(close(pos(&ed, 0), Vec2::ZERO));
        assert!(close(pos(&ed, 1), Vec2::new(200.0, 0.0)));
        assert!(close(pos(&ed, 2), Vec2::new(200.0, 200.0)));
        assert!(close(pos(&ed, 3), Vec2::new(0.0, 200.0)));
        // the pivot kept its place among the points
        assert!(close(
            ed.points_frame().unwrap().pivot.unwrap(),
            Vec2::new(50.0, 50.0)
        ));
        assert!(close(ed.points_pivot.unwrap(), Vec2::new(50.0, 50.0)));
    }

    #[test]
    fn alt_scales_from_the_centre_and_shift_keeps_the_proportions() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2, 3]);
        // an edge with Alt: both sides move
        assert!(ed.begin_handle(Handle::Scale { x: 1, y: 0 }, Vec2::new(100.0, 50.0)));
        ed.drag_handle(
            Vec2::new(150.0, 50.0),
            HandleMods {
                alt: true,
                ..Default::default()
            },
        );
        ed.end_handle();
        assert!(close(pos(&ed, 0), Vec2::new(-50.0, 0.0)), "{}", pos(&ed, 0));
        assert!(close(pos(&ed, 2), Vec2::new(150.0, 100.0)), "{}", pos(&ed, 2));
        ed.run(Action::Undo);
        // an edge with Shift: the other way grows too, about the middle
        assert!(ed.begin_handle(Handle::Scale { x: 1, y: 0 }, Vec2::new(100.0, 50.0)));
        ed.drag_handle(
            Vec2::new(200.0, 50.0),
            HandleMods {
                shift: true,
                ..Default::default()
            },
        );
        ed.end_handle();
        assert!(close(pos(&ed, 2) - pos(&ed, 0), Vec2::new(200.0, 200.0)));
        assert!((centroid(&[pos(&ed, 0), pos(&ed, 2)]).y - 50.0).abs() < 0.05);
    }

    #[test]
    fn the_pivot_drag_snaps_to_anchors_and_is_not_an_undo_step() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2, 3]);
        let undo = ed.history.undo_label().map(str::to_owned);
        assert!(ed.begin_handle(Handle::Pivot, Vec2::new(50.0, 50.0)));
        let mods = |snap: f32, snapped: Option<Vec2>| HandleMods {
            snap,
            snapped,
            ..Default::default()
        };
        // near the bottom right corner: snaps onto it
        ed.drag_handle(Vec2::new(98.0, 99.0), mods(6.0, None));
        assert!(close(ed.points_pivot.unwrap(), Vec2::new(100.0, 100.0)));
        // away from every anchor it follows the pointer, or the snapped point
        ed.drag_handle(Vec2::new(30.0, 40.0), mods(2.0, None));
        assert!(close(ed.points_pivot.unwrap(), Vec2::new(30.0, 40.0)));
        ed.drag_handle(Vec2::new(30.0, 40.0), mods(2.0, Some(Vec2::new(32.0, 40.0))));
        assert!(close(ed.points_pivot.unwrap(), Vec2::new(32.0, 40.0)));
        // an anchor still wins over a snap to a guide
        ed.drag_handle(Vec2::new(1.0, 1.0), mods(6.0, Some(Vec2::new(3.0, 1.0))));
        assert!(close(ed.points_pivot.unwrap(), Vec2::ZERO));
        ed.end_handle();
        assert_eq!(ed.history.undo_label().map(str::to_owned), undo);
        // and the points did not move
        assert!(close(pos(&ed, 2), Vec2::new(100.0, 100.0)));
        // turning now goes about the moved pivot
        let c = Vec2::ZERO;
        quarter_turn_drag(&mut ed, c);
        assert!(close(pos(&ed, 1), Vec2::new(0.0, 100.0)), "{}", pos(&ed, 1));
    }

    #[test]
    fn an_edit_on_a_later_frame_keys_the_path_and_seeds_the_first_frame() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2]);
        let rest = shape(&ed, id).path;
        ed.go_to(10);
        let before: Vec<Vec2> = (0..3).map(|k| pos(&ed, k)).collect();
        let c = centroid(&before);
        quarter_turn_drag(&mut ed, c);
        let s = shape(&ed, id);
        assert_eq!(s.path_keys.len(), 2, "{:?}", s.path_keys);
        let frames: Vec<_> = s.path_keys.iter().map(|k| k.frame).collect();
        assert!(
            frames.contains(&10) && frames.contains(&ed.keying().base),
            "{frames:?}"
        );
        // the drawing itself is unchanged, and the key on the first frame holds it
        assert_eq!(s.path, rest);
        let seed = s.path_keys.iter().find(|k| k.frame == ed.keying().base).unwrap();
        assert_eq!(seed.value, rest);
        // the points are turned on this frame
        for (k, was) in before.iter().enumerate().take(3) {
            assert!(close(pos(&ed, k), quarter_turn(c, *was)));
        }
        // a second gesture on the same frame changes that key, not the count
        let c = centroid(&(0..3).map(|k| pos(&ed, k)).collect::<Vec<_>>());
        quarter_turn_drag(&mut ed, c);
        assert_eq!(shape(&ed, id).path_keys.len(), 2);
    }

    #[test]
    fn one_gesture_is_one_undo_step_and_undo_restores_the_drawing() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2]);
        let rest = shape(&ed, id);
        let before: Vec<Vec2> = (0..4).map(|k| pos(&ed, k)).collect();
        let c = centroid(&before[..3]);
        assert!(ed.begin_handle(Handle::Rotate, c + Vec2::new(0.0, 100.0)));
        for step in 1..=6 {
            let a = std::f32::consts::FRAC_PI_2 + step as f32 * 0.2;
            ed.drag_handle(c + Vec2::from_angle(a) * 100.0, HandleMods::default());
        }
        ed.end_handle();
        assert!(!close(pos(&ed, 0), before[0]));
        assert_eq!(ed.history.undo_label(), Some("Rotate points"));
        ed.run(Action::Undo);
        // all six moves went in one step: the drawing is exactly as it was
        assert_eq!(shape(&ed, id), rest);
        for (k, p) in before.iter().enumerate() {
            assert!(close(pos(&ed, k), *p));
        }
        assert_ne!(ed.history.undo_label(), Some("Rotate points"));
    }

    #[test]
    fn a_turned_and_scaled_layer_gives_the_same_result_in_document_space() {
        let (mut ed, id) = editor();
        let mut t = ed.layer_transform(id).unwrap();
        t.position = Vec2::new(40.0, 25.0);
        t.rotation = 0.6;
        t.scale = Vec2::new(1.5, 0.75);
        ed.set_layer_transform(id, t);
        choose(&mut ed, id, &[0, 1, 2]);
        let before: Vec<Vec2> = (0..4).map(|k| pos(&ed, k)).collect();
        let c = centroid(&before[..3]);
        assert!(close(ed.points_frame().unwrap().pivot.unwrap(), c));
        quarter_turn_drag(&mut ed, c);
        for (k, was) in before.iter().enumerate().take(3) {
            assert!(close(pos(&ed, k), quarter_turn(c, *was)), "{k}: {}", pos(&ed, k));
        }
        assert!(close(pos(&ed, 3), before[3]));
        // the layer itself was not touched
        let after = ed.layer_transform(id).unwrap();
        assert!((after.rotation - 0.6).abs() < 1e-6 && close(after.scale, Vec2::new(1.5, 0.75)));
        // and a scale does the same: the far corner of the box moves to the pointer
        let f = ed.points_frame().unwrap();
        let (lo, hi) = (f.corners[0], f.corners[2]);
        assert!(ed.begin_handle(Handle::Scale { x: 1, y: 1 }, hi));
        ed.drag_handle(lo + (hi - lo) * 2.0, HandleMods::default());
        ed.end_handle();
        let g = ed.points_frame().unwrap();
        assert!(
            close(g.corners[0], lo) && close(g.corners[2], lo + (hi - lo) * 2.0),
            "{:?}",
            g.corners
        );
    }

    #[test]
    fn the_pivot_is_forgotten_when_the_choice_changes() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2]);
        ed.set_points_pivot(Vec2::new(5.0, 6.0));
        assert!(close(
            ed.points_frame().unwrap().pivot.unwrap(),
            Vec2::new(5.0, 6.0)
        ));
        ed.select_point(
            PointRef {
                layer: id,
                shape: 0,
                point: 3,
            },
            SelectMode::Toggle,
        );
        assert_eq!(ed.points_pivot, None);
        ed.set_points_pivot(Vec2::new(5.0, 6.0));
        ed.select_points_in(Vec2::new(-1.0, -1.0), Vec2::new(101.0, 1.0), false);
        assert_eq!(ed.points_pivot, None);
        ed.set_points_pivot(Vec2::new(5.0, 6.0));
        ed.select_all_points();
        assert_eq!(ed.points_pivot, None);
        // a choice changed some other way cannot revive a pivot placed for another
        ed.set_points_pivot(Vec2::new(5.0, 6.0));
        ed.points.remove(&PointRef {
            layer: id,
            shape: 0,
            point: 3,
        });
        assert!(close(
            ed.points_frame().unwrap().pivot.unwrap(),
            centroid(&[pos(&ed, 0), pos(&ed, 1), pos(&ed, 2)])
        ));
    }

    #[test]
    fn the_picker_places_a_layers_pivot_once_and_undo_takes_it_back() {
        let (mut ed, id) = editor();
        let before = ed.selection_frame().unwrap().corners;
        assert!(!ed.set_pivot_at(Vec2::new(10.0, 10.0), true, false), "not armed");
        ed.run(Action::PickPivot);
        assert!(ed.pivot_pick);
        assert_eq!(ed.tool, Tool::TransformLayer);
        assert_eq!(
            ed.status.as_ref().map(|s| s.1.as_str()),
            Some("Click to place the pivot, Esc to cancel")
        );
        // near the bottom right corner: snaps onto it, the drawing stays put
        assert!(ed.set_pivot_at(Vec2::new(97.0, 98.0), true, false));
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(100.0, 100.0)
        ));
        assert!(
            ed.selection_frame()
                .unwrap()
                .corners
                .iter()
                .zip(before)
                .all(|(a, b)| close(*a, b))
        );
        // one shot
        assert!(!ed.pivot_pick);
        assert!(!ed.set_pivot_at(Vec2::new(20.0, 20.0), true, false));
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(100.0, 100.0)
        ));
        // free: right where it was clicked
        ed.arm_pivot_pick();
        assert!(ed.set_pivot_at(Vec2::new(97.0, 98.0), true, true));
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(97.0, 98.0)
        ));
        // without the snap switch the same
        ed.arm_pivot_pick();
        assert!(ed.set_pivot_at(Vec2::new(60.0, 70.0), false, false));
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(60.0, 70.0)
        ));
        ed.run(Action::Undo);
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(97.0, 98.0)
        ));
        ed.run(Action::Undo);
        ed.run(Action::Undo);
        assert_eq!(ed.layer_transform(id).unwrap().pivot, Vec2::ZERO);
    }

    #[test]
    fn the_picker_works_through_a_turned_layer() {
        let (mut ed, id) = editor();
        let mut t = ed.layer_transform(id).unwrap();
        t.position = Vec2::new(30.0, 0.0);
        t.rotation = 0.5;
        ed.set_layer_transform(id, t);
        let before = ed.selection_frame().unwrap().corners;
        // the layer's bottom right corner, in document space
        let corner = before[2];
        ed.arm_pivot_pick();
        ed.pivot_snap = 4.0;
        assert!(ed.set_pivot_at(corner + Vec2::new(1.0, 1.0), true, false));
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(100.0, 100.0)
        ));
        assert!(
            ed.selection_frame()
                .unwrap()
                .corners
                .iter()
                .zip(before)
                .all(|(a, b)| close(*a, b))
        );
    }

    #[test]
    fn esc_cancels_the_picker_without_clearing_the_selection() {
        let (mut ed, id) = editor();
        ed.run(Action::PickPivot);
        assert!(ed.pivot_pick);
        ed.run(Action::SelectNone);
        assert!(!ed.pivot_pick);
        assert_eq!(ed.selection, [id]);
        assert!(!ed.set_pivot_at(Vec2::new(10.0, 10.0), true, false));
        assert_eq!(ed.layer_transform(id).unwrap().pivot, Vec2::ZERO);
        // changing the tool disarms it too
        ed.run(Action::PickPivot);
        ed.set_tool(Tool::Pan);
        assert!(!ed.pivot_pick);
        // nothing selected: nothing to arm
        ed.selection.clear();
        ed.arm_pivot_pick();
        assert!(!ed.pivot_pick);
    }

    #[test]
    fn the_picker_sets_the_pivot_of_chosen_points_without_an_undo_step() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[0, 1, 2, 3]);
        let undo = ed.history.undo_label().map(str::to_owned);
        ed.arm_pivot_pick();
        // near the top left corner of the points' box: snaps onto it
        assert!(ed.set_pivot_at(Vec2::new(3.0, 2.0), true, false));
        assert!(close(ed.points_pivot.unwrap(), Vec2::ZERO));
        assert_eq!(ed.history.undo_label().map(str::to_owned), undo);
        assert!(!ed.pivot_pick);
        // the layer's own pivot was not touched
        assert_eq!(ed.doc.find(id).unwrap().transform.pivot, None);
        // turning goes about it
        quarter_turn_drag(&mut ed, Vec2::ZERO);
        assert!(close(pos(&ed, 1), Vec2::new(0.0, 100.0)), "{}", pos(&ed, 1));
        // free placement
        ed.arm_pivot_pick();
        assert!(ed.set_pivot_at(Vec2::new(3.0, 2.0), true, true));
        assert!(close(ed.points_pivot.unwrap(), Vec2::new(3.0, 2.0)));
    }

    #[test]
    fn handles_of_curves_move_with_their_points_and_circles_move_whole() {
        let mut path = Path::new()
            .move_to(Vec2::new(0.0, 0.0))
            .cubic_to(Vec2::new(10.0, 0.0), Vec2::new(20.0, 10.0), Vec2::new(30.0, 10.0))
            .cubic_to(Vec2::new(40.0, 10.0), Vec2::new(50.0, 0.0), Vec2::new(60.0, 0.0))
            .close();
        // move point 1 only
        transform_chosen(
            &mut path,
            &BTreeSet::from([1]),
            Affine2::from_translation(Vec2::new(0.0, 5.0)),
        );
        assert_eq!(
            path.cmds[1],
            PathCmd::CubicTo(Vec2::new(10.0, 0.0), Vec2::new(20.0, 15.0), Vec2::new(30.0, 15.0))
        );
        // its leaving handle went along, the other end's did not
        assert_eq!(
            path.cmds[2],
            PathCmd::CubicTo(Vec2::new(40.0, 15.0), Vec2::new(50.0, 0.0), Vec2::new(60.0, 0.0))
        );
        // a closed path whose last point is on its first moves both together
        let mut circle = Path::new()
            .move_to(Vec2::new(0.0, 0.0))
            .line_to(Vec2::new(10.0, 0.0))
            .line_to(Vec2::new(0.0, 0.0))
            .close();
        transform_chosen(
            &mut circle,
            &BTreeSet::from([0]),
            Affine2::from_translation(Vec2::new(1.0, 1.0)),
        );
        assert_eq!(circle.on_curve_points()[0], Vec2::new(1.0, 1.0));
        assert_eq!(circle.on_curve_points()[2], Vec2::new(1.0, 1.0));
        assert_eq!(circle.on_curve_points()[1], Vec2::new(10.0, 0.0));
    }

    #[test]
    fn nothing_to_drag_without_two_points() {
        let (mut ed, id) = editor();
        choose(&mut ed, id, &[2]);
        assert!(ed.points_frame().is_none());
        // one point: the layer frame is back, and its handles work on the layer
        assert!(ed.begin_handle(Handle::Scale { x: 1, y: 1 }, Vec2::new(100.0, 100.0)));
        ed.drag_handle(Vec2::new(200.0, 200.0), HandleMods::default());
        ed.end_handle();
        assert!(close(ed.layer_transform(id).unwrap().scale, Vec2::splat(2.0)));
        assert!(!ed.is_dragging_handle());
    }
}
