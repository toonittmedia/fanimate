//! Running one plugin: start its entry as a child process and answer the
//! calls it writes to its standard output.
//!
//! The session is the same as `fanimate-cli script`'s: one JSON-RPC request
//! per line in, one answer per line out, every call checked against the
//! manifest's permissions by `fanimate_api::rpc::handle_line_as`. The host
//! adds what a program the user did not write needs: a time limit, a limit on
//! the length of a line and on the number of calls, and a capture of what the
//! plugin printed on its error stream. Whatever the plugin does, it ends: when
//! a limit is passed the process is killed.

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use fanimate_api::{Api, ApiError, ErrorCode, Permissions, rpc};
use fanimate_editor::Editor;
use serde_json::Value;

use crate::{Plugin, PluginError, command_line, find_node, find_python};

/// The longest request line read, in bytes (the same as the script host's).
pub const MAX_LINE: usize = 8 * 1024 * 1024;

/// How much of the plugin's error output is kept; the rest is dropped.
const MAX_STDERR: usize = 64 * 1024;

/// Limits and options for one run.
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// Replaces the manifest's time limit when set.
    pub timeout: Option<Duration>,
    /// The most calls one run may make.
    pub max_calls: usize,
    /// Where the host's `save` writes when it is given no path. Only a plugin
    /// with the `save` permission may call it.
    pub save_to: Option<PathBuf>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            timeout: None,
            max_calls: 100_000,
            save_to: None,
        }
    }
}

/// Which limit stopped a plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    /// It ran longer than it was allowed.
    Time(Duration),
    /// It made more calls than it was allowed.
    Calls(usize),
    /// It wrote a line longer than [`MAX_LINE`].
    Line,
}

impl std::fmt::Display for Limit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Time(d) => write!(f, "it ran longer than {} seconds", d.as_secs().max(1)),
            Self::Calls(n) => write!(f, "it made more than {n} calls"),
            Self::Line => write!(f, "it wrote a line longer than 8 MB"),
        }
    }
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The plugin ended on its own with this exit code (`None`: a signal).
    Exited(Option<i32>),
    /// The plugin asked to `quit` (or `shutdown`); the host ended it.
    Quit,
    /// A limit was passed and the plugin was stopped.
    Stopped(Limit),
}

/// What happened in a run.
#[derive(Clone, Debug)]
pub struct RunReport {
    pub ending: Ending,
    /// Requests the plugin made.
    pub calls: usize,
    /// Calls the manifest's permissions refused.
    pub denied: usize,
    /// Calls that were answered with an error (refused ones included).
    pub failed: usize,
    pub elapsed: Duration,
    /// What the plugin printed on its error stream (the first 64 KB).
    pub stderr: String,
}

impl RunReport {
    /// Whether the plugin finished cleanly: it exited with 0 or asked to quit.
    pub fn succeeded(&self) -> bool {
        matches!(self.ending, Ending::Quit | Ending::Exited(Some(0)))
    }

    /// One sentence for the user.
    pub fn summary(&self) -> String {
        let how = match self.ending {
            Ending::Quit | Ending::Exited(Some(0)) => "finished".to_owned(),
            Ending::Exited(Some(code)) => format!("stopped with an error (exit code {code})"),
            Ending::Exited(None) => "was ended by the system".to_owned(),
            Ending::Stopped(limit) => format!("was stopped because {limit}"),
        };
        let mut text = format!("the plugin {how} after {} call(s)", self.calls);
        if self.denied > 0 {
            text.push_str(&format!(
                "; {} call(s) were refused for lack of permission",
                self.denied
            ));
        }
        text
    }
}

/// What the two reader threads send to the loop.
enum Message {
    Line(String),
    TooLong,
    Closed,
}

/// Run `plugin` against `editor`. Fails only when the plugin cannot be
/// started (no entry, no interpreter, a program that does not exist); a
/// plugin that starts and then fails is a [`RunReport`] saying how.
pub fn run(editor: &mut Editor, plugin: &Plugin, options: &RunOptions) -> Result<RunReport, PluginError> {
    let kind = plugin
        .entry_kind()
        .ok_or_else(|| PluginError::NoEntry(plugin.manifest.name.clone()))?;
    let entry = plugin.entry_path()?;
    let python = find_python();
    let node = find_node();
    let (program, args) = command_line(kind, &entry, python.as_ref(), node.as_ref())?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(&plugin.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut cmd);
    let child = cmd.spawn().map_err(PluginError::Spawn)?;
    Ok(serve(editor, plugin, options, child))
}

