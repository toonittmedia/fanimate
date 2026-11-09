//! The Style panel: the brush and the style of new artwork, then the fill
//! and stroke of the shapes on the selected layer.
//!
//! The shape list supports drag-to-reorder: grab the ⠿ handle on the left
//! to move a shape up or down in the drawing order.

use eframe::egui::{self, RichText};
use fanimate_core::Stroke;
use fanimate_editor::{Action, StatusKind, Tool};

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
    palette(ui, shell);
    ui.separator();
    selected_shapes(ui, shell);
}

/// The project's palette: every colour its artwork uses. Click a swatch to draw
/// with it; change one with its picker to recolour everything that shows it, in
/// one undo step per drag.
fn palette(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    ui.label(RichText::new("Palette").strong().color(p.text));
    // the list is worked out when the document changes, not every frame
    let revision = shell.editor.revision();
    if shell
        .ui
        .palette_cache
        .as_ref()
        .is_none_or(|(r, _)| *r != revision)
    {
        shell.ui.palette_cache = Some((revision, shell.editor.palette()));
    }
    let entries = shell
        .ui
        .palette_cache
        .as_ref()
        .map(|(_, e)| e.clone())
        .unwrap_or_default();
    if entries.len() < 2 && shell.editor.doc.layers.is_empty() {
        ui.label(RichText::new("Colours used in the artwork appear here.").color(p.text_muted));
        return;
    }
    let can_edit = !shell.editor.playing;
    let mut change = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for (i, e) in entries.iter().enumerate() {
            let mut shown = to_color32(e.color);
            let r = ui
                .add_enabled_ui(can_edit, |ui| ui.color_edit_button_srgba(&mut shown))
                .inner;
            let r = r.on_hover_text(format!(
                "{} - used {} time{}. Change it to recolour everything that uses it.",
                e.color.to_hex(),
                e.count,
                if e.count == 1 { "" } else { "s" }
            ));
            if r.changed() {
                change = Some((i, e.color, from_color32(shown), r));
            }
        }
    });
    if let Some((_, from, to, response)) = change {
        begin_edit(shell, &response, "Replace colour");
        shell.editor.replace_colour_live(from, to);
        // the swatch now shows the new colour: refresh at once so the next frame's `from` is right
        shell.ui.palette_cache = None;
    }
    kit_note(ui, &p);
}

fn kit_note(ui: &mut egui::Ui, p: &crate::theme::Palette) {
    crate::kit::note(
        ui,
        p,
        "Colours here are the ones the artwork uses (fills, outlines, colour keys and the page; text is set in its own panel).",
    );
}

