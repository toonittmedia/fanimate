//! "Your files" on the Home tab: folders the user adds, looked through for
//! Fanimate projects.
//!
//! Each folder is read on a background thread (the folder itself and one
//! level of folders below it, so a project in its own folder is found) and
//! the Home tab shows what it finds, newest first. Reading never fails the
//! app: a folder that cannot be read has no projects, and an entry that
//! cannot be read is skipped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};

use eframe::egui;

/// The most projects shown for one folder.
pub const MAX_FILES: usize = 200;
/// The most entries looked at in one folder, so a folder of a million files
/// cannot hold the scan up.
const MAX_ENTRIES: usize = 5000;
/// The most folders below the chosen one that are looked into.
const MAX_SUBFOLDERS: usize = 500;

/// A project found in a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The project: a `.fnm` file, or a folder holding one.
    pub path: PathBuf,
    /// Its name without the extension.
    pub name: String,
    /// When it was last saved.
    pub modified: Option<SystemTime>,
}

/// Look through `dir` and the folders directly below it for projects: `.fnm`
/// files, and folders holding a `manifest.json`. Newest first, at most
/// [`MAX_FILES`]. Anything unreadable is skipped.
pub fn scan(dir: &Path) -> Vec<Found> {
    let mut found = Vec::new();
    let mut subfolders = Vec::new();
    collect(dir, &mut found, Some(&mut subfolders));
    for sub in subfolders.into_iter().take(MAX_SUBFOLDERS) {
        collect(&sub, &mut found, None);
    }
    found.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    found.truncate(MAX_FILES);
    found
}

/// Add the projects directly in `dir` to `found`, and the folders that are not
/// projects (and not hidden) to `subfolders`, when it is asked for.
fn collect(dir: &Path, found: &mut Vec<Found>, mut subfolders: Option<&mut Vec<PathBuf>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten().take(MAX_ENTRIES) {
        let path = entry.path();
        // the entry's own kind: a link to a folder is not followed, so a
        // loop of links cannot trap the scan
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if path.join("manifest.json").is_file() {
                found.push(found_at(path));
            } else if let Some(subs) = subfolders.as_mut()
                && !entry.file_name().to_string_lossy().starts_with('.')
            {
                subs.push(path);
            }
        } else if kind.is_file() && has_fnm_extension(&path) {
            found.push(found_at(path));
        }
    }
}

fn has_fnm_extension(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("fnm"))
}

fn found_at(path: PathBuf) -> Found {
    let name = path.file_stem().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    );
    // a project folder changes in its document file, not in itself
    let written = if path.is_dir() {
        path.join("document.json")
    } else {
        path.clone()
    };
    let modified = std::fs::metadata(&written)
        .or_else(|_| std::fs::metadata(&path))
        .ok()
        .and_then(|m| m.modified().ok());
    Found { path, name, modified }
}

/// "Edited 2 days ago", or the date once it is a week back; nothing when the
/// time is not known.
pub fn edited(modified: Option<SystemTime>, now: SystemTime) -> String {
    let Some(modified) = modified else {
        return String::new();
    };
    // a clock that is behind the file's makes it just now
    let secs = now.duration_since(modified).unwrap_or(Duration::ZERO).as_secs();
    let ago = |n: u64, unit: &str| format!("Edited {n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        0..=59 => "Edited just now".to_owned(),
        60..=3599 => ago(secs / 60, "minute"),
        3600..=86_399 => ago(secs / 3600, "hour"),
        86_400..=604_799 => ago(secs / 86_400, "day"),
        _ => {
            let days = modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| (d.as_secs() / 86_400) as i64);
            let (year, month, day) = civil_date(days);
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            format!("Edited {day} {} {year}", MONTHS[(month as usize - 1) % 12])
        }
    }
}

/// The calendar date (UTC) `days` days after 1 January 1970.
fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

/// What a folder's scan has found so far.
enum Scan {
    Running,
    Done(Vec<Found>),
}

