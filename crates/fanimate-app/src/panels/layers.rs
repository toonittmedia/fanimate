//! The Layers panel.
//!
//! Fixes Moho's awkward layer handling: right-click menus on every row,
//! standard selection (click, Ctrl-click, Shift-click), drag and drop to
//! reorder or nest, rename in place with a double-click or F2, and an eye
//! toggle that works with one click.

use std::sync::Arc;

use eframe::egui::{self, CursorIcon, Id, Rect, Sense, Stroke};
use fanimate_core::{LayerId, LayerKind};
use fanimate_editor::{Action, Placement, SelectMode};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;

const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 16.0;

/// Payload while dragging layers.
struct DraggedLayers(Vec<LayerId>);

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    toolbar(ui, shell);
    ui.add(
        egui::TextEdit::singleline(&mut shell.ui.layer_filter)
            .hint_text("Filter layers by name")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(2.0);

    let filter = shell.ui.layer_filter.trim().to_lowercase();
    let rows: Vec<(LayerId, usize)> = if filter.is_empty() {
        shell.editor.display_rows()
    } else {
        // filtering shows matches flat, ignoring collapsed groups
        let mut all = Vec::new();
        shell.editor.doc.walk(|l| {
            if l.name.to_lowercase().contains(&filter) {
                all.push((l.id, 0));
            }
        });
        all.reverse();
        all
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (id, depth) in rows {
                row(ui, shell, id, depth);
            }
            // empty space below the rows: click to deselect, right-click for New
            let rest = ui.allocate_response(
                egui::vec2(ui.available_width(), ui.available_height().max(40.0)),
                Sense::click(),
            );
            if rest.clicked() {
                shell.editor.selection.clear();
            }
            menus::context_menu(&rest, |ui| {
                menus::item(ui, shell, Action::NewVectorLayer);
                menus::item(ui, shell, Action::NewGroupLayer);
                menus::item(ui, shell, Action::NewBoneLayer);
                menus::item(ui, shell, Action::NewFramesLayer);
                ui.separator();
                menus::item(ui, shell, Action::Paste);
            });
        });
}

fn toolbar(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.horizontal(|ui| {
        let tip = |a: Action, shell: &Shell| {
            let s = shell.keymap.shortcut_text(a);
            if s.is_empty() {
                a.label().to_owned()
            } else {
                format!("{} ({s})", a.label())
            }
        };
        let text = shell.palette.text;
        let new = egui::Button::image_and_text(icons::image(ui.ctx(), Icon::Add, 16.0, text), "New");
        menus::menu_button(ui, new, |ui| {
            menus::item(ui, shell, Action::NewVectorLayer);
            menus::item(ui, shell, Action::NewGroupLayer);
            menus::item(ui, shell, Action::NewBoneLayer);
            menus::item(ui, shell, Action::NewFramesLayer);
            menus::item(ui, shell, Action::NewSwitchLayer);
        });
        for (icon, action) in [
            (Icon::Duplicate, Action::Duplicate),
            (Icon::Delete, Action::Delete),
            (Icon::Up, Action::MoveLayerUp),
            (Icon::Down, Action::MoveLayerDown),
        ] {
            let enabled = shell.editor.can_run(action);
            if icons::button(ui, icon, 24.0, false, !enabled)
                .on_hover_text(tip(action, shell))
                .clicked()
                && enabled
            {
                shell.pending.push(action);
            }
        }
    });
}

fn type_icon(kind: &LayerKind) -> Icon {
    match kind {
        LayerKind::Vector { .. } => Icon::VectorLayer,
        LayerKind::Group { .. } => Icon::GroupLayer,
        LayerKind::Bone { .. } => Icon::BoneLayer,
        LayerKind::Image(_) => Icon::ImageLayer,
        LayerKind::Frames { .. } => Icon::FramesLayer,
        LayerKind::Switch { .. } => Icon::SwitchLayer,
        LayerKind::Audio(_) => Icon::AudioLayer,
    }
}

