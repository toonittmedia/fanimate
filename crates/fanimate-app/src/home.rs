//! The Home tab: start a new file, open a recent one, or try a template.
//!
//! It fills the window under the tab strip while it is in front. It is a
//! page, not a dialog: the document tabs stay one click away, and nothing
//! on it blocks the rest of the app.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, FontId, Rect, RichText, ScrollArea, Sense, Stroke, pos2, vec2};
use fanimate_editor::{Action, Preset, Sample};

use crate::app::Shell;
use crate::docs::Startup;
use crate::menus;

const NAV_WIDTH: f32 = 240.0;
const CARD_WIDTH: f32 = 168.0;
const CARD_HEIGHT: f32 = 148.0;
const PREVIEW_HEIGHT: f32 = 96.0;
const ROW_HEIGHT: f32 = 46.0;

/// The part of the Home tab showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Home,
    Recent,
    Templates,
}

impl Section {
    const ALL: [Section; 3] = [Self::Home, Self::Recent, Self::Templates];

    fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Recent => "Recent",
            Self::Templates => "Templates",
        }
    }
}

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    egui::Panel::left("home-nav")
        .exact_size(NAV_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.border))
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ui, |ui| nav(ui, shell));
    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(40, 28)),
        )
        .show(ui, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(860.0);
                match shell.ui.home_section {
                    Section::Home => home(ui, shell),
                    Section::Recent => recent(ui, shell, usize::MAX),
                    Section::Templates => templates(ui, shell),
                }
            });
        });
}

fn nav(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let width = ui.available_width();
    let new = egui::Button::new(RichText::new("New file").color(p.on_button).strong()).fill(p.button);
    if ui.add_sized([width, 32.0], new).clicked() {
        shell.pending.push(Action::New);
    }
    if ui.add_sized([width, 32.0], egui::Button::new("Open…")).clicked() {
        shell.pending.push(Action::Open);
    }
    if ui
        .add_sized(
            [width, 28.0],
            egui::Button::new("Import Moho or Animate…").frame(false),
        )
        .clicked()
    {
        shell.pending.push(Action::OpenOther);
    }
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);
    for section in Section::ALL {
        let selected = shell.ui.home_section == section;
        let (rect, r) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
        if selected {
            ui.painter().rect_filled(rect, 4.0, p.accent.gamma_multiply(0.25));
        } else if r.hovered() {
            ui.painter().rect_filled(rect, 4.0, p.raised);
        }
        ui.painter().text(
            pos2(rect.left() + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            section.label(),
            FontId::proportional(13.0),
            if selected { p.text } else { p.text_muted },
        );
        if r.clicked() {
            shell.ui.home_section = section;
        }
    }

    // what opens first, also set in Preferences
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        let mut chosen = shell.startup;
        egui::ComboBox::from_id_salt("home-startup")
            .selected_text(chosen.label())
            .width(width)
            .show_ui(ui, |ui| {
                for s in Startup::ALL {
                    ui.selectable_value(&mut chosen, s, s.label())
                        .on_hover_text(s.description());
                }
            });
        ui.label(
            RichText::new("When Fanimate starts")
                .size(11.5)
                .color(p.text_muted),
        );
        shell.startup = chosen;
    });
}

fn home(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    ui.label(RichText::new("Welcome to Fanimate").size(26.0).strong());
    ui.label(
        RichText::new("Rig, draw and animate 2D characters. Free, on every platform.").color(p.text_muted),
    );
    ui.add_space(22.0);
    section_title(ui, shell, "Start a new file fast");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        for preset in Preset::ALL {
            let (width, height) = preset.size();
            let detail = format!("{width:.0} × {height:.0} px · {:.0} fps", preset.fps());
            if card(ui, shell, preset.name(), &detail, Some(width / height)).clicked() {
                shell.pending.push(Action::NewPreset(preset));
            }
        }
    });
    if !shell.recent.is_empty() {
        ui.add_space(26.0);
        section_title(ui, shell, "Recent");
        recent(ui, shell, 5);
        if shell.recent.len() > 5 && ui.link("See all recent files").clicked() {
            shell.ui.home_section = Section::Recent;
        }
    }
}

fn section_title(ui: &mut egui::Ui, shell: &Shell, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong().color(shell.palette.text));
    ui.add_space(8.0);
}

