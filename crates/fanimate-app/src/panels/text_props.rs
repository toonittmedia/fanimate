//! The Text panel: a text layer's words, how they are set, and its animators.
//!
//! It has a tab of its own (beside Bones) so the Properties panel does not
//! grow past the screen for a text layer. Every value is edited live, with one
//! undo step for a drag or a burst of typing (`begin_edit`, then the editor's
//! `change_*` commands). The animators' values follow the keying rule, and the
//! diamond beside each one adds or removes a key on the playhead frame.
//!
//! Rows are the shared inspector rows of `kit` (a label, the controls, and the
//! keyframe diamond at the right edge); the animators are a plain list, one
//! row each, and the one that is picked opens its controls underneath.

use eframe::egui::{self, RichText};
use fanimate_core::{LayerId, SelectorShape, Stroke, TextAlign, TextDirection, TextPreset, TextUnit, Vec2};
use fanimate_editor::{Action, AnimatorProp, PropValue};

use super::properties::{begin_edit, from_color32, heading, to_color32};
use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::kit::{NUMBER_WIDTH, number, number_sized, prop_row, prop_row_full};
use crate::menus;

/// A number box for something with few digits (a width, a seed).
const SMALL_BOX: f32 = 52.0;
/// Height of a row of the animators list.
const LIST_ROW: f32 = 26.0;

/// The Text tab: the selected text layer's settings, or what to do to get one.
pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    let Some(id) = shell.editor.selected_text() else {
        heading(ui, shell, "Text");
        ui.label(
            RichText::new(
                "Select a text layer, or choose the Text tool and click the canvas, to set its words, \
                 look and animators here.",
            )
            .color(shell.palette.text_muted),
        );
        ui.add_space(6.0);
        if ui.button(Action::NewTextLayer.label()).clicked() {
            shell.pending.push(Action::NewTextLayer);
        }
        return;
    };
    words_and_look(ui, shell, id);
    on_a_path(ui, shell, id);
    animators(ui, shell, id);
}

/// What Properties shows for a text layer: where its settings went.
pub fn pointer(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.add_space(8.0);
    ui.label(
        RichText::new("The words, font and animators are in the Text panel.").color(shell.palette.text_muted),
    );
    if ui.button("Open Text panel").clicked() {
        shell.ui.open_tab = Some(super::Tab::Text);
    }
}

/// The font menu: every font Fanimate can use, the shipped ones and the ones
/// installed on the computer together, in one list under a search box (there
/// can be hundreds). Returns the response of the choice when one was made.
fn font_picker(ui: &mut egui::Ui, id: LayerId, family: &mut String) -> Option<egui::Response> {
    let mut chosen = None;
    let state = ui.make_persistent_id(("text-font-menu", id.0));
    egui::ComboBox::from_id_salt(("text-font", id.0))
        .selected_text(family.clone())
        .width(ui.available_width())
        .height(FONT_MENU_HEIGHT)
        // the search box must be clickable without closing the menu, so a
        // choice closes it by hand
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.set_min_width(FONT_MENU_WIDTH);
            chosen = font_rows(ui, family, state);
            if chosen.is_some() {
                ui.close();
            }
        });
    chosen
}

