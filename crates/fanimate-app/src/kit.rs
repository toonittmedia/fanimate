//! Building blocks the dialogs share: one modal frame, and grouped, aligned
//! form sections, so Preferences, Export, Shortcuts and the rest look and
//! behave alike.
//!
//! A dialog is a modal: it blocks the rest of the app (clicks and shortcuts)
//! until it is closed, as a Windows message box does. It has a fixed size, so
//! switching a tab or a page inside it never makes it jump.

use eframe::egui::{self, Align, Id, Layout, Margin, RichText, Stroke, Ui, vec2};

use crate::icons::{self, Icon};
use crate::theme::Palette;

/// Height taken by the title row and its rule.
const HEADER_HEIGHT: f32 = 40.0;
/// Space round the content.
const PADDING: f32 = 16.0;

/// How a dialog closes besides its close button and Esc.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    /// A click outside closes it (settings, lists).
    Closes,
    /// A click outside does nothing, so a half-filled form is not lost.
    Ignored,
}

/// Show a modal of `size` (the content area, without the padding). The body
/// gets the room under the title. Returns whether the dialog should close.
pub fn modal(
    ctx: &egui::Context,
    p: &Palette,
    id: &str,
    title: &str,
    size: egui::Vec2,
    backdrop: Backdrop,
    body: impl FnOnce(&mut Ui),
) -> bool {
    let frame = egui::Frame::NONE
        .fill(p.raised)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(6)
        .inner_margin(Margin::same(PADDING as i8))
        .shadow(egui::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: egui::Color32::from_black_alpha(if p.dark { 110 } else { 40 }),
        });
    let mut close = false;
    let m = egui::Modal::new(Id::new(("dialog", id)))
        .frame(frame)
        .backdrop_color(egui::Color32::from_black_alpha(if p.dark { 130 } else { 70 }))
        .show(ctx, |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);
            ui.horizontal(|ui| {
                ui.set_height(HEADER_HEIGHT - 12.0);
                ui.label(RichText::new(title).size(16.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icons::button(ui, Icon::Close, 24.0, false, false)
                        .on_hover_text("Close (Esc)")
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            ui.separator();
            ui.add_space(6.0);
            let room = vec2(size.x, size.y - HEADER_HEIGHT);
            ui.allocate_ui(room, |ui| {
                ui.set_min_size(room);
                ui.set_max_size(room);
                body(ui);
            });
        });
    let outside = backdrop == Backdrop::Closes && m.backdrop_response.clicked();
    close
        || outside
        || (m.is_top_modal && !m.any_popup_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)))
}

/// A titled group: a small heading above a bordered panel.
pub fn section(ui: &mut Ui, p: &Palette, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(title).size(11.5).strong().color(p.text_muted));
    ui.add_space(2.0);
    egui::Frame::NONE
        .fill(p.bg)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(6)
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // fields stand out from the group's own fill
            ui.visuals_mut().extreme_bg_color = p.raised;
            add(ui);
        });
    ui.add_space(10.0);
}

/// Rows of a label and its control, with the labels lined up.
pub fn form(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, add: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([14.0, 8.0])
        .min_col_width(100.0)
        .show(ui, add);
}

/// A row of a form: the label, then the controls, then the end of the row.
pub fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(label);
    ui.horizontal(add);
    ui.end_row();
}

/// A muted note under a control.
pub fn note(ui: &mut Ui, p: &Palette, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(11.5).color(p.text_muted));
}

/// The primary (filled) button.
pub fn primary(p: &Palette, text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(p.on_button)).fill(p.button)
}

/// Buttons along the bottom edge of the body, right-aligned: the first
/// given ends up leftmost. `add` draws them right to left, so put the
/// primary action first.
pub fn footer(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::bottom_up(Align::Max), |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
}

/// A key chord in a small rounded box.
pub fn chord(ui: &mut Ui, p: &Palette, text: &str) {
    egui::Frame::NONE
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().size(11.5).color(p.accent_strong));
        });
}

// ---------------------------------------------------------------------
// Inspector rows (Properties, Text, and the other docked editing panels)

/// Height of a row in an inspector panel.
pub const ROW_HEIGHT: f32 = 22.0;
/// Width of a row's label column, so the controls line up down the panel.
pub const LABEL_WIDTH: f32 = 78.0;
/// Width of a row's right-hand cell, where the keyframe diamond (or a small
/// button) sits at the edge of the dock.
pub const END_WIDTH: f32 = 24.0;
/// Width of a number box: wide enough for the longest value it takes (such as
/// `-1000.00` or `-1000.0%`), so a box never changes size as its value does.
pub const NUMBER_WIDTH: f32 = 68.0;

/// One row of an inspector panel: the label, the controls (which take the
/// room between the label and the right-hand cell, starting at the left of
/// it), and a right-hand cell that stays at the edge of the dock whatever the
/// panel's width. Returns the rectangle of that cell, for the keyframe diamond.
pub fn prop_row(ui: &mut Ui, label: &str, controls: impl FnOnce(&mut Ui)) -> egui::Rect {
    prop_row_inner(ui, label, true, controls)
}

