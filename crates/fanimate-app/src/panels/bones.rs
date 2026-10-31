//! The Bones panel: the selected bone's settings, and the rig as a tree.
//!
//! Parenting works as it does on the canvas with the Reparent Bone tool: drag
//! a row onto the bone that should be its parent (or between rows to make it
//! a sibling, or below the list to make it a root bone), pick a parent from
//! the Parent menu, or right-click a row. A bone keeps its place in the rest
//! pose when it is reparented. Not in the default workspace layout; add it
//! via the Rig workspace or Window menu.

use std::sync::Arc;

use eframe::egui::{self, CursorIcon, Id, Rect, RichText, Sense, Stroke};
use fanimate_core::{LayerId, LayerKind};
use fanimate_editor::{Action, BoneRef};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;
use crate::panels::properties::{begin_edit, end_edits, heading};

const ROW_H: f32 = 24.0;
const INDENT: f32 = 16.0;
/// The panel width from which the list and the settings sit side by side.
const WIDE: f32 = 560.0;
/// Room for the expand arrow at the start of a row.
const ARROW_W: f32 = 14.0;

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

    if shell.editor.bone_rows(layer_id).is_empty() {
        heading(ui, shell, "Bones");
        ui.label(RichText::new("No bones yet. Draw them with the Add Bone tool.").color(muted));
        return;
    }

    // The list comes first and stays where it is when a bone is picked. In a
    // wide panel (the bottom dock) the settings sit beside it; in a narrow one
    // (the right column) they follow it.
    let selected = shell.editor.selected_bone().filter(|b| b.layer == layer_id);
    reveal_selected(shell, layer_id);
    if ui.available_width() >= WIDE {
        ui.columns(2, |cols| {
            hierarchy(&mut cols[0], shell, layer_id);
            if let Some(b) = selected {
                bone_editor(&mut cols[1], shell, b);
            }
        });
    } else {
        hierarchy(ui, shell, layer_id);
        if let Some(b) = selected {
            ui.add_space(8.0);
            bone_editor(ui, shell, b);
        }
    }

    rig_health(ui, shell, layer_id);
}

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

fn hierarchy(ui: &mut egui::Ui, shell: &mut Shell, layer: LayerId) {
    heading(ui, shell, "Hierarchy");
    tree(ui, shell, layer);
    ui.label(
        RichText::new("Drag a bone onto its new parent. The Reparent Bone tool does the same on the canvas.")
            .size(11.0)
            .color(shell.palette.text_muted),
    );
}

/// Drag payload while reparenting bones.
struct DraggedBone {
    layer: LayerId,
    index: usize,
}

/// What a row names, for drawing and dropping.
struct Row {
    index: usize,
    depth: usize,
    name: String,
    parent: Option<usize>,
    has_children: bool,
    pinned: bool,
}

/// The rows of the tree, in tree order, with their details.
fn rows_of(shell: &Shell, layer: LayerId) -> Vec<Row> {
    let ed = &shell.editor;
    let bone = |i: usize| ed.bone(BoneRef { layer, bone: i });
    let all = ed.bone_rows(layer);
    all.iter()
        .filter_map(|&(index, depth)| {
            let b = bone(index)?;
            Some(Row {
                index,
                depth,
                name: b.name.clone(),
                parent: b.parent,
                has_children: all
                    .iter()
                    .any(|&(c, _)| bone(c).is_some_and(|cb| cb.parent == Some(index))),
                pinned: b.pinned,
            })
        })
        .collect()
}

/// A bone picked on the canvas (or anywhere else) is shown: the bones above
/// it are opened.
fn reveal_selected(shell: &mut Shell, layer: LayerId) {
    let now = shell.editor.selected_bone().filter(|b| b.layer == layer);
    if now == shell.ui.bones_seen {
        return;
    }
    shell.ui.bones_seen = now;
    let Some(b) = now else { return };
    let mut cur = shell.editor.bone(b).and_then(|bone| bone.parent);
    let mut guard = 0;
    while let Some(p) = cur {
        shell.ui.bones_collapsed.remove(&(layer, p));
        cur = shell
            .editor
            .bone(BoneRef { layer, bone: p })
            .and_then(|bone| bone.parent);
        guard += 1;
        if guard > 10_000 {
            break;
        }
    }
}