/// The folders on the Home tab and their projects.
pub struct YourFiles {
    folders: Vec<PathBuf>,
    scans: HashMap<PathBuf, Scan>,
    tx: Sender<(PathBuf, Vec<Found>)>,
    rx: Receiver<(PathBuf, Vec<Found>)>,
}

impl Default for YourFiles {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl YourFiles {
    /// The folders remembered from the last session (each looked through the
    /// first time the Home tab shows).
    pub fn new(mut folders: Vec<PathBuf>) -> Self {
        let mut seen = Vec::new();
        folders.retain(|f| {
            let first = !seen.contains(f);
            seen.push(f.clone());
            first
        });
        let (tx, rx) = channel();
        Self {
            folders,
            scans: HashMap::new(),
            tx,
            rx,
        }
    }

    pub fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    /// Add a folder. False if it is not a folder or is already there.
    pub fn add(&mut self, dir: PathBuf) -> bool {
        if !dir.is_dir() || self.folders.contains(&dir) {
            return false;
        }
        self.folders.push(dir);
        true
    }

    /// Take a folder off the list. Its files are not touched.
    pub fn remove(&mut self, dir: &Path) {
        self.folders.retain(|f| f != dir);
        self.scans.remove(dir);
    }

    /// Look through every folder again.
    pub fn refresh(&mut self) {
        self.scans.clear();
    }

    /// What was found in `dir`; none while it is still being looked through.
    pub fn files(&self, dir: &Path) -> Option<&[Found]> {
        match self.scans.get(dir)? {
            Scan::Running => None,
            Scan::Done(files) => Some(files),
        }
    }

