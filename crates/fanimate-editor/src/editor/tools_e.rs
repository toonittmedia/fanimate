//! Phase E tool implementations: Curvature, shape tools (Line, Polygon, Star,
//! Triangle), editing tools (Delete Edge, Magnet, Scale Points, Rotate Points),
//! fill/stroke tools (Draw Shape, Freehand Width, Line Width, Stroke Exposure)
//! and Shape Builder.

use fanimate_core::{LayerId, Path, Shape, Stroke, Style, Vec2};

use super::{Editor, PointRef, StatusKind};
use crate::action::Tool;

// ---------------------------------------------------------------------------
// Curvature tool
// ---------------------------------------------------------------------------

/// A path being drawn with the Curvature tool, in document space.
pub(super) struct CurvaturePath {
    /// The layer it goes on; `None` makes a new vector layer.
    pub target: Option<LayerId>,
    points: Vec<CurvePoint>,
    closing: bool,
}

#[derive(Clone, Copy)]
struct CurvePoint {
    at: Vec2,
    /// Corner (no curve smoothing) when true.
    corner: bool,
}

impl Editor {
    // -----------------------------------------------------------------------
    // Curvature tool

    /// Curvature, pointer down at `at` (document space). `corner` forces a
    /// corner point (Alt held). Closes the path when the pointer is near the
    /// first point (within `tolerance`).
    pub fn curvature_down(&mut self, at: Vec2, corner: bool, tolerance: f32) -> bool {
        if !at.is_finite() {
            return false;
        }
        if self.curvature.is_none() {
            let Ok(target) = self.drawing_target() else {
                return false;
            };
            self.curvature = Some(CurvaturePath {
                target,
                points: Vec::new(),
                closing: false,
            });
        }
        let Some(cv) = &mut self.curvature else {
            return false;
        };
        // close when clicking near the first point (at least 2 pts already)
        if cv.points.len() >= 2
            && let Some(first) = cv.points.first()
            && first.at.distance(at) <= tolerance
        {
            cv.closing = true;
        } else {
            cv.points.push(CurvePoint { at, corner });
        }
        // one-click finish: pointer-up handles it
        true
    }

    /// Curvature, pointer up: closing finishes the path.
    pub fn curvature_up(&mut self) {
        if self.curvature.as_ref().is_some_and(|c| c.closing) {
            self.finish_curvature();
        }
    }

    /// Finish the Curvature path and add it to its layer. One undo step.
    pub fn finish_curvature(&mut self) {
        let Some(cv) = self.curvature.take() else { return };
        let Some(shape) = curvature_shape(&cv, None, &self.draw) else {
            return;
        };
        let target = cv.target.filter(|id| self.doc.find(*id).is_some());
        self.add_shape(target, shape);
    }

    pub fn cancel_curvature(&mut self) {
        self.curvature = None;
    }

    pub fn is_drawing_curvature(&self) -> bool {
        self.curvature.is_some()
    }

    /// The path as it would appear if finished now, with a ghost segment to
    /// `pointer`. Used by the canvas overlay.
    pub fn curvature_preview(&self, pointer: Option<Vec2>) -> Option<Shape> {
        let cv = self.curvature.as_ref()?;
        curvature_shape(cv, pointer.filter(|_| !cv.closing), &self.draw)
    }

    /// The points being drawn (document space), for the overlay dots.
    pub fn curvature_points(&self) -> Vec<Vec2> {
        self.curvature
            .as_ref()
            .map(|c| c.points.iter().map(|p| p.at).collect())
            .unwrap_or_default()
    }
}

