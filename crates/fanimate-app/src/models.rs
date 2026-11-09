//! Preferences > AI models: what models are installed, install one from a
//! file, remove one. The rules (licence, provenance, signature, hash) are
//! `fanimate-ml`'s; this is only the window onto them. Every model is judged
//! when it is installed and again when it is loaded, nothing loads while AI
//! features are off, and no model is bundled or downloaded by Fanimate: a
//! model is a file the user chooses to install.

use eframe::egui::{self, RichText};
use fanimate_editor::StatusKind;
use fanimate_ml::{Installed, ModelStore};

use crate::app::Shell;
use crate::kit;

/// What the page remembers between frames.
#[derive(Default)]
pub struct ModelsUi {
    listed: Option<Vec<Installed>>,
    /// The last install or remove, and whether it worked.
    message: Option<(bool, String)>,
}

fn store() -> Option<ModelStore> {
    crate::data::dir().map(|d| ModelStore::new(d.join("models")))
}

/// The keys a model package must be signed by: the release key that signs updates.
fn trusted() -> [[u8; 32]; 1] {
    [crate::update::PUBLIC_KEY]
}

/// Install the package in `bytes` into `store`, saying what happened.
fn install(store: &ModelStore, bytes: &[u8]) -> (bool, String) {
    match store.install(bytes, &trusted()) {
        Ok(done) => (
            true,
            format!("Installed {} {}.", done.manifest.name, done.manifest.version),
        ),
        Err(e) => (false, format!("Not installed: {e}.")),
    }
}

pub fn page(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let Some(store) = store() else {
        ui.label("This computer names no folder for Fanimate's data, so models cannot be kept.");
        return;
    };
    if shell.models_ui.listed.is_none() {
        shell.models_ui.listed = Some(store.list());
    }
    kit::section(ui, &p, "AI models", |ui| {
        kit::note(
            ui,
            &p,
            "Fanimate's AI tools that use a model run on this computer and are off until you turn them on. \
             No model comes with Fanimate: you install one from a file. Fanimate accepts a model only if its licence \
             is a permissive one, its training data is documented (commissioned, donated, public domain or openly \
             licensed, never art scraped from the web), its signature is Fanimate's and its weights are the ones it \
             was signed with. The classical tools (clean up lines, inbetweens, auto-rig) need no model.",
        );
        match crate::assistant::unavailable(shell) {
            Some(why) => {
                ui.label(RichText::new(format!("Models are off: {why}")).color(p.text_muted));
            }
            None => {
                ui.label(RichText::new("AI features are on.").color(p.text_muted));
            }
        }
        ui.add_space(6.0);
        let listed = shell.models_ui.listed.clone().unwrap_or_default();
        if listed.is_empty() {
            ui.label("No models are installed.");
        }
        let mut remove = None;
        for model in &listed {
            let m = &model.manifest;
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} {}", m.name, m.version)).strong());
                ui.label(RichText::new(m.task.label()).color(p.text_muted));
                if ui.button("Remove").clicked() {
                    remove = Some(m.id.clone());
                }
            });
            kit::note(
                ui,
                &p,
                format!(
                    "{} - trained on: {} ({})",
                    m.licence,
                    m.provenance.training_data,
                    m.provenance.consent.name()
                ),
            );
        }
        if let Some(id) = remove {
            shell.models_ui.message = Some(match store.remove(&id) {
                Ok(()) => (true, "Removed.".to_owned()),
                Err(e) => (false, format!("Not removed: {e}.")),
            });
            shell.models_ui.listed = None;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Install from file…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_title("Install a model")
                    .add_filter("Fanimate model", &["fnmodel"])
                    .pick_file()
            {
                shell.models_ui.message = Some(match std::fs::read(&path) {
                    Ok(bytes) if bytes.len() <= fanimate_ml::package::MAX_PACKAGE_BYTES => {
                        install(&store, &bytes)
                    }
                    Ok(_) => (false, "Not installed: the file is larger than 1 GiB.".to_owned()),
                    Err(e) => (false, format!("Not installed: {e}.")),
                });
                shell.models_ui.listed = None;
                if shell.models_ui.message.as_ref().is_some_and(|m| m.0) {
                    shell.editor.set_status(StatusKind::Success, "Model installed");
                }
            }
            if ui.button("Open models folder").clicked() {
                let _ = std::fs::create_dir_all(store.dir());
                crate::links::open(&store.dir().to_string_lossy());
            }
        });
        if let Some((ok, text)) = &shell.models_ui.message {
            let color = if *ok { p.text_muted } else { p.error };
            ui.label(RichText::new(text).color(color));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_refuses_a_package_nobody_it_trusts_signed_and_says_why() {
        let dir = std::env::temp_dir().join(format!("fanimate-app-models-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let store = ModelStore::new(&dir);
        // a package signed by some other key is not the release key's: refused, nothing installed
        let manifest: fanimate_ml::ModelManifest = serde_json::from_value(serde_json::json!({
            "schema": 1, "id": "test-model", "name": "Test model", "version": "1", "task": "speech_to_text",
            "runtime": "onnx", "weights": "model.onnx", "size": 1, "sha256": "0".repeat(64),
            "licence": "MIT", "licence_file": "LICENSE.txt", "source": "https://example.org/m",
            "provenance": {"training_data": "Recordings donated by forty speakers with consent",
                           "consent": "donated", "scraped_art": false}
        }))
        .unwrap();
        let package = fanimate_ml::pack(manifest, b"weights", b"licence text", &[5; 32]).unwrap();
        let (ok, text) = install(&store, &package);
        assert!(!ok && text.contains("signature"), "{text}");
        assert!(store.list().is_empty());
        let (ok, text) = install(&store, b"not a package at all");
        assert!(!ok && text.starts_with("Not installed"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
