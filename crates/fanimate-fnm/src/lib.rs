//! The open Fanimate `.fnm` project format.
//!
//! A `.fnm` project is one file: a ZIP archive that holds this folder layout.
//! A project folder with the same layout also opens, and stays a folder when
//! saved.
//!
//! ```text
//! scene.fnm/
//! ├── manifest.json      format name and version
//! ├── document.json      canvas, timing and the order of top-level layers
//! └── layers/
//!     ├── 0001.json      one file per top-level layer (children inline)
//!     └── 0002.json
//! ```
//!
//! Splitting layers into separate files keeps Git diffs small and lets two
//! artists edit different layers without conflicts. The full specification
//! is in `docs/spec/fnm.md`.
//!
//! Every load runs the [validator](validate), so damaged files are reported
//! (or repaired) instead of silently slowing the app down.

pub mod history;
mod io;
pub mod journal;
pub mod validate;
pub mod zip;

pub use history::{HistoryRecord, HistoryState};
pub use io::{
    FORMAT_NAME, FORMAT_VERSION, MAX_THUMBNAIL_BYTES, THUMBNAIL_NAME, is_media_name, load, load_and_repair,
    load_archive, load_from, read_thumbnail, save, save_folder, save_with_thumbnail,
};
pub use journal::{Journal, Recoverable, SessionInfo, find_recoverable};
pub use validate::{Issue, Report, Severity, repair, validate};

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum FnmError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: invalid JSON: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{0}: not a Fanimate project (no manifest.json with format \"fnm\")")]
    NotFnm(PathBuf),
    #[error("project uses .fnm version {found}; this build reads up to version {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
}

pub type Result<T> = std::result::Result<T, FnmError>;