fn row(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId, depth: usize) {
    let Some(layer) = shell.editor.doc.find(id) else {
        return;
    };
    let (name, visible, is_container, kind_icon, label) = (
        layer.name.clone(),
        layer.visible,
        layer.kind.children().is_some(),
        type_icon(&layer.kind),
        layer.shown_label(),
    );
    let collapsed = shell.editor.collapsed.contains(&id);
    let selected = shell.editor.is_selected(id);
    let p = shell.palette;

    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 3.0, p.accent.gamma_multiply(0.35));
    } else if response.hovered() {
        painter.rect_filled(rect, 3.0, p.raised);
    }

    // eye toggle
    let eye_rect = Rect::from_min_size(rect.min + egui::vec2(4.0, 0.0), egui::vec2(20.0, ROW_HEIGHT));
    let eye = ui.interact(eye_rect, Id::new(("eye", id)), Sense::click());
    icons::paint(
        &painter,
        Rect::from_center_size(eye_rect.center(), egui::vec2(16.0, 16.0)),
        if visible { Icon::Eye } else { Icon::EyeClosed },
        if visible { p.text } else { p.text_muted },
    );
    if eye
        .on_hover_text(if visible { "Hide layer" } else { "Show layer" })
        .clicked()
    {
        shell.editor.set_visible(id, !visible);
    }

    // the padlock at the end of the row: shown when locked, or on hover
    let locked = shell.editor.doc.find(id).is_some_and(|l| l.locked);
    let lock_rect = Rect::from_min_size(
        egui::pos2(rect.right() - 24.0, rect.top()),
        egui::vec2(20.0, ROW_HEIGHT),
    );
    if locked || response.hovered() {
        let lock = ui.interact(lock_rect, Id::new(("lock", id)), Sense::click());
        icons::paint(
            &painter,
            Rect::from_center_size(lock_rect.center(), egui::vec2(14.0, 14.0)),
            if locked { Icon::Lock } else { Icon::Unlock },
            if locked {
                p.text
            } else {
                p.text_muted.gamma_multiply(0.6)
            },
        );
        if lock
            .on_hover_text(if locked {
                "Locked: the canvas and tools leave this layer alone. Click to unlock."
            } else {
                "Lock this layer"
            })
            .clicked()
        {
            shell.editor.set_locked(id, !locked);
        }
    }

    // label colour swatch
    let swatch = Rect::from_center_size(
        egui::pos2(eye_rect.right() + 8.0, rect.center().y),
        egui::vec2(10.0, 10.0),
    );
    painter.rect_filled(swatch, 2.0, crate::theme::label_color(label));

    // expand arrow for groups and bone layers
    let x0 = swatch.right() + 4.0 + depth as f32 * INDENT;
    let arrow_rect = Rect::from_min_size(egui::pos2(x0, rect.top()), egui::vec2(16.0, ROW_HEIGHT));
    if is_container {
        let arrow = ui.interact(arrow_rect, Id::new(("arrow", id)), Sense::click());
        icons::paint(
            &painter,
            Rect::from_center_size(arrow_rect.center(), egui::vec2(16.0, 16.0)),
            if collapsed { Icon::Expand } else { Icon::Collapse },
            p.text_muted,
        );
        if arrow.clicked() {
            if collapsed {
                shell.editor.collapsed.remove(&id);
            } else {
                shell.editor.collapsed.insert(id);
            }
        }
    }
    icons::paint(
        &painter,
        Rect::from_center_size(
            egui::pos2(arrow_rect.right() + 10.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        ),
        kind_icon,
        p.text_muted,
    );

    // name, or the rename field
    let name_rect = Rect::from_min_max(egui::pos2(arrow_rect.right() + 24.0, rect.top()), rect.max);
    let renaming = shell.ui.renaming.as_ref().is_some_and(|(r, _)| *r == id);
    if renaming {
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(name_rect.shrink2(egui::vec2(0.0, 2.0))));
        let (_, text) = shell.ui.renaming.as_mut().expect("checked above");
        let mut output = egui::TextEdit::singleline(text)
            .desired_width(f32::INFINITY)
            .show(&mut child);
        let edit = output.response;
        // focus once when renaming starts, with the name selected, so a
        // click anywhere else can take focus away and commit the rename
        if std::mem::take(&mut shell.ui.rename_focus) {
            edit.request_focus();
            let all = egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(text.chars().count()),
            );
            output.state.cursor.set_char_range(Some(all));
            output.state.store(child.ctx(), edit.id);
        }
        let escape = child.input(|i| i.key_pressed(egui::Key::Escape));
        if escape {
            shell.ui.renaming = None;
        } else if edit.lost_focus()
            && let Some((id, text)) = shell.ui.renaming.take()
        {
            // Enter, Tab or a click elsewhere commits
            shell.editor.rename(id, &text);
        }
    } else {
        let color = if visible { p.text } else { p.text_muted };
        painter.text(
            egui::pos2(name_rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            &name,
            egui::FontId::proportional(13.0),
            color,
        );
    }

    // selection and rename
    let mods = ui.input(|i| i.modifiers);
    if response.clicked() {
        let mode = if mods.command {
            SelectMode::Toggle
        } else if mods.shift {
            SelectMode::Range
        } else {
            SelectMode::Replace
        };
        shell.editor.select(id, mode);
    }
    if response.double_clicked() {
        shell.ui.renaming = Some((id, name.clone()));
        shell.ui.rename_focus = true;
    }
    if shell.editor.rename_request == Some(id) {
        shell.editor.rename_request = None;
        shell.ui.renaming = Some((id, name.clone()));
        shell.ui.rename_focus = true;
    }

    // right-click: select the row first unless it is already selected,
    // as in every other editor
    if response.secondary_clicked() && !selected {
        shell.editor.select(id, SelectMode::Replace);
    }
    menus::context_menu(&response, |ui| menus::layer_context(ui, shell));

    drag_and_drop(ui, shell, &response, rect, id, is_container);
}

