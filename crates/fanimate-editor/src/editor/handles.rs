//! The selection frame and its handles: scale, rotate, and move the pivot.
//!
//! The frame is the box around the selected layers. For one layer it is the
//! layer's own box, so it turns with the layer; for several it is the
//! document-space box around all of them. Dragging a handle changes the
//! layers' transforms through [`Editor::set_layer_transform`], so the
//! auto-key rule holds: a channel that is animated gets a key on this frame.
//!
//! One gesture is one undo step: [`Editor::begin_handle`] records it, and
//! [`Editor::drag_handle`] is then called as the pointer moves.

use fanimate_core::{Affine2, AnimTransform, LayerId, Transform, Vec2, evaluate};

use super::{Editor, StatusKind};

/// Which handle is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// A corner or an edge of the box: -1 is the left or top side, 1 the
    /// right or bottom side, 0 the middle (that direction is left alone).
    Scale {
        x: i8,
        y: i8,
    },
    Rotate,
    /// The pivot dot, a single layer's centre of rotation and scale.
    Pivot,
}

/// The keys held and the screen scale, as a handle drag needs them.
#[derive(Clone, Copy, Debug, Default)]
pub struct HandleMods {
    /// Scale: keep the proportions. Rotate: step by 15 degrees.
    pub shift: bool,
    /// Scale: from the centre, not from the opposite side.
    pub alt: bool,
    /// How close, in document units, the pivot has to come to one of the
    /// box's nine anchors to snap to it.
    pub snap: f32,
}

/// The box around the selection, in document space.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionFrame {
    /// Top left, top right, bottom right and bottom left of the box. For one
    /// layer these are the corners of its own box, so they turn with it.
    pub corners: [Vec2; 4],
    /// Each selected layer's own box, for outlining them one by one, and
    /// the layers they belong to.
    pub members: Vec<[Vec2; 4]>,
    pub layers: Vec<LayerId>,
    /// The pivot, for a single layer.
    pub pivot: Option<Vec2>,
    /// The layer that sets the frame's colour: the last one selected.
    pub primary: LayerId,
    /// Whether the sides can be stretched independently. Always for one
    /// layer; for several, only when none is turned (stretching a turned
    /// layer sideways would shear it, which a transform cannot hold).
    pub free_scale: bool,
}

impl SelectionFrame {
    pub fn is_multiple(&self) -> bool {
        self.members.len() > 1
    }

    pub fn center(&self) -> Vec2 {
        (self.corners[0] + self.corners[2]) / 2.0
    }

    /// The middle of the side from corner `a` to the next.
    pub fn edge(&self, a: usize) -> Vec2 {
        (self.corners[a % 4] + self.corners[(a + 1) % 4]) / 2.0
    }

    /// The position of a scale handle: `x` and `y` as in [`Handle::Scale`].
    pub fn scale_handle(&self, x: i8, y: i8) -> Vec2 {
        let side = |s: i8| (s as f32 + 1.0) / 2.0;
        let (u, v) = (side(x), side(y));
        let top = self.corners[0].lerp(self.corners[1], u);
        let bottom = self.corners[3].lerp(self.corners[2], u);
        top.lerp(bottom, v)
    }

    /// Which way is "below" the box, as a unit vector: away from the centre
    /// through the middle of the bottom side. The rotate handle goes there.
    pub fn below(&self) -> Vec2 {
        (self.edge(2) - self.center()).try_normalize().unwrap_or(Vec2::Y)
    }
}

struct Root {
    id: LayerId,
    t0: Transform,
    /// The layer's parent space to document space, when the drag began.
    parent: Affine2,
}

pub(super) struct HandleDrag {
    handle: Handle,
    roots: Vec<Root>,
    /// The box being dragged: in the layer's own space for one layer, in
    /// document space for several.
    lo: Vec2,
    hi: Vec2,
    /// Rotate: the pivot, or the middle of the box, in document space, and
    /// where the pointer started round it.
    centre: Vec2,
    start_angle: f32,
    /// Pivot: the transform as it was, to move the pivot from afresh each time.
    original: Option<AnimTransform>,
    free_scale: bool,
}

fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// How far the pointer has taken a side: the anchor that stays put and the
/// ratio the box grows by, along one axis. `s` is the side being dragged.
fn axis(s: i8, lo: f32, hi: f32, pointer: f32, from_centre: bool) -> (f32, f32) {
    let centre = (lo + hi) / 2.0;
    if s == 0 {
        return (centre, 1.0);
    }
    let dragged = if s > 0 { hi } else { lo };
    let anchor = if from_centre {
        centre
    } else if s > 0 {
        lo
    } else {
        hi
    };
    let span = dragged - anchor;
    if span.abs() < 1e-6 {
        return (anchor, 1.0);
    }
    (anchor, (pointer - anchor) / span)
}

