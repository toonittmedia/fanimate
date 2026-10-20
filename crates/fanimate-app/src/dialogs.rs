//! Dialogs: first-run welcome, preferences, shortcut list, about, the
//! "save changes?" prompt, the command palette, reporting a crash and
//! updating Fanimate.

use eframe::egui::{self, Id, Key, RichText, vec2};
use fanimate_editor::{Action, KeymapKind};

use crate::app::{Discard, Shell, run_unchecked, save};
use crate::kit::{self, Backdrop};
use crate::theme::Theme;

#[derive(Default)]
pub struct PaletteState {
    query: String,
    selected: usize,
    /// The search field has had focus; it asks once, when the palette opens.
    focused: bool,
}

pub fn show(ctx: &egui::Context, shell: &mut Shell) {
    // recovering work after a crash comes before everything else
    if !shell.recovery.offered.is_empty() {
        crate::recovery::dialog(ctx, shell);
        return;
    }
    // then the crash that lost it, if the user wants to report it
    if shell.crash.is_some() {
        crash(ctx, shell);
        return;
    }
    if shell.ui.show_welcome {
        welcome(ctx, shell);
    }
    if shell.ui.confirm_discard.is_some() {
        confirm_discard(ctx, shell);
    }
    if shell.ui.palette.is_some() {
        palette(ctx, shell);
    }
    if shell.ui.show_preferences {
        preferences(ctx, shell);
    }
    if shell.ui.show_shortcuts {
        shortcuts(ctx, shell);
    }
    if shell.ui.show_about {
        about(ctx, shell);
    }
    if shell.ui.show_update {
        update(ctx, shell);
    }
}

fn primary(shell: &Shell, text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(shell.palette.on_button))
        .fill(shell.palette.button)
}

/// "Fanimate closed unexpectedly": report the crash, or not.
fn crash(ctx: &egui::Context, shell: &mut Shell) {
    let Some(pending) = &shell.crash else { return };
    let muted = shell.palette.text_muted;
    let mut report = false;
    let mut close = false;
    let mut ask = shell.crash_reports;
    egui::Modal::new(Id::new("crash-report")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new("Fanimate closed unexpectedly").size(16.0).strong());
        ui.label(
            RichText::new(
                "Reporting the crash helps fix it. Fanimate opens a new issue on GitHub with the report \
                 filled in, for you to read and send. Nothing is sent unless you send it.",
            )
            .color(muted),
        );
        ui.add_space(6.0);
        egui::CollapsingHeader::new("Show the report").show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                let mut text = pending.text.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );
            });
        });
        ui.add_space(6.0);
        ui.checkbox(&mut ask, "Offer to report crashes");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(primary(shell, "Report on GitHub…")).clicked() {
                report = true;
            }
            if ui.button("Don't report").clicked() {
                close = true;
            }
        });
    });
    shell.crash_reports = ask;
    if report {
        crate::links::open(&crate::crash::issue_url(pending));
    }
    if (report || close)
        && let Some(p) = shell.crash.take()
    {
        crate::crash::dismiss(&p);
    }
}

