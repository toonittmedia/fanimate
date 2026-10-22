//! The Home tab: start a new file, open a recent one, or try a template.
//!
//! It fills the window under the tab strip while it is in front. It is a
//! page, not a dialog: the document tabs stay one click away, and nothing
//! on it blocks the rest of the app.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{self, Align2, FontId, Rect, RichText, ScrollArea, Sense, Stroke, pos2, vec2};
use fanimate_editor::{Action, Preset, Sample};

use crate::app::Shell;
use crate::docs::Startup;
use crate::menus;
use crate::yourfiles::{self, Found};

const NAV_WIDTH: f32 = 240.0;
const CARD_WIDTH: f32 = 168.0;
const CARD_HEIGHT: f32 = 148.0;
const PREVIEW_HEIGHT: f32 = 96.0;
const ROW_HEIGHT: f32 = 52.0;

/// The part of the Home tab showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Home,
    Recent,
    YourFiles,
    Templates,
}

impl Section {
    const ALL: [Section; 4] = [Self::Home, Self::Recent, Self::YourFiles, Self::Templates];

    fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Recent => "Recent",
            Self::YourFiles => "Your files",
            Self::Templates => "Templates",
        }
    }
}

/// How many templates and projects per folder the Home page itself shows;
/// the rest are one click away in their own section.
const HOME_TEMPLATES: usize = 4;
const HOME_PER_FOLDER: usize = 6;

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    shell.ui.thumbs.poll(ui.ctx());
    shell.ui.your_files.poll(ui.ctx());
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
                    Section::YourFiles => {
                        ui.label(RichText::new("Your files").size(22.0).strong());
                        ui.add_space(8.0);
                        your_files(ui, shell, usize::MAX);
                    }
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
            if card(ui, shell, preset.name(), &detail, Some(width / height), None).clicked() {
                shell.pending.push(Action::NewPreset(preset));
            }
        }
    });
    ui.add_space(26.0);
    section_title(ui, shell, "Recent");
    recent(ui, shell, 5);
    if shell.recent.len() > 5 && ui.link("See all recent files").clicked() {
        shell.ui.home_section = Section::Recent;
    }
    ui.add_space(26.0);
    section_title(ui, shell, "Your files");
    your_files(ui, shell, HOME_PER_FOLDER);
    ui.add_space(26.0);
    section_title(ui, shell, "Templates");
    template_cards(ui, shell, HOME_TEMPLATES);
    if Sample::ALL.len() > HOME_TEMPLATES && ui.link("See all templates").clicked() {
        shell.ui.home_section = Section::Templates;
    }
    ui.add_space(20.0);
}

fn section_title(ui: &mut egui::Ui, shell: &Shell, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong().color(shell.palette.text));
    ui.add_space(8.0);
}

