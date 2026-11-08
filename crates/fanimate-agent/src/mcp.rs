//! A Model Context Protocol server over the API: any agent that speaks MCP
//! (Claude Code, Codex, Antigravity and others) can list Fanimate's tools and
//! call them. It is the same call table the script hosts use, so what an agent
//! can do is exactly what a script can do, with the same permissions.
//!
//! The protocol is JSON-RPC 2.0, one message per line, which is what the
//! standard-input-and-output transport of MCP is. This module only does the
//! protocol; where the calls go is the [`Backend`]'s business (an editor in
//! this process, or the running app over its local socket).

use serde_json::{Value, json};

use crate::prompt::GUIDE;
use crate::tools::{self, Tool};

/// The protocol version this server prefers. A client that asks for an
/// older one it knows is answered with that one.
const LATEST: &str = "2025-06-18";
const KNOWN: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

/// Why a call did not work: the API's stable error code and a message a
/// person can act on.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolError {
    pub code: String,
    pub message: String,
}

impl ToolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Where a tool call goes.
pub trait Backend {
    /// Run one API method with its parameters (a JSON object or `null`).
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, ToolError>;
}

/// An MCP server answering one client.
pub struct Server<B: Backend> {
    backend: B,
    tools: Vec<Tool>,
}

impl<B: Backend> Server<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            tools: tools::tools(),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn into_backend(self) -> B {
        self.backend
    }

    /// The answer to one line, or `None` for a notification (and for a blank
    /// line). A line that is not JSON-RPC is answered with the protocol's
    /// error, never a panic.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, -32700, &format!("that is not JSON: {e}"))),
        };
        if !request.is_object() {
            return Some(error(Value::Null, -32600, "a request is a JSON object"));
        }
        let id = request.get("id").cloned();
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            // a response to something we never asked, or garbage: nothing to say
            return id.map(|id| error(id, -32600, "a request needs a \"method\""));
        };
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        let outcome = self.dispatch(method, &params);
        // a notification has no id and is never answered
        let id = id?;
        Some(match outcome {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
            Err((code, message)) => error(id, code, &message),
        })
    }

    fn dispatch(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let version = KNOWN.iter().find(|v| **v == asked).copied().unwrap_or(LATEST);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "fanimate", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": GUIDE,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": self.tools.iter().map(|t| json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.schema,
                })).collect::<Vec<_>>()
            })),
            "tools/call" => self.call(params),
            // notifications the client may send
            m if m.starts_with("notifications/") => Ok(Value::Null),
            other => Err((-32601, format!("{other} is not something this server does"))),
        }
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err((-32602, "tools/call needs the tool's \"name\"".into()));
        };
        if !tools::is_tool_name(name) {
            return Err((-32602, format!("there is no tool called \"{name}\"")));
        }
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(v) if v.is_object() => v.clone(),
            Some(_) => return Err((-32602, "\"arguments\" must be an object".into())),
        };
        // a failed call is a result the model can read and act on, not a
        // protocol error
        Ok(match self.backend.call(name, &arguments) {
            Ok(value) => json!({
                "content": [{"type": "text", "text": text_of(&value)}],
                "isError": false,
            }),
            Err(e) => json!({
                "content": [{"type": "text", "text": format!("{}: {}", e.code, e.message)}],
                "isError": true,
            }),
        })
    }
}

/// A call's answer as the text a model reads: a bare string as it is,
/// anything else as compact JSON.
pub fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "ok".to_owned(),
        other => other.to_string(),
    }
}

