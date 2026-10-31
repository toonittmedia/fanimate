//! The Actions panel: the selected bone layer's actions and Smart Bone
//! actions. Double-click one to edit it; the timeline and every tool then
//! work on the action until Finish Editing Action.

use eframe::egui::{self, Id, RichText, Sense, vec2};
use fanimate_editor::Action;

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;

const ROW_HEIGHT: f32 = 26.0;

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let Some(layer) = shell.editor.actions_layer() else {
        ui.add_space(8.0);
        ui.label(RichText::new("Actions and Smart Bones").strong());
        ui.label(
            RichText::new(
                "Select a bone layer to see its actions. A Smart Bone action reshapes artwork \
                 as a bone turns, such as a head turning to three-quarter view.",
            )
            .color(p.text_muted),
        );
        return;
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for action in [Action::NewAction, Action::NewSmartBoneAction] {
            let enabled = shell.editor.can_run(action);
            let icon = if action == Action::NewAction {
                Icon::Action
            } else {
                Icon::Tool(fanimate_editor::Tool::ManipulateBones)
            };
            let tip = match action {
                Action::NewSmartBoneAction => "New Smart Bone Action, driven by the selected bone",
                _ => "New Action: an animation you can insert into the timeline",
            };
            if icons::button(ui, icon, 24.0, false, !enabled)
                .on_hover_text(tip)
                .clicked()
                && enabled
            {
                shell.pending.push(action);
            }
        }
        let can_save = shell.editor.can_run(Action::SavePose);
        if ui
            .add_enabled(can_save, egui::Button::new(Action::SavePose.label()))
            .on_hover_text(
                "Save the whole rig as it is on this frame. Insert Action Here puts it back on any frame",
            )
            .clicked()
        {
            shell.pending.push(Action::SavePose);
        }
        if shell.editor.editing_action().is_some() {
            let done = egui::Button::new(RichText::new("Finish editing").color(p.on_button)).fill(p.button);
            if ui.add(done).clicked() {
                shell.pending.push(Action::FinishAction);
            }
        }
    });
    ui.separator();

    let rows: Vec<(String, bool)> = shell
        .editor
        .rig_actions(layer)
        .iter()
        .map(|a| (a.name.clone(), a.driver.is_some()))
        .collect();
    if rows.is_empty() {
        ui.label(RichText::new("No actions yet.").color(p.text_muted));
    }
    let editing = shell.editor.editing_action();
    egui::ScrollArea::vertical()
        .id_salt("action-rows")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, (name, smart)) in rows.into_iter().enumerate() {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
                let response = ui.interact(rect, Id::new(("action-row", layer, i)), Sense::click());
                let open = editing == Some((layer, i));
                let painter = ui.painter_at(rect);
                if open {
                    painter.rect_filled(rect, 3.0, p.accent.gamma_multiply(0.35));
                } else if response.hovered() {
                    painter.rect_filled(rect, 3.0, p.raised);
                }
                let icon = if smart {
                    Icon::Tool(fanimate_editor::Tool::ManipulateBones)
                } else {
                    Icon::Action
                };
                icons::paint(
                    &painter,
                    egui::Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(16.0, 16.0)),
                    icon,
                    if smart { p.bone } else { p.text_muted },
                );
                painter.text(
                    rect.left_center() + vec2(30.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &name,
                    egui::FontId::proportional(13.0),
                    p.text,
                );
                if smart {
                    painter.text(
                        rect.right_center() - vec2(8.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        "Smart Bone",
                        egui::FontId::proportional(11.0),
                        p.text_muted,
                    );
                }
                if response.double_clicked() && !open {
                    shell.pending.push(Action::EditAction(i));
                }
                menus::context_menu(&response, |ui| {
                    if open {
                        menus::item(ui, shell, Action::FinishAction);
                    } else {
                        menus::item(ui, shell, Action::EditAction(i));
                    }
                    if !smart {
                        menus::item(ui, shell, Action::InsertAction(i));
                    }
                    if shell.editor.can_run(Action::MirrorPose(i)) {
                        menus::item(ui, shell, Action::MirrorPose(i));
                    }
                    let rigs = shell.editor.other_bone_layers(layer);
                    if !rigs.is_empty() {
                        ui.menu_button("Copy to Rig", |ui| {
                            ui.set_min_width(menus::MENU_MIN_WIDTH);
                            for (id, _) in &rigs {
                                menus::item(ui, shell, Action::RetargetAction(i, id.0));
                            }
                        });
                    }
                    ui.separator();
                    menus::item(ui, shell, Action::DeleteAction(i));
                });
            }
            super::controllers::section(ui, shell, layer);
        });
}
