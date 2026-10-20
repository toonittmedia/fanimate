//! Rulers, canvas grid, guides, and snap-hint overlay for Phase F.
//!
//! The rulers run along the top and left edges of the canvas panel. Dragging
//! from a ruler drops a guide. Right-clicking a guide opens a menu to delete
//! or lock it. The corner button cycles the unit: px → pt → mm → in.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

use crate::app::Shell;

/// Width/height of the ruler strip in screen points.
pub const RULER_SIZE: f32 = 20.0;

/// Unit shown in the rulers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RulerUnit {
    #[default]
    Px,
    Pt,
    Mm,
    In,
}

impl RulerUnit {
    /// Label shown in the corner button.
    pub fn label(self) -> &'static str {
        match self {
            Self::Px => "px",
            Self::Pt => "pt",
            Self::Mm => "mm",
            Self::In => "in",
        }
    }

    /// Convert a value in document pixels to this unit.
    pub fn px_to_unit(self, px: f32, dpi: f32) -> f32 {
        match self {
            Self::Px => px,
            Self::Pt => px * 72.0 / dpi,
            Self::Mm => px * 25.4 / dpi,
            Self::In => px / dpi,
        }
    }

    /// Format a label for a major tick (no decimals for px, 1 for others).
    pub fn fmt(self, v: f32) -> String {
        match self {
            Self::Px => format!("{}", v as i32),
            Self::Pt | Self::Mm | Self::In => format!("{:.1}", v),
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Px => Self::Pt,
            Self::Pt => Self::Mm,
            Self::Mm => Self::In,
            Self::In => Self::Px,
        }
    }
}

/// Document pixels per inch: Fanimate uses 96 dpi by convention.
const DPI: f32 = 96.0;

/// Draw the horizontal (top) and vertical (left) rulers along `canvas_rect`.
/// The ruler occupies the outer `RULER_SIZE` pixels of the rect.
pub fn rulers(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    canvas_rect: Rect,
    to_doc: &impl Fn(f32, Pos2) -> fanimate_core::Vec2,
    to_screen: &impl Fn(fanimate_core::Vec2) -> Pos2,
) {
    if !shell.show_rulers {
        return;
    }
    let p = shell.palette;
    let painter = ui.painter_at(canvas_rect);
    let zoom = shell.view.zoom;

    // Ruler background
    let h_ruler = Rect::from_min_max(
        canvas_rect.min,
        Pos2::new(canvas_rect.max.x, canvas_rect.min.y + RULER_SIZE),
    );
    let v_ruler = Rect::from_min_max(
        canvas_rect.min,
        Pos2::new(canvas_rect.min.x + RULER_SIZE, canvas_rect.max.y),
    );
    let corner = Rect::from_min_max(canvas_rect.min, canvas_rect.min + Vec2::splat(RULER_SIZE));
    painter.rect_filled(h_ruler, 0.0, p.panel);
    painter.rect_filled(v_ruler, 0.0, p.panel);
    painter.rect_filled(corner, 0.0, p.raised);
    painter.hline(h_ruler.x_range(), h_ruler.bottom(), Stroke::new(1.0, p.border));
    painter.vline(v_ruler.right(), v_ruler.y_range(), Stroke::new(1.0, p.border));

    // Corner button: click to cycle the unit
    let corner_resp = ui.interact(corner, ui.id().with("ruler-corner"), Sense::click());
    let unit_label = shell.ruler_unit.label();
    painter.text(
        corner.center(),
        egui::Align2::CENTER_CENTER,
        unit_label,
        egui::FontId::proportional(9.0),
        if corner_resp.hovered() {
            p.text
        } else {
            p.text_muted
        },
    );
    if corner_resp.on_hover_text("Click to change ruler units").clicked() {
        shell.ruler_unit = shell.ruler_unit.next();
    }

    // Tick marks and labels
    draw_ruler_ticks(&painter, h_ruler, v_ruler, zoom, shell, to_screen);

    // Guides (the lines across the canvas)
    draw_guides(
        ui,
        &painter,
        shell,
        canvas_rect,
        h_ruler,
        v_ruler,
        to_doc,
        to_screen,
    );
}

