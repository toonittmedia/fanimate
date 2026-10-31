//! Crash recovery: a journal of the open project, kept up to date after
//! every action, so a crash loses at most the action being written.
//!
//! Each editing session owns a folder under a recovery root (the app puts
//! it in its data folder):
//!
//! ```text
//! recovery/
//! └── 1234-1739372400123/      one per session
//!     ├── lock                 held while the session is running
//!     ├── session.json         what the project is and where it is saved
//!     ├── project.fnm/         the project as of the last action
//!     └── history/             its undo history (see `crate::history`)
//! ```
//!
//! Writes happen on a background thread, so the interface never waits for
//! the disk. Every file is replaced atomically, and only top-level layers
//! that changed are rewritten, so recording an action costs about one small
//! file. A session that ends normally deletes its folder. At the next start,
//! a folder whose lock is free belonged to a session that crashed, and if
//! it had unsaved work the app offers to recover it.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use fanimate_core::{Document, LayerId};
use serde::{Deserialize, Serialize};

use crate::history::{self, HistoryRecord, HistoryWriter};
use crate::io::{document_file, io_err, layer_file, manifest, required_features, write_json_atomic};
use crate::{Report, Result};

const LOCK: &str = "lock";
const SESSION: &str = "session.json";
const PROJECT: &str = "project.fnm";
const HISTORY: &str = "history";

/// What a recovery folder holds, written beside the project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// The project's name.
    pub name: String,
    /// Where the project is saved; `None` if it never was.
    pub path: Option<PathBuf>,
    /// Whether there were changes not yet saved to `path`.
    pub unsaved: bool,
    /// When the journal was last written, in seconds since 1970.
    pub updated: u64,
}

struct Snapshot {
    doc: Box<Document>,
    info: SessionInfo,
    history: Option<HistoryRecord>,
}

enum Message {
    Record(Snapshot),
    /// Reply once everything before this has been written.
    Flush(Sender<()>),
}

/// The journal of one editing session.
pub struct Journal {
    dir: PathBuf,
    tx: Option<Sender<Message>>,
    worker: Option<JoinHandle<()>>,
    /// The last write error, for the app to show.
    error: Arc<Mutex<Option<String>>>,
    /// Held for the life of the session, so other instances know it is
    /// still running. Released by the OS if the process dies.
    _lock: File,
}

impl Journal {
    /// Start a session in a new folder under `root`.
    pub fn start(root: &Path) -> Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let dir = root.join(format!("{}-{stamp}", std::process::id()));
        fs::create_dir_all(dir.join(PROJECT).join("layers")).map_err(|e| io_err(&dir, e))?;
        let lock_path = dir.join(LOCK);
        let lock = File::create(&lock_path).map_err(|e| io_err(&lock_path, e))?;
        lock.lock().map_err(|e| io_err(&lock_path, e))?;

