//! What's New: the release notes, shown in the app.
//!
//! The notes are the files in `docs/releases/`, embedded when Fanimate is
//! built (`build.rs`), so each version carries its own and they work offline.
//! They open once after an update, and from Help > What's New.

use eframe::egui::{self, RichText, vec2};

use crate::app::Shell;
use crate::kit::{self, Backdrop};
use crate::markdown;

include!(concat!(env!("OUT_DIR"), "/release_notes.rs"));

/// The notes of `version` (`0.2.2`), if the app carries them.
pub fn notes_for(version: &str) -> Option<&'static str> {
    NOTES.iter().find(|(v, _)| *v == version).map(|(_, text)| *text)
}

/// The notes to open with: this version's, else the newest there are (a
/// development build has none of its own).
pub fn current() -> Option<(&'static str, &'static str)> {
    let this = env!("CARGO_PKG_VERSION");
    NOTES
        .iter()
        .find(|(v, _)| *v == this)
        .or_else(|| NOTES.first())
        .copied()
}

/// Whether an update has just been installed: Fanimate has run before, at
/// another version, and this version has notes to show. A first run shows the
/// welcome dialog instead.
pub fn show_after_update(last_seen: &str) -> bool {
    !last_seen.is_empty()
        && last_seen != env!("CARGO_PKG_VERSION")
        && notes_for(env!("CARGO_PKG_VERSION")).is_some()
}

/// The What's New dialog's state.
pub struct WhatsNew {
    version: &'static str,
    blocks: Vec<markdown::Block>,
}

impl WhatsNew {
    pub fn open(version: Option<&str>) -> Option<Self> {
        let (v, text) = match version {
            Some(v) => NOTES.iter().find(|(n, _)| *n == v).copied()?,
            None => current()?,
        };
        Some(Self {
            version: v,
            blocks: markdown::parse(text),
        })
    }
}

/// The dialog, while open.
pub fn window(ctx: &egui::Context, shell: &mut Shell) {
    let Some(mut open) = shell.ui.whats_new.take() else {
        return;
    };
    let p = shell.palette;
    let mut switch: Option<&'static str> = None;
    let title = format!("What's new in Fanimate {}", open.version);
    let closed = kit::modal(
        ctx,
        &p,
        "whats-new",
        &title,
        vec2(680.0, 540.0),
        Backdrop::Closes,
        |ui| {
            if NOTES.len() > 1 {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Release").color(p.text_muted));
                    egui::ComboBox::from_id_salt("whats-new-version")
                        .selected_text(open.version)
                        .show_ui(ui, |ui| {
                            for (v, _) in NOTES {
                                if ui.selectable_label(*v == open.version, *v).clicked() {
                                    switch = Some(v);
                                }
                            }
                        });
                });
                ui.add_space(4.0);
            }
            let height = ui.available_height();
            egui::ScrollArea::vertical()
                .id_salt(("whats-new", open.version))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width() - 8.0);
                    markdown::show(ui, &p, &open.blocks);
                });
        },
    );
    if let Some(v) = switch
        && let Some(next) = WhatsNew::open(Some(v))
    {
        open = next;
    }
    if !closed {
        shell.ui.whats_new = Some(open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_release_has_notes_newest_first() {
        assert!(!NOTES.is_empty());
        let versions: Vec<_> = NOTES
            .iter()
            .map(|(v, _)| crate::update::Version::parse(v).expect("a version"))
            .collect();
        assert!(versions.windows(2).all(|w| w[0] > w[1]), "{versions:?}");
        assert!(NOTES.iter().all(|(_, text)| !text.trim().is_empty()));
    }

    #[test]
    fn the_running_version_opens_its_own_notes_when_it_has_them() {
        let (version, _) = current().expect("there are notes");
        if notes_for(env!("CARGO_PKG_VERSION")).is_some() {
            assert_eq!(version, env!("CARGO_PKG_VERSION"));
        }
        assert!(WhatsNew::open(None).is_some());
        assert!(WhatsNew::open(Some("9.9.9")).is_none());
    }

    #[test]
    fn notes_open_after_an_update_but_not_on_a_first_run_or_the_same_version() {
        assert!(!show_after_update(""), "a first run shows the welcome dialog");
        assert!(!show_after_update(env!("CARGO_PKG_VERSION")));
        if notes_for(env!("CARGO_PKG_VERSION")).is_some() {
            assert!(show_after_update("0.0.1"));
        }
    }
}
