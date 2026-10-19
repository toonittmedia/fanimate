//! The Style panel: the brush and the style of new artwork, then the fill
//! and stroke of the shapes on the selected layer.
//!
//! The shape list supports drag-to-reorder: grab the ⠿ handle on the left
//! to move a shape up or down in the drawing order.

use eframe::egui::{self, RichText};
use fanimate_core::Stroke;
use fanimate_editor::{Action, Tool};

use crate::app::Shell;
use crate::panels::properties::{begin_edit, end_edits, from_color32, to_color32};

/// Payload while dragging a shape row: its index in the shapes list.
#[derive(Clone, Copy)]
struct DraggedShape(usize);

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.add_space(4.0);
    new_artwork(ui, shell);
    ui.separator();
    selected_shapes(ui, shell);
}

/// Settings for what is drawn next. These are not part of the document, so
/// changing them is not an undo step.
fn new_artwork(ui: &mut egui::Ui, shell: &mut Shell) {
    let muted = shell.palette.text_muted;
    ui.label(
        RichText::new("Brush and new shapes")
            .strong()
            .color(shell.palette.text),
    );
    ui.add_space(2.0);
    let d = &mut shell.editor.draw;
    egui::Grid::new("draw-style")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Line").color(muted));
            let mut line = to_color32(d.line);
            if ui.color_edit_button_srgba(&mut line).changed() {
                d.line = from_color32(line);
            }
            ui.end_row();

            ui.label(RichText::new("Width").color(muted));
            ui.add(egui::DragValue::new(&mut d.width).range(0.1..=500.0).speed(0.1))
                .on_hover_text("Brush size at full pressure, and outline width, in canvas pixels");
            ui.end_row();

            ui.label(RichText::new("Outline").color(muted));
            ui.checkbox(&mut d.outline, "").on_hover_text(
                "Give new shapes an outline in the line colour. Shapes with no fill always get one.",
            );
            ui.end_row();

            ui.label(RichText::new("Fill").color(muted));
            ui.horizontal(|ui| {
                let mut on = d.fill.is_some();
                if ui
                    .checkbox(&mut on, "")
                    .on_hover_text("Fill new shapes")
                    .changed()
                {
                    d.fill = on.then_some(d.fill.unwrap_or(fanimate_core::Color::WHITE));
                }
                if let Some(fill) = &mut d.fill {
                    let mut c = to_color32(*fill);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        *fill = from_color32(c);
                    }
                }
            });
            ui.end_row();

            ui.label(RichText::new("Smoothing").color(muted));
            let mut pct = d.smoothing * 100.0;
            if ui
                .add(
                    egui::Slider::new(&mut pct, 0.0..=100.0)
                        .suffix("%")
                        .max_decimals(0),
                )
                .on_hover_text("Smooths shaky brush strokes")
                .changed()
            {
                d.smoothing = pct / 100.0;
            }
            ui.end_row();

            ui.label(RichText::new("Taper").color(muted));
            ui.add(egui::DragValue::new(&mut d.taper).range(0.0..=500.0).speed(0.5))
                .on_hover_text("Length of the thin ends of a brush stroke, in canvas pixels");
            ui.end_row();

            ui.label(RichText::new("Eraser").color(muted));
            ui.add(egui::DragValue::new(&mut d.eraser).range(1.0..=1000.0).speed(0.5))
                .on_hover_text("Eraser size, in canvas pixels");
            ui.end_row();

            ui.label(RichText::new("Weight brush").color(muted));
            ui.add(
                egui::DragValue::new(&mut d.weight_brush)
                    .range(1.0..=2000.0)
                    .speed(0.5),
            )
            .on_hover_text("Paint Weights brush size, in canvas pixels");
            ui.end_row();
        });
    ui.add_space(4.0);
}

