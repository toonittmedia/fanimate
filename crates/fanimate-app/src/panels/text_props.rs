//! The Properties panel for a text layer: the words, how they are set, and
//! the animators.
//!
//! Every value is edited live, with one undo step for a drag or a burst of
//! typing (`begin_edit`, then the editor's `change_*` commands). The
//! animators' values follow the keying rule, and the diamond beside each one
//! adds or removes a key on the playhead frame.

use eframe::egui::{self, RichText};
use fanimate_core::{LayerId, SelectorShape, Stroke, TextAlign, TextDirection, TextPreset, TextUnit, Vec2};
use fanimate_editor::{AnimatorProp, PropValue};

use super::properties::{begin_edit, from_color32, heading, to_color32};
use crate::app::Shell;

/// The text section. Nothing is edited while the animation plays.
pub fn section(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId) {
    if shell.editor.text(id).is_none() {
        return;
    }
    let enabled = !shell.editor.playing;
    ui.add_enabled_ui(enabled, |ui| {
        words_and_look(ui, shell, id);
        animators(ui, shell, id);
    });
}

fn words_and_look(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId) {
    let Some(text) = shell.editor.text(id) else {
        return;
    };
    let look = text.look().clone();
    let mut edited = look.clone();
    let mut changed: Option<egui::Response> = None;
    let muted = shell.palette.text_muted;

    ui.add_space(8.0);
    heading(ui, shell, "Text");
    let r = ui.add(
        egui::TextEdit::multiline(&mut edited.content)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text("Type the words here"),
    );
    if r.changed() {
        changed = Some(r);
    }
    if !look.font_is_available() {
        ui.label(
            RichText::new(format!(
                "The font \"{}\" is not one Fanimate has, so the text is set in {}.",
                look.family,
                fanimate_core::text::DEFAULT_FONT
            ))
            .color(shell.palette.error),
        );
    }

    egui::Grid::new(("text-look", id.0))
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("Font");
            egui::ComboBox::from_id_salt(("text-font", id.0))
                .selected_text(edited.family.clone())
                .show_ui(ui, |ui| {
                    for family in fanimate_core::text::families() {
                        let r = ui.selectable_value(&mut edited.family, family.to_owned(), family);
                        if r.clicked() {
                            changed = Some(r);
                        }
                    }
                });
            ui.end_row();

            ui.label("Size");
            let r = ui.add(
                egui::DragValue::new(&mut edited.size)
                    .speed(0.5)
                    .range(1.0..=4096.0),
            );
            if r.changed() {
                changed = Some(r);
            }
            ui.end_row();

            ui.label("Weight");
            let r = ui.add_enabled(
                look.font_has_weight_axis(),
                egui::Slider::new(&mut edited.weight, 100.0..=900.0).step_by(1.0),
            );
            let r = r.on_hover_text("Only fonts with a weight axis (Inter) change with this");
            if r.changed() {
                changed = Some(r);
            }
            ui.end_row();

            ui.label("Letter spacing");
            let r = ui.add(
                egui::DragValue::new(&mut edited.tracking)
                    .speed(1.0)
                    .range(-1000.0..=1000.0),
            );
            let r = r.on_hover_text("In thousandths of the size, added after every character");
            if r.changed() {
                changed = Some(r);
            }
            ui.end_row();

            ui.label("Line height");
            let r = ui.add(
                egui::DragValue::new(&mut edited.line_height)
                    .speed(0.01)
                    .range(0.2..=5.0),
            );
            if r.changed() {
                changed = Some(r);
            }
            ui.end_row();

            ui.label("Align");
            ui.horizontal(|ui| {
                for (align, label) in [
                    (TextAlign::Start, "Start"),
                    (TextAlign::Center, "Centre"),
                    (TextAlign::End, "End"),
                ] {
                    let r = ui.selectable_label(edited.align == align, label);
                    if r.clicked() {
                        edited.align = align;
                        changed = Some(r);
                    }
                }
            });
            ui.end_row();

            ui.label("Direction");
            egui::ComboBox::from_id_salt(("text-direction", id.0))
                .selected_text(match edited.direction {
                    TextDirection::Auto => "From the text",
                    TextDirection::LeftToRight => "Left to right",
                    TextDirection::RightToLeft => "Right to left",
                })
                .show_ui(ui, |ui| {
                    for (d, label) in [
                        (TextDirection::Auto, "From the text"),
                        (TextDirection::LeftToRight, "Left to right"),
                        (TextDirection::RightToLeft, "Right to left"),
                    ] {
                        let r = ui.selectable_value(&mut edited.direction, d, label);
                        if r.clicked() {
                            changed = Some(r);
                        }
                    }
                });
            ui.end_row();

            ui.label("Wrap");
            ui.horizontal(|ui| {
                let mut wrap = edited.width.is_some();
                let r = ui.checkbox(&mut wrap, "At width");
                if r.changed() {
                    edited.width = wrap.then(|| look.width.unwrap_or(400.0));
                    changed = Some(r);
                }
                if let Some(width) = &mut edited.width {
                    let r = ui.add(egui::DragValue::new(width).speed(1.0).range(1.0..=100_000.0));
                    if r.changed() {
                        changed = Some(r);
                    }
                }
            });
            ui.end_row();

            ui.label("Colour");
            let mut c = to_color32(edited.fill);
            let r = ui.color_edit_button_srgba(&mut c);
            if r.changed() {
                edited.fill = from_color32(c);
                changed = Some(r);
            }
            ui.end_row();

            ui.label("Outline");
            ui.horizontal(|ui| {
                let mut on = edited.stroke.is_some();
                let r = ui.checkbox(&mut on, "");
                if r.changed() {
                    edited.stroke = on.then(|| {
                        look.stroke.unwrap_or(Stroke {
                            color: fanimate_core::Color::WHITE,
                            width: 2.0,
                        })
                    });
                    changed = Some(r);
                }
                if let Some(stroke) = &mut edited.stroke {
                    let r = ui.add(
                        egui::DragValue::new(&mut stroke.width)
                            .speed(0.1)
                            .range(0.0..=200.0),
                    );
                    if r.changed() {
                        changed = Some(r);
                    }
                    let mut c = to_color32(stroke.color);
                    let r = ui.color_edit_button_srgba(&mut c);
                    if r.changed() {
                        stroke.color = from_color32(c);
                        changed = Some(r);
                    }
                }
            });
            ui.end_row();
        });

    if let Some(r) = changed {
        begin_edit(shell, &r, "Change text");
        shell.editor.change_text(id, |l| *l = edited);
    }

    let built = shell.editor.text(id).map(|t| {
        let b = t.built();
        (
            b.count(TextUnit::Character),
            b.count(TextUnit::Word),
            b.count(TextUnit::Line),
        )
    });
    if let Some((characters, words, lines)) = built {
        ui.label(
            RichText::new(format!("{characters} characters, {words} words, {lines} lines")).color(muted),
        );
    }
    ui.horizontal(|ui| {
        if ui
            .button(fanimate_editor::Action::ConvertTextToShapes.label())
            .clicked()
        {
            shell.pending.push(fanimate_editor::Action::ConvertTextToShapes);
        }
    });
    on_a_path(ui, shell, id);
}

