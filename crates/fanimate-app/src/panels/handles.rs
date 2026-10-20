//! The selection frame on the canvas: the box around what is selected, its
//! handles (scale, a Canva-style rotate button, the pivot), the corner
//! radius handles of a rectangle, and the outline of the shape under the
//! pointer.
//!
//! The box and the handles take the colour of the layer's label, so they
//! read as that layer's. What they do is in `fanimate_editor` (and tested
//! there); this module draws them, finds which one the pointer is on, and
//! passes the drag on.

use eframe::egui::{self, Color32, CursorIcon, Pos2, Rect, Stroke, vec2};
use fanimate_core::Vec2;
use fanimate_editor::{Handle, HandleMods, RadiusTarget, SelectionFrame};

use crate::app::Shell;
use crate::icons::{self, Icon};

/// Side of a scale handle, in points.
const SQUARE: f32 = 8.0;
/// How near the pointer must be to a handle to take it.
const GRAB: f32 = 8.0;
/// How far below the box the rotate button sits, and its radius.
const ROTATE_GAP: f32 = 30.0;
const ROTATE_RADIUS: f32 = 11.0;
/// Turning by dragging just outside a corner: how far out the zone reaches.
const CORNER_ZONE: f32 = 26.0;
/// Radius handles sit at least this far in from their corner, so they can be
/// grabbed on a square corner.
const RADIUS_INSET: f32 = 18.0;
/// The pivot snaps to the box's anchors within this distance.
const SNAP: f32 = 8.0;

/// Document space and the screen, for one frame of the canvas.
#[derive(Clone, Copy)]
pub struct Screen {
    pub origin: Pos2,
    pub center: Vec2,
    pub zoom: f32,
}

impl Screen {
    pub fn point(&self, p: Vec2) -> Pos2 {
        let d = (p - self.center) * self.zoom;
        self.origin + vec2(d.x, d.y)
    }

    pub fn doc(&self, p: Pos2) -> Vec2 {
        let d = p - self.origin;
        self.center + Vec2::new(d.x, d.y) / self.zoom
    }
}

/// What the pointer is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Handle(Handle),
    Radius(usize),
}

/// The gesture in progress.
#[derive(Clone, Copy, Debug)]
pub enum Gesture {
    /// A handle of the selection frame (the editor holds the rest), and
    /// whether it scales (the one that snaps).
    Frame { scales: bool },
    /// A corner radius: which corner, and how far the pointer's radius was
    /// from the real one when it was grabbed, so the rectangle does not jump.
    Radius { corner: usize, grab: f32 },
}

/// The outline of the shape under the pointer, kept while nothing changes.
pub struct HoverCache {
    pub key: (Pos2, u64, i32, u32, u32),
    pub outline: Vec<(Vec<Vec2>, bool)>,
}

fn to_vec(p: Pos2) -> egui::Vec2 {
    p.to_vec2()
}

/// The label colour of the layer the frame belongs to.
fn frame_color(shell: &Shell, frame: &SelectionFrame) -> Color32 {
    shell
        .editor
        .doc
        .find(frame.primary)
        .map_or(shell.palette.accent, |l| {
            crate::theme::label_color(l.shown_label())
        })
}

/// Where the rotate button is, on screen.
fn rotate_button(screen: &Screen, frame: &SelectionFrame) -> Pos2 {
    let b = frame.below();
    screen.point(frame.edge(2)) + vec2(b.x, b.y) * ROTATE_GAP
}

/// The handles of the frame, with their screen positions. Edge handles only
/// where the sides can be stretched on their own.
fn scale_handles(screen: &Screen, frame: &SelectionFrame) -> Vec<(Handle, Pos2)> {
    let mut out = Vec::new();
    for (x, y) in [(-1, -1), (1, -1), (1, 1), (-1, 1)] {
        out.push((Handle::Scale { x, y }, screen.point(frame.scale_handle(x, y))));
    }
    if frame.free_scale {
        for (x, y) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            out.push((Handle::Scale { x, y }, screen.point(frame.scale_handle(x, y))));
        }
    }
    out
}