fn selected_shapes(ui: &mut egui::Ui, shell: &mut Shell) {
    let muted = shell.palette.text_muted;
    let Some(id) = shell.editor.primary() else {
        ui.label(RichText::new("Select a vector layer to edit its colours.").color(muted));
        return;
    };
    let Some(shapes) = shell.editor.layer_shapes(id).cloned() else {
        let text = if shell.editor.is_drawable(id) {
            "No drawing on this frame yet."
        } else {
            "Style applies to vector and frame-by-frame layers."
        };
        ui.label(RichText::new(text).color(muted));
        return;
    };
    if shapes.is_empty() {
        let key = shell.keymap.shortcut_text(Action::Tool(Tool::Freehand));
        let tool = if key.is_empty() {
            String::new()
        } else {
            format!(" ({key})")
        };
        ui.label(
            RichText::new(format!(
                "This layer has no shapes yet. Draw on it with Freehand{tool}."
            ))
            .color(muted),
        );
        return;
    }

    ui.label(RichText::new("Shapes").strong().color(shell.palette.text));
    ui.add_space(2.0);

    // Column header
    let p = shell.palette;
    let accent = p.accent;
    egui::Grid::new("style-header")
        .num_columns(5)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            ui.label(""); // drag handle column
            ui.label(RichText::new("Shape").color(muted));
            ui.label(RichText::new("Fill").color(muted));
            ui.label(RichText::new("Stroke").color(muted));
            ui.label(RichText::new("Width").color(muted));
            ui.end_row();
        });

    let mut edited: Option<(usize, fanimate_core::Style, egui::Response)> = None;
    let mut reorder: Option<(usize, usize)> = None;

    egui::ScrollArea::vertical()
        .max_height(200.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, shape) in shapes.iter().enumerate() {
                let mut style = shape.style;
                let row_id = ui.id().with(("shape-row", i));

                ui.horizontal(|ui| {
                    // Drag handle: a grab cursor and DnD source
                    let handle_resp = ui.allocate_response(egui::vec2(14.0, 18.0), egui::Sense::drag());
                    // Draw the ⠿ dots
                    let painter = ui.painter_at(handle_resp.rect);
                    let c = handle_resp.rect.center();
                    for dy in [-4.0_f32, 0.0, 4.0] {
                        for dx in [-2.5_f32, 2.5] {
                            painter.circle_filled(
                                egui::pos2(c.x + dx, c.y + dy),
                                1.5,
                                if handle_resp.hovered() {
                                    p.text
                                } else {
                                    p.text_muted
                                },
                            );
                        }
                    }
                    if handle_resp.hovered() || handle_resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    if handle_resp.drag_started() {
                        egui::DragAndDrop::set_payload(ui.ctx(), DraggedShape(i));
                    }

                    // Drop zone: hover over a row to show where it would land
                    if let Some(src) = egui::DragAndDrop::payload::<DraggedShape>(ui.ctx()).map(|p| p.0)
                        && src != i
                    {
                        // draw an insertion line at the top of this row
                        let top = handle_resp.rect.left_top();
                        let right = egui::pos2(ui.max_rect().right(), top.y);
                        painter.hline(top.x..=right.x, top.y, egui::Stroke::new(2.0, accent));
                        // detect drop
                        if handle_resp.hovered() && ui.input(|inp| inp.pointer.any_released()) {
                            reorder = Some((src, i));
                            egui::DragAndDrop::clear_payload(ui.ctx());
                        }
                    }

                    // Shape name
                    ui.add_sized(
                        [80.0, 18.0],
                        egui::Label::new(egui::RichText::new(&shape.name).color(p.text).size(12.0))
                            .selectable(false),
                    );

                    // Fill colour
                    let mut fill = style.fill.map(to_color32).unwrap_or(egui::Color32::TRANSPARENT);
                    let r = ui.color_edit_button_srgba(&mut fill);
                    if r.changed() {
                        style.fill = Some(from_color32(fill));
                        edited = Some((i, style, r));
                    }

                    // Stroke colour
                    let mut stroke = style.stroke.unwrap_or(Stroke {
                        color: fanimate_core::Color::BLACK,
                        width: 0.0,
                    });
                    let mut c = to_color32(stroke.color);
                    let r = ui.color_edit_button_srgba(&mut c);
                    if r.changed() {
                        stroke.color = from_color32(c);
                        style.stroke = Some(stroke);
                        edited = Some((i, style, r));
                    }

                    // Stroke width
                    let r = ui.add(
                        egui::DragValue::new(&mut stroke.width)
                            .range(0.0..=200.0)
                            .speed(0.1),
                    );
                    if r.changed() {
                        style.stroke = (stroke.width > 0.0).then_some(stroke);
                        edited = Some((i, style, r));
                    }
                });

                let _ = row_id;
            }

            // Drop at end of list
            if let Some(src) = egui::DragAndDrop::payload::<DraggedShape>(ui.ctx()).map(|p| p.0) {
                let rest = ui.allocate_response(egui::vec2(ui.available_width(), 8.0), egui::Sense::hover());
                if rest.hovered() && ui.input(|inp| inp.pointer.any_released()) {
                    reorder = Some((src, shapes.len() - 1));
                    egui::DragAndDrop::clear_payload(ui.ctx());
                }
                if rest.hovered() {
                    let y = rest.rect.top();
                    let painter = ui.painter_at(rest.rect);
                    painter.hline(rest.rect.x_range(), y, egui::Stroke::new(2.0, accent));
                }
            }
        });

    if let Some((i, style, r)) = edited {
        begin_edit(shell, &r, "Change style");
        shell.editor.set_shape_style(id, i, style);
    }
    if let Some((from, to)) = reorder {
        shell.editor.reorder_shape(id, from, to);
    }
    end_edits(ui, shell);
}