/// Running the text along a shape: choose the shape, then say which way it is
/// read and where along it the text starts (keyed by the keying rule).
fn on_a_path(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId) {
    use fanimate_editor::Action;
    let Some(text) = shell.editor.text(id) else {
        return;
    };
    let has_path = text.look().path.is_some();
    let reversed = text.look().reverse_path;
    let mut offset = text.path_offset.sample(shell.editor.current_frame() as f32);
    ui.add_space(6.0);
    heading(ui, shell, "On a path");
    if !has_path {
        ui.label(
            RichText::new("Select this layer and a layer with a shape, then choose the shape to follow.")
                .color(shell.palette.text_muted),
        );
        if ui
            .add_enabled(
                shell.editor.can_run(Action::SetTextPath),
                egui::Button::new(Action::SetTextPath.label()),
            )
            .clicked()
        {
            shell.pending.push(Action::SetTextPath);
        }
        return;
    }
    ui.horizontal(|ui| {
        let mut reverse = reversed;
        let r = ui.checkbox(&mut reverse, "Read from the end");
        if r.changed() {
            begin_edit(shell, &r, "Change text path");
            shell.editor.change_text(id, |l| l.reverse_path = reverse);
        }
        if ui.button(Action::ClearTextPath.label()).clicked() {
            shell.pending.push(Action::ClearTextPath);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Start along path");
        let r = ui.add(
            egui::DragValue::new(&mut offset)
                .speed(0.5)
                .suffix("%")
                .max_decimals(1),
        );
        if r.changed() {
            begin_edit(shell, &r, "Change text path start");
            shell.editor.change_text_path_offset(id, offset);
        }
    });
}

fn animators(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId) {
    let animators = shell
        .editor
        .text(id)
        .map(|t| t.animators.clone())
        .unwrap_or_default();
    ui.add_space(8.0);
    heading(ui, shell, "Text animators");
    if animators.is_empty() {
        ui.label(
            RichText::new("Animators move, turn, fade or tint the text a character, word or line at a time.")
                .color(shell.palette.text_muted),
        );
    }
    let mut remove = None;
    for (i, a) in animators.iter().enumerate() {
        let title = format!("{}  ({})", a.name, a.unit.label().to_lowercase());
        egui::CollapsingHeader::new(title)
            .id_salt(("text-animator", id.0, i))
            .default_open(animators.len() == 1)
            .show(ui, |ui| {
                if animator(ui, shell, id, i, a) {
                    remove = Some(i);
                }
            });
    }
    if let Some(i) = remove {
        shell.editor.remove_text_animator(id, i);
    }
    ui.add_space(4.0);
    egui::ComboBox::from_id_salt(("add-text-animator", id.0))
        .selected_text("Add animator…")
        .show_ui(ui, |ui| {
            for preset in TextPreset::ALL {
                if ui.selectable_label(false, preset.label()).clicked() {
                    shell
                        .pending
                        .push(fanimate_editor::Action::AddTextAnimator(preset));
                }
            }
        });
}

/// One animator's controls. Returns true when the user asks to remove it.
fn animator(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    i: usize,
    a: &fanimate_core::Animator,
) -> bool {
    let frame = shell.editor.current_frame() as f32;
    let mut edited = a.clone();
    let mut changed: Option<egui::Response> = None;

    ui.horizontal(|ui| {
        ui.label("Name");
        let r = ui.add(egui::TextEdit::singleline(&mut edited.name).desired_width(140.0));
        if r.changed() {
            changed = Some(r);
        }
    });
    egui::Grid::new(("animator-look", id.0, i))
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("Acts on");
            egui::ComboBox::from_id_salt(("animator-unit", id.0, i))
                .selected_text(edited.unit.label())
                .show_ui(ui, |ui| {
                    for unit in TextUnit::ALL {
                        let r = ui.selectable_value(&mut edited.unit, unit, unit.label());
                        if r.clicked() {
                            changed = Some(r);
                        }
                    }
                });
            ui.end_row();

            ui.label("Selection");
            egui::ComboBox::from_id_salt(("animator-shape", id.0, i))
                .selected_text(edited.selector.shape.label())
                .show_ui(ui, |ui| {
                    for shape in SelectorShape::ALL {
                        let r = ui.selectable_value(&mut edited.selector.shape, shape, shape.label());
                        if r.clicked() {
                            changed = Some(r);
                        }
                    }
                });
            ui.end_row();

            ui.label("Order");
            ui.horizontal(|ui| {
                let r = ui.checkbox(&mut edited.selector.random, "Random");
                if r.changed() {
                    changed = Some(r);
                }
                if edited.selector.random {
                    let r = ui.add(egui::DragValue::new(&mut edited.selector.seed).prefix("seed "));
                    if r.changed() {
                        changed = Some(r);
                    }
                }
            });
            ui.end_row();

            ui.label("Soft edge");
            let r = ui.add(
                egui::DragValue::new(&mut edited.selector.smoothness)
                    .speed(0.5)
                    .suffix("%"),
            );
            let r =
                r.on_hover_text("For the square selection: how gradually the edges of the range take effect");
            if r.changed() {
                changed = Some(r);
            }
            ui.end_row();
        });
    if let Some(r) = changed {
        begin_edit(shell, &r, "Change text animator");
        shell.editor.change_text_animator(id, i, |an| *an = edited);
    }

    ui.add_space(4.0);
    ui.label(RichText::new("Range, in percent of the text").color(shell.palette.text_muted));
    egui::Grid::new(("animator-values", id.0, i))
        .num_columns(3)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            number_row(
                ui,
                shell,
                id,
                i,
                AnimatorProp::RangeStart,
                a.selector.start.sample(frame),
                0.5,
                "",
            );
            number_row(
                ui,
                shell,
                id,
                i,
                AnimatorProp::RangeEnd,
                a.selector.end.sample(frame),
                0.5,
                "",
            );
            number_row(
                ui,
                shell,
                id,
                i,
                AnimatorProp::RangeOffset,
                a.selector.offset.sample(frame),
                0.5,
                "",
            );
        });
    ui.add_space(4.0);
    ui.label(RichText::new("What the selected text does").color(shell.palette.text_muted));
    egui::Grid::new(("animator-effects", id.0, i))
        .num_columns(3)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            pair_row(
                ui,
                shell,
                id,
                i,
                AnimatorProp::Position,
                a.position.sample(frame),
                1.0,
            );
            // turning is kept in radians, shown in degrees
            let degrees = a.rotation.sample(frame).to_degrees();
            number_row_with(
                ui,
                shell,
                id,
                i,
                AnimatorProp::Rotation,
                degrees,
                0.5,
                "°",
                f32::to_radians,
            );
            pair_row(ui, shell, id, i, AnimatorProp::Scale, a.scale.sample(frame), 0.01);
            // opacity is kept as a multiplier, shown in percent
            let percent = a.opacity.sample(frame) * 100.0;
            number_row_with(ui, shell, id, i, AnimatorProp::Opacity, percent, 0.5, "%", |p| {
                p / 100.0
            });
            colour_row(ui, shell, id, i, AnimatorProp::Fill, a.fill.as_ref(), frame);
            number_row(
                ui,
                shell,
                id,
                i,
                AnimatorProp::StrokeWidth,
                a.stroke_width.sample(frame),
                0.1,
                "",
            );
            colour_row(
                ui,
                shell,
                id,
                i,
                AnimatorProp::StrokeColour,
                a.stroke_color.as_ref(),
                frame,
            );
            number_row(
                ui,
                shell,
                id,
                i,
                AnimatorProp::Tracking,
                a.tracking.sample(frame),
                1.0,
                "",
            );
            pair_row(
                ui,
                shell,
                id,
                i,
                AnimatorProp::Anchor,
                a.anchor.sample(frame),
                1.0,
            );
        });

    ui.add_space(4.0);
    ui.button("Remove animator").clicked()
}

