//! One Fanimate, many documents.
//!
//! Opening a `.fnm` file from the file manager starts the program again. If
//! Fanimate is already running, the new process hands the files to it and
//! exits, and the running Fanimate opens them as tabs.
//!
//! The running Fanimate holds an exclusive lock on a file in its data
//! folder; the operating system drops the lock when the process ends, even
//! by a crash, so a stale lock cannot lock anyone out. A second process
//! that cannot take the lock writes its request into the `inbox` folder and
//! waits for it to be picked up. If nobody picks it up (the first Fanimate
//! is hung, say), it carries on as a Fanimate of its own rather than
//! leaving the user with nothing.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a second launch waits for the running Fanimate to answer.
const ANSWER_WITHIN: Duration = Duration::from_secs(3);

/// What this process is.
pub enum Role {
    /// The one Fanimate; keep this alive for as long as it runs.
    Primary(Claim),
    /// Another Fanimate has the files now; exit.
    Forwarded,
    /// No other Fanimate answered (or there is no data folder): a Fanimate
    /// of its own.
    Alone,
}

/// The running Fanimate's lock and inbox.
pub struct Claim {
    _lock: File,
    inbox: PathBuf,
}

impl Claim {
    /// Files other launches asked for since last time. An empty request
    /// (a launch with no files) means "bring the window forward".
    pub fn requests(&self) -> Vec<Vec<PathBuf>> {
        take_requests(&self.inbox)
    }
}

/// Where the lock and the inbox live: beside the app's settings.
fn folder() -> Option<PathBuf> {
    eframe::storage_dir("Fanimate").map(|dir| dir.join("instance"))
}

/// Become the one Fanimate, or hand `files` to the one that is running.
pub fn claim(files: &[PathBuf]) -> Role {
    match folder() {
        Some(dir) => claim_in(&dir, files, ANSWER_WITHIN),
        None => Role::Alone,
    }
}

fn claim_in(dir: &Path, files: &[PathBuf], wait: Duration) -> Role {
    let inbox = dir.join("inbox");
    if fs::create_dir_all(&inbox).is_err() {
        return Role::Alone;
    }
    let Ok(lock) = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("lock"))
    else {
        return Role::Alone;
    };
    if lock.try_lock().is_ok() {
        // requests left by launches that never got an answer are stale
        for entry in fs::read_dir(&inbox).into_iter().flatten().flatten() {
            let _ = fs::remove_file(entry.path());
        }
        return Role::Primary(Claim { _lock: lock, inbox });
    }
    drop(lock);

    let request = write_request(&inbox, files);
    let Some(request) = request else {
        return Role::Alone;
    };
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if !request.exists() {
            return Role::Forwarded;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    // nobody answered: do not leave the request for a later start to act on
    let _ = fs::remove_file(&request);
    Role::Alone
}

/// Write a request to the inbox, complete or not at all.
fn write_request(inbox: &Path, files: &[PathBuf]) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!("{}-{stamp}", std::process::id());
    let absolute = |p: &PathBuf| {
        std::path::absolute(p)
            .unwrap_or_else(|_| p.clone())
            .to_string_lossy()
            .into_owned()
    };
    let text: String = files.iter().map(|p| absolute(p) + "\n").collect();
    let tmp = inbox.join(format!("{name}.tmp"));
    let done = inbox.join(format!("{name}.open"));
    fs::write(&tmp, text).ok()?;
    if fs::rename(&tmp, &done).is_err() {
        let _ = fs::remove_file(&tmp);
        return None;
    }
    Some(done)
}

/// Read and remove every complete request in `inbox`, oldest first.
fn take_requests(inbox: &Path) -> Vec<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = fs::read_dir(inbox)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "open"))
        .collect();
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap_or_default();
            // removing it is the answer the other process is waiting for
            let _ = fs::remove_file(&path);
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-instance-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_second_launch_hands_its_files_over_and_the_first_receives_them() {
        let dir = temp("handover");
        let Role::Primary(first) = claim_in(&dir, &[], Duration::ZERO) else {
            panic!("the first launch is the one Fanimate");
        };
        // the running Fanimate answers while the second launch waits
        let inbox = dir.join("inbox");
        let waiter = std::thread::spawn(move || {
            let mut got = Vec::new();
            let until = Instant::now() + Duration::from_secs(5);
            while got.is_empty() && Instant::now() < until {
                got = first.requests();
                std::thread::sleep(Duration::from_millis(10));
            }
            got
        });
        let files = [PathBuf::from("a.fnm"), PathBuf::from("b.fnm")];
        let role = claim_in(&dir, &files, Duration::from_secs(5));
        assert!(matches!(role, Role::Forwarded));
        let got = waiter.join().unwrap();
        assert_eq!(got.len(), 1);
        let names: Vec<_> = got[0]
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.fnm", "b.fnm"]);
        // paths are made absolute, so they mean the same in the other process
        assert!(got[0].iter().all(|p| p.is_absolute()));
        assert_eq!(fs::read_dir(inbox).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_launch_with_no_files_asks_for_the_window() {
        let dir = temp("focus");
        let Role::Primary(first) = claim_in(&dir, &[], Duration::ZERO) else {
            panic!("the first launch is the one Fanimate");
        };
        let waiter = std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                let got = first.requests();
                if !got.is_empty() || Instant::now() > until {
                    return got;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        assert!(matches!(
            claim_in(&dir, &[], Duration::from_secs(5)),
            Role::Forwarded
        ));
        assert_eq!(waiter.join().unwrap(), vec![Vec::<PathBuf>::new()]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_first_fanimate_that_does_not_answer_does_not_lock_anyone_out() {
        let dir = temp("hung");
        let Role::Primary(_hung) = claim_in(&dir, &[], Duration::ZERO) else {
            panic!("the first launch is the one Fanimate");
        };
        let started = Instant::now();
        let role = claim_in(&dir, &[PathBuf::from("a.fnm")], Duration::from_millis(200));
        assert!(matches!(role, Role::Alone));
        assert!(started.elapsed() < Duration::from_secs(3));
        // and its unanswered request is not left behind
        assert_eq!(fs::read_dir(dir.join("inbox")).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_lock_is_free_again_when_the_first_fanimate_ends() {
        let dir = temp("released");
        let first = claim_in(&dir, &[], Duration::ZERO);
        assert!(matches!(first, Role::Primary(_)));
        drop(first);
        assert!(matches!(claim_in(&dir, &[], Duration::ZERO), Role::Primary(_)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_that_cannot_be_made_means_carry_on_alone() {
        let dir = temp("blocked");
        let file = dir.join("not-a-folder");
        fs::write(&file, "x").unwrap();
        assert!(matches!(claim_in(&file, &[], Duration::ZERO), Role::Alone));
        fs::remove_dir_all(dir).unwrap();
    }
}
