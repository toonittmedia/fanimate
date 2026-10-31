//! The master controllers of the selected bone layer, in the Actions panel:
//! a slider or a pad whose value plays the actions it is linked to, so one
//! control can pose a whole face or body. The commands are `Action`s (New
//! Slider, Link, Delete); dragging a value is `Editor::change_controller_value`
//! after one `begin_edit`, so a drag is one undo step.

use eframe::egui::{self, Pos2, RichText, Sense, Stroke, vec2};
use fanimate_core::LayerId;
use fanimate_core::actions::{ControllerAxis, ControllerKind};
use fanimate_editor::Action;

use super::properties::{begin_edit, end_edits};
use crate::app::Shell;
use crate::menus;

const PAD_SIZE: f32 = 112.0;

pub fn section(ui: &mut egui::Ui, shell: &mut Shell, layer: LayerId) {
    let p = shell.palette;
    ui.add_space(10.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Controllers").strong());
        for kind in [ControllerKind::Slider, ControllerKind::Pad] {
            let action = Action::NewController(kind);
            let enabled = shell.editor.can_run(action);
            if ui
                .add_enabled(enabled, egui::Button::new(action.label()))
                .on_hover_text(match kind {
                    ControllerKind::Slider => "A slider whose value plays actions, from 0 to 1",
                    ControllerKind::Pad => "A square pad with two values, each playing actions",
                })
                .clicked()
            {
                shell.pending.push(action);
            }
        }
    });
    let count = shell.editor.controllers(layer).len();
    if count == 0 {
        ui.label(
            RichText::new("A controller plays actions as you drag it. Right-click one to link actions.")
                .color(p.text_muted),
        );
    }
    let frame = shell.editor.current_frame() as f32;
    let open = shell.editor.controllers_open();
    for i in 0..count {
        let c = &shell.editor.controllers(layer)[i];
        let (name, kind) = (c.name.clone(), c.kind);
        let (x, y) = (
            c.value(ControllerAxis::X, frame),
            c.value(ControllerAxis::Y, frame),
        );
        let links: Vec<(usize, ControllerAxis, f32, f32)> =
            c.links.iter().map(|l| (l.action, l.axis, l.from, l.to)).collect();
        ui.add_space(6.0);
        let mut edited = name.clone();
        let head = ui.add_enabled(
            open,
            egui::TextEdit::singleline(&mut edited)
                .font(egui::TextStyle::Heading.resolve(ui.style()))
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY),
        );
        if head.changed() {
            begin_edit(shell, &head, "Rename controller");
            // only fails while playing or editing an action, which disable the box
            let _ = shell.editor.change_controller_name(layer, i, &edited);
        }
        for (n, (action, axis, from, to)) in links.iter().enumerate() {
            let action_name = shell
                .editor
                .rig_actions(layer)
                .get(*action)
                .map_or("?", |a| a.name.as_str())
                .to_owned();
            ui.horizontal(|ui| {
                let text = if kind == ControllerKind::Pad {
                    format!("Plays \"{action_name}\" ({})", axis_name(*axis))
                } else {
                    format!("Plays \"{action_name}\"")
                };
                ui.label(RichText::new(text).size(11.0).color(p.text_muted));
                ui.add_enabled_ui(open, |ui| {
                    let (mut a, mut b) = (*from, *to);
                    let ra = ui.add(egui::DragValue::new(&mut a).speed(0.01).max_decimals(2));
                    ui.label(RichText::new("to").size(11.0).color(p.text_muted));
                    let rb = ui.add(egui::DragValue::new(&mut b).speed(0.01).max_decimals(2));
                    for r in [ra, rb] {
                        if r.changed() {
                            begin_edit(shell, &r, "Change link range");
                            // a range of two equal numbers is refused and the old one stays
                            let _ = shell.editor.change_link_range(layer, i, n, a, b);
                        }
                    }
                })
                .response
                .on_hover_text(
                    "The value where the action is on its first frame, then where it is on its last",
                );
            });
        }
        ui.add_enabled_ui(open, |ui| match kind {
            ControllerKind::Slider => {
                let mut v = x;
                let r = ui.add(egui::Slider::new(&mut v, 0.0..=1.0).step_by(0.01).max_decimals(2));
                if r.changed() {
                    begin_edit(shell, &r, "Set controller");
                    // only fails while playing or editing an action, which disable this
                    let _ = shell
                        .editor
                        .change_controller_value(layer, i, ControllerAxis::X, v);
                }
            }
            ControllerKind::Pad => {
                if let Some((nx, ny, r)) = pad(ui, shell, x, y) {
                    begin_edit(shell, &r, "Set controller");
                    let _ = shell
                        .editor
                        .change_controller_value(layer, i, ControllerAxis::X, nx);
                    let _ = shell
                        .editor
                        .change_controller_value(layer, i, ControllerAxis::Y, ny);
                }
            }
        });
        menus::context_menu(&head, |ui| {
            menu(ui, shell, layer, i, kind, &links);
        });
    }
    end_edits(ui, shell);
}

