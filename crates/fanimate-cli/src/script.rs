//! `fanimate-cli script`: the scripting API over standard input and output,
//! one JSON request per line and one JSON answer per line (see
//! `fanimate_api::rpc`). A script in any language starts it as a child
//! process, so nothing about the engine has to be linked into the script.
//!
//! Two methods belong to the host rather than the API because they touch the
//! disk: `save` (to the path the host was started with, or to `path`) and
//! `quit`. Everything else is `fanimate_api::rpc::call`. A host is given the
//! [`Permissions`] its client has (everything, unless it was started with
//! `--permissions` or `--plugin`), and a call that needs another is refused.

use std::io::{BufRead, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use fanimate_api::{API_VERSION, Api, ApiError, ErrorCode, Permissions, rpc};
use fanimate_core::Document;
use fanimate_editor::Editor;
use serde_json::{Value, json};

/// The longest request line read, in bytes. A longer one ends the session
/// with an error rather than growing a buffer without bound.
pub const MAX_LINE: usize = 8 * 1024 * 1024;

/// How a session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// The input ended or the client asked to `quit`.
    Ended,
    /// The client asked the whole host to `shutdown`.
    Shutdown,
}

/// What one request line did.
enum Step {
    Go,
    Quit,
    Shutdown,
}

/// Serve requests from `input` until it ends, a `quit` or a `shutdown`.
/// `save_to` is where `save` writes when it is given no path, and `permissions`
/// is what the client may do. Returns an
/// error only when the output cannot be written; a bad request is an answer,
/// not a failure.
pub fn serve(
    editor: &mut Editor,
    save_to: Option<&str>,
    permissions: Permissions,
    mut input: impl BufRead,
    mut out: impl Write,
) -> std::io::Result<Flow> {
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let read = input
            .by_ref()
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buffer)?;
        if read == 0 {
            return Ok(Flow::Ended);
        }
        if buffer.len() > MAX_LINE && !buffer.ends_with(b"\n") {
            let e = ApiError::new(ErrorCode::InvalidArgument, "a request line is longer than 8 MB");
            writeln!(out, "{}", rpc::failure(Value::Null, e))?;
            out.flush()?;
            return Ok(Flow::Ended);
        }
        let line = String::from_utf8_lossy(&buffer);
        if line.trim().is_empty() {
            continue;
        }
        let (answer, step) = answer(editor, save_to, permissions, line.trim());
        if let Some(answer) = answer {
            writeln!(out, "{answer}")?;
            out.flush()?;
        }
        match step {
            Step::Go => {}
            Step::Quit => return Ok(Flow::Ended),
            Step::Shutdown => return Ok(Flow::Shutdown),
        }
    }
}

/// The answer to one line (none for a notification, which has no id) and
/// what the session does next.
fn answer(
    editor: &mut Editor,
    save_to: Option<&str>,
    permissions: Permissions,
    line: &str,
) -> (Option<String>, Step) {
    let host = serde_json::from_str::<Value>(line).ok().and_then(|request| {
        let method = request.get("method")?.as_str()?.to_owned();
        matches!(method.as_str(), "save" | "quit" | "shutdown").then_some((method, request))
    });
    let Some((method, request)) = host else {
        return (
            rpc::handle_line_as(&mut Api::new(editor), line, permissions),
            Step::Go,
        );
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
        return (reply(rpc::failure(id, e)), Step::Go);
    }
    match method.as_str() {
        "quit" => return (reply(rpc::reply(id, Value::Null)), Step::Quit),
        "shutdown" => return (reply(rpc::reply(id, Value::Null)), Step::Shutdown),
        _ => {}
    }
    let path = request
        .get("params")
        .and_then(|p| p.get("path"))
        .and_then(Value::as_str)
        .or(save_to);
    let Some(path) = path else {
        let e = ApiError::new(
            ErrorCode::InvalidArgument,
            "there is no path to save to: start the host with --save or give a \"path\"",
        );
        return (reply(rpc::failure(id, e)), Step::Go);
    };
    match fanimate_fnm::save(&editor.doc.mainline(), path) {
        Ok(()) => {
            editor.dirty = false;
            (reply(rpc::reply(id, Value::Null)), Step::Go)
        }
        Err(e) => (
            reply(rpc::failure(id, ApiError::new(ErrorCode::Io, e.to_string()))),
            Step::Go,
        ),
    }
}