fn drag_and_drop(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    response: &egui::Response,
    rect: Rect,
    id: LayerId,
    is_container: bool,
) {
    if response.drag_started_by(egui::PointerButton::Primary) {
        if !shell.editor.is_selected(id) {
            shell.editor.select(id, SelectMode::Replace);
        }
        egui::DragAndDrop::set_payload(ui.ctx(), DraggedLayers(shell.editor.selection.clone()));
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }

    let Some(pointer) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) else {
        return;
    };
    let placement = {
        let t = (pointer.y - rect.top()) / rect.height();
        if is_container && (0.3..0.7).contains(&t) {
            Placement::Into
        } else if t < 0.5 {
            // the panel lists the top of the stack first
            Placement::Above
        } else {
            Placement::Below
        }
    };
    if response.dnd_hover_payload::<DraggedLayers>().is_some() {
        let accent = shell.palette.accent_strong;
        let painter = ui.painter();
        match placement {
            Placement::Into => {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    3.0,
                    Stroke::new(1.5, accent),
                    egui::StrokeKind::Inside,
                );
            }
            Placement::Above => {
                painter.hline(rect.x_range(), rect.top(), Stroke::new(2.0, accent));
            }
            Placement::Below => {
                painter.hline(rect.x_range(), rect.bottom(), Stroke::new(2.0, accent));
            }
        }
    }
    if let Some(payload) = response.dnd_release_payload::<DraggedLayers>() {
        let payload: Arc<DraggedLayers> = payload;
        if !shell.editor.move_relative(&payload.0, id, placement) {
            shell
                .editor
                .set_status(fanimate_editor::StatusKind::Info, "Can't move a layer there");
        }
    }
}