fn serve(editor: &mut Editor, plugin: &Plugin, options: &RunOptions, mut child: Child) -> RunReport {
    let started = Instant::now();
    let timeout = options.timeout.unwrap_or(plugin.timeout);
    let permissions = plugin.manifest.permissions;

    let mut stdin = child.stdin.take();
    let (tx, rx) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        thread::spawn(move || read_lines(out, &tx));
    }
    let stderr_reader = child.stderr.take().map(|mut e| {
        thread::spawn(move || {
            let mut kept = Vec::new();
            let mut chunk = [0u8; 4096];
            while let Ok(n) = e.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                // keep reading after the cap, so the plugin never blocks on a full pipe
                let room = MAX_STDERR.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..n.min(room)]);
            }
            String::from_utf8_lossy(&kept).into_owned()
        })
    });

    let mut report = RunReport {
        ending: Ending::Exited(None),
        calls: 0,
        denied: 0,
        failed: 0,
        elapsed: Duration::ZERO,
        stderr: String::new(),
    };
    let ending = loop {
        let left = timeout.saturating_sub(started.elapsed());
        match rx.recv_timeout(left) {
            Err(RecvTimeoutError::Timeout) => break Some(Ending::Stopped(Limit::Time(timeout))),
            Ok(Message::TooLong) => break Some(Ending::Stopped(Limit::Line)),
            Err(RecvTimeoutError::Disconnected) | Ok(Message::Closed) => break None,
            Ok(Message::Line(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                report.calls += 1;
                if report.calls > options.max_calls {
                    break Some(Ending::Stopped(Limit::Calls(options.max_calls)));
                }
                let (answer, quit) = answer(editor, options, permissions, line.trim(), &mut report);
                if let (Some(answer), Some(input)) = (answer, stdin.as_mut()) {
                    // a plugin that closed its input is on its way out
                    if writeln!(input, "{answer}").and_then(|()| input.flush()).is_err() {
                        stdin = None;
                    }
                }
                if quit {
                    break Some(Ending::Quit);
                }
            }
        }
    };
    drop(stdin);

    report.ending = match ending {
        Some(ending) => {
            // kill and reap, so no process is left behind
            child.kill().ok();
            child.wait().ok();
            ending
        }
        None => wait_for_exit(&mut child, timeout.saturating_sub(started.elapsed())),
    };
    report.elapsed = started.elapsed();
    report.stderr = stderr_reader.and_then(|h| h.join().ok()).unwrap_or_default();
    report
}

/// The plugin closed its output: give it the rest of its time to exit, then
/// stop it.
fn wait_for_exit(child: &mut Child, left: Duration) -> Ending {
    let end = Instant::now() + left.min(Duration::from_secs(5));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ending::Exited(status.code()),
            Ok(None) if Instant::now() < end => thread::sleep(Duration::from_millis(10)),
            _ => {
                child.kill().ok();
                let status = child.wait().ok();
                return Ending::Exited(status.and_then(|s| s.code()));
            }
        }
    }
}

