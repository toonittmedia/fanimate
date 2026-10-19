//! The Properties panel: layer or project settings, edited in place.
//!
//! Moho edits these in modal dialogs (Layer Settings, Project Settings)
//! that block the canvas. Here they live in a docked panel and update the
//! canvas live. Animated values follow the auto-key rule: if a channel is
//! animated, editing it sets a keyframe on the current frame.

use eframe::egui::{self, RichText};
use fanimate_core::{Color, LayerKind};
use fanimate_editor::Action;

use crate::app::Shell;

/// Call before applying a change from `r`: one undo step per drag or edit.
pub fn begin_edit(shell: &mut Shell, r: &egui::Response, label: &'static str) {
    if shell.ui.active_edit != Some(r.id) {
        shell.editor.checkpoint(label);
        shell.ui.active_edit = Some(r.id);
    }
}

/// Forget the active edit once its widget is no longer dragged or focused.
pub fn end_edits(ui: &egui::Ui, shell: &mut Shell) {
    if let Some(id) = shell.ui.active_edit
        && !ui.ctx().is_being_dragged(id)
        && !ui.ctx().memory(|m| m.has_focus(id))
    {
        shell.ui.active_edit = None;
    }
}

pub fn to_color32(c: Color) -> egui::Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(q(c.r), q(c.g), q(c.b), q(c.a))
}

pub fn from_color32(c: egui::Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color::rgba(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.add_space(4.0);
    match shell.editor.primary() {
        Some(id) if shell.editor.doc.find(id).is_some() => layer(ui, shell, id),
        _ => project(ui, shell),
    }
    end_edits(ui, shell);
}

/// The keyframe diamond beside an animated value (drawn, not a font glyph).
fn key_mark(ui: &mut egui::Ui, on: bool, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    if on {
        let (c, r) = (rect.center(), 4.0);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                c + egui::vec2(0.0, -r),
                c + egui::vec2(r, 0.0),
                c + egui::vec2(0.0, r),
                c + egui::vec2(-r, 0.0),
            ],
            color,
            egui::Stroke::NONE,
        ));
    }
}

pub fn heading(ui: &mut egui::Ui, shell: &Shell, text: &str) {
    ui.label(RichText::new(text).strong().color(shell.palette.text));
    ui.add_space(2.0);
}

