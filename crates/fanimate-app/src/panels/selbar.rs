//! The toolbar that floats below the selection on the canvas, with the
//! commands that suit what is selected: edit its points, group, unite,
//! duplicate, delete, its colours, and the rest in a menu.
//!
//! Every button queues the same `Action` as the menu, the shortcut and the
//! context menu do; the colour swatches are the one exception, as they are
//! values rather than commands (like the Style panel's).

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, Id, Margin, Pos2, Rect, Stroke, pos2, vec2};
use fanimate_core::{LayerId, LayerKind, Style};
use fanimate_editor::{Action, Tool};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;
use crate::panels::handles::Screen;
use crate::panels::properties::{begin_edit, from_color32, to_color32};

/// How far below the selection the toolbar sits: past the rotate button.
const GAP: f32 = 54.0;
/// The toolbar's distance from the canvas edge.
const MARGIN: f32 = 8.0;
const HEIGHT: f32 = 34.0;

/// The box around what is selected, on screen, or `None` with nothing to
/// hang the toolbar on.
fn selection_bounds(shell: &Shell, screen: &Screen) -> Option<Rect> {
    let points: Vec<Pos2> = match shell.editor.tool {
        Tool::TransformLayer => shell
            .ui
            .frame
            .as_ref()?
            .corners
            .iter()
            .map(|c| screen.point(*c))
            .collect(),
        Tool::SelectPoints => shell
            .editor
            .selected_shape_outlines()
            .iter()
            .flatten()
            .flat_map(|(pts, _)| pts.iter().map(|p| screen.point(*p)))
            .collect(),
        _ => return None,
    };
    let mut it = points.into_iter().filter(|p| p.x.is_finite() && p.y.is_finite());
    let first = it.next()?;
    Some(it.fold(Rect::from_min_max(first, first), |r, p| {
        r.union(Rect::from_min_max(p, p))
    }))
}

/// `at`, moved the least that keeps a toolbar of `size` inside the canvas.
fn place_rect(at: Pos2, canvas: Rect, size: egui::Vec2) -> Pos2 {
    pos2(
        at.x.clamp(
            canvas.left() + MARGIN,
            (canvas.right() - size.x - MARGIN).max(canvas.left() + MARGIN),
        ),
        at.y.clamp(
            canvas.top() + MARGIN,
            (canvas.bottom() - size.y - MARGIN).max(canvas.top() + MARGIN),
        ),
    )
}

/// Where the toolbar's top left goes: centred below the selection, or above
/// it when there is no room below, and always inside the canvas.
fn place(bounds: Rect, canvas: Rect, size: egui::Vec2) -> Pos2 {
    let mut y = bounds.bottom() + GAP;
    if y + size.y > canvas.bottom() - MARGIN {
        y = bounds.top() - GAP - size.y + 24.0;
    }
    let x = bounds.center().x - size.x / 2.0;
    place_rect(pos2(x, y), canvas, size)
}

pub fn show(ctx: &egui::Context, shell: &mut Shell, canvas: Rect, screen: &Screen) {
    let ed = &shell.editor;
    let quiet = shell.ui.handle_gesture.is_some()
        || shell.ui.point_gesture.is_some()
        || shell.ui.drag_origin.is_some()
        || shell.ui.renaming.is_some()
        || ed.playing
        || ed.selection.is_empty()
        || ed.is_dragging_keys();
    if quiet || !matches!(ed.tool, Tool::TransformLayer | Tool::SelectPoints) {
        return;
    }
    // points mode needs shapes to talk about
    if ed.tool == Tool::SelectPoints && ed.points.is_empty() {
        return;
    }
    let Some(bounds) = selection_bounds(shell, screen) else {
        return;
    };
    let size = shell
        .ui
        .selbar_size
        .unwrap_or(vec2(320.0, HEIGHT))
        .max(vec2(40.0, HEIGHT));
    // dragged by its grip, but only for as long as the same layers are chosen
    if shell.ui.selbar_for != shell.editor.selection {
        shell.ui.selbar_for = shell.editor.selection.clone();
        shell.ui.selbar_offset = egui::Vec2::ZERO;
    }
    let base = place(bounds, canvas, size);
    let pos = place_rect(base + shell.ui.selbar_offset, canvas, size);
    shell.ui.selbar_offset = pos - base;
    let p = shell.palette;
    let mut push = Vec::new();
    let area = egui::Area::new(Id::new("selection-toolbar"))
        .order(egui::Order::Middle)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(6.0)
                .inner_margin(Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.spacing_mut().button_padding = vec2(8.0, 4.0);
                    ui.horizontal(|ui| buttons(ui, shell, &mut push));
                })
        });
    shell.ui.selbar_size = Some(area.response.rect.size());
    shell.pending.extend(push);
}