/// The Update Fanimate dialog.
fn update(ctx: &egui::Context, shell: &mut Shell) {
    use crate::update::{Install, State, Version};
    let state = shell.updater.state();
    let p = shell.palette;
    let muted = p.text_muted;
    let mut close = false;
    // with release notes to read, the dialog is taller
    let size = match &state {
        State::Available(r) if !r.notes.trim().is_empty() => vec2(560.0, 470.0),
        _ => vec2(420.0, 190.0),
    };
    let closed = kit::modal(
        ctx,
        &p,
        "update",
        "Update Fanimate",
        size,
        Backdrop::Closes,
        |ui| match &state {
            State::Idle | State::Checking { .. } | State::UpToDate | State::Failed(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking for updates…");
                });
            }
            State::Available(r) => {
                ui.label(RichText::new(format!("Fanimate {} is available", r.version)).strong());
                ui.label(RichText::new(format!("You have version {}.", Version::current())).color(muted));
                crate::links::link(ui, "Release page", &r.page);
                ui.add_space(4.0);
                if !r.notes.trim().is_empty() {
                    // the notes, with room left for the buttons under them
                    let blocks = crate::markdown::parse(&r.notes);
                    egui::Frame::NONE
                        .fill(p.bg)
                        .stroke(egui::Stroke::new(1.0, p.border))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("update-notes")
                                .max_height((ui.available_height() - 90.0).max(80.0))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width() - 8.0);
                                    crate::markdown::show(ui, &p, &blocks);
                                });
                        });
                    ui.add_space(6.0);
                }
                match (&r.file, &shell.install) {
                    (Some(_), _) => {
                        ui.horizontal(|ui| {
                            if ui.add(kit::primary(&p, "Download")).clicked()
                                && let Some(dir) = eframe::storage_dir("Fanimate")
                            {
                                shell.updater.download(r.clone(), dir.join("updates"));
                                close = true;
                            }
                            if ui.button("Skip this version").clicked() {
                                shell.updates.skipped = Some(r.version.to_string());
                                shell.updater.reset();
                                close = true;
                            }
                        });
                        kit::note(
                            ui,
                            &p,
                            "The download runs in the background; progress shows in the status bar.",
                        );
                    }
                    (None, Install::Managed) => {
                        ui.label(
                            RichText::new("Update Fanimate through your system's software centre.")
                                .color(muted),
                        );
                    }
                    (None, _) => {
                        ui.label(
                            RichText::new(
                                "This copy of Fanimate cannot update itself. Download the new version:",
                            )
                            .color(muted),
                        );
                        crate::links::link(ui, "Fanimate releases", crate::update::DOWNLOAD_PAGE);
                    }
                }
            }
            State::Downloading(r) => {
                ui.label(format!("Downloading Fanimate {}…", r.version));
                let bar = match shell.updater.progress() {
                    Some(f) => egui::ProgressBar::new(f).show_percentage(),
                    None => egui::ProgressBar::new(0.0).animate(true),
                };
                ui.add(bar.desired_width(ui.available_width()).fill(p.accent));
                kit::note(
                    ui,
                    &p,
                    "You can close this and keep working. Progress shows in the status bar.",
                );
            }
            State::Ready(r, _) => {
                ui.label(RichText::new(format!("Fanimate {} is ready to install", r.version)).strong());
                ui.label(
                    RichText::new(
                        "Fanimate closes to install it and opens again. You are asked to save changes first.",
                    )
                    .color(muted),
                );
                ui.add_space(6.0);
                if ui.add(kit::primary(&p, "Restart and Update")).clicked() {
                    shell.pending.push(Action::InstallUpdate);
                    close = true;
                }
            }
        },
    );
    shell.ui.show_update = !(closed || close);
}

/// What opens first. Also on the Home tab.
fn startup_choice(ui: &mut egui::Ui, shell: &mut Shell) {
    for startup in crate::docs::Startup::ALL {
        let selected = shell.startup == startup;
        let r = ui.radio(selected, startup.label());
        r.clone().on_hover_text(startup.description());
        if r.clicked() {
            shell.startup = startup;
        }
    }
}

fn keymap_choice(ui: &mut egui::Ui, shell: &mut Shell) {
    for kind in KeymapKind::ALL {
        let selected = shell.keymap.kind == kind;
        let frame = egui::Frame::group(ui.style())
            .fill(if selected {
                shell.palette.accent.gamma_multiply(0.25)
            } else {
                shell.palette.bg
            })
            .stroke(egui::Stroke::new(
                1.0,
                if selected {
                    shell.palette.accent
                } else {
                    shell.palette.border
                },
            ));
        let inner = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            let radio = ui.radio(selected, RichText::new(kind.label()).strong());
            ui.label(RichText::new(kind.description()).color(shell.palette.text_muted));
            radio.clicked()
        });
        // the whole card is clickable, not just the radio button
        let card = inner.response.interact(egui::Sense::click());
        if (inner.inner || card.clicked()) && !selected {
            shell.set_keymap(kind);
        }
        ui.add_space(4.0);
    }
}

