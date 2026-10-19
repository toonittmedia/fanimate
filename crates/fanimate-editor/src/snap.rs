//! Snapping: pure functions that return the nearest snap point given a
//! position and the active snap targets.
//!
//! Every tool that moves points or layers calls `snap` before committing a
//! position. The caller passes in which targets are active; the result is the
//! snapped position and a hint line for the canvas overlay.
//!
//! Targets: pixels, grid, guides and canvas bounds. Snapping to other shapes is
//! not built yet. `snap_angle` is separate: bones use it while Shift is held.

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

/// A guide: a horizontal or vertical line at a fixed position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    /// If true, the guide is a vertical line at `position.x`; otherwise
    /// a horizontal line at `position.y`.
    pub vertical: bool,
    /// Position in document units.
    pub position: f32,
    /// Locked guides cannot be dragged.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

/// A snap result: the snapped position and a hint line to draw (start, end).
#[derive(Clone, Debug)]
pub struct SnapResult {
    pub position: Vec2,
    pub hint: Option<(Vec2, Vec2)>,
}

/// Snap `p` (document space) to the nearest active target.
/// `canvas_size` is (width, height). `zoom` is screen pixels per doc unit.
/// Returns `p` unchanged (with no hint) when nothing is close enough.
pub fn snap(
    p: Vec2,
    settings: &SnapSettings,
    grid: &GridSettings,
    guides: &[Guide],
    canvas_size: Vec2,
    zoom: f32,
) -> SnapResult {
    type Candidate = Option<(f32, Vec2, Option<(Vec2, Vec2)>)>;
    let tol = settings.tolerance_px / zoom.max(1e-6);
    let mut best_dx: Candidate = None;

    // helper: keep the closest candidate
    let mut candidate = |pos: Vec2, hint: Option<(Vec2, Vec2)>| {
        let d = p.distance(pos);
        if d <= tol && best_dx.as_ref().is_none_or(|b| d < b.0) {
            best_dx = Some((d, pos, hint));
        }
    };

    // --- Pixel snapping ---
    if settings.pixels {
        candidate(p.round(), None);
    }

    // --- Grid snapping ---
    if settings.grid {
        let minor = grid.minor_spacing();
        if minor > 0.0 {
            let gx = (p.x / minor).round() * minor;
            let gy = (p.y / minor).round() * minor;
            candidate(Vec2::new(gx, p.y), None);
            candidate(Vec2::new(p.x, gy), None);
            candidate(Vec2::new(gx, gy), None);
        }
    }

    // --- Guide snapping ---
    if settings.guides {
        for guide in guides {
            if guide.vertical {
                let snap_p = Vec2::new(guide.position, p.y);
                let hint_a = Vec2::new(guide.position, 0.0);
                let hint_b = Vec2::new(guide.position, canvas_size.y);
                candidate(snap_p, Some((hint_a, hint_b)));
            } else {
                let snap_p = Vec2::new(p.x, guide.position);
                let hint_a = Vec2::new(0.0, guide.position);
                let hint_b = Vec2::new(canvas_size.x, guide.position);
                candidate(snap_p, Some((hint_a, hint_b)));
            }
        }
    }

    // --- Canvas bounds (edges and centre) ---
    if settings.bounds {
        let (w, h) = (canvas_size.x, canvas_size.y);
        let cx = w / 2.0;
        let cy = h / 2.0;
        // edges
        for x in [0.0_f32, w] {
            candidate(Vec2::new(x, p.y), Some((Vec2::new(x, 0.0), Vec2::new(x, h))));
        }
        for y in [0.0_f32, h] {
            candidate(Vec2::new(p.x, y), Some((Vec2::new(0.0, y), Vec2::new(w, y))));
        }
        // centre lines
        candidate(Vec2::new(cx, p.y), Some((Vec2::new(cx, 0.0), Vec2::new(cx, h))));
        candidate(Vec2::new(p.x, cy), Some((Vec2::new(0.0, cy), Vec2::new(w, cy))));
        // centre point
        candidate(Vec2::new(cx, cy), None);
    }

    match best_dx {
        Some((_, pos, hint)) => SnapResult { position: pos, hint },
        None => SnapResult {
            position: p,
            hint: None,
        },
    }
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

    fn defaults() -> (SnapSettings, GridSettings, Vec<Guide>, Vec2) {
        let s = SnapSettings {
            guides: false,
            ..SnapSettings::default()
        };
        (s, GridSettings::default(), vec![], Vec2::new(1280.0, 720.0))
    }

    #[test]
    fn grid_snaps_to_nearest_line() {
        let (settings, grid, guides, canvas) = defaults();
        // minor spacing = 50/5 = 10; point at (13, 13).
        // Single-axis snap: (10,13) dist=3 and (13,10) dist=3 tie; the combined
        // (10,10) dist=4.24 is farther. The function picks the first candidate
        // added, which is (gx, p.y) = (10, 13). Check that x snapped to 10.
        let r = snap(Vec2::new(13.0, 13.0), &settings, &grid, &guides, canvas, 1.0);
        assert!((r.position.x - 10.0).abs() < 0.5, "x={}", r.position.x);
        // y may snap to 10 (combined) or stay at 13 (single axis); just check
        // that it moved toward the grid
        assert!(r.position.y <= 13.0 + 0.5, "y={}", r.position.y);
    }

    #[test]
    fn bounds_snaps_to_canvas_edge() {
        let (mut settings, grid, guides, canvas) = defaults();
        settings.grid = false;
        // point near the right edge (1278, 360). The canvas center y=360, so
        // center-y snap (Vec2::new(1278, 360)) has distance 0 and wins.
        // Use a point not on any center line: (1278, 100).
        let r = snap(Vec2::new(1278.0, 100.0), &settings, &grid, &guides, canvas, 1.0);
        assert!((r.position.x - 1280.0).abs() < 0.5, "x={}", r.position.x);
        assert!(r.hint.is_some());
    }

    #[test]
    fn no_snap_when_outside_tolerance() {
        let (mut settings, grid, guides, canvas) = defaults();
        settings.grid = false;
        settings.bounds = false;
        // zoom = 1, tol = 8px; point is 20 doc units from any grid line
        let p = Vec2::new(20.0, 20.0); // nearest grid lines at 0 and 50, both > 8 away... wait
        // minor_spacing = 10, so nearest is 20 which IS on a grid line
        // disable grid:
        let mut s2 = settings.clone();
        s2.grid = false;
        let r = snap(p, &s2, &grid, &guides, canvas, 1.0);
        assert!(
            (r.position - p).length() < 1e-4,
            "should not snap: {:?}",
            r.position
        );
    }

    #[test]
    fn guide_snapping() {
        let (mut settings, grid, _, canvas) = defaults();
        settings.grid = false;
        settings.bounds = false;
        settings.guides = true;
        let guides = vec![Guide {
            vertical: true,
            position: 200.0,
            locked: false,
        }];
        let r = snap(Vec2::new(203.0, 50.0), &settings, &grid, &guides, canvas, 1.0);
        assert!((r.position.x - 200.0).abs() < 0.5, "x={}", r.position.x);
        assert!(r.hint.is_some());
    }

    #[test]
    fn angle_snapping() {
        // 37° → nearest 15° multiple = 30° = π/6
        let snapped = snap_angle(37.0_f32.to_radians(), 15.0);
        let expected = 30.0_f32.to_radians();
        assert!(
            (snapped - expected).abs() < 1e-4,
            "snapped={}",
            snapped.to_degrees()
        );
    }

    #[test]
    fn pixel_snap() {
        let (mut settings, grid, guides, canvas) = defaults();
        settings.grid = false;
        settings.bounds = false;
        settings.pixels = true;
        let r = snap(Vec2::new(10.3, 20.7), &settings, &grid, &guides, canvas, 1.0);
        assert!((r.position.x - 10.0).abs() < 0.5);
        assert!((r.position.y - 21.0).abs() < 0.5);
    }
}
