//! The link between a running Fanimate and the programs that drive it.
//!
//! The app listens on `127.0.0.1` (this computer only), with a random token,
//! and writes where and how to connect into a small file in the user's own
//! folders. An agent started from outside (Claude Code, Codex, Antigravity)
//! runs `fanimate-cli mcp --app`, which reads that file, connects, and carries
//! the agent's calls to the open project. The file says nothing a program
//! running as the same user could not find out anyway; the token is there to
//! keep out other users of the computer and web pages that try
//! `http://127.0.0.1`.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use crate::mcp::{Backend, ToolError};

/// The longest answer read from the app, in bytes.
const MAX_ANSWER: u64 = 16 * 1024 * 1024;

/// How to reach the running app.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub port: u16,
    pub token: String,
    /// The app's process, so a file left by a crash can be told from a live one.
    pub pid: u32,
}

/// Where the file is: `FANIMATE_HOST_FILE` if set (tests, unusual setups),
/// otherwise a per-user folder.
pub fn file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FANIMATE_HOST_FILE") {
        return Some(PathBuf::from(path));
    }
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let dir = if cfg!(windows) {
        env("APPDATA")?.join("Fanimate")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/Fanimate")
    } else if let Some(run) = env("XDG_RUNTIME_DIR") {
        run.join("fanimate")
    } else {
        env("XDG_DATA_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".local/share")))?
            .join("fanimate")
    };
    Some(dir.join("host.json"))
}

impl Link {
    /// Write the file so agents can find this app. Readable by the user only
    /// where the system has such a thing.
    pub fn publish(&self) -> std::io::Result<()> {
        let path = file().ok_or_else(|| std::io::Error::other("no folder for the link file"))?;
        self.publish_to(&path)
    }

    fn publish_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = json!({"host": "127.0.0.1", "port": self.port, "token": self.token, "pid": self.pid});
        // beside the real file, then renamed over it, so a reader never sees half of it
        let part = path.with_extension("json.part");
        std::fs::write(&part, text.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&part, path)
    }

    /// Remove the file, if it is still this app's.
    pub fn retire(&self) {
        if let Some(path) = file() {
            self.retire_from(&path);
        }
    }

    fn retire_from(&self, path: &std::path::Path) {
        if Self::read_from(path).is_some_and(|l| l == *self) {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn read() -> Option<Link> {
        Self::read_from(&file()?)
    }

    fn read_from(path: &std::path::Path) -> Option<Link> {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        Some(Link {
            port: u16::try_from(value.get("port")?.as_u64()?).ok()?,
            token: value.get("token")?.as_str()?.to_owned(),
            pid: u32::try_from(value.get("pid").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0),
        })
    }
}

/// A random token of 32 hex digits, from the system's source of randomness.
pub fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("no source of randomness for the token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether two tokens are equal, taking as long whatever they share.
pub fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A [`Backend`] that sends every call to the running app. It connects when
/// the first call is made, so an agent can list the tools before Fanimate is
/// open, and again if the connection has dropped.
pub struct Remote {
    stream: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
}

impl Default for Remote {
    fn default() -> Self {
        Self::new()
    }
}

const NOT_RUNNING: &str = "Fanimate is not reachable. Open Fanimate and, in the Assistant panel or Preferences, \
     switch on \"Let other programs control Fanimate\"";

impl Remote {
    pub fn new() -> Self {
        Self {
            stream: None,
            next_id: 1,
        }
    }

    fn connect() -> Result<(BufReader<TcpStream>, TcpStream), ToolError> {
        let unreachable = || ToolError::new("unavailable", NOT_RUNNING);
        let link = Link::read().ok_or_else(unreachable)?;
        let stream = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, link.port)),
            Duration::from_secs(3),
        )
        .map_err(|_| unreachable())?;
        stream.set_read_timeout(Some(Duration::from_secs(300))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(10))).ok();
        let reader = BufReader::new(stream.try_clone().map_err(|_| unreachable())?);
        let mut connection = (reader, stream);
        let auth = json!({"jsonrpc": "2.0", "id": 0, "method": "auth", "params": {"token": link.token}});
        let answer = exchange(&mut connection, &auth).map_err(|_| unreachable())?;
        if answer.get("error").is_some() {
            return Err(ToolError::new(
                "unauthorized",
                "Fanimate refused the connection (it may have been restarted); try again",
            ));
        }
        Ok(connection)
    }
}

