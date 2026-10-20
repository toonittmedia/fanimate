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