fn tree(ui: &mut egui::Ui, shell: &mut Shell, layer: LayerId) {
    let rows = rows_of(shell, layer);
    let selected = shell
        .editor
        .selected_bone()
        .filter(|b| b.layer == layer)
        .map(|b| b.bone);
    let dragging = egui::DragAndDrop::has_payload_of_type::<DraggedBone>(ui.ctx());

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        // below a closed bone its descendants are left out
        let mut hidden_below: Option<usize> = None;
        let mut shade = false;
        for row in &rows {
            if let Some(d) = hidden_below {
                if row.depth > d {
                    continue;
                }
                hidden_below = None;
            }
            if row.has_children && shell.ui.bones_collapsed.contains(&(layer, row.index)) {
                hidden_below = Some(row.depth);
            }
            bone_row(ui, shell, layer, row, selected == Some(row.index), shade);
            shade = !shade;
        }
        root_drop_zone(ui, shell, layer, dragging);
    });
}

fn bone_row(ui: &mut egui::Ui, shell: &mut Shell, layer: LayerId, row: &Row, selected: bool, shade: bool) {
    let p = shell.palette;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 3.0, p.accent.gamma_multiply(0.35));
    } else if response.hovered() {
        painter.rect_filled(rect, 3.0, p.raised);
    } else if shade {
        painter.rect_filled(rect, 0.0, p.bg.gamma_multiply(0.5));
    }

    // the lines that join a bone to the ones above it, as in a tree view
    let level_x = |depth: usize| rect.left() + 8.0 + depth as f32 * INDENT;
    let guide = Stroke::new(1.0, p.border);
    for d in 1..=row.depth {
        let x = level_x(d - 1) + ARROW_W / 2.0;
        // the line of this bone's own level stops at its middle
        let bottom = if d == row.depth {
            rect.center().y
        } else {
            rect.bottom()
        };
        painter.vline(x, rect.top()..=bottom, guide);
    }
    if row.depth > 0 {
        let x = level_x(row.depth - 1) + ARROW_W / 2.0;
        let y = rect.center().y;
        painter.hline(x..=level_x(row.depth) + ARROW_W / 2.0, y, guide);
        // the arrow points at the parent, as the one on the canvas does
        let head = if selected { p.accent_strong } else { p.text_muted };
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x + 1.0, y),
                egui::pos2(x + 6.0, y - 3.0),
                egui::pos2(x + 6.0, y + 3.0),
            ],
            head,
            Stroke::NONE,
        ));
    }

    // the arrow that opens or closes the bones below
    let collapsed = shell.ui.bones_collapsed.contains(&(layer, row.index));
    let arrow_rect = Rect::from_min_size(
        egui::pos2(level_x(row.depth), rect.top()),
        egui::vec2(ARROW_W, ROW_H),
    );
    if row.has_children {
        let arrow = ui.interact(
            arrow_rect,
            Id::new(("bone-arrow", layer, row.index)),
            Sense::click(),
        );
        icons::paint(
            &painter,
            Rect::from_center_size(arrow_rect.center(), egui::vec2(14.0, 14.0)),
            if collapsed { Icon::Expand } else { Icon::Collapse },
            p.text_muted,
        );
        if arrow
            .on_hover_text(if collapsed {
                "Show the bones below"
            } else {
                "Hide the bones below"
            })
            .clicked()
        {
            if collapsed {
                shell.ui.bones_collapsed.remove(&(layer, row.index));
            } else {
                shell.ui.bones_collapsed.insert((layer, row.index));
            }
        }
    }
    let icon_center = egui::pos2(arrow_rect.right() + 9.0, rect.center().y);
    icons::paint(
        &painter,
        Rect::from_center_size(icon_center, egui::vec2(14.0, 14.0)),
        Icon::BoneLayer,
        if selected { p.accent_strong } else { p.bone },
    );

    // name, or the rename field
    let name_left = icon_center.x + 12.0;
    let renaming = shell
        .ui
        .bone_rename
        .as_ref()
        .is_some_and(|(lid, bi, _)| *lid == layer && *bi == row.index);
    if renaming {
        let name_rect = Rect::from_min_max(
            egui::pos2(name_left, rect.top() + 2.0),
            egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_rect));
        let (_, _, text) = shell.ui.bone_rename.as_mut().expect("checked above");
        let mut output = egui::TextEdit::singleline(text)
            .desired_width(f32::INFINITY)
            .show(&mut child);
        let edit = output.response;
        // asks for focus once, so a click elsewhere commits the name
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
            let b = BoneRef { layer: lid, bone: bi };
            shell.editor.checkpoint("Rename bone");
            shell.editor.edit_bone(b, |bone| bone.name = new_name.clone());
        }
    } else {
        painter.text(
            egui::pos2(name_left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &row.name,
            egui::FontId::proportional(13.0),
            if selected { p.text } else { p.text_muted },
        );
        if row.pinned {
            painter.text(
                egui::pos2(rect.right() - 8.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                "pinned",
                egui::FontId::proportional(11.0),
                p.text_muted,
            );
        }
    }

    let this = BoneRef {
        layer,
        bone: row.index,
    };
    if response.clicked() {
        shell.editor.select_bone(Some(this));
    }
    if response.double_clicked() {
        shell.ui.bone_rename = Some((layer, row.index, row.name.clone()));
        shell.ui.bone_rename_focus = true;
    }
    // right-click picks the row first, as in the Layers panel
    if response.secondary_clicked() && !selected {
        shell.editor.select_bone(Some(this));
    }
    menus::context_menu(&response, |ui| row_menu(ui, shell, this, row));

    drag_and_drop(ui, shell, &response, rect, layer, row, level_x(row.depth));
}

/// The right-click menu of a bone: parenting without dragging.
fn row_menu(ui: &mut egui::Ui, shell: &mut Shell, b: BoneRef, row: &Row) {
    if ui
        .add_enabled(row.parent.is_some(), egui::Button::new("Select Parent"))
        .clicked()
    {
        shell
            .editor
            .select_bone(row.parent.map(|bone| BoneRef { layer: b.layer, bone }));
        ui.close();
    }
    let choices = shell.editor.parent_choices(b);
    ui.add_enabled_ui(!choices.is_empty(), |ui| {
        ui.menu_button("Parent To", |ui| parent_items(ui, shell, b, &choices));
    });
    if ui
        .add_enabled(row.parent.is_some(), egui::Button::new("Make Root Bone"))
        .on_hover_text("Take the bone out from under its parent. It stays where it is.")
        .clicked()
    {
        shell.editor.set_bone_parent(b, None);
        ui.close();
    }
    ui.separator();
    if ui.button("Rename").clicked() {
        shell.ui.bone_rename = Some((b.layer, b.bone, row.name.clone()));
        shell.ui.bone_rename_focus = true;
        ui.close();
    }
    menus::item(ui, shell, Action::DeleteBone);
}

/// The bones `b` can be put under, in tree order, indented like the tree.
fn parent_items(ui: &mut egui::Ui, shell: &mut Shell, b: BoneRef, choices: &[usize]) {
    for row in rows_of(shell, b.layer) {
        if !choices.contains(&row.index) {
            continue;
        }
        let label = format!("{}{}", "   ".repeat(row.depth), row.name);
        if ui.button(label).clicked() {
            shell.editor.set_bone_parent(b, Some(row.index));
            ui.close();
        }
    }
}

/// Where a dragged bone lands on a row, from where the pointer is on it.
#[derive(Clone, Copy, PartialEq)]
enum Drop {
    /// On the row's middle: becomes a child of that bone.
    Into,
    /// Between rows: becomes a sibling of that bone (same parent).
    Beside { below: bool },
}

fn drag_and_drop(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    response: &egui::Response,
    rect: Rect,
    layer: LayerId,
    row: &Row,
    text_x: f32,
) {
    if response.drag_started_by(egui::PointerButton::Primary) {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DraggedBone {
                layer,
                index: row.index,
            },
        );
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    let Some(pointer) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) else {
        return;
    };
    let t = (pointer.y - rect.top()) / rect.height();
    let drop = if (0.25..0.75).contains(&t) {
        Drop::Into
    } else {
        Drop::Beside { below: t >= 0.5 }
    };
    let Some(dragged) = response
        .dnd_hover_payload::<DraggedBone>()
        .filter(|d| d.layer == layer)
    else {
        return;
    };
    let from = BoneRef {
        layer,
        bone: dragged.index,
    };
    let choices = shell.editor.parent_choices(from);
    let current = shell.editor.bone(from).and_then(|b| b.parent);
    // the parent the bone would get, and whether that is allowed and a change
    let new_parent = match drop {
        Drop::Into => Some(row.index),
        Drop::Beside { .. } => row.parent,
    };
    let allowed = match new_parent {
        Some(p) => choices.contains(&p),
        None => current.is_some(),
    };
    if !allowed {
        ui.ctx().set_cursor_icon(CursorIcon::NotAllowed);
        return;
    }
    let accent = shell.palette.accent_strong;
    let painter = ui.painter();
    match drop {
        Drop::Into => {
            painter.rect_stroke(
                rect.shrink(1.0),
                3.0,
                Stroke::new(1.5, accent),
                egui::StrokeKind::Inside,
            );
        }
        Drop::Beside { below } => {
            let y = if below { rect.bottom() } else { rect.top() };
            painter.hline(text_x..=rect.right(), y, Stroke::new(2.0, accent));
        }
    }
    if response.dnd_release_payload::<DraggedBone>().is_some() {
        shell.editor.set_bone_parent(from, new_parent);
    }
}