fn theme_choice(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.horizontal_wrapped(|ui| {
        for theme in Theme::ALL {
            if ui.selectable_label(shell.theme == theme, theme.label()).clicked() {
                shell.set_theme(ui.ctx(), theme);
            }
        }
    });
}

fn welcome(ctx: &egui::Context, shell: &mut Shell) {
    let modal = egui::Modal::new(Id::new("welcome")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((shell.icon.id(), egui::vec2(40.0, 40.0))));
            ui.vertical(|ui| {
                ui.label(RichText::new("Welcome to Fanimate").size(20.0).strong());
                ui.label(RichText::new("Free to animate.").color(shell.palette.accent_strong));
            });
        });
        ui.add_space(10.0);
        ui.label(RichText::new("Keyboard shortcuts").strong());
        ui.label(
            RichText::new(
                "Coming from Moho? Keep its shortcuts. You can switch any time in Edit › Keyboard Shortcuts.",
            )
            .color(shell.palette.text_muted),
        );
        ui.add_space(4.0);
        keymap_choice(ui, shell);
        ui.add_space(6.0);
        ui.label(RichText::new("Theme").strong());
        theme_choice(ui, shell);
        ui.add_space(6.0);
        ui.checkbox(&mut shell.updates.check, "Check for updates automatically")
            .on_hover_text("Fanimate asks before downloading or installing anything");
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("Start animating").color(shell.palette.on_button))
                        .fill(shell.palette.button),
                )
                .clicked()
            {
                shell.ui.show_welcome = false;
            }
        });
    });
    if modal.should_close() {
        shell.ui.show_welcome = false;
    }
}

/// The documents a "save changes?" question is about: the front one for a
/// closing tab, every unsaved one for quitting.
fn unsaved_names(shell: &Shell, pending: Discard) -> Vec<String> {
    match pending {
        Discard::CloseTab => vec![crate::docs::tab_info(&shell.editor).title],
        Discard::Action(_) => shell
            .docs
            .infos()
            .filter(|t| t.dirty)
            .map(|t| t.title.clone())
            .collect(),
    }
}