/// Settings for what is drawn next. These are not part of the document, so
/// changing them is not an undo step.
fn new_artwork(ui: &mut egui::Ui, shell: &mut Shell) {
    let muted = shell.palette.text_muted;
    ui.label(
        RichText::new("Brushes and new shapes")
            .strong()
            .color(shell.palette.text),
    );
    ui.add_space(2.0);
    style_line(ui, shell);
    ui.add_space(4.0);
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

            ui.label(RichText::new("Hardness").color(muted));
            let mut hard = d.hardness * 100.0;
            if ui
                .add(egui::Slider::new(&mut hard, 0.0..=100.0).suffix("%").max_decimals(0))
                .on_hover_text("How hard the edge of a stroke is on a raster layer: 0 is all soft, 100 a sharp edge")
                .changed()
            {
                d.hardness = hard / 100.0;
            }
            ui.end_row();

            ui.label(RichText::new("Flow").color(muted));
            let mut flow = d.brush_flow * 100.0;
            if ui
                .add(egui::Slider::new(&mut flow, 1.0..=100.0).suffix("%").max_decimals(0))
                .on_hover_text("How strongly each dab of a raster brush lays paint. Lower builds the colour up gradually as you go over it again.")
                .changed()
            {
                d.brush_flow = (flow / 100.0).clamp(0.0, 1.0);
            }
            ui.end_row();

            ui.label(RichText::new("Pressure to flow").color(muted));
            let mut fp = d.brush_flow_pressure * 100.0;
            if ui
                .add(egui::Slider::new(&mut fp, 0.0..=100.0).suffix("%").max_decimals(0))
                .on_hover_text("How much a pen's pressure also lowers the flow of a raster brush, so a light touch lays lighter paint. 0 leaves pressure changing only the size. A mouse has no pressure.")
                .changed()
            {
                d.brush_flow_pressure = (fp / 100.0).clamp(0.0, 1.0);
            }
            ui.end_row();

            ui.label(RichText::new("Smudge").color(muted));
            let mut smudge = d.brush_smudge * 100.0;
            if ui
                .add(egui::Slider::new(&mut smudge, 0.0..=100.0).suffix("%").max_decimals(0))
                .on_hover_text("Above 0 the raster Brush drags the paint under it along the stroke instead of laying the line colour. 0 paints normally.")
                .changed()
            {
                d.brush_smudge = (smudge / 100.0).clamp(0.0, 1.0);
            }
            ui.end_row();

            ui.label(RichText::new("Spacing").color(muted));
            let mut spacing = d.brush_spacing * 100.0;
            if ui
                .add(egui::Slider::new(&mut spacing, 5.0..=100.0).suffix("%").max_decimals(0))
                .on_hover_text("The distance between the dabs of a raster brush, as a share of its size. Small is a smooth line; large shows separate dots.")
                .changed()
            {
                d.brush_spacing = (spacing / 100.0).clamp(0.05, 1.0);
            }
            ui.end_row();

            ui.label(RichText::new("Pen pressure").color(muted));
            ui.checkbox(&mut d.pressure, "").on_hover_text(
                "A pen's pressure sets the width of a brush stroke. Off draws an even width. A mouse has no pressure, so it draws at full width either way.",
            );
            ui.end_row();

            ui.label(RichText::new("Eraser").color(muted));
            ui.add(egui::DragValue::new(&mut d.eraser).range(1.0..=1000.0).speed(0.5))
                .on_hover_text("Eraser size, in canvas pixels");
            ui.end_row();

            ui.label(RichText::new("Bucket tolerance").color(muted));
            let mut tolerance = d.fill_tolerance * 100.0;
            if ui
                .add(egui::Slider::new(&mut tolerance, 0.0..=50.0).suffix("%").max_decimals(0))
                .on_hover_text("How different a pixel's colour may be from the one you click and still be filled by the Paint Bucket on a raster layer. Higher fills across soft edges and noise.")
                .changed()
            {
                d.fill_tolerance = (tolerance / 100.0).clamp(0.0, 1.0);
            }
            ui.end_row();

            ui.label(RichText::new("Bucket gap").color(muted));
            ui.add(egui::DragValue::new(&mut d.fill_gap).range(0..=8).suffix(" px"))
                .on_hover_text("The Paint Bucket on a raster layer stays inside an outline that has a break up to about twice this wide. 0 lets the paint out of any break.");
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

/// A name being asked for on the style line: a new style's, or the chosen
/// one's new name.
#[derive(Clone)]
struct Naming {
    rename: bool,
    text: String,
    focus: bool,
}

/// Choosing, saving and managing styles on one line: the chosen style, a
/// save button, and a menu for the rest. Asking for a name takes the place of
/// the line, so it never grows taller.
fn style_line(ui: &mut egui::Ui, shell: &mut Shell) {
    let id = egui::Id::new("style-naming");
    if let Some(mut naming) = ui.data(|d| d.get_temp::<Naming>(id)) {
        let mut output = egui::TextEdit::singleline(&mut naming.text)
            .hint_text(if naming.rename {
                "New name"
            } else {
                "Name for the new style"
            })
            .desired_width(ui.available_width())
            .show(ui);
        let edit = output.response.clone();
        if std::mem::take(&mut naming.focus) {
            // asked for once, with the suggestion selected so typing replaces it
            edit.request_focus();
            let all = egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(naming.text.chars().count()),
            );
            output.state.cursor.set_char_range(Some(all));
            output.state.store(ui.ctx(), edit.id);
        }
        if edit.lost_focus() {
            // Enter or a click elsewhere keeps the name; Esc drops it
            let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
            ui.data_mut(|d| d.remove::<Naming>(id));
            if !cancelled {
                finish_naming(shell, &naming);
            }
        } else {
            ui.data_mut(|d| d.insert_temp(id, naming));
        }
        return;
    }

    let p = shell.palette;
    let edited = shell.styles.is_edited(&shell.editor.draw);
    let current = shell.styles.current().to_owned();
    let built_in = shell.styles.current_is_built_in();
    let shown = if edited {
        format!("{current} (edited)")
    } else {
        current.clone()
    };
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let width = (ui.available_width() - 22.0 - 20.0 - spacing * 2.0).max(60.0);
        egui::ComboBox::from_id_salt("style-choice")
            .selected_text(shown)
            .width(width)
            .show_ui(ui, |ui| {
                let names: Vec<String> = shell.styles.all().map(|s| s.name.clone()).collect();
                for name in names {
                    let label = if shell.styles.default_name() == name {
                        format!("{name} (default)")
                    } else {
                        name.clone()
                    };
                    if ui.selectable_label(name == current, label).clicked() {
                        shell.styles.select(&name, &mut shell.editor.draw);
                    }
                }
            })
            .response
            .on_hover_text("The style of new brush strokes and shapes");

        let update = edited && !built_in;
        let tip = if update {
            "Update this style with the look below"
        } else {
            "Save the look below as a new style"
        };
        if icons::button(ui, Icon::Save, 22.0, false, false)
            .on_hover_text(tip)
            .clicked()
        {
            if update {
                update_style(shell);
            } else {
                begin_naming(ui, &format!("{current} copy"), false);
            }
        }

        let more = egui::Button::new(RichText::new("…").color(p.text)).frame(false);
        menus::menu_button(ui, more, |ui| {
            let item = |ui: &mut egui::Ui, enabled: bool, text: &str| {
                let clicked = ui.add_enabled(enabled, egui::Button::new(text)).clicked();
                if clicked {
                    ui.close();
                }
                clicked
            };
            if item(ui, true, "Save as new style…") {
                begin_naming(ui, &format!("{current} copy"), false);
            }
            if item(ui, update, &format!("Update \"{current}\"")) {
                update_style(shell);
            }
            if item(ui, !built_in, "Rename…") {
                begin_naming(ui, &current, true);
            }
            if item(ui, !built_in, "Delete") {
                match shell.styles.delete(&mut shell.editor.draw) {
                    Ok(()) => shell.editor.set_status(StatusKind::Info, "Style deleted"),
                    Err(e) => shell.editor.set_status(StatusKind::Warning, e.message()),
                }
            }
            ui.separator();
            if item(ui, true, "Export style…") {
                export_style(shell);
            }
            if item(ui, true, "Import style…") {
                import_style(shell);
            }
            ui.separator();
            if item(ui, shell.styles.default_name() != current, "Set as default") {
                shell.styles.set_default();
                shell
                    .editor
                    .set_status(StatusKind::Info, format!("\"{current}\" is the default style"));
            }
            if item(ui, true, "Reset to default") {
                shell.styles.reset(&mut shell.editor.draw);
            }
        })
        .on_hover_text("More");
    });
}