/// A flat button with an icon and a label, queueing `action` when clicked.
fn button(ui: &mut egui::Ui, shell: &Shell, push: &mut Vec<Action>, icon: Icon, label: &str, action: Action) {
    let enabled = shell.editor.can_run(action);
    let tint = if enabled {
        shell.palette.text
    } else {
        shell.palette.text_muted
    };
    let image = icons::image(ui.ctx(), icon, 14.0, tint);
    let shortcut = shell.keymap.shortcut_text(action);
    let tip = if shortcut.is_empty() {
        action.label().to_owned()
    } else {
        format!("{} ({shortcut})", action.label())
    };
    let b = egui::Button::image_and_text(image, egui::RichText::new(label).color(tint)).frame(false);
    if ui.add_enabled(enabled, b).on_hover_text(tip).clicked() {
        push.push(action);
    }
}

/// The grip at the toolbar's left edge: drag it to move the toolbar out of
/// the way of the artwork.
fn grip(ui: &mut egui::Ui, shell: &mut Shell) {
    let (rect, r) = ui.allocate_exact_size(vec2(14.0, 24.0), egui::Sense::drag());
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        Icon::Grip,
        if r.hovered() || r.dragged() {
            shell.palette.text
        } else {
            shell.palette.text_muted
        },
    );
    if r.hovered() || r.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
    }
    if r.dragged() {
        shell.ui.selbar_offset += r.drag_delta();
    }
    r.on_hover_text("Drag to move this toolbar");
}

fn buttons(ui: &mut egui::Ui, shell: &mut Shell, push: &mut Vec<Action>) {
    grip(ui, shell);
    let ed = &shell.editor;
    let layers = ed.selected_roots();
    let single_vector = layers.len() == 1
        && matches!(
            ed.doc.find(layers[0]).map(|l| &l.kind),
            Some(LayerKind::Vector { .. })
        );
    match ed.tool {
        Tool::TransformLayer => {
            if single_vector {
                button(
                    ui,
                    shell,
                    push,
                    Icon::Tool(Tool::SelectPoints),
                    "Edit Points",
                    Action::Tool(Tool::SelectPoints),
                );
            }
            if layers.len() > 1 {
                button(ui, shell, push, Icon::GroupLayer, "Group", Action::GroupLayers);
            }
            // one layer has a pivot, and so do two or more chosen points
            if layers.len() == 1 || shell.editor.points.len() > 1 {
                button(ui, shell, push, Icon::Pivot, "", Action::PickPivot);
            }
        }
        _ => {
            button(
                ui,
                shell,
                push,
                Icon::Tool(Tool::TransformLayer),
                "Transform",
                Action::Tool(Tool::TransformLayer),
            );
            if shell.editor.can_run(Action::Union) {
                button(ui, shell, push, Icon::Union, "Unite", Action::Union);
            }
        }
    }
    if shell.editor.expandable_shapes() > 0
        && (shell.editor.points.is_empty() || shell.editor.radius_target().is_some())
    {
        button(
            ui,
            shell,
            push,
            Icon::Tool(Tool::Rectangle),
            "Expand",
            Action::ExpandShape,
        );
    }
    ui.separator();
    swatches(ui, shell);
    ui.separator();
    if shell
        .editor
        .can_run(Action::Align(fanimate_editor::AlignTo::Left))
        && shell.editor.tool == Tool::TransformLayer
    {
        let align = egui::Button::image_and_text(
            icons::image(ui.ctx(), Icon::Align, 14.0, shell.palette.text),
            egui::RichText::new("Align").color(shell.palette.text),
        )
        .frame(false);
        menus::menu_button(ui, align, |ui| menus::align_items(ui, shell))
            .on_hover_text("Line the layers up: one with the canvas, several with each other");
        button(ui, shell, push, Icon::Lock, "", Action::ToggleLock);
    }
    button(ui, shell, push, Icon::Duplicate, "", Action::Duplicate);
    button(ui, shell, push, Icon::Delete, "", Action::Delete);
    menus::menu_button(
        ui,
        egui::Button::new(egui::RichText::new("…").color(shell.palette.text)).frame(false),
        |ui| {
            menus::layer_context(ui, shell);
        },
    )
    .on_hover_text("More");
}