#[allow(clippy::too_many_arguments)]
fn number_row(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    i: usize,
    prop: AnimatorProp,
    value: f32,
    speed: f64,
    suffix: &str,
) {
    number_row_with(ui, shell, id, i, prop, value, speed, suffix, |v| v);
}

/// A number with its label and diamond. `store` turns what is shown into what
/// is kept (degrees into radians, percent into a multiplier).
#[allow(clippy::too_many_arguments)]
fn number_row_with(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    i: usize,
    prop: AnimatorProp,
    value: f32,
    speed: f64,
    suffix: &str,
    store: impl Fn(f32) -> f32,
) {
    ui.label(prop.label());
    let mut shown = value;
    let r = ui.add(
        egui::DragValue::new(&mut shown)
            .speed(speed)
            .suffix(suffix)
            .max_decimals(2),
    );
    if r.changed() {
        begin_edit(shell, &r, "Change text animator");
        shell
            .editor
            .change_animator_value(id, i, prop, PropValue::Number(store(shown)));
    }
    key_diamond(ui, shell, id, i, prop);
    ui.end_row();
}

fn pair_row(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    i: usize,
    prop: AnimatorProp,
    value: Vec2,
    speed: f64,
) {
    ui.label(prop.label());
    let mut shown = value;
    let mut changed = None;
    ui.horizontal(|ui| {
        for v in [&mut shown.x, &mut shown.y] {
            let r = ui.add(egui::DragValue::new(v).speed(speed).max_decimals(2));
            if r.changed() {
                changed = Some(r);
            }
        }
    });
    if let Some(r) = changed {
        begin_edit(shell, &r, "Change text animator");
        shell
            .editor
            .change_animator_value(id, i, prop, PropValue::Pair(shown));
    }
    key_diamond(ui, shell, id, i, prop);
    ui.end_row();
}