/// "Change “Rig” to a group? This removes 12 bones and 3 actions."
pub fn confirm_convert(ctx: &egui::Context, shell: &mut Shell) {
    let Some((to, lost)) = shell.ui.confirm_convert.clone() else {
        return;
    };
    let name = shell
        .editor
        .primary()
        .and_then(|id| shell.editor.doc.find(id))
        .map(|l| l.name.clone())
        .unwrap_or_default();
    let p = shell.palette;
    let mut answer: Option<bool> = None;
    let modal = egui::Modal::new(Id::new("confirm-convert")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.label(
            RichText::new(format!("Change “{name}” to a {}?", to.label().to_lowercase()))
                .size(16.0)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(format!("This removes {lost}. The layers inside stay."));
        ui.label(RichText::new("You can undo it right after.").color(p.text_muted));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.add(kit::primary(&p, "Change")).clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    if modal.should_close() {
        answer = Some(false);
    }
    match answer {
        Some(true) => {
            shell.ui.confirm_convert = None;
            shell.pending.push(Action::ConfirmConvertLayerType(to));
        }
        Some(false) => shell.ui.confirm_convert = None,
        None => {}
    }
}

fn confirm_discard(ctx: &egui::Context, shell: &mut Shell) {
    let Some(pending) = shell.ui.confirm_discard else {
        return;
    };
    let names = unsaved_names(shell, pending);
    let mut answer: Option<bool> = None; // Some(save?) ; None = undecided
    let mut cancel = false;
    let modal = egui::Modal::new(Id::new("confirm-discard")).show(ctx, |ui| {
        ui.set_width(380.0);
        let title = match names.as_slice() {
            [one] => format!("Save changes to “{one}”?"),
            many => format!("Save changes to {} documents?", many.len()),
        };
        ui.label(RichText::new(title).size(16.0).strong());
        if names.len() > 1 {
            for name in &names {
                ui.label(RichText::new(format!("•  {name}")).color(shell.palette.text));
            }
        }
        ui.label(
            RichText::new("Your changes will be lost if you don't save them.")
                .color(shell.palette.text_muted),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let save_label = if names.len() > 1 { "Save All" } else { "Save" };
            if ui
                .add(
                    egui::Button::new(RichText::new(save_label).color(shell.palette.on_button))
                        .fill(shell.palette.button),
                )
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button("Don't Save").clicked() {
                answer = Some(false);
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if cancel || modal.should_close() {
        shell.ui.confirm_discard = None;
        return;
    }
    let Some(save_first) = answer else { return };
    shell.ui.confirm_discard = None;
    if save_first {
        // each unsaved document comes to the front to be saved, so a
        // Save As dialog is about the right one; any failure keeps all work
        let dirty: Vec<usize> = match pending {
            Discard::CloseTab => shell.docs.active().into_iter().collect(),
            Discard::Action(_) => (0..shell.docs.len())
                .filter(|i| shell.docs.info(*i).is_some_and(|t| t.dirty))
                .collect(),
        };
        for tab in dirty {
            shell.switch_tab(tab);
            if !save(shell, false) {
                return; // save cancelled or failed: keep the work
            }
        }
    }
    match pending {
        Discard::CloseTab => {
            if let Some(tab) = shell.docs.active() {
                shell.close_tab(tab);
            }
        }
        Discard::Action(action) => run_unchecked(ctx, shell, action),
    }
}

fn palette(ctx: &egui::Context, shell: &mut Shell) {
    let Some(state) = shell.ui.palette.as_mut() else {
        return;
    };
    let mut matches = fanimate_editor::search(&Action::all(), &state.query, &shell.recent_actions);
    // what can run now comes first, keeping the search order otherwise
    matches.sort_by_key(|a| !shell.editor.can_run(*a));
    state.selected = state.selected.min(matches.len().saturating_sub(1));

    let (up, down, enter, escape) = ctx.input(|i| {
        (
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Enter),
            i.key_pressed(Key::Escape),
        )
    });
    if down {
        state.selected = (state.selected + 1).min(matches.len().saturating_sub(1));
    }
    if up {
        state.selected = state.selected.saturating_sub(1);
    }

    let mut run: Option<Action> = None;
    let modal = egui::Modal::new(Id::new("command-palette")).show(ctx, |ui| {
        ui.set_width(440.0);
        let state = shell.ui.palette.as_mut().expect("checked above");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("Type a command…")
                .desired_width(f32::INFINITY),
        );
        if !state.focused {
            edit.request_focus();
            state.focused = true;
        }
        if edit.changed() {
            state.selected = 0;
        }
        let selected = state.selected;
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (i, action) in matches.iter().enumerate() {
                let enabled = shell.editor.can_run(*action);
                let text = RichText::new(action.label()).color(if enabled {
                    shell.palette.text
                } else {
                    shell.palette.text_muted
                });
                let r = ui.add(
                    egui::Button::selectable(i == selected, text)
                        .shortcut_text(shell.keymap.shortcut_text(*action))
                        .min_size(egui::vec2(ui.available_width(), 0.0)),
                );
                if i == selected {
                    r.scroll_to_me(None);
                }
                if r.clicked() && enabled {
                    run = Some(*action);
                }
            }
        });
    });
    if enter
        && let Some(a) = matches.get(shell.ui.palette.as_ref().map_or(0, |s| s.selected))
        && shell.editor.can_run(*a)
    {
        run = Some(*a);
    }
    if escape || modal.should_close() || run.is_some() {
        shell.ui.palette = None;
    }
    if let Some(a) = run {
        shell.pending.push(a);
        // most recent first, a few kept
        shell.recent_actions.retain(|r| *r != a);
        shell.recent_actions.insert(0, a);
        shell.recent_actions.truncate(8);
    }
}

/// The pages of Preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PrefsPage {
    #[default]
    General,
    Interface,
    Canvas,
    Video,
}

impl PrefsPage {
    const ALL: [PrefsPage; 4] = [Self::General, Self::Interface, Self::Canvas, Self::Video];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Interface => "Interface",
            Self::Canvas => "Canvas",
            Self::Video => "Video formats",
        }
    }
}

