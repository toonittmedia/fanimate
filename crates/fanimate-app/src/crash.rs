//! Crash reports, sent only when the user chooses.
//!
//! When Fanimate panics, the panic hook writes a short report (version,
//! system, graphics, what failed and where) to the `crashes` folder beside
//! the settings. Each running session also holds a locked marker file
//! there; a marker whose lock is free belongs to a session that ended
//! without closing, which catches crashes no hook sees (a graphics driver
//! taking the process down, a power cut).
//!
//! On the next start Fanimate asks whether to report the crash. Reporting
//! opens a new GitHub issue in the browser, filled in with the report, for
//! the user to read, edit and send. Fanimate itself never sends anything,
//! and "Never ask" in Preferences turns the question off.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const ISSUES: &str = "https://github.com/toonittmedia/fanimate/issues/new";
/// Browsers and GitHub cope with addresses up to about 8 KB, and encoding
/// can nearly triple a report's length.
const MAX_BODY: usize = 2500;

/// Graphics adapter name, filled in once the window has a GPU, for reports.
static GPU: OnceLock<String> = OnceLock::new();

pub fn set_gpu(name: &str) {
    let _ = GPU.set(name.to_owned());
}

/// The crash folder, beside the settings.
pub fn dir() -> Option<PathBuf> {
    crate::data::dir().map(|d| d.join("crashes"))
}

/// Install the panic hook that writes a report before the default hook
/// prints the panic.
pub fn install_hook() {
    let Some(dir) = dir() else { return };
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".into());
        let place = info
            .location()
            .map_or_else(String::new, |l| format!("{}:{}", l.file(), l.line()));
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let text = report(&format!("{message}\n  at {place}"), &backtrace);
        // a report that cannot be written is lost; the crash still is not
        let _ = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(report_name()), text));
        default(info);
    }));
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn report_name() -> String {
    format!("crash-{}-{}.txt", now(), std::process::id())
}

/// A crash report's text.
fn report(what: &str, backtrace: &str) -> String {
    format!(
        "Fanimate {}\nSystem: {} {}\nGraphics: {}\n\nWhat failed:\n{what}\n\nBacktrace:\n{backtrace}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        GPU.get().map_or("not started", String::as_str),
    )
}

/// How sessions on this computer ended, kept so a report can say how
/// often Fanimate crashes here (the crash-free figure Gate 1 measures).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub sessions: u64,
    pub crashes: u64,
}

/// A crash from an earlier session, waiting for the user's decision.
#[derive(Debug, PartialEq)]
pub struct Pending {
    /// The report written by the panic hook, if there was one.
    pub file: Option<PathBuf>,
    pub text: String,
}