/// Every font name once, in alphabetical order. A name Fanimate ships is not
/// listed again when it is also installed.
fn all_fonts() -> Vec<String> {
    let mut names: Vec<String> = fanimate_core::text::families()
        .into_iter()
        .map(str::to_owned)
        .collect();
    for name in fanimate_core::text::system_families() {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    names.sort_by_cached_key(|n| n.to_lowercase());
    names
}

/// The search box and the list under it; the response of the row picked, if
/// one was. Typing filters, Up and Down move the highlight, Enter picks it.
fn font_rows(ui: &mut egui::Ui, family: &mut String, state: egui::Id) -> Option<egui::Response> {
    let (query_id, seen_id, highlight_id) =
        (state.with("query"), state.with("seen"), state.with("highlight"));
    let mut query: String = ui.data_mut(|d| d.get_temp(query_id)).unwrap_or_default();
    // the menu was just opened when it was not drawn on the pass before this one
    let pass = ui.ctx().cumulative_pass_nr();
    let just_opened = ui
        .data_mut(|d| d.get_temp::<u64>(seen_id))
        .is_none_or(|last| pass > last + 1);
    ui.data_mut(|d| d.insert_temp(seen_id, pass));
    if just_opened {
        query.clear();
    }
    let search = ui.add(
        egui::TextEdit::singleline(&mut query)
            .hint_text("Search fonts")
            .desired_width(f32::INFINITY),
    );
    // focus once when the menu opens, so typing searches at once
    if just_opened {
        search.request_focus();
    }
    let needle = query.trim().to_lowercase();
    let fonts = all_fonts();
    let matches: Vec<&String> = fonts
        .iter()
        .filter(|n| n.to_lowercase().contains(&needle))
        .collect();

    let mut highlight: usize = ui.data_mut(|d| d.get_temp(highlight_id)).unwrap_or(0);
    if search.changed() || just_opened {
        highlight = 0;
    }
    let (down, up, enter) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::Enter),
        )
    });
    let moved = down || up;
    if down {
        highlight += 1;
    }
    if up {
        highlight = highlight.saturating_sub(1);
    }
    highlight = highlight.min(matches.len().saturating_sub(1));
    ui.data_mut(|d| {
        d.insert_temp(query_id, query);
        d.insert_temp(highlight_id, highlight);
    });

    let mut chosen = None;
    if matches.is_empty() {
        ui.weak("No font matches");
        return None;
    }
    if enter && let Some(name) = matches.get(highlight) {
        *family = (*name).clone();
        chosen = Some(search.clone());
    }
    let row_height = ui.spacing().interact_size.y;
    let mut area = egui::ScrollArea::vertical()
        .max_height(FONT_LIST_HEIGHT)
        .auto_shrink([false, true]);
    if moved {
        // keep the highlighted row in view
        area = area.vertical_scroll_offset((highlight as f32 * row_height - 3.0 * row_height).max(0.0));
    }
    area.show_rows(ui, row_height, matches.len(), |ui, range| {
        for i in range {
            let name = matches[i];
            let r = ui.selectable_label(*name == *family || i == highlight, name);
            if r.clicked() {
                *family = name.clone();
                chosen = Some(r);
            }
        }
    });
    chosen
}

/// Wide enough for the longest common family names.
const FONT_MENU_WIDTH: f32 = 240.0;
/// Room for the search box and a good run of fonts under it.
const FONT_MENU_HEIGHT: f32 = 420.0;
const FONT_LIST_HEIGHT: f32 = 340.0;