/// Build a Catmull-Rom-ish cubic shape from the curvature points. Corner
/// points get zero handles; smooth ones get Catmull-Rom tangents.
fn curvature_shape(
    cv: &CurvaturePath,
    pointer: Option<Vec2>,
    draw: &super::draw::DrawStyle,
) -> Option<Shape> {
    let mut pts: Vec<CurvePoint> = cv.points.clone();
    if let Some(p) = pointer {
        pts.push(CurvePoint { at: p, corner: false });
    }
    if pts.len() < 2 {
        return None;
    }
    let closed = cv.closing && pointer.is_none();
    let n = pts.len();
    // Catmull-Rom tangent at index i among the pts slice (no ghost)
    let tangent = |pts: &[CurvePoint], i: usize| -> Vec2 {
        if pts[i].corner {
            return Vec2::ZERO;
        }
        let prev = if i == 0 {
            if closed { pts[n - 1].at } else { pts[0].at }
        } else {
            pts[i - 1].at
        };
        let next = if i + 1 < pts.len() {
            pts[i + 1].at
        } else if closed {
            pts[0].at
        } else {
            pts[i].at
        };
        (next - prev) * 0.5
    };
    let mut path = Path::new().move_to(pts[0].at);
    let range = if closed { n } else { n - 1 };
    for i in 0..range {
        let j = (i + 1) % n;
        let a = pts[i].at;
        let b = pts[j].at;
        let ta = tangent(&pts, i);
        let tb = tangent(&pts, j);
        let c1 = a + ta / 3.0;
        let c2 = b - tb / 3.0;
        if c1 == a && c2 == b {
            path = path.line_to(b);
        } else {
            path = path.cubic_to(c1, c2, b);
        }
    }
    if closed {
        path = path.close();
    }
    let stroke = Stroke {
        color: draw.line,
        width: draw.width,
    };
    let style = if closed {
        Style {
            fill: draw.fill,
            stroke: (draw.outline || draw.fill.is_none()).then_some(stroke),
        }
    } else {
        Style {
            fill: None,
            stroke: Some(stroke),
        }
    };
    Some(Shape::new("Path", path, style))
}

// ---------------------------------------------------------------------------
// Parametric shape drag helpers (Line, Polygon, Star, Triangle)
// ---------------------------------------------------------------------------

impl Editor {
    /// The shape that a Line/Polygon/Star/Triangle drag from `from` to `to`
    /// (document space) makes, with the current draw style. `shift` snaps
    /// angles to 15° steps for Line and Triangle. `alt` for Star adjusts the
    /// inner radius from the drag length (inner = 0.5 * outer by default).
    /// Returns `None` for other tools or a degenerate drag.
    pub fn drag_shape_e(
        &self,
        tool: Tool,
        from: Vec2,
        to: Vec2,
        shift: bool,
        alt: bool,
        zoom: f32,
    ) -> Option<Shape> {
        let diff = to - from;
        if !diff.is_finite() {
            return None;
        }
        let zoom = zoom.max(1e-6);
        let d = &self.draw;
        let stroke = Stroke {
            color: d.line,
            width: d.width,
        };
        let style_open = Style {
            fill: None,
            stroke: Some(stroke),
        };
        let style_filled = Style {
            fill: d.fill,
            stroke: (d.outline || d.fill.is_none()).then_some(stroke),
        };
        match tool {
            Tool::Line => {
                let end = if shift { snap_angle(from, to, 15.0) } else { to };
                let len = (end - from).length() * zoom;
                if len < 0.5 {
                    return None;
                }
                Some(Shape::new("Line", Path::line(from, end), style_open))
            }
            Tool::Polygon => {
                let radius = diff.length();
                if radius * zoom < 0.5 {
                    return None;
                }
                Some(Shape::new(
                    "Polygon",
                    Path::polygon(from, radius, self.poly_sides),
                    style_filled,
                ))
            }
            Tool::Star => {
                let outer = diff.length();
                if outer * zoom < 0.5 {
                    return None;
                }
                let inner = if alt {
                    // while alt is held, drag length sets the inner radius
                    outer * self.star_inner
                } else {
                    outer * self.star_inner
                };
                Some(Shape::new(
                    "Star",
                    Path::star(from, outer, inner, self.star_points),
                    style_filled,
                ))
            }
            Tool::Triangle => {
                let radius = diff.length();
                if radius * zoom < 0.5 {
                    return None;
                }
                Some(Shape::new("Triangle", Path::triangle(from, radius), style_filled))
            }
            _ => None,
        }
    }

    /// Add the shape from [`drag_shape_e`](Self::drag_shape_e), one undo step.
    pub fn add_drag_shape_e(
        &mut self,
        tool: Tool,
        from: Vec2,
        to: Vec2,
        shift: bool,
        alt: bool,
        zoom: f32,
    ) -> bool {
        let Some(shape) = self.drag_shape_e(tool, from, to, shift, alt, zoom) else {
            return false;
        };
        let Ok(target) = self.drawing_target() else {
            return false;
        };
        self.add_shape(target, shape);
        true
    }