/// What is under `pos`, nearest first: a radius handle, the pivot, the
/// rotate button, a scale handle, then the zone just outside a corner,
/// which turns too.
pub fn hit_test(
    screen: &Screen,
    frame: Option<&SelectionFrame>,
    radius: Option<&RadiusTarget>,
    pos: Pos2,
) -> Option<Hit> {
    if let Some(target) = radius {
        for corner in 0..4 {
            let at = screen.point(target.handle(corner, radius_inset(screen, target, corner)));
            if at.distance(pos) <= GRAB {
                return Some(Hit::Radius(corner));
            }
        }
    }
    let frame = frame?;
    if let Some(pivot) = frame.pivot
        && screen.point(pivot).distance(pos) <= GRAB - 2.0
    {
        return Some(Hit::Handle(Handle::Pivot));
    }
    if rotate_button(screen, frame).distance(pos) <= ROTATE_RADIUS + 2.0 {
        return Some(Hit::Handle(Handle::Rotate));
    }
    let mut best: Option<(f32, Handle)> = None;
    for (handle, at) in scale_handles(screen, frame) {
        let d = at.distance(pos);
        if d <= GRAB && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, handle));
        }
    }
    if let Some((_, handle)) = best {
        return Some(Hit::Handle(handle));
    }
    // just outside a corner, where the box is not: turn
    let centre = screen.point(frame.center());
    for corner in frame.corners {
        let c = screen.point(corner);
        let outward = to_vec(c) - to_vec(centre);
        let d = to_vec(pos) - to_vec(c);
        if outward.length() > 1.0
            && d.length() <= CORNER_ZONE
            && d.dot(outward / outward.length()) > GRAB / 2.0
        {
            return Some(Hit::Handle(Handle::Rotate));
        }
    }
    None
}

/// How far in from its corner a radius handle is drawn: the radius, but
/// never so close to the corner it cannot be grabbed.
fn radius_inset(screen: &Screen, target: &RadiusTarget, corner: usize) -> f32 {
    let reach = (RADIUS_INSET / screen.zoom).min(target.rect.max_radius());
    target.rect.corner_radii()[corner % 4].max(reach)
}

/// The cursor for what is under the pointer.
pub fn cursor(screen: &Screen, frame: Option<&SelectionFrame>, hit: Hit) -> CursorIcon {
    match hit {
        Hit::Radius(_) => CursorIcon::Crosshair,
        Hit::Handle(Handle::Pivot) => CursorIcon::Move,
        // Windows has no rotate cursor to ask for; the crosshair says "a
        // handle of its own"
        Hit::Handle(Handle::Rotate) => CursorIcon::Crosshair,
        Hit::Handle(Handle::Scale { x, y }) => {
            let Some(frame) = frame else {
                return CursorIcon::Default;
            };
            let from = to_vec(screen.point(frame.center()));
            let to = to_vec(screen.point(frame.scale_handle(x, y)));
            let d = to - from;
            let degrees = d.y.atan2(d.x).to_degrees().rem_euclid(180.0);
            match ((degrees + 22.5) / 45.0) as usize % 4 {
                0 => CursorIcon::ResizeHorizontal,
                1 => CursorIcon::ResizeNwSe,
                2 => CursorIcon::ResizeVertical,
                _ => CursorIcon::ResizeNeSw,
            }
        }
    }
}

/// Begin, continue or end a handle gesture from the pointer. Returns whether
/// a handle owns the pointer now, so the tool does not also move the layer.
pub fn interact(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    screen: &Screen,
    frame: Option<&SelectionFrame>,
) -> bool {
    let radius = shell.editor.radius_target();
    if response.drag_started_by(egui::PointerButton::Primary)
        && shell.ui.handle_gesture.is_none()
        && let Some(press) = ctx.input(|i| i.pointer.press_origin())
    {
        shell.ui.handle_gesture = match hit_test(screen, frame, radius.as_ref(), press) {
            Some(Hit::Handle(h)) => {
                shell
                    .editor
                    .begin_handle(h, screen.doc(press))
                    .then_some(Gesture::Frame {
                        scales: matches!(h, fanimate_editor::Handle::Scale { .. }),
                    })
            }
            Some(Hit::Radius(corner)) => {
                radius
                    .filter(|_| shell.editor.begin_radius_edit())
                    .map(|t| Gesture::Radius {
                        corner,
                        grab: t.radius_at(corner, screen.doc(press)) - t.rect.corner_radii()[corner],
                    })
            }
            None => None,
        };
    }
    let Some(gesture) = shell.ui.handle_gesture else {
        return false;
    };
    if let Some(pos) = response
        .interact_pointer_pos()
        .or_else(|| ctx.pointer_latest_pos())
    {
        let mods = ctx.input(|i| i.modifiers);
        match gesture {
            Gesture::Frame { scales } => {
                // a scaling handle snaps like a point being dragged: to the
                // grid, guides, the canvas and other shapes. Turning and the
                // pivot have their own steps (15 degrees, the nine anchors).
                let at = if scales {
                    crate::panels::canvas::do_snap(shell, screen.doc(pos))
                } else {
                    screen.doc(pos)
                };
                shell.editor.drag_handle(
                    at,
                    HandleMods {
                        shift: mods.shift,
                        alt: mods.alt,
                        snap: SNAP / screen.zoom,
                    },
                );
            }
            Gesture::Radius { corner, grab } => {
                if let Some(t) = shell.editor.radius_target() {
                    let radius = t.radius_at(corner, screen.doc(pos)) - grab;
                    // a handle moves all four corners; with Alt, only its own
                    if mods.alt {
                        shell.editor.set_one_corner_radius(corner, radius);
                    } else {
                        shell.editor.set_corner_radius(radius);
                    }
                }
            }
        }
    }
    if response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
        shell.editor.end_handle();
        shell.ui.handle_gesture = None;
        return false;
    }
    ctx.request_repaint();
    true
}

