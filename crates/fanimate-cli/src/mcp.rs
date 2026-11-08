//! `fanimate-cli mcp`: Fanimate as an MCP server over standard input and
//! output, which is how agents such as Claude Code, Codex and Antigravity are
//! given tools.
//!
//! Two kinds of backend:
//!
//! - `--app` carries every call to the running Fanimate over its local socket
//!   (`fanimate_agent::link`), so the agent works on the project open in the
//!   window, with the user watching and Ctrl+Z available. The tools can be
//!   listed with Fanimate closed; a call then says how to turn the link on.
//! - Otherwise an editor in this process works on a project file (or a new
//!   one), saving after each change to `--save` (the file itself by default),
//!   for agents running on a server with no window.

use std::io::{BufRead, Read, Write};

use fanimate_agent::link::Remote;
use fanimate_agent::mcp::{Backend, Server, ToolError};
use fanimate_api::{Api, ApiError, ErrorCode, Permissions, rpc};
use fanimate_editor::Editor;
use serde_json::Value;

/// The longest request line read, in bytes.
const MAX_LINE: usize = 8 * 1024 * 1024;

/// An editor in this process behind the call table, with the permissions its
/// client was given.
pub struct Local {
    editor: Editor,
    save_to: Option<String>,
    permissions: Permissions,
}

impl Local {
    pub fn new(editor: Editor, save_to: Option<String>, permissions: Permissions) -> Self {
        Self {
            editor,
            save_to,
            permissions,
        }
    }
}

fn failure(e: ApiError) -> ToolError {
    ToolError::new(e.code.as_str(), e.message)
}

impl Backend for Local {
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, ToolError> {
        if let Some(needed) = rpc::required(method).filter(|p| !self.permissions.contains(*p)) {
            return Err(failure(ApiError::new(
                ErrorCode::PermissionDenied,
                format!("\"{method}\" needs the \"{}\" permission", needed.as_str()),
            )));
        }
        let before = self.editor.revision();
        let answer = rpc::call(&mut Api::new(&mut self.editor), method, params).map_err(failure)?;
        // a project file is kept up to date as the agent works, so closing the
        // agent never loses anything
        if self.editor.revision() != before
            && self.permissions.contains(fanimate_api::Permission::Save)
            && let Some(path) = &self.save_to
        {
            fanimate_fnm::save(&self.editor.doc.mainline(), path)
                .map_err(|e| failure(ApiError::new(ErrorCode::Io, e.to_string())))?;
            self.editor.dirty = false;
        }
        Ok(answer)
    }
}

/// Serve MCP on `input` and `out` until the input ends.
pub fn serve<B: Backend>(backend: B, mut input: impl BufRead, mut out: impl Write) -> std::io::Result<()> {
    let mut server = Server::new(backend);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let read = input
            .by_ref()
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        if buffer.len() > MAX_LINE && !buffer.ends_with(b"\n") {
            writeln!(
                out,
                "{}",
                serde_json::json!({"jsonrpc": "2.0", "id": null,
                    "error": {"code": -32600, "message": "a request line is longer than 8 MB"}})
            )?;
            return out.flush();
        }
        if let Some(answer) = server.handle_line(&String::from_utf8_lossy(&buffer)) {
            writeln!(out, "{answer}")?;
            out.flush()?;
        }
    }
}

/// The backend for the running app.
pub fn app() -> Remote {
    Remote::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::open;

    fn run(backend: impl Backend, input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        serve(backend, input.as_bytes(), &mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn call(id: u32, name: &str, arguments: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{name}","arguments":{arguments}}}}}"#
        )
    }

    #[test]
    fn an_agent_builds_something_and_the_file_has_it() {
        let dir = std::env::temp_dir().join(format!("fanimate-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.fnm").to_string_lossy().into_owned();
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#.to_owned(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_owned(),
            call(3, "add_layer", r#"{"kind":"vector","name":"Ball"}"#),
            call(4, "add_shape", r##"{"layer":1,"shape":{"kind":"ellipse","x":-20,"y":-20,"width":40,"height":40,"fill":"#e63946"}}"##),
            call(5, "set_layer_transform", r#"{"id":1,"frame":24,"x":300,"y":200}"#),
        ]
        .join("\n");
        let answers = run(
            Local::new(open(None).unwrap(), Some(file.clone()), Permissions::all()),
            &input,
        );
        // the notification gets no answer
        assert_eq!(answers.len(), 5);
        assert!(answers[1]["result"]["tools"].as_array().unwrap().len() > 50);
        for a in &answers[2..] {
            assert_eq!(a["result"]["isError"], false, "{a}");
        }
        let (doc, report) = fanimate_fnm::load(&file).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(doc.layers.len(), 1);
        assert_eq!(doc.layers[0].name, "Ball");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_client_without_edit_may_look_and_not_touch() {
        let answers = run(
            Local::new(
                open(None).unwrap(),
                None,
                Permissions::parse_list("read").unwrap(),
            ),
            &[
                call(1, "layers", "{}"),
                call(2, "add_layer", r#"{"kind":"vector","name":"X"}"#),
            ]
            .join("\n"),
        );
        assert_eq!(answers[0]["result"]["isError"], false);
        assert_eq!(answers[1]["result"]["isError"], true);
        assert!(
            answers[1]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("permission_denied")
        );
    }

    #[test]
    fn errors_come_back_with_the_apis_code() {
        let answers = run(
            Local::new(open(None).unwrap(), None, Permissions::all()),
            &call(1, "rename_layer", r#"{"id":99,"name":"x"}"#),
        );
        assert_eq!(answers[0]["result"]["isError"], true);
        assert!(
            answers[0]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("not_found")
        );
    }

    #[test]
    fn the_app_backend_lists_tools_even_when_the_app_is_closed() {
        // no link file is published in the test environment unless an app runs
        let answers = run(app(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        assert!(answers[0]["result"]["tools"].as_array().unwrap().len() > 50);
    }
}