/// Style > Export style…: the chosen style as a `.fnstyle` file to share.
fn export_style(shell: &mut Shell) {
    let name = shell.styles.current().to_owned();
    let Some(style) = shell.styles.get(&name).cloned() else {
        return;
    };
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export this style")
        .set_file_name(format!("{}.fnstyle", safe.trim()))
        .add_filter("Fanimate style", &["fnstyle"])
        .save_file()
    else {
        return;
    };
    match std::fs::write(&path, fanimate_fnm::style_file::to_text(&style)) {
        Ok(()) => shell
            .editor
            .set_status(StatusKind::Success, format!("Exported \"{name}\"")),
        Err(e) => shell.editor.set_status(
            StatusKind::Error,
            format!("The style could not be written to {}: {e}", path.display()),
        ),
    }
}

/// Style > Import style…: read a `.fnstyle` file, add it to the list and choose it.
fn import_style(shell: &mut Shell) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import a style")
        .add_filter("Fanimate style", &["fnstyle"])
        .pick_file()
    else {
        return;
    };
    // never read more than a style file can be
    let read = std::fs::File::open(&path).and_then(|f| {
        use std::io::Read;
        let mut text = String::new();
        f.take(fanimate_fnm::style_file::MAX_STYLE_FILE as u64 + 1)
            .read_to_string(&mut text)?;
        Ok(text)
    });
    let result = match read {
        Ok(text) => fanimate_fnm::style_file::from_text(&text),
        Err(e) => Err(format!("{}: {e}", path.display())),
    };
    match result {
        Ok(style) => match shell.styles.import(&style, &mut shell.editor.draw) {
            Ok(name) => shell
                .editor
                .set_status(StatusKind::Success, format!("Imported \"{name}\"")),
            Err(e) => shell.editor.set_status(StatusKind::Warning, e.message()),
        },
        Err(message) => shell.editor.set_status(StatusKind::Warning, message),
    }
}

fn begin_naming(ui: &egui::Ui, text: &str, rename: bool) {
    let naming = Naming {
        rename,
        text: text.to_owned(),
        focus: true,
    };
    ui.data_mut(|d| d.insert_temp(egui::Id::new("style-naming"), naming));
}

fn update_style(shell: &mut Shell) {
    match shell.styles.update(&shell.editor.draw) {
        Ok(()) => shell.editor.set_status(StatusKind::Info, "Style updated"),
        Err(e) => shell.editor.set_status(StatusKind::Warning, e.message()),
    }
}

fn finish_naming(shell: &mut Shell, naming: &Naming) {
    let result = if naming.rename {
        shell.styles.rename(&naming.text).map(|()| "Style renamed")
    } else {
        shell
            .styles
            .save_as(&naming.text, &shell.editor.draw)
            .map(|_| "Style saved")
    };
    match result {
        Ok(done) => shell.editor.set_status(StatusKind::Info, done),
        Err(e) => shell.editor.set_status(StatusKind::Warning, e.message()),
    }
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

    // no scroll area of its own: the whole panel scrolls, so a small screen
    // has one place to scroll, not a list inside a list
    ui.scope(|ui| {
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