fn words_and_look(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId) {
    let Some(text) = shell.editor.text(id) else {
        return;
    };
    let look = text.look().clone();
    let mut edited = look.clone();
    let mut changed: Option<egui::Response> = None;
    let muted = shell.palette.text_muted;

    heading(ui, shell, "Words");
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
                "The font \"{}\" is not installed here, so the text is set in {}.",
                look.family,
                fanimate_core::text::DEFAULT_FONT
            ))
            .color(shell.palette.error),
        );
    }
    ui.add_space(6.0);

    prop_row_full(ui, "Font", |ui| {
        if let Some(r) = font_picker(ui, id, &mut edited.family) {
            changed = Some(r);
        }
    });
    prop_row_full(ui, "Size", |ui| {
        let r = number(
            ui,
            egui::DragValue::new(&mut edited.size)
                .speed(0.5)
                .range(1.0..=4096.0),
        );
        if r.changed() {
            changed = Some(r);
        }
    });
    prop_row_full(ui, "Weight", |ui| {
        ui.spacing_mut().interact_size.x = NUMBER_WIDTH;
        ui.style_mut().override_font_id = Some(crate::theme::mono(12.5));
        let r = ui.add_enabled(
            look.font_has_weight_axis(),
            crate::kit::value_slider(&mut edited.weight, 100.0..=900.0).step_by(1.0),
        );
        let r = r.on_hover_text("Only fonts with a weight axis (Inter) change with this");
        if r.changed() {
            changed = Some(r);
        }
    });
    prop_row_full(ui, "Spacing", |ui| {
        let r = number(
            ui,
            egui::DragValue::new(&mut edited.tracking)
                .speed(1.0)
                .range(-1000.0..=1000.0),
        );
        let r = r.on_hover_text("Letter spacing, in thousandths of the size, added after every character");
        if r.changed() {
            changed = Some(r);
        }
    });
    prop_row_full(ui, "Line height", |ui| {
        let r = number(
            ui,
            egui::DragValue::new(&mut edited.line_height)
                .speed(0.01)
                .range(0.2..=5.0),
        );
        if r.changed() {
            changed = Some(r);
        }
    });
    prop_row_full(ui, "Align", |ui| {
        // close together, so the three fit the narrowest panel
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.spacing_mut().button_padding.x = 5.0;
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
    prop_row_full(ui, "Direction", |ui| {
        egui::ComboBox::from_id_salt(("text-direction", id.0))
            .selected_text(match edited.direction {
                TextDirection::Auto => "From the text",
                TextDirection::LeftToRight => "Left to right",
                TextDirection::RightToLeft => "Right to left",
            })
            .width(ui.available_width())
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
    });
    prop_row_full(ui, "Wrap", |ui| {
        let mut wrap = edited.width.is_some();
        let r = ui.checkbox(&mut wrap, "At width");
        if r.changed() {
            edited.width = wrap.then(|| look.width.unwrap_or(400.0));
            changed = Some(r);
        }
        if let Some(width) = &mut edited.width {
            let r = number_sized(
                ui,
                SMALL_BOX + 8.0,
                egui::DragValue::new(width).speed(1.0).range(1.0..=100_000.0),
            );
            if r.changed() {
                changed = Some(r);
            }
        }
    });
    prop_row_full(ui, "Colour", |ui| {
        let mut c = to_color32(edited.fill);
        let r = ui.color_edit_button_srgba(&mut c);
        if r.changed() {
            edited.fill = from_color32(c);
            changed = Some(r);
        }
    });
    prop_row_full(ui, "Outline", |ui| {
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
            let r = number_sized(
                ui,
                SMALL_BOX,
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
    ui.add_space(2.0);
    if ui.button(Action::ConvertTextToShapes.label()).clicked() {
        shell.pending.push(Action::ConvertTextToShapes);
    }
}

/// Running the text along a shape: choose the shape, then say which way it is
/// read and where along it the text starts (keyed by the keying rule).
fn on_a_path(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId) {
    let Some(text) = shell.editor.text(id) else {
        return;
    };
    let has_path = text.look().path.is_some();
    let reversed = text.look().reverse_path;
    let mut offset = text.path_offset.sample(shell.editor.current_frame() as f32);
    ui.add_space(10.0);
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
    prop_row_full(ui, "Start", |ui| {
        let r = number(
            ui,
            egui::DragValue::new(&mut offset)
                .speed(0.5)
                .suffix("%")
                .max_decimals(1),
        );
        r.clone().on_hover_text("Where along the path the text starts");
        if r.changed() {
            begin_edit(shell, &r, "Change text path start");
            shell.editor.change_text_path_offset(id, offset);
        }
    });
}

/// The animators: a header with the Add menu at its right, then a list, one row
/// each. The picked row opens its controls underneath.
fn animators(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId) {
    let animators = shell
        .editor
        .text(id)
        .map(|t| t.animators.clone())
        .unwrap_or_default();
    let count = animators.len();

    // a new animator (from the Add menu, the Edit menu or a script) is picked
    let count_id = ui.make_persistent_id(("text-animator-count", id.0));
    let before: Option<usize> = ui.data_mut(|d| d.get_temp(count_id));
    ui.data_mut(|d| d.insert_temp(count_id, count));
    if before.is_some_and(|b| count > b) {
        shell.ui.text_animator = Some((id, count - 1));
    }
    let mut picked = shell
        .ui
        .text_animator
        .filter(|(layer, i)| *layer == id && *i < count)
        .map(|(_, i)| i);

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        heading(ui, shell, "Animators");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_add = shell.editor.can_run(Action::AddTextAnimator(TextPreset::Wave));
            ui.add_enabled_ui(can_add, |ui| {
                ui.menu_button("+ Add", |ui| {
                    for preset in TextPreset::ALL {
                        menus::item(ui, shell, Action::AddTextAnimator(preset));
                    }
                })
                .response
                .on_hover_text("Add an animator: it moves, turns, fades or tints the text a character, word or line at a time");
            });
        });
    });
    if animators.is_empty() {
        ui.label(
            RichText::new(
                "None yet. Animators move, turn, fade or tint the text a character, word or line at a time.",
            )
            .color(shell.palette.text_muted),
        );
    }

    for (i, a) in animators.iter().enumerate() {
        let open = picked == Some(i);
        if animator_row(ui, shell, id, i, a, open) {
            picked = if open { None } else { Some(i) };
        }
        if open {
            let p = shell.palette;
            egui::Frame::NONE
                .fill(p.bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(4)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    animator(ui, shell, id, i, a);
                });
            ui.add_space(4.0);
        }
    }
    shell.ui.text_animator = picked.map(|i| (id, i));
}

