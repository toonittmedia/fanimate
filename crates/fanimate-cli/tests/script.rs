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

/// Starts a host with `args`, sends `requests` and returns the answers.
fn session(args: &[&str], requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .arg("script")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("fanimate-cli runs");
    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
    }
    drop(stdin);
    let answers = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect();
    child.wait().unwrap();
    answers
}

#[test]
fn a_plugin_manifest_limits_what_the_host_does_for_its_client() {
    let dir = std::env::temp_dir().join(format!("fanimate-plugin-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = dir.join("plugin.json");
    std::fs::write(
        &manifest,
        json!({"name": "Reader", "version": "1.0.0", "api": "0.1", "permissions": ["read"]}).to_string(),
    )
    .unwrap();
    let saved = dir.join("nope.fnm");
    let answers = session(
        &["--plugin", &manifest.to_string_lossy()],
        &[
            json!({"id": 1, "method": "layers"}),
            json!({"id": 2, "method": "add_layer", "params": {"kind": "vector", "name": "x"}}),
            json!({"id": 3, "method": "save", "params": {"path": saved.to_string_lossy()}}),
            json!({"id": 4, "method": "shutdown"}),
            json!({"id": 5, "method": "layers"}),
        ],
    );
    assert_eq!(answers[0]["result"], json!([]));
    for n in [1, 2, 3] {
        assert_eq!(
            answers[n]["error"]["data"]["code"], "permission_denied",
            "{}",
            answers[n]
        );
    }
    assert!(
        answers[1]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("\"edit\"")
    );
    assert!(!saved.exists(), "a refused save writes nothing");
    // the refused shutdown did not end the session
    assert_eq!(answers[4]["id"], 5);
    assert_eq!(
        answers[4]["result"],
        json!([]),
        "the refused edit changed nothing"
    );

    // the same client with edit and save permissions may do both
    let answers = session(
        &[
            "--permissions",
            "read,edit,save",
            "--save",
            &saved.to_string_lossy(),
        ],
        &[
            json!({"id": 1, "method": "add_layer", "params": {"kind": "vector", "name": "x"}}),
            json!({"id": 2, "method": "save"}),
        ],
    );
    assert!(answers.iter().all(|a| a.get("error").is_none()), "{answers:?}");
    assert!(saved.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_bad_permission_or_manifest_stops_the_host_before_it_serves() {
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
            .arg("script")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let out = run(&["--permissions", "read,root"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no permission \"root\""));
    let out = run(&["--plugin", "/no/such/plugin.json"]);
    assert!(!out.status.success());
    let out = run(&["--plugin", "a.json", "--permissions", "read"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("not both"));

    // plugin check names what is asked for, and what is wrong
    let dir = std::env::temp_dir().join(format!("fanimate-plugin-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let check = |file: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
            .args(["plugin", "check", &file.to_string_lossy()])
            .output()
            .unwrap()
    };
    let good = dir.join("good.json");
    std::fs::write(
        &good,
        json!({"name": "Titles", "version": "2.0", "api": "0.1", "entry": "main.py",
               "permissions": ["read", "edit"]})
        .to_string(),
    )
    .unwrap();
    let out = check(&good);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("Titles 2.0") && text.contains("permissions: read, edit"),
        "{text}"
    );
    let bad = dir.join("bad.json");
    std::fs::write(
        &bad,
        json!({"name": "x", "version": "1", "api": "0.9", "permissions": []}).to_string(),
    )
    .unwrap();
    let out = check(&bad);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("API 0.9"));
    std::fs::remove_dir_all(&dir).ok();
}