/// A ratio kept at least a little way from 0, so a box cannot collapse flat.
fn keep_open(r: f32) -> f32 {
    if r.abs() < 0.01 { 0.01_f32.copysign(r) } else { r }
}

impl Editor {
    /// Each layer's box in its own space (low and high corner), at this
    /// frame, or `None` for a layer with nothing drawn. One evaluation for
    /// all of them.
    fn layer_bounds(&self, ids: &[LayerId]) -> Vec<Option<(Vec2, Vec2)>> {
        let items = evaluate(&self.doc, self.frame);
        ids.iter()
            .map(|&id| {
                let inv = self.world(id).inverse();
                if !inv.is_finite() {
                    return None;
                }
                let mut bounds: Option<(Vec2, Vec2)> = None;
                let mut add = |q: Vec2| {
                    if q.is_finite() {
                        bounds = Some(match bounds {
                            None => (q, q),
                            Some((lo, hi)) => (lo.min(q), hi.max(q)),
                        });
                    }
                };
                for item in items.iter().filter(|i| self.doc.is_within(i.layer, id)) {
                    let to_layer = inv * item.transform;
                    if let Some(image) = &item.image {
                        image
                            .vertices
                            .iter()
                            .for_each(|(p, _)| add(to_layer.transform_point2(*p)));
                    } else {
                        for (points, _) in item.shape.path.polylines(0.25) {
                            points.iter().for_each(|p| add(to_layer.transform_point2(*p)));
                        }
                    }
                }
                bounds
            })
            .collect()
    }

    /// Where a layer's pivot is, for showing and for the Properties grid, in
    /// the layer's space. A layer without a pivot of its own shows it at the
    /// middle of its box, because that is where turning it puts one (without
    /// moving it); a layer that cannot take one cleanly shows its origin.
    fn shown_pivot(&self, id: LayerId, lo: Vec2, hi: Vec2) -> Option<Vec2> {
        let layer = self.doc.find(id)?;
        Some(match layer.transform.pivot {
            Some(p) => p,
            None if layer.transform.pivot_moves_cleanly() => (lo + hi) / 2.0,
            None => Vec2::ZERO,
        })
    }

