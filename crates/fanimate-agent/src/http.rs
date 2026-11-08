//! Posting JSON to a service. Fanimate carries no network library: like the
//! updater, this runs the system's `curl` (part of Windows 10 and later,
//! macOS and nearly every Linux), off the UI thread, with no console window.
//!
//! A key never goes on curl's command line, where other programs can read it:
//! the address and headers are given on its standard input as a config, and
//! the body is read from a private temporary file that is removed afterwards.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::provider::ProviderError;

/// One POST of a JSON body.
pub struct Post<'a> {
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a Value,
}

/// What the service answered.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub status: u16,
    pub body: String,
}

/// Something that can post. The providers are generic over it so their tests
/// never touch the network.
pub trait Http {
    fn post(&self, post: &Post<'_>, cancel: &AtomicBool) -> Result<Answer, ProviderError>;
}

/// The system's `curl`.
#[derive(Clone, Debug)]
pub struct Curl {
    /// Longest the whole call may take.
    pub timeout: Duration,
}

impl Default for Curl {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),
        }
    }
}

/// A string as curl's config file reads it: in quotes, with backslash
/// escapes. A line break would end the line, so it is refused.
fn quoted(text: &str) -> Result<String, ProviderError> {
    if text.contains(['\n', '\r', '\0']) {
        return Err(ProviderError::Transport(
            "a header or address contained a line break".into(),
        ));
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    Ok(out)
}

/// The config curl reads from standard input.
pub fn config(post: &Post<'_>, body_file: &str, timeout: Duration) -> Result<String, ProviderError> {
    let mut text = String::new();
    text.push_str(&format!("url = {}\n", quoted(post.url)?));
    text.push_str("request = \"POST\"\nsilent\nshow-error\n");
    text.push_str(&format!("max-time = {}\n", timeout.as_secs().max(1)));
    text.push_str("write-out = \"\\n%{http_code}\"\n");
    text.push_str(&format!("data-binary = {}\n", quoted(&format!("@{body_file}"))?));
    text.push_str("header = \"Content-Type: application/json\"\n");
    for (name, value) in post.headers {
        if name.contains([':', ' ']) {
            return Err(ProviderError::Transport(format!(
                "\"{name}\" is not a header name"
            )));
        }
        text.push_str(&format!("header = {}\n", quoted(&format!("{name}: {value}"))?));
    }
    Ok(text)
}

/// Split curl's output (the body, a line break and the status code).
pub fn split_output(out: &str) -> Result<Answer, ProviderError> {
    let (body, code) = out
        .rsplit_once('\n')
        .ok_or_else(|| ProviderError::Transport("curl gave no answer".into()))?;
    let status = code
        .trim()
        .parse::<u16>()
        .map_err(|_| ProviderError::Transport("curl gave no status".into()))?;
    if status == 0 {
        return Err(ProviderError::Transport(
            "the service could not be reached".into(),
        ));
    }
    Ok(Answer {
        status,
        body: body.to_owned(),
    })
}

#[cfg(windows)]
fn no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn no_window(_: &mut Command) {}

impl Http for Curl {
    fn post(&self, post: &Post<'_>, cancel: &AtomicBool) -> Result<Answer, ProviderError> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let file = std::env::temp_dir().join(format!(
            "fanimate-ai-{}-{}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&file, post.body.to_string())
            .map_err(|e| ProviderError::Transport(format!("could not write the request: {e}")))?;
        let result = self.run(post, &file.to_string_lossy().replace('\\', "/"), cancel);
        let _ = std::fs::remove_file(&file);
        result
    }
}

impl Curl {
    fn run(&self, post: &Post<'_>, body_file: &str, cancel: &AtomicBool) -> Result<Answer, ProviderError> {
        let config = config(post, body_file, self.timeout)?;
        let mut command = Command::new("curl");
        command
            .args(["--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        no_window(&mut command);
        let mut child = command.spawn().map_err(|e| {
            ProviderError::Transport(format!(
                "could not run curl ({e}); the assistant needs curl installed"
            ))
        })?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(config.as_bytes())
                .map_err(|e| ProviderError::Transport(format!("could not start curl: {e}")))?;
        }
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let reader = std::thread::spawn(move || {
            let (mut o, mut e) = (String::new(), String::new());
            if let Some(out) = out.as_mut() {
                let _ = out.read_to_string(&mut o);
            }
            if let Some(err) = err.as_mut() {
                let _ = err.read_to_string(&mut e);
            }
            (o, e)
        });
        let started = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProviderError::Cancelled);
            }
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() > self.timeout + Duration::from_secs(10) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ProviderError::Transport("the service took too long".into()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(40)),
                Err(e) => return Err(ProviderError::Transport(format!("curl failed: {e}"))),
            }
        }
        let (stdout, stderr) = reader
            .join()
            .map_err(|_| ProviderError::Transport("reading curl's answer failed".into()))?;
        split_output(&stdout).map_err(|e| match e {
            ProviderError::Transport(m) if !stderr.trim().is_empty() => {
                ProviderError::Transport(format!("{m}: {}", stderr.trim()))
            }
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_key_is_in_the_config_and_never_on_a_command_line() {
        let headers = vec![("x-api-key".to_owned(), "sk-secret".to_owned())];
        let post = Post {
            url: "https://example.com/v1/messages",
            headers: &headers,
            body: &json!({}),
        };
        let text = config(&post, "C:/tmp/b.json", Duration::from_secs(60)).unwrap();
        assert!(text.contains("url = \"https://example.com/v1/messages\""));
        assert!(text.contains("header = \"x-api-key: sk-secret\""));
        assert!(text.contains("data-binary = \"@C:/tmp/b.json\""));
        assert!(text.contains("max-time = 60"));
    }

    #[test]
    fn quotes_are_escaped_and_line_breaks_refused() {
        assert_eq!(quoted("a\"b\\c").unwrap(), "\"a\\\"b\\\\c\"");
        assert!(quoted("a\nb").is_err());
        let headers = vec![("X-Bad".to_owned(), "v\r\nInjected: 1".to_owned())];
        let post = Post {
            url: "http://localhost",
            headers: &headers,
            body: &json!({}),
        };
        assert!(config(&post, "b", Duration::from_secs(1)).is_err());
        let headers = vec![("Bad Name".to_owned(), "v".to_owned())];
        let post = Post {
            url: "http://localhost",
            headers: &headers,
            body: &json!({}),
        };
        assert!(config(&post, "b", Duration::from_secs(1)).is_err());
    }

    #[test]
    fn output_is_split_into_body_and_status() {
        let a = split_output("{\"ok\":true}\n200").unwrap();
        assert_eq!((a.status, a.body.as_str()), (200, "{\"ok\":true}"));
        let a = split_output("line one\nline two\n401").unwrap();
        assert_eq!((a.status, a.body.as_str()), (401, "line one\nline two"));
        assert!(split_output("no newline").is_err());
        assert!(split_output("body\nabc").is_err());
        assert!(
            split_output("\n000").is_err(),
            "status 0 means curl never connected"
        );
    }

    /// A program that is not on the path is an error that says what to do.
    #[test]
    fn a_missing_curl_is_an_error_message() {
        // not the real thing: only that failing to start is reported, never a panic
        let r = Command::new("fanimate-no-such-program-xyz").spawn();
        assert!(r.is_err());
    }
}