/// Send one request line and read the one answer line.
fn exchange(connection: &mut (BufReader<TcpStream>, TcpStream), request: &Value) -> std::io::Result<Value> {
    writeln!(connection.1, "{request}")?;
    connection.1.flush()?;
    let mut line = String::new();
    let read = std::io::Read::take(&mut connection.0, MAX_ANSWER).read_line(&mut line)?;
    if read == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "the app closed the link",
        ));
    }
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, ToolError> {
        // one retry on a connection that has gone stale
        for attempt in 0..2 {
            if self.stream.is_none() {
                self.stream = Some(Self::connect()?);
            }
            let id = self.next_id;
            self.next_id += 1;
            let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            let connection = self.stream.as_mut().expect("connected above");
            match exchange(connection, &request) {
                Ok(answer) => return answer_of(&answer),
                Err(_) if attempt == 0 => self.stream = None,
                Err(_) => break,
            }
        }
        self.stream = None;
        Err(ToolError::new("unavailable", NOT_RUNNING))
    }
}

/// A JSON-RPC answer as a result, or the API's error code and message.
pub fn answer_of(answer: &Value) -> Result<Value, ToolError> {
    if let Some(error) = answer.get("error") {
        let code = error
            .pointer("/data/code")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_owned();
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the call failed")
            .to_owned();
        return Err(ToolError { code, message });
    }
    Ok(answer.get("result").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_hex_and_compared_whole() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(same_token(&a, &a) && !same_token(&a, &b) && !same_token(&a, "short"));
    }

    #[test]
    fn answers_become_results_or_coded_errors() {
        assert_eq!(answer_of(&json!({"result": 5})).unwrap(), json!(5));
        assert_eq!(answer_of(&json!({"result": null})).unwrap(), Value::Null);
        let e = answer_of(
            &json!({"error": {"code": -32000, "message": "no layer 9", "data": {"code": "not_found"}}}),
        )
        .unwrap_err();
        assert_eq!((e.code.as_str(), e.message.as_str()), ("not_found", "no layer 9"));
        let e = answer_of(&json!({"error": {}})).unwrap_err();
        assert_eq!(e.code, "error");
    }

    #[test]
    fn the_link_file_is_published_read_and_retired() {
        let dir = std::env::temp_dir().join(format!("fanimate-link-test-{}", std::process::id()));
        let path = dir.join("host.json");
        let link = Link {
            port: 4242,
            token: "abc".into(),
            pid: 7,
        };
        link.publish_to(&path).unwrap();
        assert_eq!(Link::read_from(&path), Some(link.clone()));
        // another app's file is not ours to remove
        let other = Link {
            port: 1,
            token: "zzz".into(),
            pid: 8,
        };
        other.retire_from(&path);
        assert!(path.exists());
        link.retire_from(&path);
        assert!(!path.exists());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Link::read_from(&path), None);
        std::fs::write(&path, r#"{"port": 99999, "token": "x"}"#).unwrap();
        assert_eq!(Link::read_from(&path), None, "a port that is not a port");
        assert_eq!(Link::read_from(&dir.join("missing.json")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_remote_with_nothing_listening_says_how_to_fix_it() {
        // a port nothing listens on and no file: `connect` fails whichever way
        let mut remote = Remote::new();
        if Link::read().is_none() {
            let e = remote.call("layers", &json!({})).unwrap_err();
            assert_eq!(e.code, "unavailable");
            assert!(e.message.contains("Let other programs control Fanimate"));
        }
    }
}