/// The shapes the colours apply to: those with chosen points, else every
/// shape on the selected vector layers.
fn target_shapes(shell: &Shell) -> Vec<(LayerId, usize)> {
    let ed = &shell.editor;
    if !ed.points.is_empty() {
        let chosen: BTreeSet<(LayerId, usize)> = ed.points.iter().map(|r| (r.layer, r.shape)).collect();
        return chosen.into_iter().collect();
    }
    ed.selected_roots()
        .into_iter()
        .filter_map(|id| Some((id, ed.layer_shapes(id)?.len())))
        .flat_map(|(id, n)| (0..n).map(move |i| (id, i)))
        .collect()
}

/// The fill and stroke colours of the selection, as two swatches.
fn swatches(ui: &mut egui::Ui, shell: &mut Shell) {
    let targets = target_shapes(shell);
    let Some(&(first_layer, first_index)) = targets.first() else {
        return;
    };
    let Some(style) = shell
        .editor
        .layer_shapes(first_layer)
        .and_then(|s| s.get(first_index))
        .map(|s| s.style_at(shell.editor.frame))
    else {
        return;
    };
    let mut fill = style.fill.map_or(Color32::TRANSPARENT, to_color32);
    let r = ui.color_edit_button_srgba(&mut fill).on_hover_text("Fill");
    if r.changed() {
        restyle(shell, &r, &targets, |s| s.fill = Some(from_color32(fill)));
    }
    let mut stroke = style.stroke.map_or(Color32::TRANSPARENT, |s| to_color32(s.color));
    let r = ui.color_edit_button_srgba(&mut stroke).on_hover_text("Stroke");
    if r.changed() {
        restyle(shell, &r, &targets, |s| {
            let width = s.stroke.map_or(1.0, |o| o.width);
            s.stroke = Some(fanimate_core::Stroke {
                color: from_color32(stroke),
                width: if width > 0.0 { width } else { 1.0 },
            });
        });
    }
}

/// Change the style of `targets`, as one undo step for one drag of a colour.
fn restyle(shell: &mut Shell, r: &egui::Response, targets: &[(LayerId, usize)], change: impl Fn(&mut Style)) {
    begin_edit(shell, r, "Change style");
    for &(layer, index) in targets {
        let Some(mut style) = shell
            .editor
            .layer_shapes(layer)
            .and_then(|s| s.get(index))
            .map(|s| s.style_at(shell.editor.frame))
        else {
            continue;
        };
        change(&mut style);
        shell.editor.set_shape_style(layer, index, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toolbar_sits_below_the_selection_and_stays_on_the_canvas() {
        let canvas = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let size = vec2(300.0, HEIGHT);
        let bounds = Rect::from_min_size(pos2(300.0, 100.0), vec2(100.0, 80.0));
        let at = place(bounds, canvas, size);
        assert_eq!(at.x, bounds.center().x - 150.0);
        assert_eq!(at.y, bounds.bottom() + GAP);
        // no room below: above the selection
        let low = Rect::from_min_size(pos2(300.0, 480.0), vec2(100.0, 80.0));
        assert!(place(low, canvas, size).y < low.top());
        // never off the sides
        let left = Rect::from_min_size(pos2(0.0, 100.0), vec2(20.0, 20.0));
        assert!(place(left, canvas, size).x >= MARGIN);
        let right = Rect::from_min_size(pos2(790.0, 100.0), vec2(5.0, 20.0));
        assert!(place(right, canvas, size).x + size.x <= canvas.right() - MARGIN + 0.001);
    }
}
