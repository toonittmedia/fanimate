//! My poses: the user's personal library of poses and actions, shown at the
//! bottom of the Actions panel. A pose is kept from a rig's action list
//! (right-click > Save to My Poses) and put on any bone layer by clicking it:
//! bones are paired by name, as in Copy to Rig. The library is a file in the
//! user's data folder (`poses.json`); the rules are in
//! `fanimate_core::poses` and the file text in `fanimate_editor::pose_library`.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use fanimate_core::LayerId;
use fanimate_core::poses::PoseLibrary;
use fanimate_editor::StatusKind;

use crate::app::Shell;

const FILE: &str = "poses.json";

fn path() -> Option<PathBuf> {
    eframe::storage_dir("Fanimate").map(|dir| dir.join(FILE))
}

/// The library on disk. A missing file is an empty library; a file that
/// cannot be read is left alone (and reported by the caller), so a damaged
/// library is never overwritten by an empty one.
pub fn load() -> (PoseLibrary, Option<String>) {
    let Some(path) = path() else {
        return (PoseLibrary::default(), None);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match fanimate_editor::pose_library::parse(&text) {
            Ok(library) => (library, None),
            Err(e) => (PoseLibrary::default(), Some(e)),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (PoseLibrary::default(), None),
        Err(e) => (
            PoseLibrary::default(),
            Some(format!("the pose library could not be read: {e}")),
        ),
    }
}

/// Write the library beside its place, then rename it over the old one.
fn save(shell: &mut Shell) {
    if shell.pose_library_unreadable {
        // the file on disk was damaged: keep it for the user to look at
        shell.editor.set_status(
            StatusKind::Warning,
            "The pose library file could not be read, so it was not overwritten",
        );
        return;
    }
    let Some(path) = path() else {
        return;
    };
    let text = fanimate_editor::pose_library::to_text(&shell.pose_library);
    let part = path.with_extension("json.part");
    let result = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&part, text))
        .and_then(|()| std::fs::rename(&part, &path));
    if let Err(e) = result {
        shell
            .editor
            .set_status(StatusKind::Error, format!("The pose library was not saved: {e}"));
    }
}

/// Keep action `index` of the rig `layer` in the library.
pub fn keep(shell: &mut Shell, layer: LayerId, index: usize) {
    let result = shell.editor.pose_for_library(layer, index).and_then(|pose| {
        shell
            .pose_library
            .add(pose)
            .map(|i| shell.pose_library.poses[i].name().to_owned())
    });
    match result {
        Ok(name) => {
            save(shell);
            shell
                .editor
                .set_status(StatusKind::Success, format!("Kept \"{name}\" in My Poses"));
        }
        Err(e) => shell.editor.set_status(StatusKind::Info, e),
    }
}

pub fn section(ui: &mut egui::Ui, shell: &mut Shell, layer: LayerId) {
    let p = shell.palette;
    ui.add_space(10.0);
    ui.separator();
    ui.label(RichText::new("My poses").strong());
    if shell.pose_library.poses.is_empty() {
        ui.label(
            RichText::new("Right-click a pose or action above and choose Save to My Poses. It is then here for every rig.")
                .color(p.text_muted),
        );
        return;
    }
    let mut add = None;
    let mut remove = None;
    for (i, pose) in shell.pose_library.poses.iter().enumerate() {
        let label = if pose.is_pose() {
            pose.name().to_owned()
        } else {
            format!("{} (action)", pose.name())
        };
        let response = ui
            .add(
                egui::Button::new(label)
                    .frame(false)
                    .min_size(egui::vec2(ui.available_width(), 24.0)),
            )
            .on_hover_text("Click to add it to the selected rig. Bones are matched by name.");
        if response.clicked() {
            add = Some(i);
        }
        crate::menus::context_menu(&response, |ui| {
            if ui.button("Add to This Rig").clicked() {
                add = Some(i);
                ui.close();
            }
            if ui.button("Remove from My Poses").clicked() {
                remove = Some(i);
                ui.close();
            }
        });
    }
    if let Some(i) = add {
        let pose = shell.pose_library.poses[i].clone();
        match shell.editor.insert_library_pose(layer, &pose) {
            Ok((new, note)) => {
                let name = shell.editor.rig_actions(layer)[new].name.clone();
                shell
                    .editor
                    .set_status(StatusKind::Success, format!("Added \"{name}\" to this rig{note}"));
            }
            Err(e) => shell.editor.set_status(StatusKind::Info, e),
        }
    }
    if let Some(i) = remove {
        shell.pose_library.remove(i);
        save(shell);
    }
}