/// A card: a preview on top (a page of the given proportions, or nothing),
/// then a name and a line of detail. Returns the click.
fn card(ui: &mut egui::Ui, shell: &Shell, name: &str, detail: &str, page: Option<f32>) -> egui::Response {
    let p = shell.palette;
    let (rect, r) = ui.allocate_exact_size(vec2(CARD_WIDTH, CARD_HEIGHT), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, p.panel);
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, if r.hovered() { p.accent } else { p.border }),
        egui::StrokeKind::Inside,
    );
    let preview = Rect::from_min_size(
        rect.min + vec2(8.0, 8.0),
        vec2(rect.width() - 16.0, PREVIEW_HEIGHT),
    );
    painter.rect_filled(preview, 4.0, p.bg);
    if let Some(aspect) = page {
        // the page, at the file's proportions, as large as the preview allows
        let room = preview.shrink(12.0);
        let (w, h) = if aspect >= room.width() / room.height() {
            (room.width(), room.width() / aspect)
        } else {
            (room.height() * aspect, room.height())
        };
        let page = Rect::from_center_size(preview.center(), vec2(w, h));
        painter.rect_filled(page, 2.0, p.raised);
        painter.rect_stroke(
            page,
            2.0,
            Stroke::new(1.0, p.text_muted),
            egui::StrokeKind::Inside,
        );
    }
    painter.text(
        pos2(rect.left() + 12.0, preview.bottom() + 10.0),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(13.0),
        p.text,
    );
    painter.text(
        pos2(rect.left() + 12.0, preview.bottom() + 28.0),
        Align2::LEFT_TOP,
        detail,
        FontId::proportional(11.5),
        p.text_muted,
    );
    r
}

/// Up to `limit` recent files; click to open, right-click for the menu.
fn recent(ui: &mut egui::Ui, shell: &mut Shell, limit: usize) {
    let p = shell.palette;
    if shell.recent.is_empty() {
        ui.label(RichText::new("Files you open or save appear here.").color(p.text_muted));
        return;
    }
    let mut open: Option<PathBuf> = None;
    let mut forget: Option<PathBuf> = None;
    for path in shell.recent.iter().take(limit) {
        let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        if r.hovered() {
            ui.painter().rect_filled(rect, 4.0, p.raised);
        }
        let (name, folder) = describe(path);
        ui.painter().text(
            pos2(rect.left() + 12.0, rect.top() + 8.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(13.5),
            p.text,
        );
        ui.painter().text(
            pos2(rect.left() + 12.0, rect.top() + 27.0),
            Align2::LEFT_TOP,
            folder,
            FontId::proportional(11.5),
            p.text_muted,
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, p.border.gamma_multiply(0.5)),
        );
        let r = r.on_hover_text(path.display().to_string());
        if r.clicked() {
            open = Some(path.clone());
        }
        menus::context_menu(&r, |ui| {
            if ui.button("Open").clicked() {
                open = Some(path.clone());
                ui.close();
            }
            if ui.button("Remove from recent files").clicked() {
                forget = Some(path.clone());
                ui.close();
            }
        });
    }
    if let Some(path) = forget {
        shell.recent.retain(|p| *p != path);
    }
    if let Some(path) = open {
        shell.open_path(&path);
    }
}

/// A file's name without its extension, and the folder it is in.
fn describe(path: &Path) -> (String, String) {
    let name = path.file_stem().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    let folder = path
        .parent()
        .map_or_else(String::new, |p| p.display().to_string());
    (name, folder)
}

fn templates(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.label(RichText::new("Templates").size(22.0).strong());
    ui.label(
        RichText::new("Sample projects to look inside. They open as new, unsaved files.")
            .color(shell.palette.text_muted),
    );
    ui.add_space(16.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        for sample in Sample::ALL {
            let detail = match sample {
                Sample::Hello => "A bone-rigged letter",
                Sample::Benchmark => "40 bones, the speed test",
                Sample::StressTest => "400 bones, for stress",
            };
            if card(ui, shell, sample.name(), detail, None).clicked() {
                shell.pending.push(Action::OpenSample(sample));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_shown_by_name_and_folder() {
        let (name, folder) = describe(Path::new("/work/shows/walk cycle.fnm"));
        assert_eq!(name, "walk cycle");
        assert!(folder.contains("shows"), "{folder}");
        let (name, _) = describe(Path::new("scene.fnm"));
        assert_eq!(name, "scene");
    }

    #[test]
    fn every_section_has_a_name() {
        for s in Section::ALL {
            assert!(!s.label().is_empty());
        }
        assert_eq!(Section::default(), Section::Home);
    }
}
