//! The Assistant column: a conversation about the open project, with what the
//! model changed shown as cards that can be undone as one step.
//!
//! Setup (a provider, a key, permission to send) is in the panel, not a
//! dialog: it shows by itself until it is done, and the gear brings it back.
//! The message box runs on `Enter`; `Shift+Enter` is a new line. What is sent
//! with a message (the project's size, the playhead, the selected layers) is
//! written under the box, so nothing leaves without being visible.

use eframe::egui::{self, Align, Id, Key, Layout, Margin, Modifiers, RichText, Stroke};

use crate::app::Shell;
use crate::assistant::{self, Entry, ProviderKind, Source};
use crate::icons::{self, Icon};
use crate::kit;
use crate::menus;

const MARGIN: i8 = 10;

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    header(ui, shell);
    if let Some(why) = assistant::unavailable(shell) {
        egui::Frame::NONE
            .inner_margin(Margin::same(MARGIN))
            .show(ui, |ui| {
                kit::note(ui, &p, why);
                if shell.ai.off && ui.button("Turn on AI features").clicked() {
                    shell.ai.off = false;
                }
            });
        return;
    }
    // the message box is the bottom of the column; the transcript fills what is left
    egui::Panel::bottom(Id::new("assistant-composer"))
        .resizable(false)
        .frame(egui::Frame::NONE.fill(p.panel).inner_margin(Margin::same(MARGIN)))
        .show(ui, |ui| composer(ui, shell));
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.panel))
        .show(ui, |ui| body(ui, shell));
}

fn header(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    egui::Frame::NONE
        .inner_margin(Margin::symmetric(MARGIN, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(icons::image(ui.ctx(), Icon::Assistant, 16.0, p.accent));
                ui.label(RichText::new("Assistant").strong().size(13.0).color(p.accent));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icons::button(ui, Icon::Close, 22.0, false, false)
                        .on_hover_text("Close the Assistant")
                        .clicked()
                    {
                        shell.ui.toggle_assistant = true;
                    }
                    if icons::button(ui, Icon::Settings, 22.0, shell.assistant.settings_open, false)
                        .on_hover_text("Provider, key and other programs")
                        .clicked()
                    {
                        shell.assistant.settings_open = !shell.assistant.settings_open;
                    }
                    if !shell.assistant.entries.is_empty()
                        && ui
                            .small_button("New chat")
                            .on_hover_text("Start the conversation over")
                            .clicked()
                    {
                        shell.assistant.clear();
                    }
                });
            });
        });
}

/// Whether the person still has to choose a key or allow sending.
fn needs_setup(shell: &Shell) -> bool {
    let ai = &shell.ai;
    !ai.is_allowed() || (!ai.is_local() && shell.keys.get(ai.provider).is_none())
}

fn body(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            egui::Frame::NONE.inner_margin(Margin::symmetric(MARGIN, 0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if shell.assistant.settings_open || needs_setup(shell) {
                    setup(ui, shell);
                }
                if shell.assistant.entries.is_empty() && !needs_setup(shell) {
                    kit::note(
                        ui,
                        &p,
                        "Ask for an animation or an edit, for example \"fade the title in over 12 frames\". \
                         Every change is one undo step, and what the Assistant adds is labelled in the file.",
                    );
                }
                transcript(ui, shell);
                if let Some(doing) = shell.assistant.working().map(str::to_owned) {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(format!("{doing}…")).color(p.text_muted));
                    });
                }
                ui.add_space(8.0);
            });
        });
}

fn transcript(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    for index in 0..shell.assistant.entries.len() {
        ui.add_space(8.0);
        match &shell.assistant.entries[index] {
            Entry::You(text) => {
                let text = text.clone();
                let response = egui::Frame::NONE
                    .fill(p.raised)
                    .corner_radius(8)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(&text);
                    })
                    .response;
                copy_menu(&response, text);
            }
            Entry::Reply(text) => {
                let text = text.clone();
                let response = ui.label(&text);
                copy_menu(&response, text);
            }
            Entry::Notice(text) => {
                ui.label(RichText::new(text).italics().color(p.text_muted));
            }
            Entry::Problem(text) => {
                ui.label(RichText::new(text).color(p.error));
            }
            Entry::Changes(_) => card(ui, shell, index),
        }
    }
}

fn copy_menu(response: &egui::Response, text: String) {
    menus::context_menu(response, |ui| {
        if ui.button("Copy").clicked() {
            ui.ctx().copy_text(text);
            ui.close();
        }
    });
}