/// Draw the frame and its handles in the label colour.
pub fn paint_frame(
    painter: &egui::Painter,
    shell: &Shell,
    screen: &Screen,
    frame: &SelectionFrame,
    hover: Option<Hit>,
) {
    let p = shell.palette;
    let color = frame_color(shell, frame);
    let line = Stroke::new(1.5, color);
    let corners: Vec<Pos2> = frame.corners.iter().map(|c| screen.point(*c)).collect();
    // several layers: each one's own box, thin, under the box around them all
    if frame.is_multiple() {
        for member in &frame.members {
            let pts: Vec<Pos2> = member.iter().map(|c| screen.point(*c)).collect();
            painter.add(egui::Shape::closed_line(
                pts,
                Stroke::new(1.0, color.gamma_multiply(0.7)),
            ));
        }
    }
    painter.add(egui::Shape::closed_line(corners, line));

    // the rotate button: a round button below the box on a short stem
    let button = rotate_button(screen, frame);
    let bottom = screen.point(frame.edge(2));
    painter.line_segment([bottom, button], Stroke::new(1.0, color.gamma_multiply(0.7)));
    let over = hover == Some(Hit::Handle(Handle::Rotate));
    painter.circle_filled(button, ROTATE_RADIUS, if over { p.raised } else { p.panel });
    painter.circle_stroke(button, ROTATE_RADIUS, line);
    icons::paint(
        painter,
        Rect::from_center_size(button, vec2(13.0, 13.0)),
        Icon::Rotate,
        p.text,
    );

    // hollow squares for scaling
    for (handle, at) in scale_handles(screen, frame) {
        let r = Rect::from_center_size(at, vec2(SQUARE, SQUARE));
        let over = hover == Some(Hit::Handle(handle));
        painter.rect_filled(r, 1.0, if over { color } else { p.panel });
        painter.rect_stroke(r, 1.0, line, egui::StrokeKind::Inside);
    }

    // the pivot: a ring with a dot
    if let Some(pivot) = frame.pivot {
        let at = screen.point(pivot);
        let over = hover == Some(Hit::Handle(Handle::Pivot));
        painter.circle_filled(at, 5.5, if over { p.raised } else { p.panel });
        painter.circle_stroke(at, 5.5, line);
        painter.circle_filled(at, 1.8, color);
    }
}

/// Draw the corner radius handles of a rectangle: round dots inside the
/// corners, along the diagonals.
pub fn paint_radius(
    painter: &egui::Painter,
    shell: &Shell,
    screen: &Screen,
    target: &RadiusTarget,
    hover: Option<Hit>,
) {
    let p = shell.palette;
    let color = shell
        .editor
        .doc
        .find(target.layer)
        .map_or(p.accent, |l| crate::theme::label_color(l.shown_label()));
    for corner in 0..4 {
        let at = screen.point(target.handle(corner, radius_inset(screen, target, corner)));
        let over = hover == Some(Hit::Radius(corner));
        painter.circle_filled(at, 4.5, if over { color } else { p.panel });
        painter.circle_stroke(at, 4.5, Stroke::new(1.5, color));
        painter.circle_filled(at, 1.2, if over { p.panel } else { color });
    }
}

/// The small label beside the pointer while a handle is dragged: the new
/// size, or the angle.
pub fn paint_readout(painter: &egui::Painter, shell: &Shell, pointer: Pos2) {
    let Some(text) = shell.editor.handle_readout() else {
        return;
    };
    let p = shell.palette;
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(12.0), p.text);
    let rect = Rect::from_min_size(pointer + vec2(16.0, 16.0), galley.size() + vec2(12.0, 6.0));
    painter.rect_filled(rect, 4.0, p.raised);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(6.0, 3.0), galley, p.text);
}

