//! Where Fanimate keeps what belongs to the user, not to a project: settings,
//! the crash and recovery folders, the pose library, plugin grants, the
//! thumbnail cache and the single-instance lock. Normally the system's
//! application data folder; `FANIMATE_DATA_DIR` names another one, so a test
//! copy of the program can run without touching the real one (it is what the
//! GUI checks use: setting `APPDATA` is not enough, because the system's
//! folder is not read from that variable).

use std::path::PathBuf;

/// The folder, or `None` when the system names none and no override is set.
pub fn dir() -> Option<PathBuf> {
    std::env::var_os("FANIMATE_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| eframe::storage_dir("Fanimate"))
}