fn draw_ruler_ticks(
    painter: &egui::Painter,
    h_ruler: Rect,
    v_ruler: Rect,
    zoom: f32,
    shell: &Shell,
    to_screen: &impl Fn(fanimate_core::Vec2) -> Pos2,
) {
    let p = shell.palette;
    let center = shell.view.center;
    let unit = shell.ruler_unit;

    // Find a nice tick spacing in document pixels: aim for ~60-80px between major ticks
    let target_px = 60.0;
    let raw = target_px / zoom;
    // raw is in document pixels; convert to the display unit to find a nice step
    let raw_unit = unit.px_to_unit(raw, DPI);
    let nice_unit = nice_step(raw_unit.max(1e-6));
    // convert back to document pixels for drawing
    let nice_doc = match unit {
        RulerUnit::Px => nice_unit,
        RulerUnit::Pt => nice_unit * DPI / 72.0,
        RulerUnit::Mm => nice_unit * DPI / 25.4,
        RulerUnit::In => nice_unit * DPI,
    };
    let nice = nice_doc.max(1e-6);

    // visible document range (in document pixels)
    let left_doc = (h_ruler.left() - h_ruler.center().x) / zoom + center.x;
    let right_doc = (h_ruler.right() - h_ruler.center().x) / zoom + center.x;
    let top_doc = (v_ruler.top() - v_ruler.center().y) / zoom + center.y;
    let bottom_doc = (v_ruler.bottom() - v_ruler.center().y) / zoom + center.y;

    let first_x = (left_doc / nice).floor() as i64;
    let last_x = (right_doc / nice).ceil() as i64;
    for i in first_x..=last_x {
        let dx = i as f32 * nice;
        let sx = to_screen(fanimate_core::Vec2::new(dx, 0.0)).x;
        if sx < h_ruler.left() + RULER_SIZE || sx > h_ruler.right() {
            continue;
        }
        let major = (i % 5) == 0;
        let tick_h = if major { 8.0 } else { 4.0 };
        painter.vline(
            sx,
            h_ruler.bottom() - tick_h..=h_ruler.bottom(),
            Stroke::new(1.0, p.text_muted),
        );
        if major {
            let label = unit.fmt(unit.px_to_unit(dx, DPI));
            painter.text(
                Pos2::new(sx + 2.0, h_ruler.min.y + 4.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(9.0),
                p.text_muted,
            );
        }
    }

    let first_y = (top_doc / nice).floor() as i64;
    let last_y = (bottom_doc / nice).ceil() as i64;
    for i in first_y..=last_y {
        let dy = i as f32 * nice;
        let sy = to_screen(fanimate_core::Vec2::new(0.0, dy)).y;
        if sy < v_ruler.top() + RULER_SIZE || sy > v_ruler.bottom() {
            continue;
        }
        let major = (i % 5) == 0;
        let tick_w = if major { 8.0 } else { 4.0 };
        painter.hline(
            v_ruler.right() - tick_w..=v_ruler.right(),
            sy,
            Stroke::new(1.0, p.text_muted),
        );
        if major {
            // turned a quarter, reading upwards from the tick, so a long
            // number stays inside the 20 px strip instead of being cut off
            let label = unit.fmt(unit.px_to_unit(dy, DPI));
            let galley = painter.layout_no_wrap(label, egui::FontId::proportional(9.0), p.text_muted);
            let at = Pos2::new(v_ruler.min.x + 3.0, sy - 3.0);
            painter
                .with_clip_rect(v_ruler.intersect(painter.clip_rect()))
                .add(
                    egui::epaint::TextShape::new(at, galley, p.text_muted)
                        .with_angle(-std::f32::consts::FRAC_PI_2),
                );
        }
    }

    // cursor position marker
    if let Some(cursor) = painter.ctx().pointer_latest_pos() {
        let doc = fanimate_core::Vec2::new(
            (cursor.x - h_ruler.center().x) / zoom + center.x,
            (cursor.y - v_ruler.center().y) / zoom + center.y,
        );
        let cx = to_screen(fanimate_core::Vec2::new(doc.x, 0.0)).x;
        let cy = to_screen(fanimate_core::Vec2::new(0.0, doc.y)).y;
        painter.vline(cx, h_ruler.min.y..=h_ruler.max.y, Stroke::new(1.0, p.accent));
        painter.hline(v_ruler.min.x..=v_ruler.max.x, cy, Stroke::new(1.0, p.accent));
    }
}

/// The guides across the canvas, which can be dragged (back onto a ruler to
/// remove one), locked and deleted from a right-click menu; and dragging out
/// of a ruler makes a new one. They belong to the project.
#[allow(clippy::too_many_arguments)]
fn draw_guides(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    shell: &mut Shell,
    canvas_rect: Rect,
    h_ruler: Rect,
    v_ruler: Rect,
    to_doc: &impl Fn(f32, Pos2) -> fanimate_core::Vec2,
    to_screen: &impl Fn(fanimate_core::Vec2) -> Pos2,
) {
    let guide_color = Color32::from_rgb(0x40, 0xA0, 0xFF).gamma_multiply(0.8);
    let zoom = shell.view.zoom;
    let mut delete: Option<usize> = None;
    let mut lock: Option<usize> = None;
    let guides = shell.editor.doc.guides.clone();
    for (idx, guide) in guides.iter().enumerate() {
        let color = if guide.locked {
            guide_color.gamma_multiply(0.45)
        } else {
            guide_color
        };
        // where the line is on screen, and a thin strip around it to grab
        let (line, grab) = if guide.vertical {
            let sx = to_screen(fanimate_core::Vec2::new(guide.position, 0.0)).x;
            if sx < canvas_rect.left() || sx > canvas_rect.right() {
                continue;
            }
            (
                [
                    Pos2::new(sx, canvas_rect.top()),
                    Pos2::new(sx, canvas_rect.bottom()),
                ],
                Rect::from_center_size(
                    Pos2::new(sx, canvas_rect.center().y),
                    Vec2::new(6.0, canvas_rect.height()),
                ),
            )
        } else {
            let sy = to_screen(fanimate_core::Vec2::new(0.0, guide.position)).y;
            if sy < canvas_rect.top() || sy > canvas_rect.bottom() {
                continue;
            }
            (
                [
                    Pos2::new(canvas_rect.left(), sy),
                    Pos2::new(canvas_rect.right(), sy),
                ],
                Rect::from_center_size(
                    Pos2::new(canvas_rect.center().x, sy),
                    Vec2::new(canvas_rect.width(), 6.0),
                ),
            )
        };
        painter.line_segment(line, Stroke::new(1.0, color));
        let r = ui.interact(grab, ui.id().with(("guide", idx)), Sense::click_and_drag());
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(if guide.locked {
                egui::CursorIcon::NotAllowed
            } else if guide.vertical {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::ResizeVertical
            });
        }
        if r.drag_started_by(egui::PointerButton::Primary) && !guide.locked {
            shell.editor.begin_guide_drag();
            shell.ui.guide_drag = Some(idx);
        }
        if shell.ui.guide_drag == Some(idx)
            && let Some(pos) = r.interact_pointer_pos()
        {
            let at = to_doc(zoom, pos);
            shell
                .editor
                .move_guide(idx, if guide.vertical { at.x } else { at.y });
        }
        if r.drag_stopped() && shell.ui.guide_drag == Some(idx) {
            shell.ui.guide_drag = None;
            // let go over a ruler, and it goes back where it came from
            if let Some(pos) = r.interact_pointer_pos()
                && (h_ruler.contains(pos) || v_ruler.contains(pos))
            {
                delete = Some(idx);
            }
        }
        crate::menus::context_menu(&r, |ui| {
            if ui.button("Delete guide").clicked() {
                delete = Some(idx);
                ui.close();
            }
            if ui
                .button(if guide.locked {
                    "Unlock guide"
                } else {
                    "Lock guide"
                })
                .clicked()
            {
                lock = Some(idx);
                ui.close();
            }
        });
    }
    if let Some(idx) = delete {
        shell.editor.delete_guide(idx);
    } else if let Some(idx) = lock {
        shell.editor.toggle_guide_lock(idx);
    }

    // Drag a guide out of a ruler, as in other drawing programs: out of the
    // top ruler comes a horizontal line, out of the left ruler a vertical one.
    // The line follows the pointer while it is dragged, and is placed where
    // the pointer lets go, if that is on the canvas.
    let new_guide = Stroke::new(1.0, guide_color);
    let h_resp = ui.interact(h_ruler, ui.id().with("h-ruler-drag"), Sense::drag());
    if h_resp.dragged()
        && let Some(pos) = h_resp.interact_pointer_pos()
        && pos.y > h_ruler.bottom()
    {
        painter.hline(canvas_rect.x_range(), pos.y, new_guide);
    }
    if h_resp.drag_stopped()
        && let Some(pos) = h_resp.interact_pointer_pos()
        && pos.y > h_ruler.bottom()
    {
        shell.editor.add_guide(false, to_doc(zoom, pos).y);
    }
    let v_resp = ui.interact(v_ruler, ui.id().with("v-ruler-drag"), Sense::drag());
    if v_resp.dragged()
        && let Some(pos) = v_resp.interact_pointer_pos()
        && pos.x > v_ruler.right()
    {
        painter.vline(pos.x, canvas_rect.y_range(), new_guide);
    }
    if v_resp.drag_stopped()
        && let Some(pos) = v_resp.interact_pointer_pos()
        && pos.x > v_ruler.right()
    {
        shell.editor.add_guide(true, to_doc(zoom, pos).x);
    }
    if h_resp.dragged() || v_resp.dragged() {
        ui.ctx().request_repaint();
    }
}

