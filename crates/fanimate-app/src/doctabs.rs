//! The tab strip under the title bar: Home, then one tab per open document.
//!
//! Click a tab to bring it forward, drag it to reorder, middle-click or
//! click its x to close it, right-click for its menu. A document with
//! unsaved changes shows a dot where the x is until the pointer is over it.

use eframe::egui::{self, FontId, Id, Rect, ScrollArea, Sense, Stroke, pos2, vec2};
use fanimate_editor::Action;

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;

const HEIGHT: f32 = 32.0;
const PAD: f32 = 12.0;
const CLOSE: f32 = 16.0;
const MIN_WIDTH: f32 = 96.0;
/// Longest title shown, in characters; the rest is cut with an ellipsis.
const MAX_CHARS: usize = 26;

/// What the user did to the strip this frame, applied once it is drawn.
#[derive(Default)]
struct Clicks {
    show_home: bool,
    switch_to: Option<usize>,
    close: Option<usize>,
    new_file: bool,
    reorder: Option<(usize, usize)>,
}

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    shell.sync_tab_info();
    let p = shell.palette;
    let mut clicks = Clicks::default();
    egui::Panel::top("doc-tabs")
        .exact_size(HEIGHT)
        .frame(egui::Frame::NONE.fill(p.bg))
        .show(ui, |ui| {
            let strip = ui.max_rect();
            ui.painter()
                .hline(strip.x_range(), strip.bottom() - 0.5, Stroke::new(1.0, p.border));
            ScrollArea::horizontal()
                .id_salt("doc-tabs-scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        tabs(ui, shell, &mut clicks);
                    });
                });
        });

    if clicks.show_home {
        shell.show_home();
    }
    if let Some((from, to)) = clicks.reorder {
        shell.docs.move_tab(from, to);
    }
    if let Some(tab) = clicks.switch_to {
        shell.switch_tab(tab);
    }
    if let Some(tab) = clicks.close {
        shell.request_close_tab(tab);
    }
    if clicks.new_file {
        shell.pending.push(Action::New);
    }
}

fn tabs(ui: &mut egui::Ui, shell: &mut Shell, clicks: &mut Clicks) {
    let home_front = shell.docs.home_front();
    let active = shell.docs.active();

    // Home: always there, never closed
    let home = tab(
        ui,
        shell,
        "Home",
        Some(Icon::Home),
        home_front,
        false,
        false,
        None,
    );
    if home.body.clicked() {
        clicks.show_home = true;
    }
    menus::context_menu(&home.body, |ui| {
        menus::item(ui, shell, Action::New);
        menus::item(ui, shell, Action::Open);
    });

    let infos: Vec<_> = shell.docs.infos().cloned().collect();
    let mut rects = Vec::with_capacity(infos.len());
    let mut dragged = None;
    for (i, info) in infos.iter().enumerate() {
        let selected = active == Some(i) && !home_front;
        let tooltip = info.path.as_ref().map_or_else(
            || format!("{} (not saved yet)", info.title),
            |p| p.display().to_string(),
        );
        let title = shorten(&info.title);
        // a stable id, so a dragged tab stays the dragged one as it moves
        let id = Id::new(("doc-tab", &info.path, &info.title));
        let t = tab_with_id(
            ui,
            shell,
            id,
            &title,
            None,
            selected,
            info.dirty,
            true,
            Some(tooltip),
        );
        rects.push(t.body.rect);
        if t.close || t.body.middle_clicked() {
            clicks.close = Some(i);
        } else if t.body.clicked() || t.body.secondary_clicked() && active != Some(i) {
            clicks.switch_to = Some(i);
        }
        if t.body.dragged() {
            dragged = Some(i);
        }
        menus::context_menu(&t.body, |ui| {
            menus::item(ui, shell, Action::CloseDocument);
            menus::item(ui, shell, Action::CloseOtherDocuments);
            ui.separator();
            menus::item(ui, shell, Action::Save);
            menus::item(ui, shell, Action::SaveAs);
        });
        if selected && std::mem::take(&mut shell.ui.scroll_tab_into_view) {
            ui.scroll_to_rect(t.body.rect, None);
        }
    }
    // a dragged tab goes where the pointer is
    if let (Some(from), Some(pointer)) = (dragged, ui.input(|i| i.pointer.hover_pos())) {
        let to = rects
            .iter()
            .position(|r| pointer.x >= r.left() && pointer.x < r.right())
            // past either end: the first or last place
            .unwrap_or(if pointer.x < rects[0].left() {
                0
            } else {
                rects.len() - 1
            });
        if to != from {
            clicks.reorder = Some((from, to));
        }
    }

    // a new file, beside the last tab
    let (rect, r) = ui.allocate_exact_size(vec2(HEIGHT, HEIGHT), Sense::click());
    let p = shell.palette;
    if r.hovered() {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(4.0, 5.0)), 4.0, p.raised);
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        Icon::Add,
        if r.hovered() { p.text } else { p.text_muted },
    );
    if r.on_hover_text("New file").clicked() {
        clicks.new_file = true;
    }
}