fn axis_name(axis: ControllerAxis) -> &'static str {
    match axis {
        ControllerAxis::X => "across",
        ControllerAxis::Y => "down",
    }
}

/// A square pad with a dot at (x, y). Returns the new values while the user
/// presses or drags in it, and the response that made them.
fn pad(ui: &mut egui::Ui, shell: &Shell, x: f32, y: f32) -> Option<(f32, f32, egui::Response)> {
    let p = shell.palette;
    let (rect, _) = ui.allocate_exact_size(vec2(PAD_SIZE, PAD_SIZE), Sense::hover());
    let r = ui.interact(rect, ui.next_auto_id(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, p.raised);
    painter.rect_stroke(rect, 3.0, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    let mid = rect.center();
    let line = Stroke::new(1.0, p.border);
    painter.line_segment(
        [Pos2::new(rect.left(), mid.y), Pos2::new(rect.right(), mid.y)],
        line,
    );
    painter.line_segment(
        [Pos2::new(mid.x, rect.top()), Pos2::new(mid.x, rect.bottom())],
        line,
    );
    let dot = Pos2::new(rect.left() + x * rect.width(), rect.top() + y * rect.height());
    painter.circle_filled(dot, 5.0, p.accent);
    if r.is_pointer_button_down_on()
        && let Some(pos) = r.interact_pointer_pos()
    {
        let nx = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let ny = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        if (nx, ny) != (x, y) {
            return Some((nx, ny, r));
        }
    }
    None
}

fn menu(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    layer: LayerId,
    controller: usize,
    kind: ControllerKind,
    links: &[(usize, ControllerAxis, f32, f32)],
) {
    let axes: &[ControllerAxis] = if kind == ControllerKind::Pad {
        &[ControllerAxis::X, ControllerAxis::Y]
    } else {
        &[ControllerAxis::X]
    };
    let regular: Vec<usize> = shell
        .editor
        .rig_actions(layer)
        .iter()
        .enumerate()
        .filter(|(_, a)| a.driver.is_none())
        .map(|(i, _)| i)
        .collect();
    ui.menu_button("Play Action", |ui| {
        if regular.is_empty() {
            ui.label(RichText::new("No actions to play yet").color(shell.palette.text_muted));
        }
        for action in &regular {
            for axis in axes {
                menus::item(ui, shell, Action::LinkController(controller, *action, *axis));
            }
        }
    });
    if !links.is_empty() {
        ui.menu_button("Stop Playing", |ui| {
            let mut seen = Vec::new();
            for (action, ..) in links {
                if !seen.contains(action) {
                    seen.push(*action);
                    menus::item(ui, shell, Action::UnlinkController(controller, *action));
                }
            }
        });
    }
    ui.separator();
    menus::item(ui, shell, Action::DeleteController(controller));
}
