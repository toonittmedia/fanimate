//! The dockable panels and the status bar.

mod actions;
pub mod canvas;
pub mod handles;
mod history;
mod layers;
mod properties;
mod selbar;
mod style;
pub mod timeline;
mod tools;
mod xsheet;

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use fanimate_editor::StatusKind;
use serde::{Deserialize, Serialize};

use crate::app::Shell;
use crate::icons::{self, Icon};

const STATUS_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Tools,
    Canvas,
    Timeline,
    Layers,
    Properties,
    Style,
    Actions,
    History,
    XSheet,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Self::Canvas,
        Self::Tools,
        Self::Layers,
        Self::Timeline,
        Self::Properties,
        Self::Style,
        Self::Actions,
        Self::History,
        Self::XSheet,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Tools => "Tools",
            Self::Canvas => "Canvas",
            Self::Timeline => "Timeline",
            Self::Layers => "Layers",
            Self::Properties => "Properties",
            Self::Style => "Style",
            Self::Actions => "Actions",
            Self::History => "History",
            Self::XSheet => "X-Sheet",
        }
    }
}

pub struct TabViewer<'a> {
    pub shell: &'a mut Shell,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(("fanimate-tab", *tab))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        let shell = &mut *self.shell;
        match tab {
            Tab::Tools => tools::show(ui, shell),
            Tab::Canvas => canvas::show(ui, shell),
            Tab::Timeline => timeline::show(ui, shell),
            Tab::Layers => layers::show(ui, shell),
            Tab::Properties => properties::show(ui, shell),
            Tab::Style => style::show(ui, shell),
            Tab::History => history::show(ui, shell),
            Tab::XSheet => xsheet::show(ui, shell),
            Tab::Actions => actions::show(ui, shell),
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        *tab != Tab::Canvas
    }

    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        match tab {
            Tab::Canvas | Tab::Timeline | Tab::Layers | Tab::History | Tab::XSheet => [false, false],
            _ => [false, true],
        }
    }
}

/// The status bar's update button, while an update waits.
fn update_label(shell: &Shell) -> Option<String> {
    use crate::update::State;
    match shell.updater.state() {
        State::Available(r) if shell.updates.skipped.as_deref() != Some(&r.version.to_string()) => {
            Some(format!("Update to {}", r.version))
        }
        State::Downloading(r) => Some(format!("Downloading {}", r.version)),
        State::Ready(r, _) => Some(format!("Restart to update to {}", r.version)),
        _ => None,
    }
}

/// Bottom bar: messages or the tool hint on the left, state on the right.
pub fn status_bar(ui: &mut egui::Ui, shell: &mut Shell) {
    // expire messages after a few seconds
    let current = shell.editor.status.as_ref().map(|(_, m)| m.clone());
    match (&current, &shell.ui.status_since) {
        (Some(m), Some((seen, at))) if m == seen => {
            if at.elapsed() > STATUS_TIMEOUT {
                shell.editor.status = None;
                shell.ui.status_since = None;
            }
        }
        (Some(m), _) => shell.ui.status_since = Some((m.clone(), Instant::now())),
        (None, _) => shell.ui.status_since = None,
    }
    if shell.editor.status.is_some() {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }

    let p = shell.palette;
    egui::Panel::bottom("status")
        .exact_size(24.0)
        .frame(
            egui::Frame::NONE
                .fill(p.bg)
                .inner_margin(egui::Margin::symmetric(10, 0)),
        )
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                // the Home tab shows no document, so no document state
                let home = shell.docs.home_front();
                let (text, color): (String, egui::Color32) = match &shell.editor.status {
                    Some((StatusKind::Error, m)) => (m.clone(), p.error),
                    Some((StatusKind::Warning, m)) => (m.clone(), p.keyframe),
                    Some((StatusKind::Success, m)) => (m.clone(), p.success),
                    Some((StatusKind::Info, m)) => (m.clone(), p.text),
                    None if home => (String::new(), p.text_muted),
                    None => (
                        format!("{}: {}", shell.editor.tool.label(), shell.editor.tool.hint()),
                        p.text_muted,
                    ),
                };
                let hover = text.clone();
                // state on the right first, so a long message is cut short
                // with an ellipsis instead of running under it
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let muted = |s: String| RichText::new(s).size(12.0).color(p.text_muted);
                    if let Some(label) = update_label(shell) {
                        let button = egui::Button::new(RichText::new(label).size(12.0).color(p.on_button))
                            .fill(p.button);
                        if ui.add(button).clicked() {
                            shell.ui.show_update = true;
                        }
                        ui.separator();
                    }
                    ui.label(muted(shell.gpu_name.clone()));
                    ui.separator();
                    let keyboard = icons::image(ui.ctx(), Icon::Keyboard, 14.0, p.text_muted);
                    let keymap =
                        egui::Button::image_and_text(keyboard, muted(shell.keymap.kind.label().into()))
                            .frame_when_inactive(false);
                    if ui
                        .add(keymap)
                        .on_hover_text("Keyboard shortcuts. Click to change.")
                        .clicked()
                    {
                        shell.ui.show_preferences = true;
                    }
                    ui.separator();
                    if !home {
                        ui.label(muted(format!(
                            "{:.0}%",
                            shell.view.zoom * ui.ctx().pixels_per_point() * 100.0
                        )));
                        ui.separator();
                    }
                    if let Some((name, fraction)) = shell.renders.active.clone() {
                        let bar = egui::ProgressBar::new(fraction)
                            .desired_width(70.0)
                            .desired_height(6.0)
                            .fill(p.accent);
                        let r = ui
                            .add(bar)
                            .on_hover_text(format!("Rendering {name}. Click to see the render queue."))
                            .interact(egui::Sense::click());
                        if r.clicked() {
                            shell.pending.push(fanimate_editor::Action::RenderQueue);
                        }
                        ui.label(muted(format!("Rendering {:.0}%", fraction * 100.0)));
                        ui.separator();
                    }
                    if !home {
                        let doc = &shell.editor.doc;
                        ui.label(muted(format!(
                            "Frame {} of {}",
                            shell.editor.current_frame(),
                            doc.end
                        )));
                        ui.separator();
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(color)).truncate())
                            .on_hover_text(&hover);
                    });
                });
            });
        });
}