    /// Start the scans that are due and take in the ones that finished. Call
    /// each frame the list is on screen.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let due: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|f| !self.scans.contains_key(*f))
            .cloned()
            .collect();
        for dir in due {
            self.start(dir);
        }
        while let Ok((dir, files)) = self.rx.try_recv() {
            // a folder taken off the list, or looked through again since,
            // has no use for this answer
            if let Some(scan) = self.scans.get_mut(&dir)
                && matches!(scan, Scan::Running)
            {
                *scan = Scan::Done(files);
                ctx.request_repaint();
            }
        }
        if self.scans.values().any(|s| matches!(s, Scan::Running)) {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn start(&mut self, dir: PathBuf) {
        let tx = self.tx.clone();
        let target = dir.clone();
        let spawned = std::thread::Builder::new()
            .name("fanimate-scan".into())
            .spawn(move || {
                let files = scan(&target);
                let _ = tx.send((target, files));
            });
        // without a thread the folder simply shows nothing
        let state = if spawned.is_ok() {
            Scan::Running
        } else {
            Scan::Done(Vec::new())
        };
        self.scans.insert(dir, state);
    }
}

/// Ask for a folder with the system's own picker, starting in `start`.
pub fn pick_folder(start: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("Add a folder of Fanimate projects");
    if let Some(dir) = start {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-yourfiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(found: &[Found]) -> Vec<&str> {
        found.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn projects_are_found_in_the_folder_and_one_level_below() {
        let dir = scratch("levels");
        std::fs::write(dir.join("top.fnm"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join("Upper.FNM"), b"x").unwrap();
        // a project saved as a folder is found by its manifest, and not looked into
        let folder = dir.join("rig.fnm");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        std::fs::write(folder.join("manifest.json"), b"{}").unwrap();
        std::fs::write(folder.join("inner").join("deep.fnm"), b"x").unwrap();
        // a plain folder holding a project
        std::fs::create_dir_all(dir.join("shows")).unwrap();
        std::fs::write(dir.join("shows").join("below.fnm"), b"x").unwrap();
        std::fs::write(dir.join("shows").join("below.fnm.saving"), b"x").unwrap();
        // two levels down and hidden folders are not looked into
        std::fs::create_dir_all(dir.join("shows").join("old")).unwrap();
        std::fs::write(dir.join("shows").join("old").join("deeper.fnm"), b"x").unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git").join("hidden.fnm"), b"x").unwrap();
        // a folder with a manifest not named .fnm is a project too
        std::fs::create_dir_all(dir.join("plain")).unwrap();
        std::fs::write(dir.join("plain").join("manifest.json"), b"{}").unwrap();

        let found = scan(&dir);
        let mut got = names(&found);
        got.sort_unstable();
        assert_eq!(got, ["Upper", "below", "plain", "rig", "top"]);
        let rig = found.iter().find(|f| f.name == "rig").unwrap();
        assert_eq!(rig.path, folder);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newest_comes_first_and_the_list_is_capped() {
        let dir = scratch("cap");
        for i in 0..MAX_FILES + 30 {
            std::fs::write(dir.join(format!("p{i:03}.fnm")), b"x").unwrap();
        }
        let found = scan(&dir);
        assert_eq!(found.len(), MAX_FILES);
        assert!(
            found.windows(2).all(|w| w[0].modified >= w[1].modified),
            "newest first"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_that_is_not_there_has_no_projects() {
        let dir = scratch("gone");
        let missing = dir.join("missing");
        assert!(scan(&missing).is_empty());
        // a file where a folder was expected
        let file = dir.join("file.fnm");
        std::fs::write(&file, b"x").unwrap();
        assert!(scan(&file).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_list_of_folders_ignores_repeats_and_things_that_are_not_folders() {
        let dir = scratch("list");
        let a = dir.join("a");
        std::fs::create_dir_all(&a).unwrap();
        let mut files = YourFiles::new(vec![a.clone(), a.clone()]);
        assert_eq!(files.folders(), std::slice::from_ref(&a));
        assert!(!files.add(a.clone()), "already there");
        assert!(!files.add(dir.join("nothing")), "not a folder");
        let b = dir.join("b");
        std::fs::create_dir_all(&b).unwrap();
        assert!(files.add(b.clone()));
        assert_eq!(files.folders(), [a.clone(), b.clone()]);
        files.remove(&a);
        assert_eq!(files.folders(), [b]);
        assert!(files.files(&a).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_scan_on_a_thread_comes_back_through_poll() {
        let dir = scratch("thread");
        std::fs::write(dir.join("one.fnm"), b"x").unwrap();
        let mut files = YourFiles::new(vec![dir.clone()]);
        let ctx = egui::Context::default();
        assert!(files.files(&dir).is_none(), "not looked through yet");
        let mut waited = 0;
        while files.files(&dir).is_none() && waited < 200 {
            files.poll(&ctx);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        let found = files.files(&dir).expect("the scan finished");
        assert_eq!(names(found), ["one"]);
        // looking again starts over
        files.refresh();
        assert!(files.files(&dir).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn edit_times_are_told_in_plain_words() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let ago = |s: u64| edited(Some(now - Duration::from_secs(s)), now);
        assert_eq!(edited(None, now), "");
        assert_eq!(ago(5), "Edited just now");
        assert_eq!(ago(60), "Edited 1 minute ago");
        assert_eq!(ago(5 * 60), "Edited 5 minutes ago");
        assert_eq!(ago(3600), "Edited 1 hour ago");
        assert_eq!(ago(2 * 86_400), "Edited 2 days ago");
        // a file from the future is just now
        assert_eq!(
            edited(Some(now + Duration::from_secs(90)), now),
            "Edited just now"
        );
        // older than a week is a date: 1_800_000_000 is 15 January 2027 (UTC)
        assert_eq!(ago(14 * 86_400), "Edited 1 Jan 2027");
    }

    #[test]
    fn dates_count_from_1970() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(59), (1970, 3, 1));
        // 1972 was a leap year
        assert_eq!(civil_date(365 * 2 + 59), (1972, 2, 29));
        assert_eq!(civil_date(19_723), (2024, 1, 1));
        assert_eq!(civil_date(20_000), (2024, 10, 4));
    }
}