/// What was changed, with Undo and Show.
fn card(ui: &mut egui::Ui, shell: &mut Shell, index: usize) {
    let p = shell.palette;
    let Entry::Changes(changes) = &shell.assistant.entries[index] else {
        return;
    };
    let (who, lines, undone) = (changes.who, changes.lines.clone(), changes.undone);
    let undoable = assistant::card_undoable(shell, changes);
    let mut undo = false;
    let mut show = false;
    let response = egui::Frame::NONE
        .fill(p.raised)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(8)
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(who).size(11.0).color(p.text_muted));
            for line in &lines {
                ui.label(line);
            }
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if undone {
                    ui.label(RichText::new("Undone").color(p.text_muted));
                } else {
                    undo = ui
                        .add_enabled(undoable, egui::Button::new("Undo").small())
                        .on_hover_text(if undoable {
                            "Take back everything in this card"
                        } else {
                            "Something else has changed since, so use Edit > Undo or the History panel"
                        })
                        .clicked();
                    show = ui
                        .small_button("Show")
                        .on_hover_text("Select what it changed")
                        .clicked();
                }
            });
        })
        .response;
    let text = lines.join("\n");
    menus::context_menu(&response, |ui| {
        if ui
            .add_enabled(undoable, egui::Button::new("Undo these changes"))
            .clicked()
        {
            undo = true;
            ui.close();
        }
        if !undone && ui.button("Show what changed").clicked() {
            show = true;
            ui.close();
        }
        if ui.button("Copy").clicked() {
            ui.ctx().copy_text(text);
            ui.close();
        }
    });
    if undo {
        assistant::undo_card(shell, index);
    }
    if show {
        assistant::show_card(shell, index);
    }
}

/// Provider, key, permission and the other programs. Shows by itself until
/// there is a key and permission.
fn setup(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let title = if needs_setup(shell) {
        "Get started"
    } else {
        "Settings"
    };
    kit::section(ui, &p, title, |ui| {
        provider_rows(ui, shell);
        ui.add_space(6.0);
        permission_rows(ui, shell);
        ui.add_space(8.0);
        programs_rows(ui, shell);
        ui.add_space(8.0);
        if ui
            .checkbox(&mut shell.ai.off, "Turn off all AI features")
            .on_hover_text("Hides the Assistant and Ctrl+K, and closes the link for other programs")
            .changed()
            && shell.ai.off
        {
            shell.ui.toggle_assistant = shell.ui.assistant_shown;
        }
    });
}

fn provider_rows(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    ui.horizontal(|ui| {
        ui.label("Provider");
        egui::ComboBox::from_id_salt("assistant-provider")
            .selected_text(shell.ai.provider.label())
            .show_ui(ui, |ui| {
                for kind in ProviderKind::ALL {
                    ui.selectable_value(&mut shell.ai.provider, kind, kind.label());
                }
            });
    });
    if shell.ai.provider == ProviderKind::Custom {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut shell.ai.custom_name).desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            ui.label("Address");
            ui.add(egui::TextEdit::singleline(&mut shell.ai.custom_url).desired_width(f32::INFINITY));
        });
        kit::note(
            ui,
            &p,
            "Any service or local server that speaks the OpenAI chat API, such as Ollama or LM Studio. \
             An address on this computer needs no key.",
        );
    }
    ui.horizontal(|ui| {
        ui.label("Model");
        let kind = shell.ai.provider;
        // right to left: the list button takes its own width, and the box gets exactly what is
        // left. (A box sized as "the rest minus a guess" overflowed by one item spacing each
        // frame, and the column grew to its widest size by itself.)
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !kind.suggestions().is_empty() {
                egui::ComboBox::from_id_salt("assistant-model")
                    .selected_text("")
                    .width(20.0)
                    .show_ui(ui, |ui| {
                        for suggestion in kind.suggestions() {
                            if ui
                                .selectable_label(shell.ai.model() == *suggestion, *suggestion)
                                .clicked()
                            {
                                *shell.ai.model_mut() = (*suggestion).to_owned();
                            }
                        }
                    });
            }
            ui.add(egui::TextEdit::singleline(shell.ai.model_mut()).desired_width(ui.available_width()));
        });
    });
    key_rows(ui, shell);
}

