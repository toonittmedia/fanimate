//! Snapping: pure functions that return the nearest snap point given a
//! position and the active snap targets.
//!
//! Every tool that moves points or layers calls `snap` before committing a
//! position. The caller passes in which targets are active; the result is the
//! snapped position and a hint line for the canvas overlay.
//!
//! Targets: pixels, grid, guides, canvas bounds, and the points, edges and
//! centres of other shapes. `snap_angle` is separate: bones use it while
//! Shift is held.

pub use fanimate_core::Guide;
use fanimate_core::Vec2;
use serde::{Deserialize, Serialize};

/// Which snap targets are active.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapSettings {
    /// Snap to the pixel grid (integer coordinates).
    pub pixels: bool,
    /// Snap to the document grid.
    pub grid: bool,
    /// Snap to placed guides.
    pub guides: bool,
    /// Snap to the canvas edges and centre.
    pub bounds: bool,
    /// Snap to the points, edges and centres of other shapes.
    pub shapes: bool,
    /// The step for angle snapping (Shift while adding a bone), in degrees (default 15).
    pub angle_step: f32,
    /// Snap tolerance in screen pixels.
    pub tolerance_px: f32,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            pixels: false,
            grid: true,
            guides: true,
            bounds: true,
            shapes: true,
            angle_step: 15.0,
            tolerance_px: 8.0,
        }
    }
}

/// Grid parameters for the canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridSettings {
    /// Major grid spacing in document units.
    pub spacing: f32,
    /// Number of subdivisions between major lines.
    pub subdivisions: u32,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            spacing: 50.0,
            subdivisions: 5,
        }
    }
}

impl GridSettings {
    /// Minor (sub-division) spacing.
    pub fn minor_spacing(&self) -> f32 {
        let d = self.subdivisions.max(1) as f32;
        self.spacing / d
    }
}

/// Lines to snap to that come from the artwork: the x and y of other
/// shapes' points, and the edges and centres of their boxes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeLines {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
}

impl ShapeLines {
    /// Add a vertical and a horizontal line, unless one is there already.
    pub fn add(&mut self, x: f32, y: f32) {
        const SAME: f32 = 0.01;
        if x.is_finite() && !self.xs.iter().any(|v| (v - x).abs() < SAME) {
            self.xs.push(x);
        }
        if y.is_finite() && !self.ys.iter().any(|v| (v - y).abs() < SAME) {
            self.ys.push(y);
        }
    }
}

/// A snap result: the snapped position and the lines it snapped to, to draw
/// (each a start and an end).
#[derive(Clone, Debug)]
pub struct SnapResult {
    pub position: Vec2,
    pub hints: Vec<(Vec2, Vec2)>,
}

