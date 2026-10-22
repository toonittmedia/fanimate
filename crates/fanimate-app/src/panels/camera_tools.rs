//! The camera tools: Track camera, Zoom camera and Roll camera. Each drag is
//! one undo step and writes the document's camera through
//! `Editor::set_camera_*`, so the keying rule holds (the first frame changes
//! the camera itself, a later frame sets a key).
//!
//! The maths is in small functions so it is tested without a window.

use eframe::egui::{self, Pos2};
use fanimate_core::{CAMERA_ZOOM, Vec2};
use fanimate_editor::Tool;

use crate::app::Shell;

/// Pixels of dragging up that zoom the camera in by a factor of e (2.7).
const ZOOM_PIXELS_PER_E: f32 = 150.0;
/// Shift snaps the roll to steps of this many degrees, as the layer frame does.
const ROLL_STEP_DEGREES: f32 = 15.0;

pub(super) fn is_camera_tool(tool: Tool) -> bool {
    matches!(tool, Tool::TrackCamera | Tool::ZoomCamera | Tool::RollCamera)
}

/// The camera position after the pointer moved by `delta` (document units).
/// The picture follows the pointer, so the camera goes the other way.
fn tracked(position: Vec2, delta: Vec2) -> Vec2 {
    position - delta
}

/// The zoom after dragging `up` pixels upward from `start`: exponential, so
/// the same drag always changes the picture by the same ratio; kept within
/// what the renderer accepts.
fn zoomed(start: f32, up: f32) -> f32 {
    (start * (up / ZOOM_PIXELS_PER_E).exp()).clamp(*CAMERA_ZOOM.start(), *CAMERA_ZOOM.end())
}

/// An angle brought into -pi..=pi, so a pointer crossing the left side of the
/// camera does not jump a whole turn.
fn wrapped(angle: f32) -> f32 {
    let turn = std::f32::consts::TAU;
    let a = (angle + std::f32::consts::PI).rem_euclid(turn) - std::f32::consts::PI;
    if a == -std::f32::consts::PI {
        std::f32::consts::PI
    } else {
        a
    }
}

/// The roll to write for the angle gathered so far, snapped with Shift.
fn rolled(angle: f32, snap: bool) -> f32 {
    if snap {
        let step = ROLL_STEP_DEGREES.to_radians();
        (angle / step).round() * step
    } else {
        angle
    }
}

/// Where a roll drag is: the camera's centre, the pointer's last angle about
/// it, and the roll gathered so far (not yet snapped).
#[derive(Clone, Copy)]
struct Drag {
    tool: Tool,
    press_y: f32,
    start_zoom: f32,
    centre: Vec2,
    last_angle: f32,
    roll: f32,
}

fn angle_about(centre: Vec2, p: Vec2) -> f32 {
    (p.y - centre.y).atan2(p.x - centre.x)
}

/// Run the active camera tool for this frame.
pub(super) fn interact(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    let tool = shell.editor.tool;
    if !is_camera_tool(tool) || shell.look_through_camera {
        return;
    }
    let id = egui::Id::new("camera-tool-drag");
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let at_frame = shell.editor.current_frame() as f32;

    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(press) = ctx.input(|i| i.pointer.press_origin())
    {
        shell.editor.checkpoint(match tool {
            Tool::TrackCamera => "Track camera",
            Tool::ZoomCamera => "Zoom camera",
            _ => "Roll camera",
        });
        let cam = &shell.editor.doc.camera;
        let centre = shell.editor.doc.size() / 2.0 + cam.position.sample(at_frame);
        let drag = Drag {
            tool,
            press_y: press.y,
            start_zoom: cam.zoom.sample(at_frame),
            centre,
            last_angle: angle_about(centre, to_doc(center, zoom, press)),
            roll: cam.roll.sample(at_frame),
        };
        ctx.data_mut(|d| d.insert_temp(id, drag));
    }

    let Some(mut drag) = ctx.data(|d| d.get_temp::<Drag>(id)) else {
        return;
    };
    if drag.tool != tool || response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
        ctx.data_mut(|d| d.remove::<Drag>(id));
        return;
    }
    let Some(pos) = response.interact_pointer_pos() else {
        return;
    };
    match tool {
        Tool::TrackCamera => {
            let d = response.drag_delta();
            if d != egui::Vec2::ZERO {
                let now = shell.editor.doc.camera.position.sample(at_frame);
                shell
                    .editor
                    .set_camera_position(tracked(now, Vec2::new(d.x, d.y) / zoom));
            }
        }
        Tool::ZoomCamera => {
            let z = zoomed(drag.start_zoom, drag.press_y - pos.y);
            shell.editor.set_camera_zoom(z);
        }
        _ => {
            let p = to_doc(center, zoom, pos);
            // too near the centre the angle is noise
            if (p - drag.centre).length() > 1e-3 {
                let angle = angle_about(drag.centre, p);
                drag.roll += wrapped(angle - drag.last_angle);
                drag.last_angle = angle;
                ctx.data_mut(|d| d.insert_temp(id, drag));
                let snap = ctx.input(|i| i.modifiers.shift);
                shell.editor.set_camera_roll(rolled(drag.roll, snap));
            }
        }
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    #[test]
    fn the_picture_follows_the_pointer() {
        // the pointer moves right and down, so the camera goes left and up
        assert_eq!(
            tracked(Vec2::new(10.0, 10.0), Vec2::new(4.0, 6.0)),
            Vec2::new(6.0, 4.0)
        );
    }

    #[test]
    fn dragging_up_zooms_in_and_the_same_drag_gives_the_same_ratio() {
        assert!(zoomed(1.0, 50.0) > 1.0);
        assert!(zoomed(1.0, -50.0) < 1.0);
        assert_eq!(zoomed(1.0, 0.0), 1.0);
        let ratio = |start: f32| zoomed(start, 40.0) / start;
        assert!((ratio(1.0) - ratio(3.0)).abs() < 1e-4);
        // down and up by the same amount comes back
        assert!((zoomed(zoomed(2.0, 70.0), -70.0) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn zoom_stays_within_what_the_renderer_accepts() {
        assert_eq!(zoomed(1.0, 1.0e6), *CAMERA_ZOOM.end());
        assert_eq!(zoomed(1.0, -1.0e6), *CAMERA_ZOOM.start());
        assert_eq!(zoomed(1.0, f32::INFINITY), *CAMERA_ZOOM.end());
    }

    #[test]
    fn angles_wrap_the_short_way_round() {
        assert!((wrapped(0.1) - 0.1).abs() < 1e-6);
        // from just above the left side to just below it is a small turn
        assert!((wrapped(-PI + 0.1 - (PI - 0.1)) - 0.2).abs() < 1e-5);
        assert!((wrapped(PI + 0.3) - (-PI + 0.3)).abs() < 1e-5);
        assert!(wrapped(PI) <= PI && wrapped(PI) > 0.0);
    }

    #[test]
    fn clockwise_on_screen_is_a_positive_roll() {
        // y points down, so from the right of the centre to below it is clockwise
        let c = Vec2::ZERO;
        let turn = angle_about(c, Vec2::new(0.0, 1.0)) - angle_about(c, Vec2::new(1.0, 0.0));
        assert!((turn - FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn shift_snaps_the_roll_to_steps() {
        let step = ROLL_STEP_DEGREES.to_radians();
        let free = 20.0_f32.to_radians();
        assert_eq!(rolled(free, false), free);
        assert!((rolled(free, true) - step).abs() < 1e-6);
        assert!((rolled(-free, true) + step).abs() < 1e-6);
        assert_eq!(rolled(0.0, true), 0.0);
    }
}
