//! The Bones panel: a bone list for the selected bone layer, plus the
//! selected bone's settings. Not in the default workspace layout; add it
//! via the Rig workspace or Window menu.

use std::sync::Arc;

use eframe::egui::{self, Rect, RichText, Sense, Stroke};
use fanimate_core::{LayerId, LayerKind};

use crate::app::Shell;
use crate::panels::properties::{begin_edit, heading};

const ROW_H: f32 = 24.0;
const INDENT: f32 = 14.0;

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.add_space(4.0);
    let muted = shell.palette.text_muted;

    // Which bone layer are we looking at?
    let layer_id = match shell.editor.primary() {
        Some(id)
            if shell
                .editor
                .doc
                .find(id)
                .is_some_and(|l| matches!(l.kind, LayerKind::Bone { .. })) =>
        {
            id
        }
        _ => {
            ui.label(RichText::new("Select a bone layer to edit its rig.").color(muted));
            return;
        }
    };

    let (names, parents): (Vec<String>, Vec<Option<usize>>) = {
        let l = shell.editor.doc.find(layer_id).expect("just checked");
        if let LayerKind::Bone { skeleton, .. } = &l.kind {
            (
                skeleton.bones.iter().map(|b| b.name.clone()).collect(),
                skeleton.bones.iter().map(|b| b.parent).collect(),
            )
        } else {
            return;
        }
    };

    heading(ui, shell, "Bones");
    if names.is_empty() {
        ui.label(RichText::new("No bones yet. Draw them with the Add Bone tool.").color(muted));
    } else {
        bone_list(ui, shell, layer_id, &names, &parents);
    }

    // Re-read names after any list mutation this frame
    let names_now: Vec<String> = shell
        .editor
        .doc
        .find(layer_id)
        .and_then(|l| {
            if let LayerKind::Bone { skeleton, .. } = &l.kind {
                Some(skeleton.bones.iter().map(|b| b.name.clone()).collect())
            } else {
                None
            }
        })
        .unwrap_or_default();

    if let Some(b) = shell.editor.selected_bone().filter(|b| b.layer == layer_id) {
        bone_editor(ui, shell, b, &names_now);
    }

    rig_health(ui, shell, layer_id);
}

// ---------------------------------------------------------------------------
// Bone list
// ---------------------------------------------------------------------------

/// Drag payload while reordering bones.
struct DraggedBone {
    layer: LayerId,
    index: usize,
}