struct TabResponse {
    body: egui::Response,
    close: bool,
}

#[allow(clippy::too_many_arguments)]
fn tab(
    ui: &mut egui::Ui,
    shell: &Shell,
    label: &str,
    icon: Option<Icon>,
    selected: bool,
    dirty: bool,
    closable: bool,
    tooltip: Option<String>,
) -> TabResponse {
    tab_with_id(
        ui,
        shell,
        Id::new(("doc-tab", label)),
        label,
        icon,
        selected,
        dirty,
        closable,
        tooltip,
    )
}

#[allow(clippy::too_many_arguments)]
fn tab_with_id(
    ui: &mut egui::Ui,
    shell: &Shell,
    id: Id,
    label: &str,
    icon: Option<Icon>,
    selected: bool,
    dirty: bool,
    closable: bool,
    tooltip: Option<String>,
) -> TabResponse {
    let p = shell.palette;
    let font = FontId::proportional(12.5);
    let color = if selected { p.text } else { p.text_muted };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let icon_w = if icon.is_some() { 14.0 + 6.0 } else { 0.0 };
    let close_w = if closable { 8.0 + CLOSE } else { 0.0 };
    let width = PAD + icon_w + galley.size().x + close_w + PAD;
    let width = if closable { width.max(MIN_WIDTH) } else { width };

    let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());
    let body = ui.interact(rect, id, Sense::click_and_drag());
    let hovered = body.hovered();
    let painter = ui.painter();

    // flat: the front tab is the panel's colour, the rest the bar's
    let fill = if selected {
        p.panel
    } else if hovered {
        p.raised
    } else {
        p.bg
    };
    painter.rect_filled(rect, 0.0, fill);
    painter.vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, p.border));
    if selected {
        // purple marks the front tab, and the tab runs into the panel below
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(rect.width() - 1.0, 2.0)),
            0.0,
            p.accent,
        );
        painter.hline(
            rect.left() + 0.0..=rect.right() - 1.0,
            rect.bottom() - 0.5,
            Stroke::new(1.0, p.panel),
        );
    }

    let mut x = rect.left() + PAD;
    if let Some(icon) = icon {
        icons::paint(
            painter,
            Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(14.0, 14.0)),
            icon,
            color,
        );
        x += icon_w;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);

    let mut close = false;
    if closable {
        let close_rect = Rect::from_center_size(
            pos2(rect.right() - PAD - CLOSE / 2.0, rect.center().y),
            vec2(CLOSE, CLOSE),
        );
        let r = ui.interact(close_rect, id.with("close"), Sense::click());
        let show_x = selected || hovered || r.hovered();
        if r.hovered() {
            ui.painter().rect_filled(close_rect, 3.0, p.border);
        }
        if dirty && !r.hovered() && !hovered {
            // unsaved: a dot until the pointer comes over the tab
            ui.painter().circle_filled(close_rect.center(), 3.5, p.text_muted);
        } else if show_x {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(close_rect.center(), vec2(11.0, 11.0)),
                Icon::Close,
                if r.hovered() { p.text } else { p.text_muted },
            );
        }
        close = r.clicked();
        r.on_hover_text("Close");
    }
    let body = match tooltip {
        Some(text) => body.on_hover_text(text),
        None => body,
    };
    TabResponse { body, close }
}

/// `title`, cut to fit a tab.
fn shorten(title: &str) -> String {
    if title.chars().count() <= MAX_CHARS {
        return title.to_owned();
    }
    let cut: String = title.chars().take(MAX_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_titles_are_cut_with_an_ellipsis() {
        assert_eq!(shorten("walk"), "walk");
        let long = "a very long project name that keeps going and going";
        let short = shorten(long);
        assert!(short.chars().count() <= MAX_CHARS);
        assert!(short.ends_with('…'));
        assert_eq!(shorten(&"é".repeat(MAX_CHARS)), "é".repeat(MAX_CHARS));
    }
}