/// A card: a preview on top (a page of the given proportions, or nothing),
/// then a name and a line of detail. Returns the click.
fn card(
    ui: &mut egui::Ui,
    shell: &Shell,
    name: &str,
    detail: &str,
    page: Option<f32>,
    preview_image: Option<&egui::TextureHandle>,
) -> egui::Response {
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
    if let Some(texture) = preview_image {
        // the real first frame, as large as the preview allows
        let size = texture.size_vec2();
        let scale = ((preview.width() - 8.0) / size.x).min((preview.height() - 8.0) / size.y);
        let shown = Rect::from_center_size(preview.center(), size * scale);
        egui::Image::new((texture.id(), shown.size())).paint_at(ui, shown);
        painter.rect_stroke(shown, 2.0, Stroke::new(1.0, p.border), egui::StrokeKind::Outside);
    } else if let Some(aspect) = page {
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
    // names and details are cut with an ellipsis at the card's edge; the
    // tooltip carries the whole text
    let room = rect.width() - 24.0;
    let cut_name = one_line(
        painter,
        pos2(rect.left() + 12.0, preview.bottom() + 10.0),
        name,
        FontId::proportional(13.0),
        p.text,
        room,
    );
    let cut_detail = one_line(
        painter,
        pos2(rect.left() + 12.0, preview.bottom() + 28.0),
        detail,
        FontId::proportional(11.5),
        p.text_muted,
        room,
    );
    if cut_name || cut_detail {
        return r.on_hover_text(format!("{name}\n{detail}"));
    }
    r
}

/// Paint `text` on one line no wider than `width`, ending in an ellipsis when
/// it does not fit. Returns whether it was cut.
fn one_line(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: &str,
    font: FontId,
    color: egui::Color32,
    width: f32,
) -> bool {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = painter.layout_job(job);
    let cut = galley.elided;
    painter.galley(pos, galley, color);
    cut
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
        // the first frame of the project, small, at the left
        let thumb = Rect::from_min_size(rect.min + vec2(8.0, 4.0), vec2(64.0, ROW_HEIGHT - 8.0));
        ui.painter().rect_filled(thumb, 3.0, p.bg);
        if let Some(texture) = shell.ui.thumbs.file(path) {
            let size = texture.size_vec2();
            let scale = ((thumb.width() - 2.0) / size.x).min((thumb.height() - 2.0) / size.y);
            let shown = Rect::from_center_size(thumb.center(), size * scale);
            egui::Image::new((texture.id(), shown.size())).paint_at(ui, shown);
        }
        let room = rect.width() - 24.0 - 72.0;
        one_line(
            ui.painter(),
            pos2(rect.left() + 84.0, rect.top() + 8.0),
            &name,
            FontId::proportional(13.5),
            p.text,
            room,
        );
        one_line(
            ui.painter(),
            pos2(rect.left() + 84.0, rect.top() + 27.0),
            &folder,
            FontId::proportional(11.5),
            p.text_muted,
            room,
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
        RichText::new("Starting points and samples to look inside. They open as new, unsaved files.")
            .color(shell.palette.text_muted),
    );
    ui.add_space(16.0);
    template_cards(ui, shell, usize::MAX);
}

/// The first `limit` templates as cards; a click opens one as a new file.
fn template_cards(ui: &mut egui::Ui, shell: &mut Shell, limit: usize) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
        for sample in Sample::ALL.into_iter().take(limit) {
            let preview = shell.ui.thumbs.sample(sample).cloned();
            let r = card(ui, shell, sample.name(), sample.detail(), None, preview.as_ref());
            if r.clicked() {
                shell.pending.push(Action::OpenSample(sample));
            }
            menus::context_menu(&r, |ui| {
                if ui.button("Open as a new file").clicked() {
                    shell.pending.push(Action::OpenSample(sample));
                    ui.close();
                }
            });
        }
    });
}

/// A folder's own name, and the folder it is in.
fn folder_label(dir: &Path) -> (String, String) {
    let name = dir
        .file_name()
        .map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
    let parent = dir.parent().map_or_else(String::new, |p| p.display().to_string());
    (name, parent)
}