fn preferences(ctx: &egui::Context, shell: &mut Shell) {
    let p = shell.palette;
    let closed = kit::modal(
        ctx,
        &p,
        "preferences",
        "Preferences",
        vec2(660.0, 470.0),
        Backdrop::Closes,
        |ui| {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(150.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(150.0);
                        for page in PrefsPage::ALL {
                            let selected = shell.ui.prefs_page == page;
                            let (rect, r) = ui.allocate_exact_size(vec2(150.0, 30.0), egui::Sense::click());
                            if selected {
                                ui.painter().rect_filled(rect, 4.0, p.accent.gamma_multiply(0.25));
                            } else if r.hovered() {
                                ui.painter().rect_filled(rect, 4.0, p.panel);
                            }
                            ui.painter().text(
                                egui::pos2(rect.left() + 10.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                page.label(),
                                egui::FontId::proportional(13.0),
                                if selected { p.text } else { p.text_muted },
                            );
                            if r.clicked() {
                                shell.ui.prefs_page = page;
                            }
                        }
                    },
                );
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                let height = ui.available_height();
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .id_salt(("prefs-page", shell.ui.prefs_page.label()))
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| match shell.ui.prefs_page {
                            PrefsPage::General => prefs_general(ui, shell),
                            PrefsPage::Interface => prefs_interface(ui, shell),
                            PrefsPage::Canvas => prefs_canvas(ui, shell),
                            PrefsPage::Video => {
                                kit::section(ui, &p, "Video formats", |ui| video_formats(ui, shell));
                            }
                        });
                });
            });
        },
    );
    if closed {
        shell.ui.show_preferences = false;
    }
}

fn prefs_general(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    kit::section(ui, &p, "When Fanimate starts", |ui| startup_choice(ui, shell));
    kit::section(ui, &p, "Updates and crashes", |ui| {
        ui.checkbox(&mut shell.updates.check, "Check for updates automatically")
            .on_hover_text("Turn off to stay on this version, for example during a production");
        if ui.button("Check now").clicked() {
            shell.pending.push(Action::CheckForUpdates);
        }
        ui.add_space(4.0);
        ui.checkbox(&mut shell.crash_reports, "After a crash, offer to report it");
    });
}

fn prefs_interface(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    kit::section(ui, &p, "Keyboard shortcuts", |ui| {
        keymap_choice(ui, shell);
        if ui.button("Show all shortcuts…").clicked() {
            shell.ui.show_shortcuts = true;
        }
    });
    kit::section(ui, &p, "Theme", |ui| theme_choice(ui, shell));
}