/// A colour that an animator may or may not change: a checkbox to turn it on
/// (starting from the text's own colour), the colour and its diamond.
fn colour_row(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    i: usize,
    prop: AnimatorProp,
    channel: Option<&fanimate_core::Channel<fanimate_core::Color>>,
    frame: f32,
) {
    let (on_label, on_undo, off_undo) = match prop {
        AnimatorProp::StrokeColour => ("Tint outline", "Tint text outline", "Stop tinting text outline"),
        _ => ("Tint", "Tint text", "Stop tinting text"),
    };
    ui.label(prop.label());
    ui.horizontal(|ui| {
        let mut tint = channel.is_some();
        let r = ui.checkbox(&mut tint, on_label);
        if r.changed() {
            if tint {
                begin_edit(shell, &r, on_undo);
                let base = shell.editor.text(id).map_or(fanimate_core::Color::WHITE, |t| {
                    if prop == AnimatorProp::StrokeColour {
                        t.look().stroke.map_or(t.look().fill, |st| st.color)
                    } else {
                        t.look().fill
                    }
                });
                shell
                    .editor
                    .change_animator_value(id, i, prop, PropValue::Colour(base));
            } else {
                begin_edit(shell, &r, off_undo);
                if prop == AnimatorProp::StrokeColour {
                    shell.editor.clear_animator_stroke_colour(id, i);
                } else {
                    shell.editor.clear_animator_fill(id, i);
                }
            }
        }
        if let Some(channel) = channel {
            let mut c = to_color32(channel.sample(frame));
            let r = ui.color_edit_button_srgba(&mut c);
            if r.changed() {
                begin_edit(shell, &r, "Change text animator");
                shell
                    .editor
                    .change_animator_value(id, i, prop, PropValue::Colour(from_color32(c)));
            }
        }
    });
    if channel.is_some() {
        key_diamond(ui, shell, id, i, prop);
    }
    ui.end_row();
}