/// Draw the canvas grid (minor and major lines) under the artwork.
/// Call this before the GPU texture is drawn.
pub fn grid(
    painter: &egui::Painter,
    shell: &Shell,
    canvas_rect: Rect,
    to_screen: &impl Fn(fanimate_core::Vec2) -> Pos2,
) {
    if !shell.show_grid {
        return;
    }
    let zoom = shell.view.zoom;
    let center = shell.view.center;
    let minor = shell.grid.minor_spacing();
    let major = shell.grid.spacing;
    if minor <= 0.0 || major <= 0.0 {
        return;
    }

    // translucent, so they show on the white page as well as the pasteboard
    let minor_color = Color32::from_rgba_unmultiplied(120, 120, 140, 40);
    let major_color = Color32::from_rgba_unmultiplied(120, 120, 140, 95);

    // visible document range
    let left_doc = (canvas_rect.left() - canvas_rect.center().x) / zoom + center.x;
    let right_doc = (canvas_rect.right() - canvas_rect.center().x) / zoom + center.x;
    let top_doc = (canvas_rect.top() - canvas_rect.center().y) / zoom + center.y;
    let bottom_doc = (canvas_rect.bottom() - canvas_rect.center().y) / zoom + center.y;

    // skip drawing if grid lines would be too dense
    let px_per_minor = minor * zoom;
    if px_per_minor < 4.0 {
        return;
    }

    let first_x = (left_doc / minor).floor() as i64;
    let last_x = (right_doc / minor).ceil() as i64;
    for i in first_x..=last_x {
        let dx = i as f32 * minor;
        let sx = to_screen(fanimate_core::Vec2::new(dx, 0.0)).x;
        let is_major = (dx / major).fract().abs() < 0.01 || ((dx + major * 0.5) / major).fract().abs() > 0.99;
        painter.vline(
            sx,
            canvas_rect.y_range(),
            Stroke::new(1.0, if is_major { major_color } else { minor_color }),
        );
    }

    let first_y = (top_doc / minor).floor() as i64;
    let last_y = (bottom_doc / minor).ceil() as i64;
    for i in first_y..=last_y {
        let dy = i as f32 * minor;
        let sy = to_screen(fanimate_core::Vec2::new(0.0, dy)).y;
        let is_major = (dy / major).fract().abs() < 0.01 || ((dy + major * 0.5) / major).fract().abs() > 0.99;
        painter.hline(
            canvas_rect.x_range(),
            sy,
            Stroke::new(1.0, if is_major { major_color } else { minor_color }),
        );
    }
}

/// Draw a snap hint line (start, end in document space) above the artwork.
#[allow(dead_code)]
pub fn snap_hint(
    painter: &egui::Painter,
    hint: Option<(fanimate_core::Vec2, fanimate_core::Vec2)>,
    to_screen: &impl Fn(fanimate_core::Vec2) -> Pos2,
    color: Color32,
) {
    if let Some((a, b)) = hint {
        let sa = to_screen(a);
        let sb = to_screen(b);
        painter.line_segment([sa, sb], Stroke::new(1.0, color.gamma_multiply(0.6)));
    }
}

/// Pick a "nice" step size close to `raw` (1, 2, 5, 10, 20, 50, …).
fn nice_step(raw: f32) -> f32 {
    if raw <= 0.0 {
        return 1.0;
    }
    let exp = raw.log10().floor();
    let base = 10.0_f32.powf(exp);
    let frac = raw / base;
    if frac < 1.5 {
        base
    } else if frac < 3.5 {
        2.0 * base
    } else if frac < 7.5 {
        5.0 * base
    } else {
        10.0 * base
    }
}