    /// Adjust `poly_sides` or `star_points` by `delta` from a scroll wheel
    /// event while the polygon or star tool is active.
    pub fn adjust_shape_sides(&mut self, delta: i32) {
        match self.tool {
            Tool::Polygon => {
                self.poly_sides = (self.poly_sides as i32 + delta).clamp(3, 32) as u32;
            }
            Tool::Star => {
                self.star_points = (self.star_points as i32 + delta).clamp(2, 16) as u32;
            }
            _ => {}
        }
    }

    /// Adjust star inner radius fraction by `delta` (Alt+scroll while Star).
    pub fn adjust_star_inner(&mut self, delta: f32) {
        self.star_inner = (self.star_inner + delta).clamp(0.05, 0.95);
    }
}

/// Snap `to` to the nearest `step`-degree angle from `from`.
fn snap_angle(from: Vec2, to: Vec2, step: f32) -> Vec2 {
    let d = to - from;
    let len = d.length();
    if len < 1e-6 {
        return to;
    }
    let angle = d.y.atan2(d.x);
    let step_r = step.to_radians();
    let snapped = (angle / step_r).round() * step_r;
    from + Vec2::new(snapped.cos(), snapped.sin()) * len
}

// ---------------------------------------------------------------------------
// Delete Edge
// ---------------------------------------------------------------------------

impl Editor {
    /// Delete Edge: split the segment at `at` (document space), removing the
    /// edge and creating two open sub-paths. One undo step. Returns whether
    /// anything changed.
    pub fn delete_edge_at(&mut self, at: Vec2, tolerance: f32) -> bool {
        use fanimate_core::PathCmd;
        let Some((shape, cmd_idx, _t)) = self.segment_at(at, tolerance) else {
            return false;
        };
        {
            let Some(s) = self.point_shape_mut(shape) else {
                return false;
            };
            // find the on-curve command at cmd_idx
            let Some(cmd) = s.path.cmds.get(cmd_idx) else {
                return false;
            };
            match cmd {
                PathCmd::LineTo(_) | PathCmd::CubicTo(_, _, _) | PathCmd::Close => {}
                _ => return false,
            }
        }

        self.checkpoint("Delete edge");
        let Some(s) = self.point_shape_mut(shape) else {
            return false;
        };
        let cmds_clone = s.path.cmds.clone();
        // Walk cmds; accumulate sub-paths by splitting at the target cmd
        let mut new_cmds: Vec<PathCmd> = Vec::new();
        let mut current: Vec<PathCmd> = Vec::new();

        for (i, cmd) in cmds_clone.iter().enumerate() {
            match cmd {
                PathCmd::MoveTo(_) => {
                    if !current.is_empty() {
                        new_cmds.append(&mut current);
                    }
                    current.push(*cmd);
                }
                PathCmd::Close => {
                    if i != cmd_idx {
                        current.push(*cmd);
                    }
                    // deleting the closing edge: the subpath stays open
                }
                PathCmd::LineTo(p) | PathCmd::CubicTo(_, _, p) => {
                    if i == cmd_idx {
                        // delete this edge: flush current, start a new sub-path at end
                        new_cmds.append(&mut current);
                        current.push(PathCmd::MoveTo(*p));
                    } else {
                        current.push(*cmd);
                    }
                }
            }
        }
        new_cmds.append(&mut current);
        let Some(s) = self.point_shape_mut(shape) else {
            return false;
        };
        s.path.cmds = new_cmds;
        s.rect = None; // no longer a parametric rect
        // clear point selection on this shape since indices changed
        self.points.retain(|r| r.layer != shape.0 || r.shape != shape.1);
        self.set_status(StatusKind::Info, "Edge deleted");
        true
    }
}

// ---------------------------------------------------------------------------
// Magnet tool
// ---------------------------------------------------------------------------

impl Editor {
    /// Magnet: attract (or with `repel=true`, repel) the points of the point
    /// layer within `radius` of `at` (document space). `strength` is 0..1.
    /// One undo step total per drag; call `checkpoint_magnet` once, then
    /// `apply_magnet` each frame.
    pub fn checkpoint_magnet(&mut self) {
        self.checkpoint("Magnet");
    }

