//! `fanimate-cli script` as a child process: what a plugin or a pipeline
//! script in any language does. It writes requests to the child's standard
//! input and reads one answer per line from its standard output.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn a_child_process_scripts_a_project_from_nothing_to_a_saved_file() {
    let dir = std::env::temp_dir().join(format!("fanimate-script-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("title.fnm");
    let mut child = Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .args(["script", "--save", &file.to_string_lossy()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("fanimate-cli runs");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |request: Value| -> Value {
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line:?}"))
    };

    assert_eq!(
        ask(json!({"id": 1, "method": "api.version"}))["result"],
        fanimate_api::API_VERSION
    );
    let made = ask(json!({"id": 2, "method": "add_text_layer",
        "params": {"content": "Made by a script", "x": 120, "y": 90}}));
    let id = made["result"].as_u64().expect("a layer id");
    let animator =
        ask(json!({"id": 3, "method": "add_text_animator", "params": {"layer": id, "preset": "typewriter"}}));
    assert_eq!(animator["result"], 0);
    // a bad call is an error answer and the session goes on
    let bad = ask(json!({"id": 4, "method": "rename_layer", "params": {"id": 4242, "name": "x"}}));
    assert_eq!(bad["error"]["data"]["code"], "not_found");
    assert!(ask(json!({"id": 5, "method": "save"})).get("error").is_none());
    assert_eq!(ask(json!({"id": 6, "method": "quit"}))["id"], 6);
    drop(stdin);
    assert!(child.wait().unwrap().success());

    let (doc, report) = fanimate_fnm::load(&file).expect("the saved file opens");
    assert!(report.is_clean(), "{report}");
    assert_eq!(doc.layers.len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}