fn bone_list(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    layer_id: LayerId,
    names: &[String],
    parents: &[Option<usize>],
) {
    let p = shell.palette;
    let depths: Vec<usize> = (0..names.len()).map(|i| bone_depth(names, parents, i)).collect();
    let selected = shell
        .editor
        .selected_bone()
        .filter(|b| b.layer == layer_id)
        .map(|b| b.bone);

    egui::ScrollArea::vertical()
        .id_salt("bones-panel-list")
        .max_height(240.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, name) in names.iter().enumerate() {
                let depth = depths[i];
                let is_selected = selected == Some(i);

                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                if is_selected {
                    painter.rect_filled(rect, 2.0, p.accent.gamma_multiply(0.35));
                } else if response.hovered() {
                    painter.rect_filled(rect, 2.0, p.raised);
                }

                let x_name = rect.left() + 6.0 + depth as f32 * INDENT;

                let renaming = shell
                    .ui
                    .bone_rename
                    .as_ref()
                    .is_some_and(|(lid, bi, _)| *lid == layer_id && *bi == i);

                if renaming {
                    let name_rect = Rect::from_min_max(
                        egui::pos2(x_name, rect.top() + 2.0),
                        egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
                    );
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_rect));
                    let (_, _, text) = shell.ui.bone_rename.as_mut().expect("checked above");
                    let mut output = egui::TextEdit::singleline(text)
                        .desired_width(f32::INFINITY)
                        .show(&mut child);
                    let edit = output.response;
                    if std::mem::take(&mut shell.ui.bone_rename_focus) {
                        edit.request_focus();
                        let all = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(text.chars().count()),
                        );
                        output.state.cursor.set_char_range(Some(all));
                        output.state.store(child.ctx(), edit.id);
                    }
                    if child.input(|i| i.key_pressed(egui::Key::Escape)) {
                        shell.ui.bone_rename = None;
                    } else if edit.lost_focus()
                        && let Some((lid, bi, new_name)) = shell.ui.bone_rename.take()
                    {
                        let b = fanimate_editor::BoneRef { layer: lid, bone: bi };
                        shell.editor.checkpoint("Rename bone");
                        shell.editor.edit_bone(b, |bone| bone.name = new_name.clone());
                    }
                } else {
                    painter.text(
                        egui::pos2(x_name, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        name,
                        egui::FontId::proportional(13.0),
                        if is_selected { p.text } else { p.text_muted },
                    );
                }

                if response.clicked() {
                    shell.editor.select_bone(Some(fanimate_editor::BoneRef {
                        layer: layer_id,
                        bone: i,
                    }));
                }
                if response.double_clicked() {
                    shell.ui.bone_rename = Some((layer_id, i, name.clone()));
                    shell.ui.bone_rename_focus = true;
                }

                bone_drag_and_drop(ui, shell, &response, rect, layer_id, i, names, parents);
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn bone_drag_and_drop(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    response: &egui::Response,
    rect: Rect,
    layer_id: LayerId,
    index: usize,
    names: &[String],
    parents: &[Option<usize>],
) {
    if response.drag_started_by(egui::PointerButton::Primary) {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DraggedBone {
                layer: layer_id,
                index,
            },
        );
    }
    let Some(pointer) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) else {
        return;
    };
    let t = (pointer.y - rect.top()) / rect.height();
    #[derive(PartialEq)]
    enum P {
        Above,
        Into,
        Below,
    }
    let placement = if (0.25..0.75).contains(&t) {
        P::Into
    } else if t < 0.5 {
        P::Above
    } else {
        P::Below
    };

    if response.dnd_hover_payload::<DraggedBone>().is_some() {
        let accent = shell.palette.accent_strong;
        let painter = ui.painter();
        match placement {
            P::Into => {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    2.0,
                    Stroke::new(1.5, accent),
                    egui::StrokeKind::Inside,
                );
            }
            P::Above => {
                painter.hline(rect.x_range(), rect.top(), Stroke::new(2.0, accent));
            }
            P::Below => {
                painter.hline(rect.x_range(), rect.bottom(), Stroke::new(2.0, accent));
            }
        }
    }
    if let Some(payload) = response.dnd_release_payload::<DraggedBone>() {
        let payload: Arc<DraggedBone> = payload;
        if payload.layer != layer_id {
            return;
        }
        let from = payload.index;
        match placement {
            P::Into => {
                if from != index {
                    shell.editor.set_bone_parent(
                        fanimate_editor::BoneRef {
                            layer: layer_id,
                            bone: from,
                        },
                        Some(index),
                    );
                }
            }
            P::Above => {
                let to = if from < index {
                    index.saturating_sub(1)
                } else {
                    index
                };
                shell.editor.reorder_bone(layer_id, from, to);
            }
            P::Below => {
                let to = if from > index {
                    (index + 1).min(names.len().saturating_sub(1))
                } else {
                    index
                };
                shell.editor.reorder_bone(layer_id, from, to);
            }
        }
        let _ = parents;
    }
}

// ---------------------------------------------------------------------------
// Selected bone editor
// ---------------------------------------------------------------------------