/// The strip under the rows: a bone let go here becomes a root bone.
fn root_drop_zone(ui: &mut egui::Ui, shell: &mut Shell, layer: LayerId, dragging: bool) {
    let height = if dragging { 30.0 } else { 6.0 };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    if !dragging {
        return;
    }
    let p = shell.palette;
    let over = response.hovered();
    let payload = response
        .dnd_hover_payload::<DraggedBone>()
        .filter(|d| d.layer == layer);
    let can = payload.as_ref().is_some_and(|d| {
        shell
            .editor
            .bone(BoneRef { layer, bone: d.index })
            .is_some_and(|b| b.parent.is_some())
    });
    let color = if over && can { p.accent_strong } else { p.border };
    ui.painter().rect_stroke(
        rect.shrink2(egui::vec2(2.0, 3.0)),
        3.0,
        Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop here to make a root bone",
        egui::FontId::proportional(12.0),
        if over && can { p.text } else { p.text_muted },
    );
    if over && payload.is_some() && !can {
        ui.ctx().set_cursor_icon(CursorIcon::NotAllowed);
    }
    if can && let Some(dropped) = response.dnd_release_payload::<DraggedBone>() {
        let dropped: Arc<DraggedBone> = dropped;
        shell.editor.set_bone_parent(
            BoneRef {
                layer,
                bone: dropped.index,
            },
            None,
        );
    }
}