fn key_rows(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let kind = shell.ai.provider;
    if shell.ai.is_local() {
        return;
    }
    match shell.keys.source(kind) {
        Source::Variable => {
            kit::note(ui, &p, format!("Using the key in {}.", kind.key_variable()));
        }
        Source::Saved => {
            ui.horizontal(|ui| {
                ui.label("A key is saved on this computer.");
                if ui.small_button("Forget").clicked() {
                    shell.assistant.key_message = Some(match shell.keys.forget(kind) {
                        Ok(()) => "Key forgotten.".to_owned(),
                        Err(e) => format!("Could not forget the key: {e}"),
                    });
                }
            });
        }
        Source::None => {
            ui.horizontal(|ui| {
                ui.label("Key");
                // right to left, as the Model row: Save keeps its width, the box takes the rest
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let draft = shell.assistant.key_draft.trim().to_owned();
                    if ui
                        .add_enabled(!draft.is_empty(), kit::primary(&p, "Save"))
                        .clicked()
                    {
                        shell.assistant.key_message = Some(match shell.keys.save(kind, &draft) {
                            Ok(()) => "Key saved.".to_owned(),
                            Err(e) => format!("Could not save the key: {e}"),
                        });
                        shell.assistant.key_draft.clear();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut shell.assistant.key_draft)
                            .password(true)
                            .hint_text("Paste your key")
                            .desired_width(ui.available_width()),
                    );
                });
            });
            if let Some(page) = kind.key_page() {
                ui.horizontal(|ui| {
                    if ui.link("Get a key").clicked() {
                        crate::links::open(page);
                    }
                    kit::note(ui, &p, format!("or set {}", kind.key_variable()));
                });
            }
        }
    }
    if let Some(message) = &shell.assistant.key_message {
        kit::note(ui, &p, message.clone());
    }
}

fn permission_rows(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let destination = shell.ai.destination();
    if shell.ai.is_allowed() {
        ui.horizontal(|ui| {
            kit::note(ui, &p, format!("Sending to {destination} is allowed."));
            if ui.small_button("Stop allowing").clicked() {
                let id = shell.ai.target_id();
                shell.ai.allowed.retain(|a| *a != id);
            }
        });
        return;
    }
    kit::note(
        ui,
        &p,
        format!(
            "Each message sends your words, the project's size, the playhead and the names of the selected \
             layers to {destination}, and what the Assistant asks to look at. Nothing is sent until you press Send."
        ),
    );
    if ui
        .add(kit::primary(&p, &format!("Allow sending to {destination}")))
        .clicked()
    {
        shell.ai.allow();
    }
}

fn programs_rows(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    ui.checkbox(&mut shell.ai.programs, "Let other programs control Fanimate")
        .on_hover_text("Claude Code, Codex and Antigravity can work on the open project");
    if let Some(error) = &shell.assistant.host_error {
        ui.label(RichText::new(error).color(p.error));
    }
    if !shell.ai.programs {
        return;
    }
    if shell.assistant.linked() {
        kit::note(ui, &p, "The link is on. It listens on this computer only.");
    }
    kit::note(
        ui,
        &p,
        "They work on the project in front, can change it but not save or close it, and what they add is \
         labelled in the file. Run one of these once, in a terminal, to connect it:",
    );
    let cli =
        assistant::cli_path().map_or_else(|| "fanimate-cli".to_owned(), |c| c.to_string_lossy().into_owned());
    for (agent, label) in [
        ("claude", "Claude Code"),
        ("codex", "Codex"),
        ("agy", "Antigravity"),
    ] {
        ui.horizontal(|ui| {
            ui.label(label);
            if ui.small_button("Copy command").clicked() {
                ui.ctx().copy_text(assistant::connect_command(agent, &cli));
            }
        });
    }
}

fn composer(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let id = Id::new("assistant-input");
    let running = shell.assistant.running();
    let ctx = ui.ctx().clone();

    // Enter sends, Shift+Enter is a new line: take the key before the box sees it
    let focused = ui.memory(|m| m.has_focus(id));
    let mut send = focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));

    let editable = !running;
    let response = ui.add_enabled(
        editable,
        egui::TextEdit::multiline(&mut shell.assistant.input)
            .id(id)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text("Ask for an animation or an edit"),
    );
    if std::mem::take(&mut shell.assistant.focus_input) {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let _ = &response;

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        kit::note(ui, &p, assistant::context(shell).chip());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if running {
                if icons::button(ui, Icon::Stop, 26.0, false, false)
                    .on_hover_text("Stop. Changes already made stay, and can be undone")
                    .clicked()
                {
                    shell.assistant.stop();
                }
            } else {
                let ready = !shell.assistant.input.trim().is_empty();
                let clicked = ui
                    .add_enabled(
                        ready,
                        egui::Button::image(icons::image(&ctx, Icon::Send, 14.0, p.on_button)).fill(p.button),
                    )
                    .on_hover_text("Send (Enter)")
                    .clicked();
                send |= clicked;
            }
        });
    });
    if send && !running {
        assistant::send(&ctx, shell);
    }
}