fn bone_editor(ui: &mut egui::Ui, shell: &mut Shell, b: fanimate_editor::BoneRef, names: &[String]) {
    let Some(bone) = shell.editor.bone(b).cloned() else {
        return;
    };
    ui.add_space(6.0);
    heading(ui, shell, "Selected bone");
    let mut edited = bone.clone();
    let mut changed: Option<(egui::Response, &'static str)> = None;
    let mut new_parent: Option<Option<usize>> = None;
    egui::Grid::new("bone-props")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("Name");
            let r = ui.add(egui::TextEdit::singleline(&mut edited.name).desired_width(140.0));
            if r.changed() {
                changed = Some((r, "Rename bone"));
            }
            ui.end_row();

            ui.label("Parent");
            let cur = edited
                .parent
                .and_then(|p| names.get(p))
                .map(|s| s.as_str())
                .unwrap_or("(root)");
            egui::ComboBox::from_id_salt(("bone-parent", b.bone))
                .selected_text(cur)
                .width(140.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(edited.parent.is_none(), "(root)").clicked()
                        && edited.parent.is_some()
                    {
                        new_parent = Some(None);
                    }
                    for (i, n) in names.iter().enumerate() {
                        if i == b.bone {
                            continue;
                        }
                        if ui.selectable_label(edited.parent == Some(i), n).clicked()
                            && edited.parent != Some(i)
                        {
                            new_parent = Some(Some(i));
                        }
                    }
                });
            ui.end_row();

            ui.label("Length");
            let r = ui.add(
                egui::DragValue::new(&mut edited.length)
                    .range(0.5..=100_000.0)
                    .speed(0.5),
            );
            if r.changed() {
                changed = Some((r, "Edit bone"));
            }
            ui.end_row();

            ui.label("Pinned");
            let r = ui
                .checkbox(&mut edited.pinned, "")
                .on_hover_text("Stays put when bones further down its chain are dragged");
            if r.changed() {
                changed = Some((r, "Pin bone"));
            }
            ui.end_row();

            ui.label("Angle limits");
            ui.horizontal(|ui| {
                let mut on = edited.limits.is_some();
                let r = ui.checkbox(&mut on, "");
                if r.changed() {
                    edited.limits = on.then_some([-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2]);
                    changed = Some((r, "Change bone limits"));
                }
                if let Some([lo, hi]) = &mut edited.limits {
                    let (mut a, mut z) = (lo.to_degrees(), hi.to_degrees());
                    let r1 = ui.add(
                        egui::DragValue::new(&mut a)
                            .range(-360.0..=z)
                            .suffix("°")
                            .speed(0.5),
                    );
                    ui.label("to");
                    let r2 = ui.add(
                        egui::DragValue::new(&mut z)
                            .range(a..=360.0)
                            .suffix("°")
                            .speed(0.5),
                    );
                    *lo = a.to_radians();
                    *hi = z.to_radians();
                    for r in [r1, r2] {
                        if r.changed() {
                            changed = Some((r, "Change bone limits"));
                        }
                    }
                }
            });
            ui.end_row();

            ui.label("Strength");
            let mut s = edited.influence();
            let r = ui
                .add(egui::DragValue::new(&mut s).range(1.0..=100_000.0).speed(0.5))
                .on_hover_text("How far the bone reaches when artwork is bound smoothly");
            if r.changed() {
                edited.strength = Some(s);
                changed = Some((r, "Change bone strength"));
            }
            ui.end_row();
        });
    if let Some((r, label)) = changed {
        begin_edit(shell, &r, label);
        shell.editor.edit_bone(b, |bone| *bone = edited);
    }
    if let Some(parent) = new_parent {
        shell.editor.set_bone_parent(b, parent);
    }
}

// ---------------------------------------------------------------------------
// Rig health
// ---------------------------------------------------------------------------

fn rig_health(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId) {
    let doc = &shell.editor.doc;
    let findings: Vec<String> = fanimate_core::health::check(doc)
        .into_iter()
        .filter(|f| doc.is_within(f.layer, id))
        .map(|f| f.message)
        .collect();
    ui.add_space(8.0);
    heading(ui, shell, "Rig health");
    if findings.is_empty() {
        ui.label(RichText::new("No problems found.").color(shell.palette.success));
    }
    for f in findings {
        ui.label(RichText::new(f).color(shell.palette.keyframe));
    }
}

fn bone_depth(names: &[String], parents: &[Option<usize>], i: usize) -> usize {
    let mut depth = 0;
    let mut cur = parents.get(i).copied().flatten();
    while let Some(p) = cur {
        depth += 1;
        cur = parents.get(p).copied().flatten();
        if depth > names.len() {
            break;
        }
    }
    depth
}
