//! The X-sheet: the same timing as the timeline, laid out as traditional
//! animators write it. Frames run down the page, one column per layer. A
//! frame-by-frame layer's column numbers each drawing where it starts and
//! draws a line down its exposure; a switch layer's column names the
//! layer each key shows, like a lip sync chart; other layers mark their
//! keys.

use eframe::egui::{self, Id, Rect, RichText, Sense, Stroke, pos2, vec2};
use fanimate_core::{Frame, LayerId, LayerKind};
use fanimate_editor::{Action, SelectMode};

use crate::app::Shell;
use crate::menus;

const ROW: f32 = 18.0;
const COLUMN: f32 = 72.0;
const FRAME_COLUMN: f32 = 44.0;
const HEADER: f32 = 24.0;

/// What a column shows on one frame.
#[derive(Clone, PartialEq)]
enum Cell {
    Empty,
    /// A drawing starts here; its number.
    Drawing(usize),
    /// The drawing above is still exposed.
    Held,
    /// A key on a layer that is not frame-by-frame.
    Key,
    /// A switch layer's key: the layer it shows, as a mouth chart is read.
    Shows(String),
}

struct Column {
    id: LayerId,
    name: String,
    label: egui::Color32,
    cells: Vec<Cell>,
}

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let (start, end) = (shell.editor.doc.start, shell.editor.doc.end);
    let frames = (end - start + 1).max(0) as usize;
    let columns: Vec<Column> = shell
        .editor
        .display_rows()
        .into_iter()
        .filter_map(|(id, _)| {
            let layer = shell.editor.doc.find(id)?;
            let mut cells = vec![Cell::Empty; frames];
            match &layer.kind {
                LayerKind::Frames { cels } => {
                    for (n, cel) in cels.iter().enumerate() {
                        let until = cels.get(n + 1).map_or(end + 1, |c| c.frame);
                        for f in cel.frame.max(start)..until.min(end + 1) {
                            cells[(f - start) as usize] = if f == cel.frame {
                                Cell::Drawing(n + 1)
                            } else {
                                Cell::Held
                            };
                        }
                    }
                }
                LayerKind::Switch { switch, .. } => {
                    for k in switch {
                        if (start..=end).contains(&k.frame) {
                            cells[(k.frame - start) as usize] = Cell::Shows(k.value.clone());
                        }
                    }
                }
                // groups and bone layers hold other columns; their own keys
                // are shown too, so nothing is hidden
                _ => {
                    for f in layer.key_frames() {
                        if (start..=end).contains(&f) {
                            cells[(f - start) as usize] = Cell::Key;
                        }
                    }
                }
            }
            Some(Column {
                id,
                name: layer.name.clone(),
                label: crate::theme::label_color(layer.shown_label()),
                cells,
            })
        })
        .collect();
    if columns.is_empty() {
        ui.label(RichText::new("No layers yet.").color(p.text_muted));
        return;
    }
    let now = shell.editor.current_frame();
    let width = FRAME_COLUMN + COLUMN * columns.len() as f32;

    egui::ScrollArea::horizontal().id_salt("xsheet-h").show(ui, |ui| {
        ui.set_min_width(width);
        // header: layer names over their columns
        let (header, _) = ui.allocate_exact_size(vec2(width, HEADER), Sense::hover());
        let painter = ui.painter_at(header);
        painter.text(
            header.left_center() + vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "Frame",
            egui::FontId::proportional(11.0),
            p.text_muted,
        );
        for (c, col) in columns.iter().enumerate() {
            let x = header.left() + FRAME_COLUMN + c as f32 * COLUMN;
            let r = Rect::from_min_size(pos2(x, header.top()), vec2(COLUMN, HEADER));
            painter.rect_filled(Rect::from_min_size(r.min, vec2(COLUMN, 3.0)), 0.0, col.label);
            let selected = shell.editor.is_selected(col.id);
            painter.text(
                r.center() + vec2(0.0, 1.0),
                egui::Align2::CENTER_CENTER,
                truncate(&col.name, 10),
                egui::FontId::proportional(12.0),
                if selected { p.accent_strong } else { p.text },
            );
            let resp = ui.interact(r, Id::new(("xsheet-head", col.id)), Sense::click());
            if resp.clicked() {
                shell.editor.select(col.id, SelectMode::Replace);
            }
            if resp.secondary_clicked() && !selected {
                shell.editor.select(col.id, SelectMode::Replace);
            }
            menus::context_menu(&resp, |ui| menus::layer_context(ui, shell));
        }

        egui::ScrollArea::vertical()
            .id_salt("xsheet-v")
            .auto_shrink([false, false])
            .show_rows(ui, ROW, frames, |ui, range| {
                for i in range {
                    let f = start + i as Frame;
                    row(ui, shell, &columns, i, f, f == now, width);
                }
            });
    });
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    columns: &[Column],
    i: usize,
    f: Frame,
    current: bool,
    width: f32,
) {
    let p = shell.palette;
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    let painter = ui.painter_at(rect);
    if current {
        painter.rect_filled(rect, 0.0, p.keyframe.gamma_multiply(0.18));
    }
    // a heavier rule every second, as on printed sheets
    let fps = shell.editor.doc.fps.max(1.0).round() as Frame;
    let rule = if (f - shell.editor.doc.start) % fps == fps - 1 {
        p.border
    } else {
        p.border.gamma_multiply(0.4)
    };
    painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, rule));
    painter.text(
        rect.left_center() + vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        f.to_string(),
        crate::theme::mono(11.0),
        if current { p.text } else { p.text_muted },
    );
    for (c, col) in columns.iter().enumerate() {
        let x = rect.left() + FRAME_COLUMN + c as f32 * COLUMN;
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(COLUMN, ROW));
        painter.vline(
            cell.left(),
            cell.y_range(),
            Stroke::new(1.0, p.border.gamma_multiply(0.4)),
        );
        let selected = shell.editor.is_selected(col.id);
        if selected {
            painter.rect_filled(cell, 0.0, p.accent.gamma_multiply(0.12));
        }
        match &col.cells[i] {
            Cell::Shows(name) => {
                painter.text(
                    cell.center(),
                    egui::Align2::CENTER_CENTER,
                    truncate(name, 9),
                    egui::FontId::proportional(12.0),
                    p.text,
                );
            }
            Cell::Drawing(n) => {
                painter.text(
                    cell.center(),
                    egui::Align2::CENTER_CENTER,
                    n.to_string(),
                    crate::theme::mono(12.0),
                    p.text,
                );
            }
            Cell::Held => {
                painter.vline(
                    cell.center().x,
                    cell.y_range().shrink(1.0),
                    Stroke::new(1.5, col.label),
                );
            }
            Cell::Key => {
                let (c, s) = (cell.center(), 4.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        c + vec2(0.0, -s),
                        c + vec2(s, 0.0),
                        c + vec2(0.0, s),
                        c + vec2(-s, 0.0),
                    ],
                    p.keyframe,
                    Stroke::NONE,
                ));
            }
            Cell::Empty => {}
        }
        let resp = ui.interact(cell, Id::new(("xsheet-cell", col.id, f)), Sense::click());
        if resp.clicked() || resp.secondary_clicked() {
            shell.editor.go_to(f);
            if !selected || resp.clicked() {
                shell.editor.select(col.id, SelectMode::Replace);
            }
        }
        menus::context_menu(&resp, |ui| {
            ui.label(
                RichText::new(format!(
                    "Frame {f} | {}",
                    crate::timecode::clock(f, shell.editor.doc.fps)
                ))
                .font(crate::theme::mono(12.0))
                .color(p.text_muted),
            );
            menus::item(ui, shell, Action::NewDrawing);
            menus::item(ui, shell, Action::AddKey);
            menus::item(ui, shell, Action::DeleteKey);
        });
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    }
}