fn error(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers `add_layer` and fails everything else, remembering the calls.
    #[derive(Default)]
    struct Fake {
        calls: Vec<(String, Value)>,
    }

    impl Backend for Fake {
        fn call(&mut self, method: &str, params: &Value) -> Result<Value, ToolError> {
            self.calls.push((method.to_owned(), params.clone()));
            match method {
                "add_layer" => Ok(json!(7)),
                "layers" => Ok(json!("two layers")),
                _ => Err(ToolError::new("not_found", "there is no layer 9")),
            }
        }
    }

    fn ask(server: &mut Server<Fake>, request: Value) -> Value {
        serde_json::from_str(&server.handle_line(&request.to_string()).expect("an answer")).unwrap()
    }

    #[test]
    fn initialize_echoes_a_known_version_and_offers_tools() {
        let mut s = Server::new(Fake::default());
        let a = ask(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05"}}),
        );
        assert_eq!(a["result"]["protocolVersion"], "2024-11-05");
        assert!(a["result"]["capabilities"]["tools"].is_object());
        assert_eq!(a["result"]["serverInfo"]["name"], "fanimate");
        let a = ask(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}),
        );
        assert_eq!(a["result"]["protocolVersion"], LATEST);
        assert!(a["result"]["instructions"].as_str().unwrap().contains("frame"));
    }

    #[test]
    fn notifications_are_never_answered() {
        let mut s = Server::new(Fake::default());
        assert!(
            s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
        assert!(s.handle_line("   ").is_none());
    }

    #[test]
    fn lists_every_tool_with_a_schema() {
        let mut s = Server::new(Fake::default());
        let a = ask(&mut s, json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}));
        let list = a["result"]["tools"].as_array().unwrap();
        assert_eq!(list.len(), tools::tools().len());
        let add = list.iter().find(|t| t["name"] == "add_layer").unwrap();
        assert_eq!(add["inputSchema"]["type"], "object");
        assert!(!list.iter().any(|t| t["name"] == "save"));
    }

    #[test]
    fn a_call_goes_to_the_backend_and_comes_back_as_text() {
        let mut s = Server::new(Fake::default());
        let a = ask(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                   "params": {"name": "add_layer", "arguments": {"kind": "vector", "name": "Ball"}}}),
        );
        assert_eq!(a["result"]["isError"], false);
        assert_eq!(a["result"]["content"][0]["text"], "7");
        let (method, params) = &s.backend().calls[0];
        assert_eq!(method, "add_layer");
        assert_eq!(params["name"], "Ball");
        let a = ask(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "layers"}}),
        );
        assert_eq!(a["result"]["content"][0]["text"], "two layers");
    }

    #[test]
    fn a_failed_call_is_a_result_the_model_can_read() {
        let mut s = Server::new(Fake::default());
        let a = ask(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call",
                   "params": {"name": "rename_layer", "arguments": {"id": 9, "name": "x"}}}),
        );
        assert_eq!(a["result"]["isError"], true);
        assert_eq!(
            a["result"]["content"][0]["text"],
            "not_found: there is no layer 9"
        );
        assert!(a.get("error").is_none());
    }

    #[test]
    fn what_is_not_a_tool_never_reaches_the_backend() {
        let mut s = Server::new(Fake::default());
        for name in ["save", "shutdown", "quit", "format_disk", "api.methods"] {
            let a = ask(
                &mut s,
                json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name}}),
            );
            assert_eq!(a["error"]["code"], -32602, "{name}");
        }
        assert!(s.backend().calls.is_empty());
    }

    #[test]
    fn malformed_input_gets_the_protocols_errors() {
        let mut s = Server::new(Fake::default());
        let a: Value = serde_json::from_str(&s.handle_line("{nope").unwrap()).unwrap();
        assert_eq!(a["error"]["code"], -32700);
        let a: Value = serde_json::from_str(&s.handle_line("[1,2]").unwrap()).unwrap();
        assert_eq!(a["error"]["code"], -32600);
        let a = ask(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 8, "method": "resources/list"}),
        );
        assert_eq!(a["error"]["code"], -32601);
        let a = ask(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                   "params": {"name": "add_layer", "arguments": 5}}),
        );
        assert_eq!(a["error"]["code"], -32602);
        // an id of any JSON type is answered with the same id
        let a = ask(&mut s, json!({"jsonrpc": "2.0", "id": "abc", "method": "ping"}));
        assert_eq!(a["id"], "abc");
    }
}
