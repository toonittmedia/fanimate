//! The local socket other programs use to work on the open project.
//!
//! It listens on `127.0.0.1` only, with a random token, and writes where it
//! is into the link file (`fanimate_agent::link`) so that `fanimate-cli mcp
//! --app` can find it. Each request line is handed to the UI thread as a
//! [`Job`] and answered when the frame loop has run it, so every call goes
//! through the same editor, transactions and permissions as the Assistant's
//! own, and nothing changes the project from another thread.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use eframe::egui;
use fanimate_agent::link::{self, Link};
use serde_json::{Value, json};

/// The longest request line read, in bytes.
const MAX_LINE: usize = 8 * 1024 * 1024;
/// Connections served at once.
const MAX_CONNECTIONS: usize = 8;
/// How long a request may wait for the UI thread.
const ANSWER_WITHIN: Duration = Duration::from_secs(120);

/// Who asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The Assistant's own model.
    Assistant,
    /// A program on this computer (an agent started by the user).
    Program,
}

/// One JSON-RPC request line waiting for the UI thread, and where its answer
/// goes (`None` for a notification, which has none).
pub struct Job {
    pub line: String,
    pub origin: Origin,
    pub reply: Sender<Option<String>>,
    /// Set when the person who asked has stopped waiting: a stopped Assistant
    /// must not go on to make a change after the Stop button was pressed.
    pub cancel: Option<Arc<AtomicBool>>,
}

/// The running socket. Dropping it stops listening and removes the link file.
pub struct Host {
    link: Link,
    stop: Arc<AtomicBool>,
}

impl Host {
    pub fn start(jobs: Sender<Job>, ctx: egui::Context) -> Result<Host, String> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .map_err(|e| format!("could not listen on this computer: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let link = Link {
            port,
            token: link::new_token()?,
            pid: std::process::id(),
        };
        link.publish()
            .map_err(|e| format!("could not write the link file: {e}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (token, flag) = (link.token.clone(), stop.clone());
        std::thread::Builder::new()
            .name("fanimate-link".into())
            .spawn(move || accept_loop(listener, token, jobs, ctx, flag))
            .map_err(|e| e.to_string())?;
        Ok(Host { link, stop })
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.link.retire();
    }
}

fn accept_loop(
    listener: TcpListener,
    token: String,
    jobs: Sender<Job>,
    ctx: egui::Context,
    stop: Arc<AtomicBool>,
) {
    let open = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if open.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                    continue; // dropped: the client sees the connection close
                }
                open.fetch_add(1, Ordering::Relaxed);
                let (token, jobs, ctx, stop) = (token.clone(), jobs.clone(), ctx.clone(), stop.clone());
                let counter = open.clone();
                let spawned = std::thread::Builder::new()
                    .name("fanimate-link-client".into())
                    .spawn(move || {
                        let _ = connection(stream, &token, &jobs, &ctx, &stop);
                        counter.fetch_sub(1, Ordering::Relaxed);
                    });
                if spawned.is_err() {
                    open.fetch_sub(1, Ordering::Relaxed);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// The error answer for a request that could not be served.
fn refusal(id: Value, code: &str, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id,
           "error": {"code": -32000, "message": message, "data": {"code": code}}})
    .to_string()
}

fn connection(
    stream: TcpStream,
    token: &str,
    jobs: &Sender<Job>,
    ctx: &egui::Context,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    // the first line must be auth with the token
    let mut first = Vec::new();
    reader.by_ref().take(4096).read_until(b'\n', &mut first)?;
    let request: Value = serde_json::from_slice(&first).unwrap_or(Value::Null);
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let shown = request
        .pointer("/params/token")
        .and_then(Value::as_str)
        .unwrap_or("");
    if request.get("method").and_then(Value::as_str) != Some("auth") || !link::same_token(shown, token) {
        writeln!(
            writer,
            "{}",
            refusal(
                id,
                "unauthorized",
                "the first request must be \"auth\" with the link token"
            )
        )?;
        return Ok(());
    }
    writeln!(writer, "{}", json!({"jsonrpc": "2.0", "id": id, "result": true}))?;

    // a short timeout lets the loop notice that the app is closing; a line
    // that arrives in pieces is kept until it is whole
    writer.set_read_timeout(Some(Duration::from_secs(1)))?;
    let mut line = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        match reader
            .by_ref()
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut line)
        {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
        if !line.ends_with(b"\n") {
            if line.len() > MAX_LINE {
                writeln!(
                    writer,
                    "{}",
                    refusal(
                        Value::Null,
                        "invalid_argument",
                        "a request line is longer than 8 MB"
                    )
                )?;
                return Ok(());
            }
            continue; // more of the line is coming
        }
        let text = String::from_utf8_lossy(&line).trim().to_owned();
        line.clear();
        if text.is_empty() {
            continue;
        }
        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let id = parsed.get("id").cloned().unwrap_or(Value::Null);
        if parsed.get("method").and_then(Value::as_str) == Some("quit") {
            if parsed.get("id").is_some() {
                writeln!(writer, "{}", json!({"jsonrpc": "2.0", "id": id, "result": null}))?;
            }
            return Ok(());
        }
        let (reply, answer) = mpsc::channel();
        let sent = jobs.send(Job {
            line: text,
            origin: Origin::Program,
            reply,
            cancel: None,
        });
        let answer = match sent {
            Ok(()) => {
                ctx.request_repaint();
                match answer.recv_timeout(ANSWER_WITHIN) {
                    Ok(answer) => answer,
                    Err(_) => Some(refusal(id, "unavailable", "Fanimate did not answer in time")),
                }
            }
            Err(_) => Some(refusal(id, "unavailable", "Fanimate is closing")),
        };
        if let Some(answer) = answer {
            writeln!(writer, "{answer}")?;
            writer.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_carries_the_apis_code() {
        let v: Value = serde_json::from_str(&refusal(json!(5), "unauthorized", "no")).unwrap();
        assert_eq!(v["id"], 5);
        assert_eq!(v["error"]["data"]["code"], "unauthorized");
    }
}