// ---------------------------------------------------------------------------
// Selected bone editor
// ---------------------------------------------------------------------------

fn bone_editor(ui: &mut egui::Ui, shell: &mut Shell, b: BoneRef) {
    let Some(bone) = shell.editor.bone(b).cloned() else {
        return;
    };
    heading(ui, shell, "Selected bone");
    let mut edited = bone.clone();
    let mut changed: Option<(egui::Response, &'static str)> = None;
    let mut new_parent: Option<Option<usize>> = None;
    let rows = rows_of(shell, b.layer);
    let choices = shell.editor.parent_choices(b);
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
            ui.horizontal(|ui| {
                let current = edited
                    .parent
                    .and_then(|p| rows.iter().find(|r| r.index == p))
                    .map_or("None (root bone)", |r| r.name.as_str());
                egui::ComboBox::from_id_salt(("bone-parent", b.bone))
                    .selected_text(current)
                    .width(128.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(edited.parent.is_none(), "None (root bone)")
                            .clicked()
                            && edited.parent.is_some()
                        {
                            new_parent = Some(None);
                        }
                        // the tree, indented; a bone below this one would make a loop
                        for row in &rows {
                            if row.index == b.bone {
                                continue;
                            }
                            let label = format!("{}{}", "   ".repeat(row.depth), row.name);
                            let is_current = edited.parent == Some(row.index);
                            let ok = is_current || choices.contains(&row.index);
                            let clicked = ui
                                .add_enabled_ui(ok, |ui| ui.selectable_label(is_current, label))
                                .inner
                                .clicked();
                            if clicked && !is_current {
                                new_parent = Some(Some(row.index));
                            }
                        }
                    });
                if let Some(p) = edited.parent
                    && ui
                        .small_button("Go to")
                        .on_hover_text("Select the parent bone")
                        .clicked()
                {
                    shell.editor.select_bone(Some(BoneRef {
                        layer: b.layer,
                        bone: p,
                    }));
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
    end_edits(ui, shell);
    if let Some(parent) = new_parent {
        shell.editor.set_bone_parent(b, parent);
    }
    secondary_motion(ui, shell);
}

/// Spring settings and the button that bakes them: a tail, ear or scarf
/// follows the bones above it, swings and settles, and the swing is written
/// as ordinary keys the animator can then edit.
fn secondary_motion(ui: &mut egui::Ui, shell: &mut Shell) {
    use fanimate_core::physics::SpringParams;
    heading(ui, shell, "Secondary motion");
    ui.label(
        egui::RichText::new(
            "Makes the bone and those below it follow their keys on a spring, thrown about by the bones above. The result is baked to keys.",
        )
        .color(shell.palette.text_muted),
    );
    let mut spring = shell.editor.spring;
    egui::Grid::new("spring-settings")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("Stiffness");
            ui.add(
                egui::DragValue::new(&mut spring.stiffness)
                    .range(SpringParams::STIFFNESS_RANGE.0..=SpringParams::STIFFNESS_RANGE.1)
                    .speed(1.0),
            )
            .on_hover_text(
                "How hard the bone is pulled back to its key. 100 is a loose tail, 1000 a stiff one",
            );
            ui.end_row();
            ui.label("Damping");
            ui.add(
                egui::DragValue::new(&mut spring.damping)
                    .range(SpringParams::DAMPING_RANGE.0..=SpringParams::DAMPING_RANGE.1)
                    .speed(0.1),
            )
            .on_hover_text(
                "How fast the swing dies away. Twice the square root of the stiffness stops it overshooting",
            );
            ui.end_row();
            ui.label("Inertia");
            ui.add(
                egui::DragValue::new(&mut spring.inertia)
                    .range(SpringParams::INERTIA_RANGE.0..=SpringParams::INERTIA_RANGE.1)
                    .speed(0.02),
            )
            .on_hover_text("How much the turning of the bones above throws it. 0 is none, 1 a free weight");
            ui.end_row();
            ui.label("Gravity");
            ui.add(
                egui::DragValue::new(&mut spring.gravity)
                    .range(SpringParams::GRAVITY_RANGE.0..=SpringParams::GRAVITY_RANGE.1)
                    .speed(0.2),
            )
            .on_hover_text(
                "How hard gravity pulls the bone down the canvas. 0 is none; 10 droops a loose tail gently",
            );
            ui.end_row();
            ui.label("Wind");
            ui.add(
                egui::DragValue::new(&mut spring.wind)
                    .range(SpringParams::WIND_RANGE.0..=SpringParams::WIND_RANGE.1)
                    .speed(0.2),
            )
            .on_hover_text("How hard a steady wind blows the bone to the right. Negative blows left");
            ui.end_row();
        });
    shell.editor.spring = spring;
    let enabled = shell.editor.can_run(fanimate_editor::Action::BakeSpring);
    if ui
        .add_enabled(enabled, egui::Button::new(fanimate_editor::Action::BakeSpring.label()))
        .on_hover_text("Replaces the keys of the bone and those below it, over the whole animation. Undo brings them back")
        .clicked()
    {
        shell.pending.push(fanimate_editor::Action::BakeSpring);
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