/// A random token of 32 hex digits, from the system's source of randomness.
fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("no source of randomness for the token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether two tokens are equal, taking as long whatever they share.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Serve over a local socket: only this computer can connect (the address is
/// `127.0.0.1`), and a connection must first send
/// `{"jsonrpc": "2.0", "id": 1, "method": "auth", "params": {"token": "..."}}`
/// with the token printed on `announce`, or it is answered `unauthorized` and
/// closed. One connection is served at a time, all on the one project, until a
/// client sends `shutdown`. `announce` gets one line of JSON with the
/// `host`, `port`, `token` and `api_version` once the socket is listening.
pub fn serve_socket(
    mut editor: Editor,
    save_to: Option<&str>,
    permissions: Permissions,
    port: u16,
    mut announce: impl Write,
) -> Result<(), String> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .map_err(|e| format!("could not listen on 127.0.0.1:{port}: {e}"))?;
    let token = new_token()?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let ready = json!({"host": "127.0.0.1", "port": port, "token": token, "api_version": API_VERSION});
    writeln!(announce, "{ready}")
        .and_then(|()| announce.flush())
        .map_err(|e| e.to_string())?;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        match connection(&mut editor, save_to, permissions, stream, &token) {
            Ok(Flow::Shutdown) => break,
            // a connection that fails or ends is not the host's end
            Ok(Flow::Ended) | Err(_) => {}
        }
    }
    Ok(())
}

fn connection(
    editor: &mut Editor,
    save_to: Option<&str>,
    permissions: Permissions,
    stream: TcpStream,
    token: &str,
) -> std::io::Result<Flow> {
    // a client that connects and says nothing must not hold the host for ever
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = std::io::BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut first = Vec::new();
    reader.by_ref().take(4096).read_until(b'\n', &mut first)?;
    let request: Value = serde_json::from_slice(&first).unwrap_or(Value::Null);
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let shown = request
        .get("params")
        .and_then(|p| p.get("token"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let authorised =
        request.get("method").and_then(Value::as_str) == Some("auth") && same_token(shown, token);
    if !authorised {
        let e = ApiError::new(
            ErrorCode::Unauthorized,
            "the first request must be \"auth\" with the token the host printed",
        );
        writeln!(writer, "{}", rpc::failure(id, e))?;
        writer.flush()?;
        return Ok(Flow::Ended);
    }
    writeln!(writer, "{}", rpc::reply(id, Value::Bool(true)))?;
    writer.flush()?;
    writer.set_read_timeout(None)?;
    serve(editor, save_to, permissions, reader, writer)
}

/// A project to script: the file named, or an empty one with `--new`.
pub fn open(path: Option<&str>) -> Result<Editor, String> {
    match path {
        Some(path) => {
            let (doc, report) = fanimate_fnm::load_and_repair(path).map_err(|e| e.to_string())?;
            if report.repaired_count() > 0 {
                eprintln!(
                    "fanimate-cli: repaired {} problem(s) on opening",
                    report.repaired_count()
                );
            }
            Ok(Editor::new(doc))
        }
        None => Ok(Editor::new(Document::new("Untitled", 1920.0, 1080.0, 24.0))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mut editor: Editor, save_to: Option<String>, input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        serve(
            &mut editor,
            save_to.as_deref(),
            Permissions::all(),
            input.as_bytes(),
            &mut out,
        )
        .unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn each_line_gets_one_answer_blank_lines_are_skipped_and_quit_ends_the_session() {
        let editor = open(None).unwrap();
        let answers = run(
            editor,
            None,
            "{\"id\":1,\"method\":\"layers\"}\n\n  \nnot json\n{\"id\":2,\"method\":\"quit\"}\n{\"id\":3,\"method\":\"layers\"}\n",
        );
        assert_eq!(answers.len(), 3, "the line after quit is never read");
        assert_eq!(answers[0]["result"], serde_json::json!([]));
        assert_eq!(answers[1]["error"]["data"]["code"], "invalid_argument");
        assert_eq!(answers[2]["id"], 2);
    }

    #[test]
    fn a_script_edits_saves_and_the_file_has_the_edit() {
        let dir = std::env::temp_dir().join(format!("fanimate-script-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.fnm");
        let input = "{\"id\":1,\"method\":\"add_text_layer\",\"params\":{\"content\":\"Saved\",\"x\":10,\"y\":10}}\n{\"id\":2,\"method\":\"save\"}\n";
        let answers = run(
            open(None).unwrap(),
            Some(file.to_string_lossy().into_owned()),
            input,
        );
        assert!(answers[1].get("error").is_none(), "{}", answers[1]);
        let (doc, report) = fanimate_fnm::load(&file).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(doc.layers.len(), 1);
        // and a second host can open the file and read it back
        let again = run(
            open(Some(&file.to_string_lossy())).unwrap(),
            None,
            "{\"id\":1,\"method\":\"text\",\"params\":{\"layer\":1}}\n",
        );
        assert_eq!(again[0]["result"]["content"], "Saved", "{}", again[0]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saving_with_nowhere_to_save_is_an_error_and_a_bad_path_is_an_io_error() {
        let answers = run(
            open(None).unwrap(),
            None,
            "{\"id\":1,\"method\":\"save\"}\n{\"id\":2,\"method\":\"save\",\"params\":{\"path\":\"/no/such/folder/x/y.fnm\"}}\n",
        );
        assert_eq!(answers[0]["error"]["data"]["code"], "invalid_argument");
        assert_eq!(answers[1]["error"]["data"]["code"], "io_error");
    }

    #[test]
    fn a_missing_or_damaged_project_is_an_error_not_a_panic() {
        assert!(open(Some("/no/such/project.fnm")).is_err());
    }
}