/// The folders the user added and the projects in them, `limit` per folder.
/// A click opens a project as a tab.
fn your_files(ui: &mut egui::Ui, shell: &mut Shell, limit: usize) {
    let p = shell.palette;
    let mut add = false;
    let mut refresh = false;
    let mut remove: Option<PathBuf> = None;
    let mut open: Option<PathBuf> = None;
    let now = SystemTime::now();

    let folders = shell.ui.your_files.folders().to_vec();
    if folders.is_empty() {
        ui.label(RichText::new("Add a folder to see your projects here.").color(p.text_muted));
        ui.add_space(6.0);
        if ui.button("Add folder…").clicked() {
            add = true;
        }
    } else {
        ui.horizontal(|ui| {
            if ui.button("Add folder…").clicked() {
                add = true;
            }
            if ui.link("Refresh").clicked() {
                refresh = true;
            }
        });
        ui.add_space(8.0);
    }
    for dir in &folders {
        let (name, parent) = folder_label(dir);
        let heading = ui
            .horizontal(|ui| {
                let title = egui::Label::new(RichText::new(&name).size(13.0).strong().color(p.text))
                    .selectable(false)
                    .sense(Sense::click());
                let r = ui
                    .add(title)
                    .on_hover_text("Right-click to open this folder or take it off the list");
                ui.add(
                    egui::Label::new(RichText::new(&parent).size(11.5).color(p.text_muted))
                        .selectable(false)
                        .truncate(),
                );
                r
            })
            .inner;
        menus::context_menu(&heading, |ui| {
            if ui.button("Open folder in file manager").clicked() {
                crate::render_queue::open_path(dir);
                ui.close();
            }
            if ui.button("Remove from list").clicked() {
                remove = Some(dir.clone());
                ui.close();
            }
        });
        ui.add_space(4.0);

        let found: Option<Vec<Found>> = shell.ui.your_files.files(dir).map(|f| f.to_vec());
        match found {
            None => {
                ui.label(RichText::new("Looking for projects…").color(p.text_muted));
            }
            Some(files) if files.is_empty() => {
                ui.label(
                    RichText::new("No Fanimate projects in this folder yet. Projects you save here show up.")
                        .color(p.text_muted),
                );
            }
            Some(files) => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                    for file in files.iter().take(limit) {
                        let preview = shell.ui.thumbs.file(&file.path).cloned();
                        let detail = yourfiles::edited(file.modified, now);
                        let r = card(ui, shell, &file.name, &detail, None, preview.as_ref());
                        if r.clicked() {
                            open = Some(file.path.clone());
                        }
                        menus::context_menu(&r, |ui| {
                            if ui.button("Open").clicked() {
                                open = Some(file.path.clone());
                                ui.close();
                            }
                            if ui.button("Show in folder").clicked() {
                                crate::render_queue::show_in_folder(&file.path);
                                ui.close();
                            }
                            if ui.button("Copy path").clicked() {
                                ui.ctx().copy_text(file.path.display().to_string());
                                ui.close();
                            }
                        });
                    }
                });
                if files.len() > limit
                    && ui
                        .link(format!("Show all {} projects in this folder", files.len()))
                        .clicked()
                {
                    shell.ui.home_section = Section::YourFiles;
                }
            }
        }
        ui.add_space(14.0);
    }

    if add && let Some(dir) = yourfiles::pick_folder(shell.places.latest()) {
        shell.ui.your_files.add(dir);
    }
    if refresh {
        shell.ui.your_files.refresh();
    }
    if let Some(dir) = remove {
        shell.ui.your_files.remove(&dir);
    }
    if let Some(path) = open {
        shell.open_path(&path);
    }
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
    fn every_template_is_a_sound_project_that_saves_and_opens_whole() {
        let dir = std::env::temp_dir().join(format!("fanimate-templates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut names = std::collections::HashSet::new();
        for (i, sample) in Sample::ALL.into_iter().enumerate() {
            assert!(names.insert(sample.name()), "{} is listed twice", sample.name());
            assert!(!sample.detail().is_empty());
            let doc = sample.document();
            assert_eq!(doc.start, 1, "{} starts on frame 1", sample.name());
            let report = fanimate_fnm::validate(&doc);
            let errors: Vec<_> = report
                .issues
                .iter()
                .filter(|i| i.severity == fanimate_fnm::Severity::Error)
                .collect();
            assert!(errors.is_empty(), "{}:\n{report}", sample.name());

            let path = dir.join(format!("template-{i}.fnm"));
            fanimate_fnm::save(&doc, &path).unwrap();
            let (back, loaded) = fanimate_fnm::load(&path).unwrap();
            assert_eq!(back, doc, "{}", sample.name());
            assert_eq!(loaded.unresolved_errors().count(), 0, "{}", sample.name());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_section_has_a_name() {
        for s in Section::ALL {
            assert!(!s.label().is_empty());
        }
        assert_eq!(Section::default(), Section::Home);
    }
}
