//! Save As: pick the folder from the places used lately, type the name, and
//! only fall back to the system's chooser (Browse…) for somewhere new.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, vec2};

use crate::app::Shell;
use crate::kit::{self, Backdrop};

/// The Save As dialog's choices.
pub struct SaveForm {
    dir: PathBuf,
    /// The file's name without `.fnm`.
    name: String,
    /// The name field has had focus; it asks once, when the dialog opens.
    focused: bool,
}

impl SaveForm {
    pub fn new(shell: &Shell) -> Self {
        let editor = &shell.editor;
        let dir = editor
            .path
            .as_deref()
            .and_then(Path::parent)
            .or_else(|| shell.places.latest())
            .map(Path::to_path_buf)
            .or_else(documents_folder)
            .unwrap_or_default();
        let name = editor
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(|| editor.doc.name.clone(), |s| s.to_string_lossy().into_owned());
        Self {
            dir,
            name: clean_name(&name),
            focused: false,
        }
    }

    /// The file this would write.
    fn path(&self) -> PathBuf {
        let name = self.name.trim();
        let typed = Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("fnm"));
        self.dir.join(if typed {
            name.to_owned()
        } else {
            format!("{name}.fnm")
        })
    }

    /// Why it cannot be saved yet.
    fn problem(&self) -> Option<&'static str> {
        let name = self.name.trim();
        if self.dir.as_os_str().is_empty() || !self.dir.is_dir() {
            Some("Choose a folder that exists to save into.")
        } else if name.is_empty() {
            Some("Give the file a name.")
        } else if clean_name(name) != name {
            Some("The name cannot hold any of these characters: < > : \" / \\ | ? *")
        } else {
            None
        }
    }
}

fn documents_folder() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    let docs = home.join("Documents");
    Some(if docs.is_dir() { docs } else { home })
}

/// A project name usable as a file name.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        "Untitled".into()
    } else {
        trimmed.into()
    }
}

/// The Save As dialog, while open.
pub fn window(ctx: &egui::Context, shell: &mut Shell) {
    let Some(mut form) = shell.ui.save_as.take() else {
        return;
    };
    let p = shell.palette;
    let mut save_it = false;
    let mut cancel = false;
    let closed = kit::modal(
        ctx,
        &p,
        "save-as",
        "Save As",
        vec2(460.0, 200.0),
        Backdrop::Ignored,
        |ui| {
            kit::form(ui, "save-as-form", |ui| {
                kit::row(ui, "Folder", |ui| {
                    crate::places::folder_picker(
                        ui,
                        "save-as-folder",
                        &mut form.dir,
                        &shell.places,
                        shell.editor.path.as_deref().and_then(Path::parent),
                        "Save into",
                    );
                });
                kit::row(ui, "File name", |ui| {
                    let edit = ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(240.0));
                    if !form.focused {
                        edit.request_focus();
                        form.focused = true;
                    }
                    ui.label(RichText::new(".fnm").color(p.text_muted));
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        save_it = true;
                    }
                });
            });
            ui.add_space(6.0);
            let problem = form.problem();
            let replaces =
                form.path().exists() && form.path() != shell.editor.path.clone().unwrap_or_default();
            match problem {
                Some(text) => {
                    ui.label(RichText::new(text).size(11.5).color(p.error));
                }
                None if replaces => kit::note(
                    ui,
                    &p,
                    "A file with this name is already there and will be replaced.",
                ),
                None => {}
            }
            kit::footer(ui, |ui| {
                let label = if replaces { "Replace" } else { "Save" };
                if ui
                    .add_enabled(problem.is_none(), kit::primary(&p, label))
                    .clicked()
                {
                    save_it = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        },
    );
    if save_it && form.problem().is_none() {
        let path = form.path();
        match shell.editor.save_to(&path) {
            Ok(()) => {
                shell.remember(&path);
                shell.ui.thumbs.after_save(&path, &shell.editor.doc);
                // saved: a crash now would lose nothing
                crate::recovery::record(shell);
            }
            Err(e) => shell
                .editor
                .set_status(fanimate_editor::StatusKind::Error, format!("Could not save: {e}")),
        }
        return;
    }
    if !closed && !cancel {
        shell.ui.save_as = Some(form);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(dir: &str, name: &str) -> SaveForm {
        SaveForm {
            dir: PathBuf::from(dir),
            name: name.into(),
            focused: true,
        }
    }

    #[test]
    fn the_extension_is_added_once() {
        assert_eq!(form("/w", "walk").path(), Path::new("/w").join("walk.fnm"));
        assert_eq!(form("/w", "walk.fnm").path(), Path::new("/w").join("walk.fnm"));
        assert_eq!(
            form("/w", "take 2.5").path(),
            Path::new("/w").join("take 2.5.fnm")
        );
    }

    #[test]
    fn names_are_checked_before_saving() {
        let dir = std::env::temp_dir();
        let d = dir.to_string_lossy().into_owned();
        assert_eq!(form(&d, "walk").problem(), None);
        assert!(form(&d, "").problem().is_some());
        assert!(form(&d, "a/b").problem().is_some());
        assert!(form("", "walk").problem().is_some());
        assert!(form("/no/such/folder/anywhere", "walk").problem().is_some());
    }

    #[test]
    fn untitled_documents_get_a_usable_name() {
        assert_eq!(clean_name("Walk: take 2?"), "Walk_ take 2_");
        assert_eq!(clean_name("  "), "Untitled");
    }
}