fn layer(ui: &mut egui::Ui, shell: &mut Shell, id: fanimate_core::LayerId) {
    let Some(l) = shell.editor.doc.find(id) else {
        return;
    };
    let kind = match l.kind {
        LayerKind::Vector { .. } => "Vector layer",
        LayerKind::Group { .. } => "Group",
        LayerKind::Bone { .. } => "Bone layer",
        LayerKind::Image(_) => "Image layer",
        LayerKind::Frames { .. } => "Frame-by-frame layer",
        LayerKind::Switch { .. } => "Switch layer",
        LayerKind::Audio(_) => "Audio layer",
    };
    let mut name = l.name.clone();
    let mut visible = l.visible;
    // sound has no picture to place or fade
    let drawn = !matches!(l.kind, LayerKind::Audio(_));
    let animated = (
        l.transform.position.is_animated(),
        l.transform.rotation.is_animated(),
        l.transform.scale.is_animated(),
        l.opacity.is_animated(),
    );
    let muted = shell.palette.text_muted;
    let key_color = shell.palette.keyframe;

    heading(ui, shell, kind);
    egui::Grid::new("layer-props")
        .num_columns(3)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("Name");
            // a minimum too: the grid sizes its columns from the widest
            // row, and a sound layer has no wide rows
            let r = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(160.0)
                    .min_size(egui::vec2(160.0, 0.0)),
            );
            if r.lost_focus() {
                shell.editor.rename(id, &name);
            }
            ui.label("");
            ui.end_row();

            ui.label(if drawn { "Visible" } else { "Audible" });
            if ui.checkbox(&mut visible, "").changed() {
                shell.editor.set_visible(id, visible);
            }
            ui.label("");
            ui.end_row();

            if !drawn {
                return;
            }
            let Some(mut t) = shell.editor.layer_transform(id) else {
                return;
            };
            let mut opacity = shell.editor.layer_opacity(id).unwrap_or(1.0) * 100.0;
            // working out where the pivot is draws the layer; not while playing
            let shown_pivot = if shell.editor.playing {
                None
            } else {
                shell.editor.layer_pivot(id)
            };

            ui.label("Opacity");
            let r = ui.add(egui::Slider::new(&mut opacity, 0.0..=100.0).suffix("%"));
            if r.changed() {
                begin_edit(shell, &r, "Change opacity");
                shell.editor.set_layer_opacity(id, opacity / 100.0);
            }
            key_mark(ui, animated.3, key_color);
            ui.end_row();

            ui.label("Position");
            let mut changed = None;
            ui.horizontal(|ui| {
                for v in [&mut t.position.x, &mut t.position.y] {
                    let r = ui.add(egui::DragValue::new(v).speed(1.0).max_decimals(2));
                    if r.changed() {
                        changed = Some(r);
                    }
                }
            });
            key_mark(ui, animated.0, key_color);
            ui.end_row();

            ui.label("Rotation");
            let mut deg = t.rotation.to_degrees();
            let r = ui.add(
                egui::DragValue::new(&mut deg)
                    .speed(0.5)
                    .suffix("°")
                    .max_decimals(1),
            );
            if r.changed() {
                t.rotation = deg.to_radians();
                changed = Some(r);
            }
            key_mark(ui, animated.1, key_color);
            ui.end_row();

            ui.label("Scale");
            ui.horizontal(|ui| {
                let mut pct = t.scale * 100.0;
                for v in [&mut pct.x, &mut pct.y] {
                    let r = ui.add(egui::DragValue::new(v).speed(0.5).suffix("%").max_decimals(1));
                    if r.changed() {
                        changed = Some(r);
                    }
                }
                t.scale = pct / 100.0;
            });
            key_mark(ui, animated.2, key_color);
            ui.end_row();

            if let Some(r) = changed {
                begin_edit(shell, &r, "Transform layer");
                shell.editor.set_layer_transform(id, t);
            }

            // the centre of rotation and scale: one of nine anchors of the
            // layer's box, or any point, by number
            ui.label("Pivot");
            let mut pivot_edit = None;
            ui.horizontal(|ui| {
                if let Some((fx, fy)) = pivot_grid(ui, shell) {
                    shell.editor.move_pivot_to_anchor(fx, fy);
                }
                ui.vertical(|ui| {
                    let shown = shown_pivot.unwrap_or(t.pivot);
                    let (mut x, mut y) = (shown.x, shown.y);
                    let mut edited = None;
                    for v in [&mut x, &mut y] {
                        let r = ui.add(egui::DragValue::new(v).speed(1.0).max_decimals(1));
                        if r.changed() {
                            edited = Some(r);
                        }
                    }
                    if let Some(r) = edited {
                        pivot_edit = Some((r, fanimate_core::Vec2::new(x, y)));
                    }
                });
            });
            ui.label("");
            ui.end_row();
            if let Some((r, p)) = pivot_edit {
                begin_edit(shell, &r, "Move pivot");
                shell
                    .editor
                    .set_layer_pivot(id, fanimate_core::Vec2::new(p.x, p.y));
            }
        });
    rectangle(ui, shell);

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if !drawn {
            return;
        }
        ui.spacing_mut().item_spacing.x = 4.0;
        key_mark(ui, true, key_color);
        ui.label(
            RichText::new(format!(
                "marks animated values: editing them sets a keyframe on frame {}. \
                 Other values change for the whole animation.",
                shell.editor.current_frame()
            ))
            .size(11.0)
            .color(muted),
        );
    });
    if let Some(l) = shell.editor.doc.find(id)
        && let LayerKind::Frames { cels } = &l.kind
    {
        let count = cels.len();
        ui.add_space(8.0);
        heading(ui, shell, "Drawings");
        ui.label(RichText::new(format!("{count} drawing{}", if count == 1 { "" } else { "s" })).color(muted));
        part_picker(ui, shell);
    }
    if let Some(l) = shell.editor.doc.find(id)
        && let LayerKind::Image(img) = &l.kind
    {
        ui.add_space(8.0);
        heading(ui, shell, "Picture");
        let bound = if img.valid_skin().is_some() {
            format!(
                "Bends with the bones on a {}x{} mesh",
                img.cells()[0],
                img.cells()[1]
            )
        } else if img.bone.is_some() {
            "Follows one bone".to_owned()
        } else {
            "Not bound to bones".to_owned()
        };
        ui.label(format!("{:.0} x {:.0} px", img.width, img.height));
        ui.label(RichText::new(bound).color(muted));
        if !shell.editor.doc.media.contains_key(&img.media) {
            ui.label(RichText::new("The picture file is missing.").color(shell.palette.error));
        }
    }
    if let Some(l) = shell.editor.doc.find(id)
        && let LayerKind::Switch { children, switch } = &l.kind
    {
        let names: Vec<String> = children.iter().rev().map(|c| c.name.clone()).collect();
        let count = children.len();
        let shown =
            fanimate_core::scene::switch_choice(children, switch, shell.editor.current_frame() as f32)
                .map(|i| count - 1 - i);
        ui.add_space(8.0);
        heading(ui, shell, "Switch");
        if names.is_empty() {
            ui.label(RichText::new("Put one layer inside for each drawing to switch between.").color(muted));
        }
        // top of stack first, as in the Layers panel
        for (row, n) in names.iter().enumerate() {
            if ui
                .selectable_label(shown == Some(row), n)
                .on_hover_text("Show this layer from the current frame on")
                .clicked()
            {
                shell.pending.push(Action::SetSwitch(count - 1 - row));
            }
        }
        ui.add_space(4.0);
        if ui
            .button(Action::LipSync.label())
            .on_hover_text(
                "Keys this layer from a dialogue sound: name its layers A to H and X, \
                 or MBP, etc, E, AI, O, U, FV, L and rest",
            )
            .clicked()
        {
            shell.pending.push(Action::LipSync);
        }
        if shell.lipsync.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Listening to the sound…").color(muted));
            });
        }
    }
    if let Some(l) = shell.editor.doc.find(id)
        && let LayerKind::Audio(a) = &l.kind
    {
        let (mut start, mut volume) = (a.start, a.volume * 100.0);
        let info = shell
            .editor
            .doc
            .media
            .get(&a.media)
            .map(|f| fanimate_editor::sound_info(&f.bytes));
        ui.add_space(8.0);
        heading(ui, shell, "Sound");
        let mut changed = None;
        egui::Grid::new("audio-props")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label("Starts on frame");
                let r = ui.add(egui::DragValue::new(&mut start).speed(0.2));
                if r.changed() {
                    changed = Some(r);
                }
                ui.end_row();
                ui.label("Volume");
                let r = ui.add(egui::Slider::new(&mut volume, 0.0..=200.0).suffix("%"));
                if r.changed() {
                    changed = Some(r);
                }
                ui.end_row();
            });
        if let Some(r) = changed {
            begin_edit(shell, &r, "Change sound");
            shell.editor.set_audio(id, start, volume / 100.0);
        }
        match info {
            Some(Ok(i)) => {
                let fps = shell.editor.doc.fps;
                ui.label(
                    RichText::new(format!(
                        "{:.1} seconds ({} frames), {}",
                        i.seconds,
                        (i.seconds * fps).ceil(),
                        if i.channels == 1 { "mono" } else { "stereo" }
                    ))
                    .color(muted),
                );
            }
            Some(Err(_)) => {
                ui.label(RichText::new("The sound file cannot be read.").color(shell.palette.error));
            }
            None => {
                ui.label(RichText::new("The sound file is missing.").color(shell.palette.error));
            }
        }
    }
    if let Some(l) = shell.editor.doc.find(id)
        && matches!(l.kind, LayerKind::Bone { .. })
    {
        ui.add_space(8.0);
        if ui
            .button("Open Bones panel")
            .on_hover_text("The Bones panel shows the full rig and lets you edit each bone")
            .clicked()
        {
            shell.ui.open_tab = Some(crate::panels::Tab::Bones);
        }
    }
}

