//! Importers for other animation apps' projects.
//!
//! - **Moho** (`.moho`, `.mohoproj`; Anime Studio's `.anime` and
//!   `.animeproj`): vector layers, groups, bone layers with their skeleton
//!   and bone animation, layer and point binding, switch layers, and
//!   images that sit next to the project.
//! - **Adobe Animate** (`.fla`, `.xfl`): vector drawings, layers and
//!   folders, keyframes as frame-by-frame drawings, classic tweens on
//!   symbols, and graphic symbols.
//!
//! Each importer turns a file into an ordinary [`Document`], plus a list
//! of what did not come through exactly, so the user always knows what to
//! check. Importers are written from inspecting files we may use; see the
//! clean-room rule in `CONTRIBUTING.md`. Bad input gives an error or a
//! note, never a panic.

pub mod animate;
pub mod moho;
mod notes;
pub mod ora;
pub mod zip;

use std::path::Path;

use fanimate_core::Document;

pub use notes::Notes;

/// A project brought in from another app.
#[derive(Debug)]
pub struct Imported {
    pub doc: Document,
    /// What was simplified or left out, one phrase each.
    pub notes: Vec<String>,
}

/// The file kinds Fanimate imports as whole projects, as file dialog
/// extensions.
pub const EXTENSIONS: [&str; 6] = ["moho", "mohoproj", "anime", "animeproj", "fla", "xfl"];

/// Whether `path` names a file kind [`import`] reads.
pub fn can_import(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Read a Moho or Animate project from disk. Pictures the project uses are
/// looked for beside it.
pub fn import(path: &Path) -> Result<Imported, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported".into());
    let folder = path.parent().map(Path::to_path_buf);
    let read_beside = move |file: &str| -> Option<Vec<u8>> {
        let folder = folder.as_ref()?;
        // only plain relative names inside the project's folder
        let rel = Path::new(file);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return None;
        }
        std::fs::read(folder.join(rel)).ok()
    };
    match ext.as_str() {
        "moho" | "mohoproj" | "anime" | "animeproj" => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            moho::import(&name, &bytes, &read_beside)
        }
        "fla" => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            animate::import_fla(&name, &bytes)
        }
        "xfl" => animate::import_xfl(&name, &read_beside),
        _ => Err("Fanimate imports Moho (.moho) and Animate (.fla, .xfl) projects".into()),
    }
}