/// A polyline outline, in document space, drawn on the canvas.
pub fn paint_outline(
    painter: &egui::Painter,
    screen: &Screen,
    outline: &[(Vec<Vec2>, bool)],
    stroke: Stroke,
) {
    for (points, closed) in outline {
        if points.len() < 2 {
            continue;
        }
        let pts: Vec<Pos2> = points.iter().map(|p| screen.point(*p)).collect();
        painter.add(if *closed {
            egui::Shape::closed_line(pts, stroke)
        } else {
            egui::Shape::line(pts, stroke)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn screen() -> Screen {
        Screen {
            origin: pos2(500.0, 300.0),
            center: Vec2::new(100.0, 50.0),
            zoom: 2.0,
        }
    }

    /// A frame of a 100 x 60 box at (0, 0), not turned.
    fn frame() -> SelectionFrame {
        SelectionFrame {
            corners: [
                Vec2::new(0.0, 0.0),
                Vec2::new(100.0, 0.0),
                Vec2::new(100.0, 60.0),
                Vec2::new(0.0, 60.0),
            ],
            members: vec![[
                Vec2::new(0.0, 0.0),
                Vec2::new(100.0, 0.0),
                Vec2::new(100.0, 60.0),
                Vec2::new(0.0, 60.0),
            ]],
            layers: vec![fanimate_core::LayerId(1)],
            pivot: Some(Vec2::new(50.0, 30.0)),
            primary: fanimate_core::LayerId(1),
            free_scale: true,
        }
    }

    #[test]
    fn the_screen_maps_document_points_both_ways() {
        let s = screen();
        assert_eq!(s.point(Vec2::new(100.0, 50.0)), s.origin);
        let p = Vec2::new(37.0, -12.0);
        assert!((s.doc(s.point(p)) - p).length() < 1e-4);
    }

    #[test]
    fn handles_are_found_under_the_pointer_in_order() {
        let (s, f) = (screen(), frame());
        let at = |p: Vec2| s.point(p);
        // a corner, an edge, the pivot, the rotate button
        assert_eq!(
            hit_test(&s, Some(&f), None, at(Vec2::new(100.0, 60.0))),
            Some(Hit::Handle(Handle::Scale { x: 1, y: 1 }))
        );
        assert_eq!(
            hit_test(&s, Some(&f), None, at(Vec2::new(50.0, 0.0)) + vec2(2.0, 0.0)),
            Some(Hit::Handle(Handle::Scale { x: 0, y: -1 }))
        );
        assert_eq!(
            hit_test(&s, Some(&f), None, at(Vec2::new(50.0, 30.0))),
            Some(Hit::Handle(Handle::Pivot))
        );
        assert_eq!(
            hit_test(&s, Some(&f), None, rotate_button(&s, &f)),
            Some(Hit::Handle(Handle::Rotate))
        );
        // inside the box, away from everything: nothing, so the layer moves
        assert_eq!(hit_test(&s, Some(&f), None, at(Vec2::new(25.0, 15.0))), None);
        // outside a corner turns; outside the middle of a side does not
        assert_eq!(
            hit_test(&s, Some(&f), None, at(Vec2::new(100.0, 60.0)) + vec2(12.0, 12.0)),
            Some(Hit::Handle(Handle::Rotate))
        );
        assert_eq!(
            hit_test(&s, Some(&f), None, at(Vec2::new(50.0, -10.0)) + vec2(0.0, -12.0)),
            None
        );
        // no frame, no handles
        assert_eq!(hit_test(&s, None, None, at(Vec2::new(100.0, 60.0))), None);
    }

    #[test]
    fn a_layer_whose_sides_cannot_stretch_alone_has_corner_handles_only() {
        let (s, mut f) = (screen(), frame());
        f.free_scale = false;
        assert_eq!(scale_handles(&s, &f).len(), 4);
        f.free_scale = true;
        assert_eq!(scale_handles(&s, &f).len(), 8);
    }

    #[test]
    fn the_cursor_follows_the_direction_of_the_handle() {
        let (s, f) = (screen(), frame());
        let c = |x, y| cursor(&s, Some(&f), Hit::Handle(Handle::Scale { x, y }));
        assert_eq!(c(1, 0), CursorIcon::ResizeHorizontal);
        assert_eq!(c(0, 1), CursorIcon::ResizeVertical);
        assert_eq!(c(1, 1), CursorIcon::ResizeNwSe);
        assert_eq!(c(1, -1), CursorIcon::ResizeNeSw);
        assert_eq!(cursor(&s, Some(&f), Hit::Handle(Handle::Pivot)), CursorIcon::Move);
    }
}