    /// The frame around the selected layers, or `None` with nothing
    /// selected or nothing drawn.
    pub fn selection_frame(&self) -> Option<SelectionFrame> {
        let roots = self.selected_roots();
        let bounds = self.layer_bounds(&roots);
        let mut members = Vec::new();
        let mut layers = Vec::new();
        let mut free_scale = true;
        let mut pivot = None;
        for (&id, b) in roots.iter().zip(bounds) {
            let Some((lo, hi)) = b else { continue };
            layers.push(id);
            let world = self.world(id);
            members.push([
                world.transform_point2(lo),
                world.transform_point2(Vec2::new(hi.x, lo.y)),
                world.transform_point2(hi),
                world.transform_point2(Vec2::new(lo.x, hi.y)),
            ]);
            // axis-aligned: turned by a multiple of a quarter turn
            let x = world.matrix2.x_axis;
            free_scale &= x.x.abs() < 1e-4 || x.y.abs() < 1e-4;
            pivot = self.shown_pivot(id, lo, hi).map(|p| world.transform_point2(p));
        }
        let primary = self
            .primary()
            .filter(|p| layers.contains(p))
            .or_else(|| layers.last().copied())?;
        match members.len() {
            0 => None,
            1 => Some(SelectionFrame {
                corners: members[0],
                members,
                layers,
                pivot,
                primary,
                free_scale: true,
            }),
            _ => {
                let (lo, hi) = members
                    .iter()
                    .flatten()
                    .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                Some(SelectionFrame {
                    corners: [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)],
                    members,
                    layers,
                    pivot: None,
                    primary,
                    free_scale,
                })
            }
        }
    }

    /// Start dragging a handle of the selection frame: one undo step.
    /// `pointer` is in document space. Returns false when there is nothing
    /// to drag.
    pub fn begin_handle(&mut self, handle: Handle, pointer: Vec2) -> bool {
        let Some(frame) = self.selection_frame() else {
            return false;
        };
        let ids = frame.layers.clone();
        if frame.is_multiple() && handle == Handle::Pivot {
            return false;
        }
        self.checkpoint(match handle {
            Handle::Scale { .. } => "Scale",
            Handle::Rotate => "Rotate",
            Handle::Pivot => "Move pivot",
        });

        let single = !frame.is_multiple();
        let id = frame.primary;
        let (lo, hi, original) = if single {
            let Some((lo, hi)) = self.layer_bounds(&[id])[0] else {
                return false;
            };
            (lo, hi, self.doc.find(id).map(|l| l.transform.clone()))
        } else {
            (frame.corners[0], frame.corners[2], None)
        };

        // turning a layer that has no pivot turns it about the middle of its
        // box, as people expect, without moving it: the pivot is set first
        if single && handle == Handle::Rotate {
            let (has_pivot, clean) = self.doc.find(id).map_or((true, false), |l| {
                (l.transform.pivot.is_some(), l.transform.pivot_moves_cleanly())
            });
            if !has_pivot {
                if clean {
                    let frame_no = self.frame;
                    if let Some(l) = self.doc.find_mut(id) {
                        l.transform.set_pivot_keeping_look((lo + hi) / 2.0, frame_no);
                    }
                    self.dirty = true;
                } else {
                    self.set_status(
                        StatusKind::Info,
                        "This layer's rotation is animated, so it turns about its origin. Move its pivot first.",
                    );
                }
            }
        }

        let roots: Vec<Root> = if single { vec![id] } else { ids }
            .into_iter()
            .filter_map(|id| {
                Some(Root {
                    id,
                    t0: self.layer_transform(id)?,
                    parent: self.parent_world(id),
                })
            })
            .collect();
        let centre = match (single, roots.first()) {
            (true, Some(r)) => (r.parent * r.t0.to_affine()).transform_point2(r.t0.pivot),
            _ => frame.center(),
        };
        self.handle_drag = Some(HandleDrag {
            handle,
            roots,
            lo,
            hi,
            centre,
            start_angle: (pointer - centre).to_angle(),
            original,
            free_scale: frame.free_scale,
        });
        self.handle_readout = None;
        true
    }

    /// Move the handle to `pointer` (document space).
    pub fn drag_handle(&mut self, pointer: Vec2, mods: HandleMods) {
        let Some(drag) = self.handle_drag.take() else {
            return;
        };
        match drag.handle {
            Handle::Scale { x, y } => self.drag_scale(&drag, x, y, pointer, mods),
            Handle::Rotate => self.drag_rotate(&drag, pointer, mods),
            Handle::Pivot => self.drag_pivot(&drag, pointer, mods),
        }
        self.dirty = true;
        self.handle_drag = Some(drag);
    }

    pub fn end_handle(&mut self) {
        self.handle_drag = None;
        self.handle_readout = None;
    }

    pub fn is_dragging_handle(&self) -> bool {
        self.handle_drag.is_some()
    }

    /// What to show beside the pointer while a handle is dragged: the new
    /// size, or the angle.
    pub fn handle_readout(&self) -> Option<&str> {
        self.handle_readout.as_deref()
    }

    fn drag_scale(&mut self, drag: &HandleDrag, x: i8, y: i8, pointer: Vec2, mods: HandleMods) {
        let single = drag.roots.len() == 1;
        // the pointer in the space the box is in
        let p = match drag.roots.first() {
            Some(r) if single => {
                let world = r.parent * r.t0.to_affine();
                if !world.inverse().is_finite() {
                    return;
                }
                world.inverse().transform_point2(pointer)
            }
            _ => pointer,
        };
        let (ax, rx) = axis(x, drag.lo.x, drag.hi.x, p.x, mods.alt);
        let (ay, ry) = axis(y, drag.lo.y, drag.hi.y, p.y, mods.alt);
        let (mut anchor, mut ratio) = (Vec2::new(ax, ay), Vec2::new(rx, ry));
        // proportions: kept with Shift, and always when the sides cannot be
        // stretched on their own
        let even = mods.shift || (!single && !drag.free_scale);
        if even {
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
        let about = Affine2::from_translation(anchor)
            * Affine2::from_scale(ratio)
            * Affine2::from_translation(-anchor);

        let apply = |ed: &mut Editor, ratio_used: Affine2| -> bool {
            let mut changes = Vec::new();
            for r in &drag.roots {
                let l0 = r.t0.to_affine();
                // the box is in the layer's own space for one layer, in
                // document space for several
                let l1 = if single {
                    l0 * ratio_used
                } else {
                    r.parent.inverse() * ratio_used * r.parent * l0
                };
                match Transform::from_affine(l1, r.t0.pivot, &r.t0) {
                    Some(t) => changes.push((r.id, t)),
                    None => return false,
                }
            }
            for (id, t) in changes {
                ed.set_layer_transform(id, t);
            }
            true
        };
        let mut used = ratio;
        if !apply(self, about) && !even {
            // a stretch some layer cannot take: scale evenly instead
            let r = keep_open(ratio.x.abs().min(ratio.y.abs()).copysign(ratio.x));
            used = Vec2::splat(r);
            let even_about = Affine2::from_translation(anchor)
                * Affine2::from_scale(used)
                * Affine2::from_translation(-anchor);
            apply(self, even_about);
        }
        // the new size in document units
        let base = if single {
            let world = drag.roots[0].parent * drag.roots[0].t0.to_affine();
            let (a, b) = (
                world.transform_vector2(Vec2::new(drag.hi.x - drag.lo.x, 0.0)),
                world.transform_vector2(Vec2::new(0.0, drag.hi.y - drag.lo.y)),
            );
            Vec2::new(a.length(), b.length())
        } else {
            drag.hi - drag.lo
        };
        let size = base * used.abs();
        self.handle_readout = Some(format!("{:.0} × {:.0}", size.x, size.y));
    }

    fn drag_rotate(&mut self, drag: &HandleDrag, pointer: Vec2, mods: HandleMods) {
        let mut delta = wrap_pi((pointer - drag.centre).to_angle() - drag.start_angle);
        let single = drag.roots.len() == 1;
        if single {
            let r = &drag.roots[0];
            // a flipped parent turns the other way on screen
            if r.parent.matrix2.determinant() < 0.0 {
                delta = -delta;
            }
            let mut rotation = r.t0.rotation + delta;
            if mods.shift {
                let step = 15.0_f32.to_radians();
                rotation = (rotation / step).round() * step;
            }
            let mut t = r.t0;
            t.rotation = rotation;
            self.set_layer_transform(r.id, t);
            self.handle_readout = Some(format!("{:.0}°", rotation.to_degrees()));
            return;
        }
        if mods.shift {
            let step = 15.0_f32.to_radians();
            delta = (delta / step).round() * step;
        }
        let about = Affine2::from_translation(drag.centre)
            * Affine2::from_angle(delta)
            * Affine2::from_translation(-drag.centre);
        let mut changes = Vec::new();
        for r in &drag.roots {
            let l1 = r.parent.inverse() * about * r.parent * r.t0.to_affine();
            if let Some(t) = Transform::from_affine(l1, r.t0.pivot, &r.t0) {
                changes.push((r.id, t));
            }
        }
        for (id, t) in changes {
            self.set_layer_transform(id, t);
        }
        self.handle_readout = Some(format!("{:.0}°", delta.to_degrees()));
    }

    fn drag_pivot(&mut self, drag: &HandleDrag, pointer: Vec2, mods: HandleMods) {
        let (Some(r), Some(original)) = (drag.roots.first(), &drag.original) else {
            return;
        };
        // the pointer in the layer's space as it was when the drag began
        let world0 = r.parent * r.t0.to_affine();
        if !world0.inverse().is_finite() {
            return;
        }
        let mut pivot = world0.inverse().transform_point2(pointer);
        // the nine anchors of the box: corners, middles of the sides, centre
        let mut best: Option<(f32, Vec2)> = None;
        for fy in [0.0, 0.5, 1.0] {
            for fx in [0.0, 0.5, 1.0] {
                let a = drag.lo + (drag.hi - drag.lo) * Vec2::new(fx, fy);
                let d = world0.transform_point2(a).distance(pointer);
                if d <= mods.snap && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, a));
                }
            }
        }
        if let Some((_, a)) = best {
            pivot = a;
        }
        if !pivot.is_finite() {
            return;
        }
        let frame = self.frame;
        if let Some(l) = self.doc.find_mut(r.id) {
            l.transform = original.clone();
            l.transform.set_pivot_keeping_look(pivot, frame);
        }
    }

    /// Put the selected layer's pivot on one of the nine anchors of its box:
    /// `fx` and `fy` are 0, 0.5 or 1 from left to right and top to bottom.
    /// One undo step. Returns whether it moved.
    pub fn move_pivot_to_anchor(&mut self, fx: f32, fy: f32) -> bool {
        let Some(id) = self.primary() else { return false };
        let Some(Some((lo, hi))) = self.layer_bounds(&[id]).first().copied() else {
            return false;
        };
        let target = lo + (hi - lo) * Vec2::new(fx, fy);
        self.checkpoint("Move pivot");
        let frame = self.frame;
        let clean = self
            .doc
            .find_mut(id)
            .is_some_and(|l| l.transform.set_pivot_keeping_look(target, frame));
        if !clean {
            self.set_status(
                StatusKind::Warning,
                "The rotation or scale is animated, so the layer only stays put on this frame",
            );
        }
        true
    }

    /// Move a layer's pivot to `pivot` (its own space), by number, leaving
    /// the artwork where it is. Does not checkpoint: the Properties panel
    /// does that once per edit. Returns whether the layer stays put on every
    /// frame (it does not when the rotation or scale is animated).
    pub fn set_layer_pivot(&mut self, id: LayerId, pivot: Vec2) -> bool {
        if !pivot.is_finite() {
            return true;
        }
        let frame = self.frame;
        let clean = self
            .doc
            .find_mut(id)
            .is_none_or(|l| l.transform.set_pivot_keeping_look(pivot, frame));
        self.dirty = true;
        clean
    }

    /// Where a layer's pivot is, in its own space, as the frame shows it (see
    /// `shown_pivot`): the one it has, else the middle of its box.
    pub fn layer_pivot(&self, id: LayerId) -> Option<Vec2> {
        let (lo, hi) = self.layer_bounds(&[id]).first().copied().flatten()?;
        self.shown_pivot(id, lo, hi)
    }

    /// Where the selected layer's pivot sits in its box, as a fraction from
    /// the top left (0 to 1 each way), or `None` with none, or off the box.
    pub fn pivot_fraction(&self) -> Option<Vec2> {
        let id = self.primary()?;
        let (lo, hi) = self.layer_bounds(&[id]).first().copied().flatten()?;
        let pivot = self.shown_pivot(id, lo, hi)?;
        let size = hi - lo;
        if size.x.abs() < 1e-6 || size.y.abs() < 1e-6 {
            return None;
        }
        Some((pivot - lo) / size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, SelectMode, Tool};
    use fanimate_core::LayerKind;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 0.05
    }

    /// An editor whose selected layer holds a 100 x 60 rectangle from (10, 20).
    fn editor() -> (Editor, LayerId) {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::new(10.0, 20.0),
            Vec2::new(110.0, 80.0),
            false,
            false,
            1.0,
        );
        (ed, id)
    }

    /// A second layer with a 40 x 40 square from (200, 100).
    fn second(ed: &mut Editor) -> LayerId {
        ed.run(Action::NewVectorLayer);
        let id = ed.primary().unwrap();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::new(200.0, 100.0),
            Vec2::new(240.0, 140.0),
            false,
            false,
            1.0,
        );
        id
    }

    fn corners(ed: &Editor) -> [Vec2; 4] {
        ed.selection_frame().unwrap().corners
    }

    #[test]
    fn the_frame_is_the_box_around_the_drawing() {
        let (ed, id) = editor();
        let f = ed.selection_frame().unwrap();
        assert_eq!(f.primary, id);
        assert!(!f.is_multiple() && f.free_scale);
        assert!(close(f.corners[0], Vec2::new(10.0, 20.0)));
        assert!(close(f.corners[2], Vec2::new(110.0, 80.0)));
        assert!(close(f.center(), Vec2::new(60.0, 50.0)));
        assert!(close(f.scale_handle(1, 1), Vec2::new(110.0, 80.0)));
        assert!(close(f.scale_handle(0, -1), Vec2::new(60.0, 20.0)));
        assert!(close(f.below(), Vec2::Y));
        // nothing selected, nothing drawn: no frame
        let mut none = Editor::untitled();
        assert!(none.selection_frame().is_none());
        none.selection.clear();
        assert!(none.selection_frame().is_none());
    }

    #[test]
    fn the_frame_turns_with_a_turned_layer() {
        let (mut ed, id) = editor();
        let mut t = ed.layer_transform(id).unwrap();
        t.rotation = std::f32::consts::FRAC_PI_2;
        ed.set_layer_transform(id, t);
        let f = ed.selection_frame().unwrap();
        // a quarter turn clockwise: "below" the box now points left
        assert!(close(f.below(), -Vec2::X), "{:?}", f.below());
    }

    #[test]
    fn dragging_a_corner_scales_about_the_opposite_one() {
        let (mut ed, id) = editor();
        assert!(ed.begin_handle(Handle::Scale { x: 1, y: 1 }, Vec2::new(110.0, 80.0)));
        // the bottom right corner to twice the distance from the top left
        ed.drag_handle(Vec2::new(210.0, 140.0), HandleMods::default());
        let c = corners(&ed);
        assert!(close(c[0], Vec2::new(10.0, 20.0)), "{c:?}");
        assert!(close(c[2], Vec2::new(210.0, 140.0)), "{c:?}");
        assert_eq!(ed.handle_readout(), Some("200 × 120"));
        ed.end_handle();
        let t = ed.layer_transform(id).unwrap();
        assert!(close(t.scale, Vec2::splat(2.0)));
    }

    #[test]
    fn an_edge_scales_one_way_alt_from_the_centre_shift_evenly() {
        let (mut ed, _) = editor();
        ed.begin_handle(Handle::Scale { x: 1, y: 0 }, Vec2::new(110.0, 50.0));
        ed.drag_handle(Vec2::new(210.0, 50.0), HandleMods::default());
        let c = corners(&ed);
        assert!(
            close(c[0], Vec2::new(10.0, 20.0)) && close(c[2], Vec2::new(210.0, 80.0)),
            "{c:?}"
        );
        ed.end_handle();
        ed.run(Action::Undo);

        // from the centre: both sides move
        ed.begin_handle(Handle::Scale { x: 1, y: 0 }, Vec2::new(110.0, 50.0));
        ed.drag_handle(
            Vec2::new(160.0, 50.0),
            HandleMods {
                alt: true,
                ..Default::default()
            },
        );
        let c = corners(&ed);
        assert!(
            close(c[0], Vec2::new(-40.0, 20.0)) && close(c[2], Vec2::new(160.0, 80.0)),
            "{c:?}"
        );
        ed.end_handle();
        ed.run(Action::Undo);

        // proportions: the edge drag grows the other way too, about the middle
        ed.begin_handle(Handle::Scale { x: 1, y: 0 }, Vec2::new(110.0, 50.0));
        ed.drag_handle(
            Vec2::new(210.0, 50.0),
            HandleMods {
                shift: true,
                ..Default::default()
            },
        );
        let c = corners(&ed);
        assert!(close(c[2] - c[0], Vec2::new(200.0, 120.0)), "{c:?}");
        assert!((ed.selection_frame().unwrap().center().y - 50.0).abs() < 0.05);
        ed.end_handle();
    }

    #[test]
    fn dragging_a_corner_past_the_other_flips_the_layer_and_never_collapses_it() {
        let (mut ed, id) = editor();
        ed.begin_handle(Handle::Scale { x: 1, y: 1 }, Vec2::new(110.0, 80.0));
        ed.drag_handle(Vec2::new(-90.0, 80.0), HandleMods::default());
        let t = ed.layer_transform(id).unwrap();
        assert!(t.scale.x < 0.0, "{t:?}");
        // right on the opposite side: as thin as allowed, not zero
        ed.drag_handle(Vec2::new(10.0, 80.0), HandleMods::default());
        assert!(ed.layer_transform(id).unwrap().scale.x.abs() >= 0.01);
        ed.end_handle();
    }

    #[test]
    fn rotating_turns_about_the_middle_without_moving_the_drawing() {
        let (mut ed, id) = editor();
        assert_eq!(ed.layer_transform(id).unwrap().pivot, Vec2::ZERO);
        ed.begin_handle(Handle::Rotate, Vec2::new(60.0, 120.0));
        // the pivot was set to the centre of the box, with the layer unmoved
        let c = corners(&ed);
        assert!(
            close(c[0], Vec2::new(10.0, 20.0)) && close(c[2], Vec2::new(110.0, 80.0)),
            "{c:?}"
        );
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(60.0, 50.0)
        ));
        // the pointer goes from straight below the centre to its left: a quarter turn
        ed.drag_handle(Vec2::new(0.0, 50.0), HandleMods::default());
        let t = ed.layer_transform(id).unwrap();
        assert!(
            (t.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-3,
            "{}",
            t.rotation
        );
        assert!(close(
            ed.selection_frame().unwrap().center(),
            Vec2::new(60.0, 50.0)
        ));
        assert_eq!(ed.handle_readout(), Some("90°"));
        ed.end_handle();
        // one undo takes the pivot and the turn back together
        ed.run(Action::Undo);
        let t = ed.layer_transform(id).unwrap();
        assert_eq!((t.rotation, t.pivot), (0.0, Vec2::ZERO));
    }

    #[test]
    fn shift_turns_in_steps_of_fifteen_degrees() {
        let (mut ed, id) = editor();
        ed.begin_handle(Handle::Rotate, Vec2::new(60.0, 120.0));
        // 40 degrees round from straight below
        let a = std::f32::consts::FRAC_PI_2 + 40.0_f32.to_radians();
        ed.drag_handle(
            Vec2::new(60.0, 50.0) + Vec2::from_angle(a) * 70.0,
            HandleMods {
                shift: true,
                ..Default::default()
            },
        );
        let deg = ed.layer_transform(id).unwrap().rotation.to_degrees();
        assert!((deg - 45.0).abs() < 0.01, "{deg}");
        ed.end_handle();
    }

    #[test]
    fn an_animated_rotation_keeps_the_pivot_and_keys_the_turn() {
        let (mut ed, id) = editor();
        ed.frame = 0.0;
        ed.doc
            .find_mut(id)
            .unwrap()
            .transform
            .rotation
            .set_key(0, 0.0, fanimate_core::Interpolation::Linear);
        ed.doc.find_mut(id).unwrap().transform.rotation.set_key(
            10,
            1.0,
            fanimate_core::Interpolation::Linear,
        );
        ed.frame = 4.0;
        ed.begin_handle(Handle::Rotate, Vec2::new(60.0, 120.0));
        assert_eq!(ed.layer_transform(id).unwrap().pivot, Vec2::ZERO);
        assert!(matches!(ed.status, Some((StatusKind::Info, _))));
        ed.drag_handle(Vec2::new(0.0, 50.0), HandleMods::default());
        ed.end_handle();
        let keys = &ed.doc.find(id).unwrap().transform.rotation.keys;
        assert!(keys.iter().any(|k| k.frame == 4), "{keys:?}");
    }

    #[test]
    fn the_pivot_moves_without_moving_the_drawing_and_snaps_to_anchors() {
        let (mut ed, id) = editor();
        let before = corners(&ed);
        ed.begin_handle(Handle::Pivot, Vec2::new(60.0, 50.0));
        // near the bottom right corner: snaps onto it
        ed.drag_handle(
            Vec2::new(108.0, 79.0),
            HandleMods {
                snap: 6.0,
                ..Default::default()
            },
        );
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(110.0, 80.0)
        ));
        assert!(corners(&ed).iter().zip(before).all(|(a, b)| close(*a, b)));
        // away from every anchor: it follows the pointer
        ed.drag_handle(
            Vec2::new(30.0, 70.0),
            HandleMods {
                snap: 2.0,
                ..Default::default()
            },
        );
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(30.0, 70.0)
        ));
        assert!(corners(&ed).iter().zip(before).all(|(a, b)| close(*a, b)));
        assert_eq!(
            ed.selection_frame()
                .unwrap()
                .pivot
                .map(|p| close(p, Vec2::new(30.0, 70.0))),
            Some(true)
        );
        ed.end_handle();
        ed.run(Action::Undo);
        assert_eq!(ed.layer_transform(id).unwrap().pivot, Vec2::ZERO);
    }

    #[test]
    fn a_layer_without_a_pivot_shows_it_at_the_middle_of_its_box() {
        let (mut ed, id) = editor();
        // nothing is stored, but the frame and the grid say where turning will put it
        assert_eq!(ed.doc.find(id).unwrap().transform.pivot, None);
        assert!(close(
            ed.selection_frame().unwrap().pivot.unwrap(),
            Vec2::new(60.0, 50.0)
        ));
        assert!(close(ed.pivot_fraction().unwrap(), Vec2::splat(0.5)));
        assert!(close(ed.layer_pivot(id).unwrap(), Vec2::new(60.0, 50.0)));
        // a layer whose rotation is animated cannot take one cleanly: its origin
        ed.doc
            .find_mut(id)
            .unwrap()
            .transform
            .rotation
            .set_key(0, 0.0, fanimate_core::Interpolation::Linear);
        ed.doc
            .find_mut(id)
            .unwrap()
            .transform
            .rotation
            .set_key(8, 1.0, fanimate_core::Interpolation::Linear);
        ed.frame = 0.0;
        assert!(close(ed.selection_frame().unwrap().pivot.unwrap(), Vec2::ZERO));
    }

    #[test]
    fn the_pivot_set_from_the_nine_anchors_and_read_back() {
        let (mut ed, id) = editor();
        assert!(ed.move_pivot_to_anchor(0.5, 0.5));
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(60.0, 50.0)
        ));
        assert!(close(ed.pivot_fraction().unwrap(), Vec2::splat(0.5)));
        assert!(ed.move_pivot_to_anchor(0.0, 1.0));
        assert!(close(
            ed.layer_transform(id).unwrap().pivot,
            Vec2::new(10.0, 80.0)
        ));
        assert!(close(ed.pivot_fraction().unwrap(), Vec2::new(0.0, 1.0)));
        ed.run(Action::Undo);
        assert!(close(ed.pivot_fraction().unwrap(), Vec2::splat(0.5)));
    }

    #[test]
    fn the_pivot_can_be_set_by_number_without_moving_the_drawing() {
        let (mut ed, id) = editor();
        let before = corners(&ed);
        assert!(ed.set_layer_pivot(id, Vec2::new(33.0, 44.0)));
        assert_eq!(ed.layer_transform(id).unwrap().pivot, Vec2::new(33.0, 44.0));
        assert!(corners(&ed).iter().zip(before).all(|(a, b)| close(*a, b)));
        // not a number: nothing happens
        assert!(ed.set_layer_pivot(id, Vec2::NAN));
        assert_eq!(ed.layer_transform(id).unwrap().pivot, Vec2::new(33.0, 44.0));
    }

    #[test]
    fn several_layers_have_one_box_and_scale_together() {
        let (mut ed, a) = editor();
        let b = second(&mut ed);
        ed.select(a, SelectMode::Toggle);
        assert_eq!(ed.selection.len(), 2);
        let f = ed.selection_frame().unwrap();
        assert!(f.is_multiple() && f.pivot.is_none());
        assert_eq!(f.members.len(), 2);
        assert!(close(f.corners[0], Vec2::new(10.0, 20.0)) && close(f.corners[2], Vec2::new(240.0, 140.0)));
        // a corner dragged to twice the size: everything doubles about the top left
        assert!(ed.begin_handle(Handle::Scale { x: 1, y: 1 }, Vec2::new(240.0, 140.0)));
        ed.drag_handle(Vec2::new(470.0, 260.0), HandleMods::default());
        let c = corners(&ed);
        assert!(
            close(c[0], Vec2::new(10.0, 20.0)) && close(c[2], Vec2::new(470.0, 260.0)),
            "{c:?}"
        );
        ed.end_handle();
        assert!(close(ed.layer_transform(a).unwrap().scale, Vec2::splat(2.0)));
        assert!(close(ed.layer_transform(b).unwrap().scale, Vec2::splat(2.0)));
    }

    #[test]
    fn several_layers_turn_about_the_middle_of_their_box() {
        let (mut ed, a) = editor();
        let b = second(&mut ed);
        ed.select(a, SelectMode::Toggle);
        let centre = ed.selection_frame().unwrap().center();
        assert!(ed.begin_handle(Handle::Rotate, centre + Vec2::new(0.0, 100.0)));
        ed.drag_handle(centre + Vec2::new(-100.0, 0.0), HandleMods::default());
        ed.end_handle();
        for id in [a, b] {
            assert!((ed.layer_transform(id).unwrap().rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-3);
        }
        // the first layer's middle went a quarter turn round the middle of the box
        ed.select(a, SelectMode::Replace);
        let moved = ed.selection_frame().unwrap().center();
        let d = Vec2::new(60.0, 50.0) - centre;
        assert!(close(moved, centre + Vec2::new(-d.y, d.x)), "{moved}");
    }

    #[test]
    fn a_stretch_a_turned_layer_cannot_take_becomes_an_even_scale() {
        let (mut ed, a) = editor();
        let b = second(&mut ed);
        ed.select(a, SelectMode::Toggle);
        let mut t = ed.layer_transform(b).unwrap();
        t.rotation = 0.5;
        ed.set_layer_transform(b, t);
        let f = ed.selection_frame().unwrap();
        assert!(!f.free_scale, "a turned layer ends free stretching");
        // a side dragged out would shear the turned layer: it grows evenly
        assert!(ed.begin_handle(Handle::Scale { x: 1, y: 0 }, f.scale_handle(1, 0)));
        ed.drag_handle(
            f.scale_handle(1, 0) + Vec2::new(100.0, 0.0),
            HandleMods::default(),
        );
        for id in [a, b] {
            let s = ed.layer_transform(id).unwrap().scale;
            assert!((s.x - s.y).abs() < 1e-3, "{s}");
        }
        ed.end_handle();
    }

    #[test]
    fn nothing_to_drag_without_a_selection_or_with_the_pivot_of_several() {
        let mut ed = Editor::untitled();
        ed.selection.clear();
        assert!(!ed.begin_handle(Handle::Rotate, Vec2::ZERO));
        ed.drag_handle(Vec2::ONE, HandleMods::default());
        assert!(!ed.is_dragging_handle());
        let (mut ed, a) = editor();
        second(&mut ed);
        ed.select(a, SelectMode::Toggle);
        assert!(!ed.begin_handle(Handle::Pivot, Vec2::ZERO));
        assert!(matches!(
            ed.doc.find(a).map(|l| &l.kind),
            Some(LayerKind::Vector { .. })
        ));
    }
}