/// A row with no right-hand cell: the controls take all the room.
pub fn prop_row_full(ui: &mut Ui, label: &str, controls: impl FnOnce(&mut Ui)) {
    prop_row_inner(ui, label, false, controls);
}

fn prop_row_inner(ui: &mut Ui, label: &str, end: bool, controls: impl FnOnce(&mut Ui)) -> egui::Rect {
    let width = ui.available_width();
    ui.horizontal(|ui| {
        let gap = ui.spacing().item_spacing.x;
        // each cell is given its full width even when its content is narrower,
        // so the next cell, and above all the right-hand one, never moves
        ui.allocate_ui_with_layout(
            vec2(LABEL_WIDTH, ROW_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_size(vec2(LABEL_WIDTH, ROW_HEIGHT));
                ui.add(egui::Label::new(label).truncate());
            },
        );
        let end_room = if end { END_WIDTH + gap } else { 0.0 };
        let room = (width - LABEL_WIDTH - gap - end_room).max(0.0);
        ui.allocate_ui_with_layout(
            vec2(room, ROW_HEIGHT),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_size(vec2(room, ROW_HEIGHT));
                // sliders fill the cell, leaving room for their number box
                ui.spacing_mut().slider_width = (room - NUMBER_WIDTH - 12.0).max(40.0);
                controls(ui);
            },
        );
        if end {
            ui.allocate_exact_size(vec2(END_WIDTH, ROW_HEIGHT), egui::Sense::hover())
                .0
        } else {
            egui::Rect::NOTHING
        }
    })
    .inner
}

/// A number box of a fixed width in the monospace figure font. Use it for any
/// value that changes while the playhead moves, so the panel does not shift.
pub fn number(ui: &mut Ui, drag: egui::DragValue<'_>) -> egui::Response {
    number_sized(ui, NUMBER_WIDTH, drag)
}

/// A number box of a chosen fixed width, for values that need fewer digits.
pub fn number_sized(ui: &mut Ui, width: f32, drag: egui::DragValue<'_>) -> egui::Response {
    ui.scope(|ui| {
        ui.style_mut().override_font_id = Some(crate::theme::mono(12.5));
        ui.add_sized(vec2(width, 20.0), drag)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` in a fresh panel of `width` and return what it measured.
    fn in_panel<R>(width: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
        let ctx = egui::Context::default();
        let mut out = None;
        let mut f = Some(f);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
            ..Default::default()
        };
        let mut run = ctx.run_ui(input, |ui| {
            ui.scope(|ui| {
                ui.set_max_width(width);
                if let Some(f) = f.take() {
                    out = Some(f(ui));
                }
            });
        });
        // no renderer to upload the font atlas to
        run.textures_delta.clear();
        out.expect("the panel ran")
    }

    #[test]
    fn the_right_hand_cell_sits_at_the_edge_whatever_the_controls_take() {
        for width in [284.0, 340.0, 520.0] {
            let (left, wide) = in_panel(width, |ui| {
                let left = ui.max_rect().left();
                // controls far narrower than the room, then far wider
                let narrow = prop_row(ui, "Rotation", |ui| {
                    ui.label("0");
                });
                let wide = prop_row(ui, "Position", |ui| {
                    for _ in 0..2 {
                        number(ui, egui::DragValue::new(&mut 0.0f32));
                    }
                });
                assert!((narrow.right() - wide.right()).abs() < 0.5, "{width}");
                (left, narrow)
            });
            assert!(
                (wide.right() - (left + width)).abs() < 0.5,
                "{width}: {}",
                wide.right()
            );
        }
    }

    #[test]
    fn two_number_boxes_and_a_diamond_fit_the_narrowest_dock() {
        // the dock's content width at its 300 px minimum: margins and the scroll bar come off
        let fits = in_panel(300.0 - 16.0 - 10.0, |ui| {
            let before = ui.min_rect();
            let cell = prop_row(ui, "Position", |ui| {
                number(ui, egui::DragValue::new(&mut -1000.0f32).max_decimals(2));
                number(ui, egui::DragValue::new(&mut -1000.0f32).max_decimals(2));
            });
            cell.right() <= before.left() + ui.max_rect().width() + 0.5
                && ui.min_rect().width() <= ui.max_rect().width() + 0.5
        });
        assert!(fits);
    }

    #[test]
    fn a_number_box_keeps_its_width_as_the_value_changes() {
        let widths: Vec<f32> = [0.0f32, 5.5, -1000.0]
            .into_iter()
            .map(|mut v| in_panel(300.0, |ui| number(ui, egui::DragValue::new(&mut v)).rect.width()))
            .collect();
        assert!(
            widths.iter().all(|w| (w - NUMBER_WIDTH).abs() < 0.5),
            "{widths:?}"
        );
    }
}
