//! The folders Fanimate has saved or exported into lately, for the folder
//! drop-down in Save As and Export Video: pick one of those, and only fall
//! back to the system's folder chooser (Browse…) for somewhere new.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use serde::{Deserialize, Serialize};

/// How many folders are kept.
const KEPT: usize = 10;

/// Recent folders, most recent first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Places(Vec<PathBuf>);

impl Places {
    /// Remember `dir` as the most recent folder.
    pub fn remember(&mut self, dir: &Path) {
        if dir.as_os_str().is_empty() {
            return;
        }
        self.0.retain(|p| p != dir);
        self.0.insert(0, dir.to_path_buf());
        self.0.truncate(KEPT);
    }

    /// Remember the folder a file was written to.
    pub fn remember_file(&mut self, file: &Path) {
        if let Some(dir) = file.parent() {
            self.remember(dir);
        }
    }

    /// The folders that are still there, most recent first.
    pub fn existing(&self) -> Vec<&Path> {
        self.0
            .iter()
            .map(PathBuf::as_path)
            .filter(|p| p.is_dir())
            .collect()
    }

    /// The most recent folder that is still there.
    pub fn latest(&self) -> Option<&Path> {
        self.existing().into_iter().next()
    }
}

/// A folder's own name for the drop-down, with the folder it is in when
/// that tells two apart ("Videos", "Videos (D:\Work)").
fn label(dir: &Path, all: &[&Path]) -> String {
    let name = dir
        .file_name()
        .map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
    let twins = all
        .iter()
        .filter(|o| o.file_name() == dir.file_name() && **o != dir)
        .count();
    match (twins, dir.parent()) {
        (0, _) | (_, None) => name,
        (_, Some(parent)) => format!("{name} ({})", parent.display()),
    }
}

/// A drop-down of folders: the one chosen, the recent places, then Browse….
/// Returns whether `dir` changed. `also` is offered first (the project's own
/// folder, for example).
pub fn folder_picker(
    ui: &mut egui::Ui,
    id: &str,
    dir: &mut PathBuf,
    places: &Places,
    also: Option<&Path>,
    title: &str,
) -> bool {
    // the folder in use is always on the list, even when it is not a place
    // used lately (the default one, the first time)
    let current = dir.is_dir().then_some(dir.as_path());
    let mut offered: Vec<&Path> = Vec::new();
    for p in also.into_iter().chain(places.existing()).chain(current) {
        if !offered.contains(&p) {
            offered.push(p);
        }
    }
    let shown = if dir.as_os_str().is_empty() {
        "Choose a folder".to_owned()
    } else {
        label(dir, &offered)
    };
    let mut chosen: Option<PathBuf> = None;
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(ui.available_width().min(300.0))
        .show_ui(ui, |ui| {
            for p in &offered {
                let text = RichText::new(label(p, &offered));
                if ui
                    .selectable_label(*dir == *p, text)
                    .on_hover_text(p.display().to_string())
                    .clicked()
                {
                    chosen = Some(p.to_path_buf());
                }
            }
            if !offered.is_empty() {
                ui.separator();
            }
            if ui.selectable_label(false, "Browse…").clicked() {
                let mut dialog = rfd::FileDialog::new().set_title(title);
                if dir.is_dir() {
                    dialog = dialog.set_directory(&*dir);
                }
                chosen = dialog.pick_folder();
            }
        })
        .response
        .on_hover_text(dir.display().to_string());
    match chosen {
        Some(c) if c != *dir => {
            *dir = c;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fanimate-places-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_newest_folder_comes_first_and_none_repeats() {
        let (a, b) = (dir("a"), dir("b"));
        let mut places = Places::default();
        places.remember(&a);
        places.remember(&b);
        places.remember(&a);
        assert_eq!(places.existing(), [a.as_path(), b.as_path()]);
        assert_eq!(places.latest(), Some(a.as_path()));
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(b);
    }

    #[test]
    fn a_saved_file_remembers_its_folder() {
        let a = dir("file");
        let mut places = Places::default();
        places.remember_file(&a.join("walk.fnm"));
        assert_eq!(places.latest(), Some(a.as_path()));
        let _ = std::fs::remove_dir_all(a);
    }

    #[test]
    fn folders_that_are_gone_are_left_out() {
        let (a, gone) = (
            dir("here"),
            std::env::temp_dir().join("fanimate-places-no-such-folder"),
        );
        let mut places = Places::default();
        places.remember(&gone);
        places.remember(&a);
        places.remember(&gone);
        assert_eq!(places.existing(), [a.as_path()]);
        assert_eq!(places.latest(), Some(a.as_path()));
        let _ = std::fs::remove_dir_all(a);
    }

    #[test]
    fn only_a_few_are_kept() {
        let mut places = Places::default();
        for i in 0..KEPT + 5 {
            places.remember(Path::new(&format!("/work/{i}")));
        }
        assert_eq!(places.0.len(), KEPT);
        assert_eq!(places.0[0], Path::new(&format!("/work/{}", KEPT + 4)));
    }

    #[test]
    fn two_folders_with_one_name_say_where_they_are() {
        let (one, two) = (Path::new("/a/Videos"), Path::new("/b/Videos"));
        let all = [one, two];
        assert_eq!(label(one, &all), "Videos (/a)");
        assert_eq!(
            label(Path::new("/a/Scenes"), &[Path::new("/a/Scenes"), one]),
            "Scenes"
        );
    }

    #[test]
    fn they_are_kept_as_a_plain_list() {
        let mut places = Places::default();
        places.remember(Path::new("/work"));
        let json = serde_json::to_string(&places).unwrap();
        assert_eq!(json, "[\"/work\"]");
        assert_eq!(serde_json::from_str::<Places>(&json).unwrap(), places);
        // older settings have none
        assert_eq!(serde_json::from_str::<Places>("[]").unwrap(), Places::default());
    }
}
