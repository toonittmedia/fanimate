//! The Style panel: the brush and the style of new artwork, then the fill
//! and stroke of the shapes on the selected layer.
//!
//! The shape list supports drag-to-reorder: grab the ⠿ handle on the left
//! to move a shape up or down in the drawing order.

use eframe::egui::{self, RichText};
use fanimate_core::Stroke;
use fanimate_editor::{Action, Tool};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;
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

/// One shape's name in the list: click to select the shape on the canvas
/// (Ctrl or Shift adds), double-click to rename, the eye to hide it, and a
/// right-click menu to duplicate or delete it. A hidden shape is not drawn
/// and cannot be picked on the canvas.
fn shape_name(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    layer: fanimate_core::LayerId,
    i: usize,
    shape: &fanimate_core::Shape,
    selected: bool,
) {
    use fanimate_editor::SelectMode;
    let p = shell.palette;
    let visible = shape.visible;
    let eye = if visible { Icon::Eye } else { Icon::EyeClosed };
    if icons::button(ui, eye, 18.0, false, !visible)
        .on_hover_text(if visible {
            "Hide this shape"
        } else {
            "Show this shape"
        })
        .clicked()
    {
        shell.pending.push(Action::SetShapeVisible(layer, i, !visible));
    }
    if shell
        .ui
        .shape_rename
        .as_ref()
        .is_some_and(|(l, n, _)| *l == layer && *n == i)
    {
        let focus = std::mem::take(&mut shell.ui.shape_rename_focus);
        let Some((_, _, text)) = shell.ui.shape_rename.as_mut() else {
            return;
        };
        let edit = ui.add_sized([80.0, 18.0], egui::TextEdit::singleline(text));
        if focus {
            edit.request_focus();
        }
        if edit.lost_focus() {
            let cancelled = ui.input(|inp| inp.key_pressed(egui::Key::Escape));
            match shell.ui.shape_rename.take() {
                Some((_, _, text)) if !cancelled => shell.editor.rename_shape(layer, i, &text),
                _ => {}
            }
        }
        return;
    }
    let text = egui::RichText::new(&shape.name)
        .size(12.0)
        .color(if visible { p.text } else { p.text_muted });
    let r = ui.add_sized([80.0, 18.0], egui::Button::selectable(selected, text));
    if r.clicked() && visible {
        let m = ui.input(|inp| inp.modifiers);
        // picking a shape means working on its points
        if !shell.editor.tool.edits_points() {
            shell.editor.set_tool(Tool::SelectPoints);
        }
        shell.editor.select_shape(
            layer,
            i,
            if m.command {
                SelectMode::Toggle
            } else if m.shift {
                SelectMode::Range
            } else {
                SelectMode::Replace
            },
        );
    }
    if r.double_clicked() {
        shell.ui.shape_rename = Some((layer, i, shape.name.clone()));
        shell.ui.shape_rename_focus = true;
    }
    if r.secondary_clicked() && visible && !selected {
        shell.editor.select_shape(layer, i, SelectMode::Replace);
    }
    let r = r.on_hover_text("Click to select this shape. Double-click to rename it.");
    menus::context_menu(&r, |ui| {
        menus::item(ui, shell, Action::RenameShape(layer, i));
        menus::item(ui, shell, Action::SetShapeVisible(layer, i, !visible));
        ui.separator();
        // colours animate: key one here, then change it on another frame
        menus::item(
            ui,
            shell,
            Action::AddChannelKey(fanimate_core::ChannelId::FillColor(i)),
        );
        menus::item(
            ui,
            shell,
            Action::AddChannelKey(fanimate_core::ChannelId::StrokeColor(i)),
        );
        ui.separator();
        menus::item(ui, shell, Action::DuplicateShapes);
        menus::item(ui, shell, Action::DeleteShapes);
    });
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

    let frame = shell.editor.frame;
    let mut edited: Option<(usize, fanimate_core::Style, egui::Response)> = None;
    let mut reorder: Option<(usize, usize)> = None;
    // the shapes picked on the canvas, shown picked here too
    let selected: std::collections::HashSet<usize> = shell
        .editor
        .selected_shapes()
        .into_iter()
        .filter(|(l, _)| *l == id)
        .map(|(_, i)| i)
        .collect();
    // the Rename Shape command (F2 in this list, or the right-click menu)
    if let Some((layer, index)) = shell.editor.shape_rename_request.take()
        && layer == id
        && let Some(shape) = shapes.get(index)
    {
        shell.ui.shape_rename = Some((layer, index, shape.name.clone()));
        shell.ui.shape_rename_focus = true;
    }

    egui::ScrollArea::vertical()
        .max_height(200.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, shape) in shapes.iter().enumerate() {
                // the colours as they are on this frame, if they are animated
                let mut style = shape.style_at(frame);
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

                    shape_name(ui, shell, id, i, shape, selected.contains(&i));

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