/// Which sub-layer of a drawing the tools draw on.
pub fn part_picker(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.horizontal(|ui| {
        ui.label("Draw on");
        for part in fanimate_core::Part::ALL {
            let on = shell.editor.part == part;
            if ui.selectable_label(on, part.label()).clicked() && !on {
                shell.pending.push(fanimate_editor::Action::SetPart(part));
            }
        }
    });
    ui.label(
        RichText::new("Roughs sit under everything and are left out of renders.")
            .size(11.0)
            .color(shell.palette.text_muted),
    );
}

/// Nine anchors of the layer's box (corners, middles of the sides, centre),
/// the one the pivot is on drawn solid. Returns the one clicked, as fractions
/// across and down the box.
fn pivot_grid(ui: &mut egui::Ui, shell: &Shell) -> Option<(f32, f32)> {
    let p = shell.palette;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, p.bg);
    ui.painter().rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    // working out where the pivot is draws the layer; not while playing
    let at = if shell.editor.playing {
        None
    } else {
        shell.editor.pivot_fraction()
    };
    let mut clicked = None;
    for row in 0..3 {
        for col in 0..3 {
            let (fx, fy) = (col as f32 / 2.0, row as f32 / 2.0);
            let center = rect.min + egui::vec2(8.0 + col as f32 * 15.0, 8.0 + row as f32 * 15.0);
            let spot = egui::Rect::from_center_size(center, egui::vec2(12.0, 12.0));
            let r = ui.interact(
                spot,
                ui.id().with(("pivot-anchor", row, col)),
                egui::Sense::click(),
            );
            let here = at.is_some_and(|a| (a.x - fx).abs() < 0.02 && (a.y - fy).abs() < 0.02);
            if here {
                ui.painter().circle_filled(center, 4.0, p.accent);
            } else {
                let c = if r.hovered() { p.text } else { p.text_muted };
                ui.painter().circle_stroke(center, 3.0, egui::Stroke::new(1.0, c));
            }
            if r.on_hover_text("Put the pivot here").clicked() {
                clicked = Some((fx, fy));
            }
        }
    }
    clicked
}