/// One animator's row in the list: an arrow, its name, what it acts on and a
/// delete button. Returns true when the row was clicked (to open or close it).
fn animator_row(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    i: usize,
    a: &fanimate_core::Animator,
    open: bool,
) -> bool {
    let p = shell.palette;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), LIST_ROW), egui::Sense::click());
    let painter = ui.painter();
    if open {
        painter.rect_filled(rect, 4.0, p.accent.gamma_multiply(0.28));
    } else if response.hovered() {
        painter.rect_filled(rect, 4.0, p.raised);
    }
    icons::paint(
        painter,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        ),
        if open { Icon::Collapse } else { Icon::Expand },
        p.text_muted,
    );
    // the delete button sits at the right edge
    let trash_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 14.0, rect.center().y),
        egui::vec2(22.0, 22.0),
    );
    let trash = ui.interact(trash_rect, response.id.with("delete"), egui::Sense::click());
    let painter = ui.painter();
    if trash.hovered() {
        painter.rect_filled(trash_rect, 4.0, p.error.gamma_multiply(0.25));
    }
    icons::paint(
        painter,
        egui::Rect::from_center_size(trash_rect.center(), egui::vec2(16.0, 16.0)),
        Icon::Delete,
        if trash.hovered() { p.error } else { p.text_muted },
    );
    // what it acts on, then the name in what is left
    let unit = a.unit.label().to_lowercase();
    let unit_galley = painter.layout_no_wrap(unit, egui::FontId::proportional(11.5), p.text_muted);
    let unit_x = trash_rect.left() - 6.0 - unit_galley.size().x;
    painter.galley(
        egui::pos2(unit_x, rect.center().y - unit_galley.size().y / 2.0),
        unit_galley,
        p.text_muted,
    );
    let name_left = rect.left() + 26.0;
    let mut job =
        egui::text::LayoutJob::simple_singleline(a.name.clone(), egui::FontId::proportional(13.0), p.text);
    job.wrap = egui::text::TextWrapping {
        max_width: (unit_x - 8.0 - name_left).max(10.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(name_left, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );

    if trash.clicked() {
        shell.pending.push(Action::RemoveTextAnimator(i));
    }
    // right-click picks the row first, as in every list
    if response.secondary_clicked() && !open {
        shell.ui.text_animator = Some((id, i));
    }
    let count_id = ui.make_persistent_id(("text-animator-count", id.0));
    menus::context_menu(&response, |ui| {
        // each of these keeps the animator it was used on picked, wherever it ends up
        let run = |ui: &mut egui::Ui, shell: &mut Shell, action: Action, picked: usize, grows: bool| {
            let r = ui.add_enabled(shell.editor.can_run(action), egui::Button::new(action.label()));
            if r.clicked() {
                shell.pending.push(action);
                shell.ui.text_animator = Some((id, picked));
                if grows {
                    // it is the copy that is picked, not the last one, as for a new animator
                    ui.ctx().data_mut(|d| {
                        let n: usize = d.get_temp(count_id).unwrap_or(0);
                        d.insert_temp(count_id, n + 1);
                    });
                }
                ui.close();
            }
        };
        run(ui, shell, Action::DuplicateTextAnimator(i), i + 1, true);
        if ui.button("Rename").clicked() {
            shell.ui.text_animator = Some((id, i));
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("text-animator-rename"), true));
            ui.close();
        }
        ui.separator();
        run(
            ui,
            shell,
            Action::MoveTextAnimator(i, -1),
            i.saturating_sub(1),
            false,
        );
        run(ui, shell, Action::MoveTextAnimator(i, 1), i + 1, false);
        ui.separator();
        run(
            ui,
            shell,
            Action::RemoveTextAnimator(i),
            i.saturating_sub(1),
            false,
        );
    });
    response.clicked() && !trash.hovered()
}

/// The short names the animator rows use, so they fit the narrowest panel.
fn short_label(prop: AnimatorProp) -> &'static str {
    match prop {
        AnimatorProp::RangeStart => "Start",
        AnimatorProp::RangeEnd => "End",
        AnimatorProp::RangeOffset => "Offset",
        AnimatorProp::Fill => "Colour",
        AnimatorProp::Tracking => "Spacing",
        AnimatorProp::StrokeWidth => "Line width",
        AnimatorProp::StrokeColour => "Line colour",
        other => other.label(),
    }
}