fn prefs_canvas(ui: &mut egui::Ui, shell: &mut Shell) {
    use crate::panels::rulers::RulerUnit;
    let p = shell.palette;
    kit::section(ui, &p, "Rulers and grid", |ui| {
        kit::form(ui, "prefs-view", |ui| {
            kit::row(ui, "Rulers", |ui| {
                ui.checkbox(&mut shell.show_rulers, "Show along the canvas edges");
            });
            kit::row(ui, "Units", |ui| {
                egui::ComboBox::from_id_salt("prefs-units")
                    .selected_text(shell.ruler_unit.label())
                    .width(80.0)
                    .show_ui(ui, |ui| {
                        for unit in [RulerUnit::Px, RulerUnit::Pt, RulerUnit::Mm, RulerUnit::In] {
                            ui.selectable_value(&mut shell.ruler_unit, unit, unit.label());
                        }
                    });
            });
            kit::row(ui, "Grid", |ui| {
                ui.checkbox(&mut shell.show_grid, "Show on the canvas");
            });
            kit::row(ui, "Spacing", |ui| {
                ui.add(
                    egui::DragValue::new(&mut shell.grid.spacing)
                        .range(1.0..=1000.0)
                        .speed(1.0)
                        .suffix(" px"),
                );
            });
            kit::row(ui, "Subdivisions", |ui| {
                ui.add(
                    egui::DragValue::new(&mut shell.grid.subdivisions)
                        .range(1..=20)
                        .speed(0.1),
                );
            });
        });
    });
    kit::section(ui, &p, "Snapping", |ui| {
        let s = &mut shell.snap;
        ui.checkbox(&mut s.pixels, "Snap to pixels");
        ui.checkbox(&mut s.grid, "Snap to grid");
        ui.checkbox(&mut s.guides, "Snap to guides");
        ui.checkbox(&mut s.bounds, "Snap to canvas bounds");
        ui.checkbox(&mut s.shapes, "Snap to other shapes");
        ui.add_space(4.0);
        kit::form(ui, "prefs-snap", |ui| {
            kit::row(ui, "Angle step (Shift)", |ui| {
                ui.add(
                    egui::DragValue::new(&mut s.angle_step)
                        .range(1.0..=90.0)
                        .suffix("°")
                        .speed(1.0),
                );
            });
        });
    });
    kit::section(ui, &p, "Onion skin", |ui| {
        let o = &mut shell.onion;
        ui.checkbox(&mut o.on, "Show onion skin");
        kit::form(ui, "prefs-onion", |ui| {
            kit::row(ui, "Frames before", |ui| {
                ui.add(egui::DragValue::new(&mut o.before).range(0..=10));
            });
            kit::row(ui, "Frames after", |ui| {
                ui.add(egui::DragValue::new(&mut o.after).range(0..=10));
            });
        });
        ui.checkbox(&mut o.keys, "Drawings and keys only")
            .on_hover_text("Show the previous and next drawings, however long they are held");
        ui.checkbox(&mut o.all_layers, "All layers")
            .on_hover_text("Otherwise only the selected layers");
    });
}

/// What makes MP4 here, and the FFmpeg that adds the other video formats.
fn video_formats(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let tools = &mut shell.video;
    let muted = |text: String| RichText::new(text).color(p.text_muted);
    if tools.looking() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(muted("Looking for video encoders…".into()));
        });
        return;
    }
    ui.label(if tools.system_h264 {
        format!(
            "MP4 (H.264) is made by {}'s built-in encoder. GIF, APNG, AVI, AV1 and picture sequences \
             are made by Fanimate.",
            crate::formats::system_name()
        )
    } else {
        "GIF, APNG, AVI, AV1 and picture sequences are made by Fanimate. MP4 (H.264) needs FFmpeg on \
         this system."
            .into()
    });
    match (&tools.ffmpeg, &tools.ffmpeg_problem) {
        (Some(f), _) => {
            ui.label(format!(
                "FFmpeg {} adds {}.",
                f.version,
                list(&tools.ffmpeg_adds())
            ));
            let path = f.path.display().to_string();
            ui.add(egui::Label::new(muted(path.clone()).size(11.0)).truncate())
                .on_hover_text(path);
        }
        (None, Some(problem)) => {
            ui.label(RichText::new(format!("FFmpeg could not be used: {problem}")).color(p.error));
        }
        (None, None) => {
            ui.label(muted(
                "FFmpeg was not found. It is free, and adds H.265, WebM, ProRes, DNxHR and animated WebP."
                    .into(),
            ));
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Choose FFmpeg…").clicked() {
            let mut dialog = rfd::FileDialog::new().set_title("Choose FFmpeg");
            if cfg!(windows) {
                dialog = dialog.add_filter("FFmpeg", &["exe"]);
            }
            if let Some(path) = dialog.pick_file() {
                tools.chosen = Some(path);
                tools.look();
            }
        }
        if ui
            .button("Look again")
            .on_hover_text("After installing FFmpeg")
            .clicked()
        {
            tools.look();
        }
        if tools.chosen.is_some()
            && ui
                .button("Find automatically")
                .on_hover_text("Forget the chosen FFmpeg and look in the usual places")
                .clicked()
        {
            tools.chosen = None;
            tools.look();
        }
    });
    crate::links::link(ui, "How to get FFmpeg", crate::formats::HOW_TO_GET_FFMPEG);
}