/// The keyframe diamond: filled when there is a key on this frame (click
/// removes it), outlined when the value is animated but has none here (click
/// adds one, holding the value), faint when it has no keys at all.
fn key_diamond(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId, i: usize, prop: AnimatorProp) {
    let (animated, key_here) = shell.editor.animator_key_state(id, i, prop);
    let p = shell.palette;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    let (c, d) = (rect.center(), 5.0);
    let diamond = vec![
        c + egui::vec2(0.0, -d),
        c + egui::vec2(d, 0.0),
        c + egui::vec2(0.0, d),
        c + egui::vec2(-d, 0.0),
    ];
    let (fill, stroke) = if key_here {
        (p.keyframe, egui::Stroke::new(1.0, p.bg))
    } else if animated {
        (egui::Color32::TRANSPARENT, egui::Stroke::new(1.5, p.keyframe))
    } else {
        let faint = if r.hovered() {
            p.text
        } else {
            p.text_muted.gamma_multiply(0.6)
        };
        (egui::Color32::TRANSPARENT, egui::Stroke::new(1.0, faint))
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(diamond, fill, stroke));
    let frame = shell.editor.current_frame();
    let tip = match (key_here, animated) {
        (true, _) => format!("Keyframe on frame {frame}. Click to remove it."),
        (false, true) => format!("Add a keyframe on frame {frame}, holding this value."),
        (false, false) => format!("Start animating this value: add a keyframe on frame {frame}."),
    };
    if r.on_hover_text(tip).clicked() {
        shell.editor.toggle_animator_key(id, i, prop);
    }
}
