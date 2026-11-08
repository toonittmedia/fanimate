//! Runs real plugin processes (Python, and Node when it is installed)
//! against an editor: the calls arrive, the permissions hold, and the
//! limits end a plugin that does not.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fanimate_core::Document;
use fanimate_editor::Editor;
use fanimate_plugin::{Ending, Limit, Plugin, PluginError, RunOptions, find_node, find_python, run};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fanimate-plugin-run-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn editor() -> Editor {
    Editor::new(Document::new("Untitled", 1920.0, 1080.0, 24.0))
}

fn make(dir: &Path, entry: &str, permissions: &str, timeout: Option<u32>, code: &str) -> Plugin {
    std::fs::create_dir_all(dir).unwrap();
    let timeout = timeout.map_or(String::new(), |t| format!(r#""timeout_seconds": {t},"#));
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"name": "Test", "version": "1.0.0", "api": "0.1", "entry": "{entry}", {timeout} "permissions": [{permissions}]}}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join(entry), code).unwrap();
    Plugin::load(dir).unwrap()
}

const PY_CLIENT: &str = r#"
import json, sys
def call(method, **params):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}) + "\n")
    sys.stdout.flush()
    return json.loads(sys.stdin.readline())
"#;

#[test]
fn a_python_plugin_edits_the_project_through_the_api() {
    if find_python().is_none() {
        eprintln!("skipped: no Python on this computer");
        return;
    }
    let dir = temp("py").join("p");
    let code = format!(
        r#"{PY_CLIENT}
a = call("add_layer", kind="vector", name="from plugin")
print("layer id", a["result"], file=sys.stderr)
"#
    );
    let plugin = make(&dir, "main.py", r#""read", "edit""#, Some(30), &code);
    let mut ed = editor();
    let report = run(&mut ed, &plugin, &RunOptions::default()).unwrap();
    assert!(report.succeeded(), "{report:?}");
    assert_eq!((report.calls, report.denied, report.failed), (1, 0, 0));
    assert_eq!(ed.doc.layers.len(), 1);
    assert_eq!(ed.doc.layers[0].name, "from plugin");
    assert!(report.stderr.contains("layer id"), "{}", report.stderr);
    // the plugin's call is one undo step
    ed.run(fanimate_editor::Action::Undo);
    assert!(ed.doc.layers.is_empty());
}

#[test]
fn a_plugin_without_the_edit_permission_changes_nothing() {
    if find_python().is_none() {
        return;
    }
    let dir = temp("denied").join("p");
    let code = format!(
        r#"{PY_CLIENT}
a = call("add_layer", kind="vector", name="nope")
sys.exit(0 if "error" in a and a["error"]["data"]["code"] == "permission_denied" else 3)
"#
    );
    let plugin = make(&dir, "main.py", r#""read""#, Some(30), &code);
    let mut ed = editor();
    let report = run(&mut ed, &plugin, &RunOptions::default()).unwrap();
    assert!(report.succeeded(), "{report:?}");
    assert_eq!((report.denied, report.failed), (1, 1));
    assert!(ed.doc.layers.is_empty());
}

#[test]
fn a_plugin_that_never_ends_is_stopped_at_its_time_limit() {
    if find_python().is_none() {
        return;
    }
    let dir = temp("slow").join("p");
    let plugin = make(&dir, "main.py", "", Some(1), "import time\ntime.sleep(60)\n");
    let mut ed = editor();
    let options = RunOptions {
        timeout: Some(Duration::from_millis(400)),
        ..RunOptions::default()
    };
    let report = run(&mut ed, &plugin, &options).unwrap();
    assert!(
        matches!(report.ending, Ending::Stopped(Limit::Time(_))),
        "{report:?}"
    );
    assert!(report.elapsed < Duration::from_secs(20));
    assert!(!report.succeeded());
}

#[test]
fn too_many_calls_stop_a_plugin() {
    if find_python().is_none() {
        return;
    }
    let dir = temp("calls").join("p");
    let code = format!("{PY_CLIENT}\nwhile True:\n    call('layers')\n");
    let plugin = make(&dir, "main.py", r#""read""#, Some(30), &code);
    let mut ed = editor();
    let options = RunOptions {
        max_calls: 5,
        ..RunOptions::default()
    };
    let report = run(&mut ed, &plugin, &options).unwrap();
    assert_eq!(report.ending, Ending::Stopped(Limit::Calls(5)), "{report:?}");
}

#[test]
fn a_failing_plugin_reports_its_exit_code_and_error_output() {
    if find_python().is_none() {
        return;
    }
    let dir = temp("fail").join("p");
    let plugin = make(
        &dir,
        "main.py",
        "",
        Some(30),
        "import sys\nsys.stderr.write('boom')\nsys.exit(7)\n",
    );
    let report = run(&mut editor(), &plugin, &RunOptions::default()).unwrap();
    assert_eq!(report.ending, Ending::Exited(Some(7)));
    assert!(report.stderr.contains("boom"));
    assert!(report.summary().contains("exit code 7"));
}

#[test]
fn a_node_plugin_runs_the_same_way() {
    if find_node().is_none() {
        eprintln!("skipped: no Node on this computer");
        return;
    }
    let dir = temp("node").join("p");
    let code = r#"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin });
process.stdout.write(JSON.stringify({ id: 1, method: 'add_layer', params: { kind: 'group', name: 'from node' } }) + '\n');
rl.on('line', () => { process.exit(0); });
"#;
    let plugin = make(&dir, "main.js", r#""read", "edit""#, Some(30), code);
    let mut ed = editor();
    let report = run(&mut ed, &plugin, &RunOptions::default()).unwrap();
    assert!(report.succeeded(), "{report:?}");
    assert_eq!(ed.doc.layers.len(), 1);
}

#[test]
fn a_plugin_that_cannot_start_is_an_error_not_a_report() {
    let dir = temp("nostart").join("p");
    let plugin = make(&dir, "tool.exe", "", None, "not a program");
    let err = run(&mut editor(), &plugin, &RunOptions::default()).unwrap_err();
    assert!(matches!(err, PluginError::Spawn(_)), "{err}");

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        r#"{"name": "NoEntry", "version": "1", "api": "0.1", "permissions": []}"#,
    )
    .unwrap();
    let plugin = Plugin::load(&dir).unwrap();
    assert!(matches!(
        run(&mut editor(), &plugin, &RunOptions::default()),
        Err(PluginError::NoEntry(_))
    ));
}

#[test]
fn a_handler_answers_the_plugins_calls_and_a_stop_ends_a_quiet_plugin() {
    if find_python().is_none() {
        return;
    }
    // the handler stands in for the app's UI thread: it answers every line itself
    let dir = temp("with").join("p");
    let code = format!(
        r#"{PY_CLIENT}
a = call("layers")
sys.exit(0 if a["result"] == "from the handler" else 4)
"#
    );
    let plugin = make(&dir, "main.py", r#""read""#, Some(30), &code);
    let mut seen = Vec::new();
    let report = fanimate_plugin::run_with(&plugin, &RunOptions::default(), &mut |line, _| {
        seen.push(line.to_owned());
        (
            Some(r#"{"jsonrpc":"2.0","id":1,"result":"from the handler"}"#.to_owned()),
            false,
        )
    })
    .unwrap();
    assert!(report.succeeded(), "{report:?}");
    assert_eq!(seen.len(), 1);
    assert!(seen[0].contains("\"layers\""));

    // a plugin that says nothing is stopped by the flag, long before its time limit
    let quiet = make(
        &temp("cancel").join("p"),
        "main.py",
        "",
        Some(60),
        "import time\ntime.sleep(60)\n",
    );
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopper = flag.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        stopper.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let options = RunOptions {
        cancel: Some(flag),
        ..RunOptions::default()
    };
    let report = fanimate_plugin::run_with(&quiet, &options, &mut |_, _| (None, false)).unwrap();
    assert_eq!(report.ending, Ending::Stopped(Limit::Cancelled), "{report:?}");
    assert!(report.elapsed < Duration::from_secs(10));
    assert!(report.summary().contains("you stopped it"));
}