/// "a, b and c".
fn list(items: &[&str]) -> String {
    match items {
        [] => "nothing Fanimate uses".into(),
        [one] => (*one).into(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn shortcuts(ctx: &egui::Context, shell: &mut Shell) {
    const MENUS: [&str; 6] = ["File", "Edit", "Layer", "Animation", "View", "Tools"];
    let p = shell.palette;
    let closed = kit::modal(
        ctx,
        &p,
        "shortcuts",
        "Keyboard Shortcuts",
        vec2(560.0, 500.0),
        Backdrop::Closes,
        |ui| {
            ui.horizontal(|ui| {
                for kind in KeymapKind::ALL {
                    if ui
                        .selectable_label(shell.keymap.kind == kind, kind.label())
                        .clicked()
                    {
                        shell.set_keymap(kind);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut shell.ui.shortcut_filter)
                            .hint_text("Search shortcuts…")
                            .desired_width(180.0),
                    );
                });
            });
            ui.add_space(6.0);
            let query = shell.ui.shortcut_filter.trim().to_lowercase();
            let height = ui.available_height();
            egui::ScrollArea::vertical()
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut any = false;
                    for menu in MENUS {
                        let rows: Vec<(&str, String)> = Action::all()
                            .into_iter()
                            .filter(|a| a.category() == menu)
                            .map(|a| (a.label(), shell.keymap.shortcut_text(a)))
                            .filter(|(label, keys)| {
                                !keys.is_empty()
                                    && (query.is_empty()
                                        || label.to_lowercase().contains(&query)
                                        || keys.to_lowercase().contains(&query))
                            })
                            .collect();
                        if rows.is_empty() {
                            continue;
                        }
                        any = true;
                        ui.label(RichText::new(menu).size(11.5).strong().color(p.text_muted));
                        egui::Grid::new(("shortcut-grid", menu))
                            .num_columns(2)
                            .min_col_width(380.0)
                            .spacing([10.0, 5.0])
                            .show(ui, |ui| {
                                for (label, keys) in rows {
                                    ui.label(label);
                                    kit::chord(ui, &p, &keys);
                                    ui.end_row();
                                }
                            });
                        ui.add_space(8.0);
                    }
                    if !any {
                        ui.label(RichText::new("No shortcut matches.").color(p.text_muted));
                    }
                });
        },
    );
    if closed {
        shell.ui.show_shortcuts = false;
    }
}

fn about(ctx: &egui::Context, shell: &mut Shell) {
    let p = shell.palette;
    let closed = kit::modal(
        ctx,
        &p,
        "about",
        "About Fanimate",
        vec2(380.0, 270.0),
        Backdrop::Closes,
        |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new((shell.icon.id(), vec2(48.0, 48.0))));
                ui.vertical(|ui| {
                    ui.label(RichText::new("Fanimate").size(20.0).strong());
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label(RichText::new("Free to animate.").color(p.accent_strong));
                });
            });
            ui.add_space(8.0);
            kit::section(ui, &p, "About", |ui| {
                ui.label("Free, open-source 2D character animation.");
                ui.label(RichText::new("Licensed under MIT or Apache 2.0.").color(p.text_muted));
                ui.label(RichText::new(format!("Graphics: {}", shell.gpu_name)).color(p.text_muted));
                crate::links::link(
                    ui,
                    "github.com/toonittmedia/fanimate",
                    "https://github.com/toonittmedia/fanimate",
                );
            });
            kit::section(ui, &p, "Ships with", |ui| {
                ui.label(RichText::new("Rhubarb Lip Sync for lip sync (MIT).").color(p.text_muted));
                ui.label(RichText::new("Inter font (OFL 1.1) and Lucide icons (ISC).").color(p.text_muted));
            });
        },
    );
    if closed {
        shell.ui.show_about = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_read_naturally() {
        assert_eq!(list(&["H.265"]), "H.265");
        assert_eq!(
            list(&["H.265", "WebM", "ProRes 4444"]),
            "H.265, WebM and ProRes 4444"
        );
    }
}