        let (tx, rx) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let worker = {
            let (dir, error) = (dir.clone(), Arc::clone(&error));
            std::thread::Builder::new()
                .name("fanimate-journal".into())
                .spawn(move || Writer::new(dir).run(rx, &error))
                .map_err(|e| io_err(root, e))?
        };
        Ok(Self {
            dir,
            tx: Some(tx),
            worker: Some(worker),
            error,
            _lock: lock,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Record the project as it is now, with its undo history if given.
    /// Returns at once; the writing happens in the background.
    pub fn record(&self, doc: &Document, path: Option<&Path>, unsaved: bool, history: Option<HistoryRecord>) {
        let info = SessionInfo {
            name: doc.name.clone(),
            path: path.map(Path::to_path_buf),
            unsaved,
            updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        if let Some(tx) = &self.tx {
            let _ = tx.send(Message::Record(Snapshot {
                doc: Box::new(doc.mainline().into_owned()),
                info,
                history: history.filter(HistoryRecord::is_consistent),
            }));
        }
    }

    /// Wait until everything recorded so far is on disk.
    pub fn flush(&self) {
        let (done, wait) = mpsc::channel();
        if let Some(tx) = &self.tx
            && tx.send(Message::Flush(done)).is_ok()
        {
            let _ = wait.recv();
        }
    }

    /// The last error writing the journal, if the latest write failed.
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }

    /// End the session normally: its work is saved or deliberately
    /// discarded, so there is nothing to recover. Deletes the folder.
    pub fn close(mut self) {
        self.stop();
        let _ = fs::remove_dir_all(&self.dir);
    }

    fn stop(&mut self) {
        drop(self.tx.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Journal {
    /// Without [`Journal::close`] (a panic, say) the folder stays, so the
    /// work can be recovered.
    fn drop(&mut self) {
        self.stop();
    }
}

/// The background writer. Remembers what it last wrote, so it rewrites only
/// the top-level layers that changed.
struct Writer {
    project: PathBuf,
    session: PathBuf,
    history: HistoryWriter,
    last: Option<Document>,
}

impl Writer {
    fn new(dir: PathBuf) -> Self {
        Self {
            project: dir.join(PROJECT),
            session: dir.join(SESSION),
            history: HistoryWriter::new(dir.join(HISTORY)),
            last: None,
        }
    }

    fn run(mut self, rx: Receiver<Message>, error: &Mutex<Option<String>>) {
        // only the newest state matters: when several arrive before the
        // disk catches up, the older ones are skipped
        let mut pending: Option<Snapshot> = None;
        loop {
            let msg = if pending.is_some() {
                rx.try_recv().ok()
            } else {
                match rx.recv() {
                    Ok(msg) => Some(msg),
                    Err(_) => break, // the session ended
                }
            };
            match msg {
                Some(Message::Record(snapshot)) => pending = Some(snapshot),
                Some(Message::Flush(done)) => {
                    self.write_pending(&mut pending, error);
                    let _ = done.send(());
                }
                None => self.write_pending(&mut pending, error),
            }
        }
    }

    fn write_pending(&mut self, pending: &mut Option<Snapshot>, error: &Mutex<Option<String>>) {
        if let Some(snapshot) = pending.take() {
            let result = self
                .write(&snapshot.doc, &snapshot.info)
                .and_then(|()| match &snapshot.history {
                    Some(h) => self.history.write(h),
                    None => Ok(()),
                });
            if let Ok(mut e) = error.lock() {
                *e = result.err().map(|e| e.to_string());
            }
        }
    }

    fn write(&mut self, doc: &Document, info: &SessionInfo) -> Result<()> {
        // the manifest lists the features the project needs, which can change
        // as the work goes on (the first generated layer, say)
        let features_changed = self
            .last
            .as_ref()
            .is_none_or(|p| required_features(p) != required_features(doc));
        if features_changed {
            write_json_atomic(&self.project.join("manifest.json"), &manifest(doc))?;
        }
        let previous = self.last.as_ref();
        let unchanged = |id: LayerId| {
            let old = previous.and_then(|p| p.layers.iter().find(|l| l.id == id));
            let new = doc.layers.iter().find(|l| l.id == id);
            old.is_some() && old == new
        };
        // layers first, then the document that lists them, so the document
        // never names a layer file that is not there
        for layer in doc.layers.iter().filter(|l| !unchanged(l.id)) {
            write_json_atomic(&self.project.join(layer_file(layer)), layer)?;
        }
        crate::io::write_media(doc, &self.project)?;
        write_json_atomic(&self.project.join("document.json"), &document_file(doc))?;
        if let Some(previous) = previous {
            for gone in previous
                .layers
                .iter()
                .filter(|old| !doc.layers.iter().any(|l| l.id == old.id))
            {
                let _ = fs::remove_file(self.project.join(layer_file(gone)));
            }
        }
        write_json_atomic(&self.session, info)?;
        self.last = Some(doc.clone());
        Ok(())
    }
}

/// Work left behind by a session that crashed.
#[derive(Clone, Debug, PartialEq)]
pub struct Recoverable {
    pub dir: PathBuf,
    pub info: SessionInfo,
}

impl Recoverable {
    /// Load the recovered project, repairing what the validator can.
    pub fn load(&self) -> Result<(Document, Report)> {
        crate::load_and_repair(self.dir.join(PROJECT))
    }

    /// The recovered project's undo history, if it was kept and every state
    /// in it is sound. A history that is missing, damaged or would need
    /// repairs is left out rather than risk undoing into a broken state;
    /// the project itself is unaffected.
    pub fn load_history(&self) -> Option<HistoryRecord> {
        // The project is shifted to 1-based frames when it is loaded, but the
        // history's states hold frames as written; a journal left by a build
        // before version 6 would put the two out of step, so its history is
        // left out (the project is unaffected).
        if crate::io::folder_version(&self.dir.join(PROJECT))? < crate::io::ONE_BASED_FRAMES {
            return None;
        }
        let mut record = history::read(&self.dir.join(HISTORY)).ok()?;
        // pictures are not in the history's own files; every picture the
        // session used is kept in the copy's media folder
        let media = read_media_dir(&self.dir.join(PROJECT));
        for state in &mut record.states {
            std::sync::Arc::make_mut(&mut state.meta).media = media.clone();
        }
        let sound = record.is_consistent()
            && record.states.iter().all(|s| {
                crate::validate(&s.document())
                    .unresolved_errors()
                    .next()
                    .is_none()
            });
        sound.then_some(record)
    }

    /// Delete the recovery folder, once its work has been recovered or the
    /// user has chosen to discard it.
    pub fn discard(&self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Every picture in a project folder's `media` folder.
fn read_media_dir(project: &Path) -> fanimate_core::Media {
    let mut media = fanimate_core::Media::new();
    let Ok(entries) = fs::read_dir(project.join("media")) else {
        return media;
    };
    for entry in entries.flatten() {
        let name = format!("media/{}", entry.file_name().to_string_lossy());
        if crate::is_media_name(&name)
            && let Ok(bytes) = fs::read(entry.path())
        {
            media.insert(name, std::sync::Arc::new(fanimate_core::MediaFile { bytes }));
        }
    }
    media
}

/// Sessions under `root` that ended without closing (their lock is free)
/// and had unsaved work, newest first. Ended sessions with nothing unsaved
/// are cleaned up. Folders of running sessions are left alone.
pub fn find_recoverable(root: &Path) -> Vec<Recoverable> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for dir in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        // a running session holds its lock; a crashed one does not
        let Ok(lock) = File::options().read(true).write(true).open(dir.join(LOCK)) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        let info = fs::read_to_string(dir.join(SESSION))
            .ok()
            .and_then(|text| serde_json::from_str::<SessionInfo>(&text).ok());
        let has_project = dir.join(PROJECT).join("document.json").is_file();
        drop(lock);
        match info {
            Some(info) if info.unsaved && has_project => found.push(Recoverable { dir, info }),
            // nothing unsaved, or nothing usable: nothing to offer
            _ => {
                let _ = fs::remove_dir_all(&dir);
            }
        }
    }
    found.sort_by_key(|r| std::cmp::Reverse(r.info.updated));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{LayerKind, samples};
    use std::process::Command;

    /// Starting a process copies every open file into it until it runs its
    /// program, session locks included, so a test that needs a dropped
    /// session's lock released must not overlap the crash test's spawn.
    static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-journal-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// A document after `n` edits: each edit adds a layer, and renames one.
    fn edited(n: usize) -> Document {
        let mut doc = samples::hello();
        for i in 0..n {
            if i % 2 == 0 {
                let layer = doc.new_layer(format!("Layer {i}"), LayerKind::vector(vec![]));
                doc.layers.push(layer);
            } else {
                doc.layers[0].name = format!("renamed {i}");
            }
        }
        doc
    }

    #[test]
    fn journal_holds_the_last_recorded_state() {
        let root = temp_root("last");
        let journal = Journal::start(&root).unwrap();
        for n in 0..10 {
            journal.record(&edited(n), Some(Path::new("scene.fnm")), true, None);
        }
        journal.flush();
        assert!(journal.error().is_none());
        // a running session is never offered for recovery
        assert!(find_recoverable(&root).is_empty());
        let (doc, report) = crate::load(journal.dir().join(PROJECT)).unwrap();
        assert_eq!(doc, edited(9));
        assert!(report.is_clean(), "{report}");
        journal.close();
        assert!(fs::read_dir(&root).unwrap().next().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    /// The manifest lists the features the project needs, so when the first
    /// layer with a provenance record appears mid-session the journal's
    /// manifest follows, and a recovered project is not mistaken for a plain one.
    #[test]
    fn the_journal_manifest_follows_the_features_a_project_needs() {
        let root = temp_root("features");
        let journal = Journal::start(&root).unwrap();
        let manifest = || fs::read_to_string(journal.dir().join(PROJECT).join("manifest.json")).unwrap();
        let mut doc = edited(2);
        journal.record(&doc, None, true, None);
        journal.flush();
        assert!(!manifest().contains("requires"), "{}", manifest());
        doc.layers[0].provenance = Some(fanimate_core::Provenance::new("auto-rig"));
        journal.record(&doc, None, true, None);
        journal.flush();
        assert!(manifest().contains("provenance@1"), "{}", manifest());
        let (loaded, report) = crate::load(journal.dir().join(PROJECT)).unwrap();
        assert_eq!(loaded, doc);
        assert!(report.is_clean(), "{report}");
        // and back, when the layer's record goes
        doc.layers[0].provenance = None;
        journal.record(&doc, None, true, None);
        journal.flush();
        assert!(!manifest().contains("requires"), "{}", manifest());
        journal.close();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn removed_layers_are_removed_from_the_journal() {
        let root = temp_root("removed");
        let journal = Journal::start(&root).unwrap();
        journal.record(&edited(5), None, true, None);
        journal.flush();
        journal.record(&edited(1), None, true, None);
        journal.flush();
        let layers = fs::read_dir(journal.dir().join(PROJECT).join("layers"))
            .unwrap()
            .count();
        assert_eq!(layers, edited(1).layers.len());
        journal.close();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn saved_sessions_left_behind_are_cleaned_up_not_offered() {
        let root = temp_root("saved");
        let _spawn = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let journal = Journal::start(&root).unwrap();
        journal.record(&edited(2), Some(Path::new("scene.fnm")), false, None);
        journal.flush();
        drop(journal); // ends without close(), as in a crash
        assert!(find_recoverable(&root).is_empty());
        assert!(fs::read_dir(&root).unwrap().next().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    /// A journal left by a build before version 6 holds frames counted from
    /// 0. The project is shifted when it is read, so its history, which is
    /// not, is left out rather than put out of step with it.
    #[test]
    fn a_history_from_before_one_based_frames_is_left_out() {
        let root = temp_root("old-history");
        let journal = Journal::start(&root).unwrap();
        let mut record = HistoryRecord::default();
        record
            .states
            .push(crate::HistoryState::capture(&edited(1), &[], None));
        journal.record(&edited(1), None, true, Some(record));
        journal.flush();
        assert!(journal.error().is_none());
        let recoverable = Recoverable {
            dir: journal.dir().to_path_buf(),
            info: SessionInfo {
                name: "old".into(),
                path: None,
                unsaved: true,
                updated: 0,
            },
        };
        // written by this version: nothing is shifted, and the history comes back
        let (doc, _) = recoverable.load().unwrap();
        assert_eq!(doc, edited(1));
        assert!(recoverable.load_history().is_some());

        // the same journal as version 5 wrote it
        let manifest = journal.dir().join(PROJECT).join("manifest.json");
        fs::write(&manifest, r#"{"format":"fnm","version":5}"#).unwrap();
        let (old, _) = recoverable.load().unwrap();
        assert_eq!(old.start, doc.start + 1);
        assert!(recoverable.load_history().is_none());
        journal.close();
        fs::remove_dir_all(root).unwrap();
    }

    const CHILD: &str = "FANIMATE_JOURNAL_CRASH_CHILD";

    /// Helper for the crash test below: in a child process, records `n`
    /// edits, waits for the last to be written, and aborts like a crash.
    /// Does nothing when run as a normal test.
    #[test]
    fn crash_child() {
        let Ok(spec) = std::env::var(CHILD) else { return };
        let (root, n) = spec.split_once('|').unwrap();
        let journal = Journal::start(Path::new(root)).unwrap();
        let n: usize = n.parse().unwrap();
        let mut record = HistoryRecord::default();
        for i in 0..=n {
            // an undo history like the editor's: one state per edit
            let like = record.states.last().cloned();
            record
                .states
                .push(crate::HistoryState::capture(&edited(i), &[], like.as_ref()));
            if i > 0 {
                record.labels.push(format!("Edit {i}"));
            }
            record.cursor = i;
            journal.record(
                &edited(i),
                Some(Path::new("scene.fnm")),
                true,
                Some(record.clone()),
            );
            if i == n {
                journal.flush();
            }
        }
        std::process::abort();
    }

    /// The M2 exit check, in miniature: a process killed mid-session leaves
    /// a journal that opens, cleanly, with the last action in it.
    #[test]
    fn work_survives_a_crashed_process() {
        let root = temp_root("crash");
        let spawn = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "journal::tests::crash_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, format!("{}|17", root.display()))
            .status()
            .unwrap();
        drop(spawn);
        assert!(!status.success(), "the child should have aborted");

        let found = find_recoverable(&root);
        assert_eq!(found.len(), 1);
        let r = &found[0];
        assert_eq!(r.info.path.as_deref(), Some(Path::new("scene.fnm")));
        assert!(r.info.unsaved);
        let (doc, report) = r.load().unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(doc, edited(17));
        // and so does its undo history, back to the start
        let history = r.load_history().unwrap();
        assert_eq!(history.cursor, 17);
        assert_eq!(history.labels.len(), 17);
        assert_eq!(history.states[0].document(), edited(0));
        assert_eq!(history.states[17].document(), doc);
        r.discard();
        assert!(find_recoverable(&root).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    const WRITER: &str = "FANIMATE_JOURNAL_WRITER_CHILD";

    /// Helper for the kill test below: in a child process, records edits
    /// one after another until killed, printing each once it is on disk.
    /// Does nothing when run as a normal test.
    #[test]
    fn writer_child() {
        let Ok(root) = std::env::var(WRITER) else { return };
        let journal = Journal::start(Path::new(&root)).unwrap();
        let mut record = HistoryRecord::default();
        // far more than a trial waits for; then wait to be killed
        for i in 0..1000 {
            let like = record.states.last().cloned();
            record
                .states
                .push(crate::HistoryState::capture(&edited(i), &[], like.as_ref()));
            if i > 0 {
                record.labels.push(format!("Edit {i}"));
            }
            record.cursor = i;
            journal.record(&edited(i), None, true, Some(record.clone()));
            journal.flush();
            println!("written {i}");
        }
        loop {
            std::thread::park();
        }
    }

    /// The edit a writer child's output line confirms. The test harness
    /// prints the test's name without a newline, so the first confirmation
    /// shares its line.
    fn confirmed(line: &str) -> Option<usize> {
        let at = line.find("written ")?;
        line[at + "written ".len()..].trim().parse().ok()
    }

    /// Gate 1's crash recovery test: a process killed outright at a random
    /// moment, 100 times, mid-write or between writes, always leaves a
    /// journal that opens cleanly with the last finished action (or the
    /// one being written, if it got to disk before the kill).
    #[test]
    fn work_survives_100_kills_at_random_moments() {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        let root = temp_root("kills");
        // xorshift: the same moments every run
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut random = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for trial in 0..100 {
            let spawn = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "journal::tests::writer_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(WRITER, &root)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            drop(spawn);
            // let it finish 0 to 40 writes, then kill it within a few
            // milliseconds: sometimes between writes, sometimes during one
            let wanted = random(41);
            let mut last: Option<usize> = None;
            let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
            while last.map_or(0, |l| l as u64 + 1) < wanted {
                let line = lines.next().expect("the child stopped early").unwrap();
                if let Some(n) = confirmed(&line) {
                    last = Some(n);
                }
            }
            std::thread::sleep(std::time::Duration::from_micros(random(3000)));
            child.kill().unwrap();
            child.wait().unwrap();
            // it kept writing while we waited (on a fast disk, several
            // edits): what it confirmed before dying is still in the pipe
            for line in lines.map_while(|l| l.ok()) {
                if let Some(n) = confirmed(&line) {
                    last = Some(n);
                }
            }

            let found = find_recoverable(&root);
            let Some(last) = last else {
                // killed before its first write finished: nothing, or edit 0
                for r in &found {
                    let (doc, report) = r.load().unwrap();
                    assert!(report.is_clean(), "trial {trial}: {report}");
                    assert_eq!(doc, edited(0), "trial {trial}");
                    r.discard();
                }
                continue;
            };
            assert_eq!(found.len(), 1, "trial {trial}: one session to recover");
            let r = &found[0];
            let (doc, report) = r.load().unwrap();
            assert!(report.is_clean(), "trial {trial}: {report}");
            assert!(
                doc == edited(last) || doc == edited(last + 1),
                "trial {trial}: recovered something other than edit {last} or {}",
                last + 1
            );
            if let Some(history) = r.load_history() {
                let at = history.cursor;
                assert!(
                    at == last || at == last + 1,
                    "trial {trial}: history at {at}, wrote {last}"
                );
                assert_eq!(history.states[at].document(), edited(at), "trial {trial}");
            }
            r.discard();
        }
        fs::remove_dir_all(root).unwrap();
    }
}