/// Snap `p` (document space) to the nearest active targets. Each axis snaps
/// on its own to the nearest line (a guide, the canvas edge or centre, a
/// grid line, a line of another shape), so a point can snap to two lines at
/// once and line up with both. Pixel snapping rounds both together, when
/// nothing else is near.
/// `canvas_size` is (width, height). `zoom` is screen pixels per doc unit.
/// Returns `p` unchanged (with no hints) when nothing is close enough.
pub fn snap(
    p: Vec2,
    settings: &SnapSettings,
    grid: &GridSettings,
    guides: &[Guide],
    canvas_size: Vec2,
    zoom: f32,
    shapes: &ShapeLines,
) -> SnapResult {
    let tol = settings.tolerance_px / zoom.max(1e-6);
    let (w, h) = (canvas_size.x, canvas_size.y);
    // the nearest line on each axis: how far, where, and whether to draw it
    let mut best_x: Option<(f32, f32, bool)> = None;
    let mut best_y: Option<(f32, f32, bool)> = None;
    // the first of two equally near lines wins, so the grid, offered last,
    // gives way to anything drawn at the same place
    let offer = |best: &mut Option<(f32, f32, bool)>, at: f32, from: f32, hint: bool| {
        let d = (from - at).abs();
        if d <= tol && best.is_none_or(|b| d < b.0) {
            *best = Some((d, at, hint));
        }
    };

    if settings.guides {
        for guide in guides {
            if guide.vertical {
                offer(&mut best_x, guide.position, p.x, true);
            } else {
                offer(&mut best_y, guide.position, p.y, true);
            }
        }
    }
    if settings.bounds {
        for x in [0.0, w / 2.0, w] {
            offer(&mut best_x, x, p.x, true);
        }
        for y in [0.0, h / 2.0, h] {
            offer(&mut best_y, y, p.y, true);
        }
    }
    if settings.shapes {
        for x in &shapes.xs {
            offer(&mut best_x, *x, p.x, true);
        }
        for y in &shapes.ys {
            offer(&mut best_y, *y, p.y, true);
        }
    }
    if settings.grid {
        let minor = grid.minor_spacing();
        if minor > 0.0 {
            offer(&mut best_x, (p.x / minor).round() * minor, p.x, false);
            offer(&mut best_y, (p.y / minor).round() * minor, p.y, false);
        }
    }

    let mut position = p;
    let mut hints = Vec::new();
    if let Some((_, x, hint)) = best_x {
        position.x = x;
        if hint {
            hints.push((Vec2::new(x, 0.0), Vec2::new(x, h)));
        }
    }
    if let Some((_, y, hint)) = best_y {
        position.y = y;
        if hint {
            hints.push((Vec2::new(0.0, y), Vec2::new(w, y)));
        }
    }
    // whole pixels, for an axis nothing else claimed
    if settings.pixels {
        if best_x.is_none() {
            position.x = p.x.round();
        }
        if best_y.is_none() {
            position.y = p.y.round();
        }
    }
    SnapResult { position, hints }
}