/// One animator's controls.
fn animator(ui: &mut egui::Ui, shell: &mut Shell, id: LayerId, i: usize, a: &fanimate_core::Animator) {
    let frame = shell.editor.current_frame() as f32;
    let mut edited = a.clone();
    let mut changed: Option<egui::Response> = None;

    prop_row_full(ui, "Name", |ui| {
        let name_id = egui::Id::new(("animator-name", id.0, i));
        let r = ui.add(
            egui::TextEdit::singleline(&mut edited.name)
                .id(name_id)
                .desired_width(f32::INFINITY),
        );
        // "Rename" in the list's menu: focus the field once, with the name selected
        if ui
            .ctx()
            .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("text-animator-rename")))
            .is_some()
        {
            r.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), name_id) {
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(edited.name.chars().count()),
                );
                state.cursor.set_char_range(Some(all));
                state.store(ui.ctx(), name_id);
            }
        }
        if r.changed() {
            changed = Some(r);
        }
    });
    prop_row_full(ui, "Acts on", |ui| {
        egui::ComboBox::from_id_salt(("animator-unit", id.0, i))
            .selected_text(edited.unit.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for unit in TextUnit::ALL {
                    let r = ui.selectable_value(&mut edited.unit, unit, unit.label());
                    if r.clicked() {
                        changed = Some(r);
                    }
                }
            });
    });
    prop_row_full(ui, "Selection", |ui| {
        egui::ComboBox::from_id_salt(("animator-shape", id.0, i))
            .selected_text(edited.selector.shape.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for shape in SelectorShape::ALL {
                    let r = ui.selectable_value(&mut edited.selector.shape, shape, shape.label());
                    if r.clicked() {
                        changed = Some(r);
                    }
                }
            });
    });
    prop_row_full(ui, "Order", |ui| {
        let r = ui.checkbox(&mut edited.selector.random, "Random");
        if r.changed() {
            changed = Some(r);
        }
        if edited.selector.random {
            let r = number_sized(
                ui,
                SMALL_BOX + 8.0,
                egui::DragValue::new(&mut edited.selector.seed).prefix("seed "),
            );
            if r.changed() {
                changed = Some(r);
            }
        }
    });
    prop_row_full(ui, "Soft edge", |ui| {
        let r = number(
            ui,
            egui::DragValue::new(&mut edited.selector.smoothness)
                .speed(0.5)
                .suffix("%"),
        );
        let r = r.on_hover_text("For the square selection: how gradually the edges of the range take effect");
        if r.changed() {
            changed = Some(r);
        }
    });
    if let Some(r) = changed {
        begin_edit(shell, &r, "Change text animator");
        shell.editor.change_text_animator(id, i, |an| *an = edited);
    }

    ui.add_space(4.0);
    ui.label(RichText::new("Range, in percent of the text").color(shell.palette.text_muted));
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

    ui.add_space(4.0);
    ui.label(RichText::new("What the selected text does").color(shell.palette.text_muted));
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
    let mut shown = value;
    let cell = prop_row(ui, short_label(prop), |ui| {
        let r = number(
            ui,
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
    });
    key_diamond_at(ui, shell, cell, id, i, prop);
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
    let mut shown = value;
    let cell = prop_row(ui, short_label(prop), |ui| {
        let mut changed = None;
        for v in [&mut shown.x, &mut shown.y] {
            let r = number(ui, egui::DragValue::new(v).speed(speed).max_decimals(2));
            if r.changed() {
                changed = Some(r);
            }
        }
        if let Some(r) = changed {
            begin_edit(shell, &r, "Change text animator");
            shell
                .editor
                .change_animator_value(id, i, prop, PropValue::Pair(shown));
        }
    });
    key_diamond_at(ui, shell, cell, id, i, prop);
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
    let (on_undo, off_undo) = match prop {
        AnimatorProp::StrokeColour => ("Tint text outline", "Stop tinting text outline"),
        _ => ("Tint text", "Stop tinting text"),
    };
    let cell = prop_row(ui, short_label(prop), |ui| {
        let mut tint = channel.is_some();
        let r = ui.checkbox(&mut tint, "Change");
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
        key_diamond_at(ui, shell, cell, id, i, prop);
    }
}

/// The keyframe diamond, in the right-hand cell of its row: filled when there
/// is a key on this frame (click removes it), outlined when the value is
/// animated but has none here (click adds one, holding the value), faint when
/// it has no keys at all.
fn key_diamond_at(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    cell: egui::Rect,
    id: LayerId,
    i: usize,
    prop: AnimatorProp,
) {
    let (animated, key_here) = shell.editor.animator_key_state(id, i, prop);
    let p = shell.palette;
    let rect = egui::Rect::from_center_size(cell.center(), egui::vec2(18.0, 18.0));
    let r = ui.interact(
        rect,
        ui.id().with(("animator-key", id.0, i, prop)),
        egui::Sense::click(),
    );
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