/// This session's marker. Dropping it at a normal exit removes it.
pub struct Session {
    path: PathBuf,
    _lock: File,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Start a session in `dir`: count it, notice sessions that ended without
/// closing, and return the crash to offer, if any.
pub fn begin(dir: &Path) -> (Option<Session>, Option<Pending>) {
    if std::fs::create_dir_all(dir).is_err() {
        return (None, None);
    }
    let stats_path = dir.join("stats.json");
    let mut stats: Stats = std::fs::read(&stats_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut unclean = 0;
    let mut reports = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("session-") {
            // a running session holds its lock; an ended one does not
            let free = File::options()
                .read(true)
                .write(true)
                .open(&path)
                .is_ok_and(|f| f.try_lock().is_ok());
            if free {
                unclean += 1;
                let _ = std::fs::remove_file(&path);
            }
        } else if name.starts_with("crash-") && name.ends_with(".txt") {
            reports.push(path);
        }
    }
    stats.sessions += 1;
    stats.crashes += unclean;
    if let Ok(json) = serde_json::to_vec(&stats) {
        let _ = std::fs::write(&stats_path, json);
    }
    let session = start_session(dir);

    reports.sort();
    let pending = match reports.pop() {
        Some(file) => std::fs::read_to_string(&file).ok().map(|text| Pending {
            text: with_stats(text, stats),
            file: Some(file),
        }),
        None if unclean > 0 => Some(Pending {
            text: with_stats(
                report(
                    "Fanimate closed without an error being recorded. The system or the graphics driver \
                     may have stopped it.",
                    "(none)",
                ),
                stats,
            ),
            file: None,
        }),
        None => None,
    };
    // older reports than the newest are not offered again
    for old in reports {
        let _ = std::fs::remove_file(old);
    }
    (session, pending)
}

fn start_session(dir: &Path) -> Option<Session> {
    let path = dir.join(format!("session-{}-{}", now(), std::process::id()));
    let lock = File::create(&path).ok()?;
    lock.try_lock().ok()?;
    Some(Session { path, _lock: lock })
}

fn with_stats(text: String, stats: Stats) -> String {
    format!(
        "{text}\n\nOn this computer: {} sessions, {} ended in a crash.",
        stats.sessions, stats.crashes
    )
}

/// Forget a crash: delete its report.
pub fn dismiss(p: &Pending) {
    if let Some(f) = &p.file {
        let _ = std::fs::remove_file(f);
    }
}

/// The address of a new GitHub issue holding the report.
pub fn issue_url(p: &Pending) -> String {
    let mut body = format!(
        "**What were you doing when Fanimate closed?**\n\n\n\n**Report** (written by Fanimate; remove anything you do \
         not want to share)\n\n```\n{}\n```\n",
        p.text
    );
    if body.len() > MAX_BODY {
        let mut cut = MAX_BODY;
        while !body.is_char_boundary(cut) {
            cut -= 1;
        }
        body.truncate(cut);
        body.push_str("\n(cut short)\n```\n");
    }
    let title = p
        .text
        .lines()
        .skip_while(|l| !l.starts_with("What failed"))
        .nth(1)
        .unwrap_or("Fanimate closed unexpectedly");
    format!(
        "{ISSUES}?labels=crash&title={}&body={}",
        encode(&format!("Crash: {title}")),
        encode(&body)
    )
}

/// Percent-encode text for a query string.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fanimate-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_session_that_ends_normally_is_not_a_crash() {
        let dir = temp("clean");
        let (session, pending) = begin(&dir);
        assert!(session.is_some() && pending.is_none());
        drop(session);
        let (_s, pending) = begin(&dir);
        assert!(pending.is_none());
        let stats: Stats = serde_json::from_slice(&std::fs::read(dir.join("stats.json")).unwrap()).unwrap();
        assert_eq!(
            stats,
            Stats {
                sessions: 2,
                crashes: 0
            }
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_session_left_behind_and_a_written_report_are_offered() {
        let dir = temp("crashed");
        std::fs::create_dir_all(&dir).unwrap();
        // a marker nobody holds: a session that never closed
        std::fs::write(dir.join("session-1-1"), "").unwrap();
        let (_s, pending) = begin(&dir);
        let p = pending.unwrap();
        assert!(p.file.is_none() && p.text.contains("without an error"));
        assert!(p.text.contains("1 ended in a crash"));
        drop(_s);
        // a report from the panic hook wins, and is gone once dismissed
        std::fs::write(dir.join("crash-5-1.txt"), report("boom\n  at a.rs:1", "trace")).unwrap();
        std::fs::write(dir.join("crash-4-1.txt"), "older").unwrap();
        let (_s, pending) = begin(&dir);
        let p = pending.unwrap();
        assert!(p.text.contains("boom"));
        assert!(!dir.join("crash-4-1.txt").exists());
        dismiss(&p);
        assert!(!dir.join("crash-5-1.txt").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_issue_address_is_encoded_and_short_enough() {
        let p = Pending {
            file: None,
            text: report("index out of bounds & more\n  at x.rs:2", &"frame\n".repeat(5000)),
        };
        let url = issue_url(&p);
        assert!(url.starts_with(ISSUES));
        assert!(url.contains("title=Crash%3A%20index%20out%20of%20bounds%20%26%20more"));
        assert!(url.len() < 8000, "{}", url.len());
        assert!(!url.contains(' ') && !url.contains('\n'));
    }
}