/// Snap an angle (radians) to the nearest multiple of `step` degrees.
pub fn snap_angle(angle: f32, step_deg: f32) -> f32 {
    if step_deg <= 0.0 {
        return angle;
    }
    let step = step_deg.to_radians();
    (angle / step).round() * step
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: Vec2 = Vec2::new(1280.0, 720.0);

    /// Only the targets named are on.
    fn only(grid: bool, guides: bool, bounds: bool, shapes: bool, pixels: bool) -> SnapSettings {
        SnapSettings {
            grid,
            guides,
            bounds,
            shapes,
            pixels,
            ..SnapSettings::default()
        }
    }

    fn run(p: Vec2, s: &SnapSettings, guides: &[Guide], lines: &ShapeLines) -> SnapResult {
        snap(p, s, &GridSettings::default(), guides, CANVAS, 1.0, lines)
    }

    #[test]
    fn the_grid_snaps_both_axes_at_once() {
        // minor spacing is 50 / 5 = 10: (13, 28) is near the line x = 10 and the line y = 30
        let r = run(
            Vec2::new(13.0, 28.0),
            &only(true, false, false, false, false),
            &[],
            &ShapeLines::default(),
        );
        assert_eq!(r.position, Vec2::new(10.0, 30.0));
        assert!(r.hints.is_empty(), "grid snaps draw no line");
        // only the axis that is near a line moves
        let s = only(true, false, false, false, false);
        let r = snap(
            Vec2::new(13.0, 25.0),
            &s,
            &GridSettings::default(),
            &[],
            CANVAS,
            2.0,
            &ShapeLines::default(),
        );
        assert_eq!(r.position.x, 10.0);
        assert_eq!(
            r.position.y, 25.0,
            "5 from a grid line, and 4 is all it reaches at this zoom"
        );
    }

    #[test]
    fn a_guide_and_the_grid_each_take_their_axis() {
        let guides = [Guide {
            vertical: true,
            position: 203.0,
            locked: false,
        }];
        let r = run(
            Vec2::new(202.8, 58.0),
            &only(true, true, false, false, false),
            &guides,
            &ShapeLines::default(),
        );
        assert_eq!(r.position, Vec2::new(203.0, 60.0));
        assert_eq!(r.hints.len(), 1, "the guide is drawn, the grid is not");
    }

    #[test]
    fn a_guide_wins_over_a_grid_line_at_the_same_place() {
        let guides = [Guide {
            vertical: true,
            position: 200.0,
            locked: false,
        }];
        let r = run(
            Vec2::new(203.0, 5000.0),
            &only(true, true, false, false, false),
            &guides,
            &ShapeLines::default(),
        );
        assert_eq!(r.position.x, 200.0);
        assert_eq!(r.hints.len(), 1, "the line it snapped to is shown");
    }

    #[test]
    fn the_canvas_edges_and_centre_snap_and_show_their_line() {
        let r = run(
            Vec2::new(1278.0, 100.0),
            &only(false, false, true, false, false),
            &[],
            &ShapeLines::default(),
        );
        assert_eq!(r.position.x, 1280.0);
        assert_eq!(r.hints.len(), 1);
        let r = run(
            Vec2::new(641.0, 361.0),
            &only(false, false, true, false, false),
            &[],
            &ShapeLines::default(),
        );
        assert_eq!(r.position, Vec2::new(640.0, 360.0));
        assert_eq!(r.hints.len(), 2, "both centre lines");
    }

    #[test]
    fn other_shapes_lines_are_targets_when_it_is_on() {
        let mut lines = ShapeLines::default();
        lines.add(300.0, 410.0);
        let r = run(
            Vec2::new(304.0, 406.0),
            &only(false, false, false, true, false),
            &[],
            &lines,
        );
        assert_eq!(r.position, Vec2::new(300.0, 410.0));
        assert_eq!(r.hints.len(), 2);
        // off: they are not
        let r = run(
            Vec2::new(304.0, 406.0),
            &only(false, false, false, false, false),
            &[],
            &lines,
        );
        assert_eq!(r.position, Vec2::new(304.0, 406.0));
        assert!(r.hints.is_empty());
    }

    #[test]
    fn shape_lines_do_not_repeat() {
        let mut lines = ShapeLines::default();
        lines.add(10.0, 20.0);
        lines.add(10.0, 30.0);
        lines.add(10.004, 30.0);
        lines.add(f32::NAN, f32::INFINITY);
        assert_eq!(lines.xs, [10.0]);
        assert_eq!(lines.ys, [20.0, 30.0]);
    }

    #[test]
    fn nothing_snaps_outside_the_tolerance_and_it_is_in_screen_pixels() {
        let p = Vec2::new(15.0, 15.0);
        let s = only(true, false, false, false, false);
        // 5 doc units from the nearest grid line, 8 screen pixels of reach
        assert_ne!(run(p, &s, &[], &ShapeLines::default()).position, p);
        // zoomed out to 0.5, 8 pixels reach 16 units; zoomed in to 4, only 2
        let near = snap(
            p,
            &s,
            &GridSettings::default(),
            &[],
            CANVAS,
            4.0,
            &ShapeLines::default(),
        );
        assert_eq!(near.position, p);
        let far = snap(
            Vec2::new(14.0, 14.0),
            &s,
            &GridSettings::default(),
            &[],
            CANVAS,
            0.5,
            &ShapeLines::default(),
        );
        assert_eq!(far.position, Vec2::new(10.0, 10.0));
    }

    #[test]
    fn pixels_round_the_axes_nothing_else_claimed() {
        let s = only(false, false, false, false, true);
        let r = run(Vec2::new(10.3, 20.7), &s, &[], &ShapeLines::default());
        assert_eq!(r.position, Vec2::new(10.0, 21.0));
        // a guide takes x, the pixel grid still rounds y
        let guides = [Guide {
            vertical: true,
            position: 100.5,
            locked: false,
        }];
        let s = only(false, true, false, false, true);
        let r = run(Vec2::new(101.0, 20.7), &s, &guides, &ShapeLines::default());
        assert_eq!(r.position, Vec2::new(100.5, 21.0));
    }

    #[test]
    fn angle_snapping() {
        // 37 degrees: the nearest multiple of 15 is 30
        let snapped = snap_angle(37.0_f32.to_radians(), 15.0);
        assert!(
            (snapped - 30.0_f32.to_radians()).abs() < 1e-4,
            "{}",
            snapped.to_degrees()
        );
        assert_eq!(snap_angle(1.0, 0.0), 1.0, "a step of zero leaves the angle alone");
    }
}