/// A rectangle's corner radius, and Expand Shape: shown while one shape that
/// is still a rectangle is selected.
fn rectangle(ui: &mut egui::Ui, shell: &mut Shell) {
    let Some(target) = shell.editor.radius_target() else {
        return;
    };
    ui.add_space(8.0);
    heading(ui, shell, "Rectangle");
    let max = target.rect.max_radius();
    let relative = shell.ui.radius_relative;
    // as a percentage, 100% is the most that fits: half the shorter side
    let mut shown = if relative {
        target.rect.radius / max.max(f32::EPSILON) * 100.0
    } else {
        target.rect.radius
    };
    let mut edit = None;
    egui::Grid::new("rect-props")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("Corner radius");
            let drag = if relative {
                egui::DragValue::new(&mut shown)
                    .range(0.0..=100.0)
                    .speed(0.5)
                    .suffix("%")
            } else {
                egui::DragValue::new(&mut shown)
                    .range(0.0..=max)
                    .speed(0.5)
                    .suffix(" px")
            };
            let r = ui.add(drag.max_decimals(1));
            if r.changed() {
                edit = Some(r);
            }
            ui.end_row();
            ui.label("Rounding");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut shell.ui.radius_relative, false, "Absolute");
                ui.selectable_value(&mut shell.ui.radius_relative, true, "Relative");
            });
            ui.end_row();
        });
    if let Some(r) = edit {
        begin_edit(shell, &r, "Corner radius");
        let radius = if relative { shown / 100.0 * max } else { shown };
        shell.editor.set_corner_radius(radius);
    }
    if ui
        .button("Expand Shape")
        .on_hover_text("Turn the rectangle into a plain path, to edit point by point")
        .clicked()
    {
        shell.pending.push(Action::ExpandShape);
    }
}

fn project(ui: &mut egui::Ui, shell: &mut Shell) {
    heading(ui, shell, "Project");
    let doc = &shell.editor.doc;
    let (mut name, mut w, mut h, mut fps, mut start, mut end, mut bg) = (
        doc.name.clone(),
        doc.width,
        doc.height,
        doc.fps,
        doc.start,
        doc.end,
        to_color32(doc.background),
    );
    let mut edits: Vec<(egui::Response, &'static str)> = Vec::new();
    egui::Grid::new("project-props")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("Name");
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
            if r.changed() {
                edits.push((r, "Rename project"));
            }
            ui.end_row();
            ui.label("Size");
            ui.horizontal(|ui| {
                for v in [&mut w, &mut h] {
                    let r = ui.add(
                        egui::DragValue::new(v)
                            .range(16.0..=16384.0)
                            .speed(1.0)
                            .suffix(" px"),
                    );
                    if r.changed() {
                        edits.push((r, "Resize canvas"));
                    }
                }
            });
            ui.end_row();
            ui.label("Frame rate");
            let r = ui.add(
                egui::DragValue::new(&mut fps)
                    .range(1.0..=120.0)
                    .speed(0.1)
                    .suffix(" fps"),
            );
            if r.changed() {
                edits.push((r, "Change frame rate"));
            }
            ui.end_row();
            ui.label("Frames");
            ui.horizontal(|ui| {
                let r = ui.add(egui::DragValue::new(&mut start).range(0..=end).speed(0.2));
                if r.changed() {
                    edits.push((r, "Change frame range"));
                }
                ui.label("to");
                let r = ui.add(egui::DragValue::new(&mut end).range(start..=100_000).speed(0.2));
                if r.changed() {
                    edits.push((r, "Change frame range"));
                }
            });
            ui.end_row();
            ui.label("Background");
            let r = ui.color_edit_button_srgba(&mut bg);
            if r.changed() {
                edits.push((r, "Change background"));
            }
            ui.end_row();
        });
    if let Some((r, label)) = edits.into_iter().next() {
        begin_edit(shell, &r, label);
        let doc = &mut shell.editor.doc;
        doc.name = name;
        doc.width = w;
        doc.height = h;
        doc.fps = fps;
        doc.start = start;
        doc.end = end;
        doc.background = from_color32(bg);
        if label == "Resize canvas" {
            shell.view.fit();
        }
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new("Select a layer to edit its properties.")
            .size(11.0)
            .color(shell.palette.text_muted),
    );
}