/// Send each line of `out` to the loop; stop at the end, or at a line that
/// is too long.
fn read_lines(out: impl Read, tx: &mpsc::Sender<Message>) {
    let mut out = std::io::BufReader::new(out);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let read = out
            .by_ref()
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buffer);
        match read {
            Ok(0) | Err(_) => {
                tx.send(Message::Closed).ok();
                return;
            }
            Ok(_) if buffer.len() > MAX_LINE && !buffer.ends_with(b"\n") => {
                tx.send(Message::TooLong).ok();
                return;
            }
            Ok(_) => {
                if tx
                    .send(Message::Line(String::from_utf8_lossy(&buffer).into_owned()))
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

/// The answer to one request (none for a notification) and whether the
/// plugin asked to end the session. `save`, `quit` and `shutdown` belong to
/// the host, as in the script host.
fn answer(
    editor: &mut Editor,
    options: &RunOptions,
    permissions: Permissions,
    line: &str,
    report: &mut RunReport,
) -> (Option<String>, bool) {
    let host = serde_json::from_str::<Value>(line).ok().and_then(|request| {
        let method = request.get("method")?.as_str()?.to_owned();
        matches!(method.as_str(), "save" | "quit" | "shutdown").then_some((method, request))
    });
    let Some((method, request)) = host else {
        let answer = rpc::handle_line_as(&mut Api::new(editor), line, permissions);
        if let Some(text) = &answer {
            count(text, report);
        }
        return (answer, false);
    };
    let notification = request.get("id").is_none();
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let reply = |text: String| (!notification).then_some(text);
    if let Some(needed) = rpc::required(&method).filter(|p| !permissions.contains(*p)) {
        let e = ApiError::new(
            ErrorCode::PermissionDenied,
            format!(
                "\"{method}\" needs the \"{}\" permission ({})",
                needed.as_str(),
                needed.about()
            ),
        );
        report.denied += 1;
        report.failed += 1;
        return (reply(rpc::failure(id, e)), false);
    }
    if method != "save" {
        return (reply(rpc::reply(id, Value::Null)), true);
    }
    let path = request
        .get("params")
        .and_then(|p| p.get("path"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| options.save_to.clone());
    let text = match path {
        None => {
            report.failed += 1;
            let e = ApiError::new(
                ErrorCode::InvalidArgument,
                "there is no path to save to: run the plugin with --save or give a \"path\"",
            );
            rpc::failure(id, e)
        }
        Some(path) => match fanimate_fnm::save(&editor.doc.mainline(), &path) {
            Ok(()) => {
                editor.dirty = false;
                rpc::reply(id, Value::Null)
            }
            Err(e) => {
                report.failed += 1;
                rpc::failure(id, ApiError::new(ErrorCode::Io, e.to_string()))
            }
        },
    };
    (reply(text), false)
}

/// Count an answer that is an error, and one that is a refusal.
fn count(answer: &str, report: &mut RunReport) {
    let Ok(value) = serde_json::from_str::<Value>(answer) else {
        return;
    };
    let Some(error) = value.get("error") else {
        return;
    };
    report.failed += 1;
    let code = error
        .get("data")
        .and_then(|d| d.get("code"))
        .and_then(Value::as_str);
    if code == Some(ErrorCode::PermissionDenied.as_str()) {
        report.denied += 1;
    }
}

/// Keep a console window from opening for the plugin on Windows.
pub(crate) fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::Document;

    fn editor() -> Editor {
        Editor::new(Document::new("Untitled", 1920.0, 1080.0, 24.0))
    }

    fn report() -> RunReport {
        RunReport {
            ending: Ending::Quit,
            calls: 0,
            denied: 0,
            failed: 0,
            elapsed: Duration::ZERO,
            stderr: String::new(),
        }
    }

    #[test]
    fn a_call_the_manifest_does_not_allow_is_refused_and_counted() {
        let mut ed = editor();
        let mut r = report();
        let read_only = Permissions::NONE.with(fanimate_api::Permission::Read);
        let line = r#"{"id":1,"method":"add_layer","params":{"kind":"vector","name":"x"}}"#;
        let (answer, quit) = answer(&mut ed, &RunOptions::default(), read_only, line, &mut r);
        assert!(!quit);
        assert!(answer.unwrap().contains("permission_denied"));
        assert_eq!((r.denied, r.failed), (1, 1));
        assert!(ed.doc.layers.is_empty(), "a refused call must change nothing");
    }

    #[test]
    fn save_needs_its_permission_and_a_path_and_quit_ends_the_session() {
        let mut ed = editor();
        let mut r = report();
        let options = RunOptions::default();
        let (answer1, quit) = answer(
            &mut ed,
            &options,
            Permissions::NONE,
            r#"{"id":1,"method":"save"}"#,
            &mut r,
        );
        assert!(answer1.unwrap().contains("permission_denied") && !quit);
        let (answer2, quit) = answer(
            &mut ed,
            &options,
            Permissions::all(),
            r#"{"id":2,"method":"save"}"#,
            &mut r,
        );
        assert!(answer2.unwrap().contains("no path") && !quit);
        let (answer3, quit) = answer(
            &mut ed,
            &options,
            Permissions::NONE,
            r#"{"id":3,"method":"quit"}"#,
            &mut r,
        );
        assert!(answer3.is_some() && quit);
    }

    #[test]
    fn a_limit_is_said_in_words() {
        let r = RunReport {
            ending: Ending::Stopped(Limit::Time(Duration::from_secs(5))),
            calls: 3,
            denied: 1,
            ..report()
        };
        let text = r.summary();
        assert!(
            text.contains("5 seconds") && text.contains("3 call") && text.contains("refused"),
            "{text}"
        );
        assert!(!r.succeeded());
        assert!(report().succeeded());
    }
}