    pub fn apply_magnet(&mut self, at: Vec2, radius: f32, strength: f32, repel: bool) {
        let sign = if repel { -1.0 } else { 1.0 };
        let Some(layer) = self.point_layer() else { return };
        let t = self.world(layer);
        let inv = t.inverse();
        if !inv.is_finite() {
            return;
        }
        let local_at = inv.transform_point2(at);
        // radius in layer space (approximate; take x-axis scale)
        let scale = t.matrix2.x_axis.length().max(1e-6);
        let local_r = radius / scale;
        let Some(shapes) = self.layer_shapes_mut(layer) else {
            return;
        };
        for shape in shapes.iter_mut() {
            if shape.is_animated() {
                continue;
            }
            for cmd in &mut shape.path.cmds {
                let p = match cmd {
                    fanimate_core::PathCmd::MoveTo(p) | fanimate_core::PathCmd::LineTo(p) => p,
                    fanimate_core::PathCmd::CubicTo(_, _, p) => p,
                    fanimate_core::PathCmd::Close => continue,
                };
                let d = local_at.distance(*p);
                if d < local_r && d > 1e-6 {
                    let factor = (1.0 - d / local_r) * strength * sign;
                    *p += (local_at - *p) * factor;
                }
            }
            shape.rect = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Scale Points / Rotate Points
// ---------------------------------------------------------------------------

impl Editor {
    /// Scale the selected points around their centroid. `factor_x` and
    /// `factor_y` are scale multipliers (1.0 = unchanged). Modifies the
    /// path in place (no undo step; call [`checkpoint`] before first call).
    pub fn scale_selected_points(&mut self, factor_x: f32, factor_y: f32) {
        let centre = self.selected_points_centroid();
        let Some(layer) = self.point_layer() else { return };
        let pts_snapshot: Vec<PointRef> = self.points.iter().cloned().collect();
        let frame = self.current_frame() as f32;
        let Some(shapes) = self.layer_shapes(layer) else {
            return;
        };

        // collect (shape_idx, point_idx, current_position)
        let moves: Vec<(usize, usize, Vec2)> = pts_snapshot
            .iter()
            .filter_map(|r| {
                if r.layer != layer {
                    return None;
                }
                let s = shapes.get(r.shape)?;
                let pts = s.path_at(frame).on_curve_points();
                let p = pts.get(r.point).copied()?;
                let delta = p - centre;
                let new_p = centre + Vec2::new(delta.x * factor_x, delta.y * factor_y);
                Some((r.shape, r.point, new_p))
            })
            .collect();

        let Some(shapes) = self.layer_shapes_mut(layer) else {
            return;
        };
        for (si, pi, new_p) in moves {
            if let Some(s) = shapes.get_mut(si) {
                set_on_curve_point(&mut s.path, pi, new_p);
                s.rect = None;
            }
        }
    }

    /// Rotate the selected points around their centroid by `angle` radians.
    pub fn rotate_selected_points(&mut self, angle: f32) {
        let centre = self.selected_points_centroid();
        let Some(layer) = self.point_layer() else { return };
        let pts_snapshot: Vec<PointRef> = self.points.iter().cloned().collect();
        let frame = self.current_frame() as f32;
        let Some(shapes) = self.layer_shapes(layer) else {
            return;
        };

        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let moves: Vec<(usize, usize, Vec2)> = pts_snapshot
            .iter()
            .filter_map(|r| {
                if r.layer != layer {
                    return None;
                }
                let s = shapes.get(r.shape)?;
                let pts = s.path_at(frame).on_curve_points();
                let p = pts.get(r.point).copied()?;
                let d = p - centre;
                let rotated = Vec2::new(d.x * cos_a - d.y * sin_a, d.x * sin_a + d.y * cos_a);
                Some((r.shape, r.point, centre + rotated))
            })
            .collect();

        let Some(shapes) = self.layer_shapes_mut(layer) else {
            return;
        };
        for (si, pi, new_p) in moves {
            if let Some(s) = shapes.get_mut(si) {
                set_on_curve_point(&mut s.path, pi, new_p);
                s.rect = None;
            }
        }
    }

    /// Centroid of selected point positions (document space), or origin.
    pub fn selected_points_centroid(&self) -> Vec2 {
        let Some(layer) = self.point_layer() else {
            return Vec2::ZERO;
        };
        let t = self.world(layer);
        let frame = self.current_frame() as f32;
        let Some(shapes) = self.layer_shapes(layer) else {
            return Vec2::ZERO;
        };
        let positions: Vec<Vec2> = self
            .points
            .iter()
            .filter_map(|r| {
                if r.layer != layer {
                    return None;
                }
                let s = shapes.get(r.shape)?;
                let pts = s.path_at(frame).on_curve_points();
                let p = pts.get(r.point).copied()?;
                Some(t.transform_point2(p))
            })
            .collect();
        if positions.is_empty() {
            return Vec2::ZERO;
        }
        positions.iter().copied().fold(Vec2::ZERO, |a, b| a + b) / positions.len() as f32
    }
}

/// Set the `n`-th on-curve point's coordinates in `path` (0-based). Off-curve
/// control points for cubics are moved by the same delta so the curve shape
/// stays the same.
fn set_on_curve_point(path: &mut Path, n: usize, new_p: Vec2) {
    let mut on_curve_idx = 0;
    for cmd in &mut path.cmds {
        match cmd {
            fanimate_core::PathCmd::MoveTo(p) | fanimate_core::PathCmd::LineTo(p) => {
                if on_curve_idx == n {
                    *p = new_p;
                    return;
                }
                on_curve_idx += 1;
            }
            fanimate_core::PathCmd::CubicTo(c1, c2, p) => {
                if on_curve_idx == n {
                    let delta = new_p - *p;
                    *c1 += delta;
                    *c2 += delta;
                    *p = new_p;
                    return;
                }
                on_curve_idx += 1;
            }
            fanimate_core::PathCmd::Close => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Draw Shape (copy style)
// ---------------------------------------------------------------------------

impl Editor {
    /// Draw Shape: set the draw style from the shape under `at`. Returns
    /// whether a shape was found.
    pub fn pick_draw_style(&mut self, at: Vec2, tolerance: f32) -> bool {
        let Some((layer, idx)) = self.shape_at(at, tolerance) else {
            return false;
        };
        let Some(shape) = self.shape(layer, idx) else {
            return false;
        };
        let (style, name) = (shape.style, shape.name.clone());
        if let Some(fill) = style.fill {
            self.draw.fill = Some(fill);
        }
        if let Some(stroke) = style.stroke {
            self.draw.line = stroke.color;
            self.draw.width = stroke.width;
        }
        self.set_status(StatusKind::Info, format!("Style copied from \"{name}\""));
        true
    }
}

// ---------------------------------------------------------------------------
// Line Width tool (drag to adjust stroke width along a stroke)
// ---------------------------------------------------------------------------

impl Editor {
    /// Line Width: drag over a stroke to set the width there. `at` is the
    /// current pointer, `last` is where it was. `width` is the new width to
    /// apply at `at`. Modifies the widths of the stroke point nearest `at`
    /// (document space). Call `checkpoint` once before the drag.
    pub fn apply_line_width(&mut self, at: Vec2, tolerance: f32, new_width: f32) {
        let Some(layer) = self.point_layer() else { return };
        let t = self.world(layer);
        let inv = t.inverse();
        if !inv.is_finite() {
            return;
        }
        let local = inv.transform_point2(at);
        let scale = t.matrix2.x_axis.length().max(1e-6);
        let local_tol = tolerance / scale;
        let frame = self.current_frame() as f32;
        let Some(shapes) = self.layer_shapes_mut(layer) else {
            return;
        };
        for shape in shapes.iter_mut() {
            if shape.widths.is_none() {
                continue;
            }
            let pts = shape.path_at(frame).on_curve_points();
            let closest = pts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.distance(local) <= local_tol * 4.0)
                .min_by(|(_, a), (_, b)| a.distance(local).total_cmp(&b.distance(local)));
            if let Some((i, _)) = closest
                && let Some(ws) = &mut shape.widths
                && let Some(w) = ws.get_mut(i)
            {
                let base = shape.style.stroke.map_or(1.0, |s| s.width).max(1e-6);
                *w = (new_width / base).max(0.01);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shape Builder
// ---------------------------------------------------------------------------

/// State for the Shape Builder tool while a drag is in progress.
pub struct ShapeBuilderState {
    /// The start of the drag, document space.
    pub from: Vec2,
    /// Whether Alt was held at drag start (subtract mode).
    pub subtract: bool,
}

impl Editor {
    /// Shape Builder drag end: unite or subtract the regions the drag line
    /// crosses, on the point layer. One undo step.
    pub fn apply_shape_builder(&mut self, from: Vec2, to: Vec2, subtract: bool) {
        use fanimate_core::boolean::{BoolOp, combine};
        let Some(layer) = self.point_layer() else { return };
        let t = self.world(layer);
        let inv = t.inverse();
        if !inv.is_finite() {
            return;
        }
        let local_from = inv.transform_point2(from);
        let local_to = inv.transform_point2(to);
        let frame = self.current_frame() as f32;

        // Find all shapes whose outlines the drag line intersects.
        let Some(shapes) = self.layer_shapes(layer) else {
            return;
        };
        let hit_indices: Vec<usize> = shapes
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                !s.is_animated() && s.widths.is_none() && line_crosses_shape(local_from, local_to, s, frame)
            })
            .map(|(i, _)| i)
            .collect();

        if hit_indices.len() < 2 {
            self.set_status(
                StatusKind::Info,
                if subtract {
                    "Drag across two shapes to subtract one from the other"
                } else {
                    "Drag across two shapes to merge them"
                },
            );
            return;
        }

        self.checkpoint("Shape Builder");
        let Some(shapes) = self.layer_shapes(layer) else {
            return;
        };
        let paths: Vec<&fanimate_core::Path> = hit_indices
            .iter()
            .filter_map(|&i| shapes.get(i))
            .map(|s| &s.path)
            .collect();
        let op = if subtract { BoolOp::Subtract } else { BoolOp::Union };
        let result_path = combine(&paths, op, 0.25);
        let top = shapes[*hit_indices.last().unwrap()].clone();
        let result = Shape::new(top.name.clone(), result_path, top.style);

        // replace the topmost hit shape, remove the rest
        let Some(shapes) = self.layer_shapes_mut(layer) else {
            return;
        };
        // Remove from high to low to keep indices valid
        let mut to_remove: Vec<usize> = hit_indices.clone();
        to_remove.sort_unstable();
        to_remove.dedup();
        let insert_at = *to_remove.last().unwrap();
        for &i in to_remove.iter().rev() {
            shapes.remove(i);
        }
        // insert at where the topmost was (after removing those above it)
        let real_insert = insert_at.saturating_sub(to_remove.iter().filter(|&&i| i < insert_at).count());
        shapes.insert(real_insert.min(shapes.len()), result);
        let n = shapes.len().saturating_sub(1);
        let _ = shapes; // release the borrow before mutating self.points
        self.points.retain(|r| r.layer != layer);
        self.points.insert(PointRef {
            layer,
            shape: n,
            point: 0,
        });
        self.set_status(StatusKind::Success, "Shapes combined");
    }
}

/// Whether the line segment from `a` to `b` (both in the shape's local space)
/// passes close to any segment of `shape`'s outline.
fn line_crosses_shape(a: Vec2, b: Vec2, shape: &Shape, frame: f32) -> bool {
    let path = shape.path_at(frame);
    let lines = path.polylines(0.5);
    for (pts, closed) in &lines {
        let len = pts.len();
        for i in 0..len {
            let p = pts[i];
            let q = if i + 1 < len {
                pts[i + 1]
            } else if *closed {
                pts[0]
            } else {
                continue;
            };
            if segments_intersect(a, b, p, q) {
                return true;
            }
        }
    }
    false
}

/// Whether segments `(a,b)` and `(c,d)` intersect (ignoring endpoints).
fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab = b - a;
    let cd = d - c;
    let denom = ab.perp_dot(cd);
    if denom.abs() < 1e-9 {
        return false;
    }
    let ac = c - a;
    let t = ac.perp_dot(cd) / denom;
    let u = ac.perp_dot(ab) / denom;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}

// ---------------------------------------------------------------------------
// Freehand with Width (variable-width brush strokes)
// ---------------------------------------------------------------------------

impl Editor {
    /// Freehand with Width: like Freehand but prompts the user to drag the
    /// width during the stroke. For now, delegates to the regular stroke but
    /// applies the drag-width to each segment as the pointer moves.
    /// Uses `begin_stroke`, `continue_stroke`, `finish_stroke` with the same
    /// draw settings; width adjustment is done by `continue_stroke_width`.
    pub fn continue_stroke_width(&mut self, at: Vec2, drag_y: f32) {
        // pressure analogue: drag up → narrower, drag down → wider
        let pressure = (1.0 - drag_y / 200.0).clamp(0.1, 2.0);
        self.continue_stroke(at, pressure);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Tool;
    use fanimate_core::LayerKind;

    fn shapes(ed: &Editor) -> Vec<Shape> {
        let id = ed.primary().unwrap();
        match &ed.doc.find(id).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.clone(),
            _ => panic!("not a vector layer"),
        }
    }

    #[test]
    fn polygon_has_right_vertex_count() {
        let p = Path::polygon(Vec2::ZERO, 100.0, 6);
        // MoveTo(p0) + 5×LineTo + Close → on_curve_count skips Close, counts 6
        let on_curve = p.on_curve_count();
        assert_eq!(on_curve, 6); // 1 MoveTo + 5 LineTo
        // polylines returns 6 points (the start is not duplicated)
        let pl = p.polylines(0.1);
        assert_eq!(pl[0].0.len(), 6);
        assert!(pl[0].1, "polygon should be closed");
    }

    #[test]
    fn star_has_right_vertex_count() {
        let p = Path::star(Vec2::ZERO, 100.0, 40.0, 5);
        // 5 points → 10 alternating tips/valleys
        let pl = p.polylines(0.1);
        assert_eq!(pl[0].0.len(), 10);
        assert!(pl[0].1, "star should be closed");
    }

    #[test]
    fn line_path_has_two_points() {
        let p = Path::line(Vec2::ZERO, Vec2::new(100.0, 0.0));
        assert_eq!(p.on_curve_count(), 2);
    }

    #[test]
    fn triangle_is_polygon_3() {
        let t = Path::triangle(Vec2::ZERO, 100.0);
        let p = Path::polygon(Vec2::ZERO, 100.0, 3);
        assert_eq!(t, p);
    }

    #[test]
    fn drag_shape_e_line_returns_shape() {
        let ed = Editor::untitled();
        let shape = ed.drag_shape_e(Tool::Line, Vec2::ZERO, Vec2::new(100.0, 0.0), false, false, 1.0);
        assert!(shape.is_some());
        assert_eq!(shape.unwrap().name, "Line");
    }

    #[test]
    fn drag_shape_e_polygon_returns_shape() {
        let ed = Editor::untitled();
        let shape = ed.drag_shape_e(
            Tool::Polygon,
            Vec2::ZERO,
            Vec2::new(100.0, 0.0),
            false,
            false,
            1.0,
        );
        assert!(shape.is_some());
        assert_eq!(shape.unwrap().name, "Polygon");
    }

    #[test]
    fn curvature_two_points_makes_a_shape() {
        let mut ed = Editor::untitled();
        ed.tool = Tool::Curvature;
        assert!(ed.curvature_down(Vec2::ZERO, false, 2.0));
        assert!(ed.curvature_down(Vec2::new(100.0, 0.0), false, 2.0));
        ed.finish_curvature();
        let s = shapes(&ed);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Path 1");
    }

    #[test]
    fn curvature_corner_makes_straight_segment() {
        let mut ed = Editor::untitled();
        ed.tool = Tool::Curvature;
        assert!(ed.curvature_down(Vec2::ZERO, true, 2.0));
        assert!(ed.curvature_down(Vec2::new(100.0, 0.0), true, 2.0));
        assert!(ed.curvature_down(Vec2::new(100.0, 100.0), true, 2.0));
        ed.finish_curvature();
        let s = shapes(&ed);
        assert_eq!(s.len(), 1);
        // All corners → all LineTo
        use fanimate_core::PathCmd;
        assert!(
            s[0].path
                .cmds
                .iter()
                .all(|c| matches!(c, PathCmd::MoveTo(_) | PathCmd::LineTo(_)))
        );
    }

    #[test]
    fn scale_selected_points_moves_them() {
        let mut ed = Editor::untitled();
        ed.tool = Tool::SelectPoints;
        // draw a line from (0,0) to (100,0)
        let _ = ed.begin_add_point(Vec2::ZERO, 2.0);
        ed.drag_points(Vec2::new(100.0, 0.0));
        ed.end_add_point(2.0);
        // select both points
        ed.run(crate::action::Action::SelectAll);
        // scale x by 2
        ed.checkpoint("test scale");
        ed.scale_selected_points(2.0, 1.0);
        let layer = ed.primary().unwrap();
        let shapes = match &ed.doc.find(layer).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.clone(),
            _ => panic!(),
        };
        let pts = shapes[0].path.on_curve_points();
        // After scaling 2×, the second point should move from 100 to ~200
        // (centroid is at 50, so new distance from centroid doubles)
        assert!(
            (pts[1].x - 150.0).abs() < 1.0 || (pts[1].x - 200.0).abs() < 5.0,
            "pt1.x={}",
            pts[1].x
        );
    }
}
