//! `fanimate-cli plugin ...`: make, check, run, pack, install and list
//! plugins. The host itself is the `fanimate-plugin` crate; this is the
//! command line over it. A plugin runs as a separate program that speaks the
//! scripting API with only the permissions its manifest declares.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use fanimate_plugin::{Ending, Plugin, PluginError, RunOptions};

use super::{Args, CmdResult, USAGE, describe, read_manifest, script};

pub fn command(args: &[String]) -> CmdResult {
    let Some((sub, rest)) = args.split_first() else {
        return Err(USAGE.into());
    };
    match sub.as_str() {
        "check" => check(rest),
        "list" => list(rest),
        "new" => new(rest),
        "run" => run(rest, true),
        "test" => run(rest, false),
        "pack" => pack(rest),
        "install" => install(rest),
        other => Err(format!("unknown plugin command {other}\n{USAGE}")),
    }
}

fn plugin_error(e: PluginError) -> String {
    e.to_string()
}

/// The extra folders named by `--folder` (it may be given more than once).
fn extra_folders(args: &[String]) -> Vec<PathBuf> {
    args.windows(2)
        .filter(|w| w[0] == "--folder")
        .map(|w| PathBuf::from(&w[1]))
        .collect()
}

/// The plugins folder to install into: `--folder`, else the user's.
fn target_folder(args: &[String]) -> Result<PathBuf, String> {
    extra_folders(args)
        .into_iter()
        .next()
        .or_else(fanimate_plugin::user_folder)
        .ok_or_else(|| "this computer names no folder for plugins: give one with --folder".to_owned())
}

/// A plugin named by a folder, a `plugin.json`, or the name of an installed one.
fn find(what: &str, folders: &[PathBuf]) -> Result<Plugin, String> {
    let path = Path::new(what);
    if path.exists() {
        return Plugin::load(path).map_err(plugin_error);
    }
    let found = fanimate_plugin::discover(&fanimate_plugin::folders(folders));
    found
        .plugins
        .into_iter()
        .find(|p| p.manifest.name.eq_ignore_ascii_case(what))
        .ok_or_else(|| {
            format!("no plugin \"{what}\": give its folder or plugin.json, or the name of an installed one (plugin list)")
        })
}

fn check(args: &[String]) -> CmdResult {
    let [path] = args else {
        return Err(USAGE.into());
    };
    let manifest_file = if Path::new(path).is_dir() {
        Path::new(path)
            .join(fanimate_plugin::MANIFEST_FILE)
            .to_string_lossy()
            .into_owned()
    } else {
        path.clone()
    };
    let m = read_manifest(&manifest_file)?;
    println!("{} {} (for API {})", m.name, m.version, m.api);
    if !m.description.is_empty() {
        println!("{}", m.description);
    }
    if let Some(entry) = &m.entry {
        let kind = fanimate_plugin::EntryKind::of(entry);
        println!("starts: {entry} ({})", kind.label());
    }
    println!(
        "permissions: {}",
        if m.permissions == fanimate_api::Permissions::NONE {
            "none".to_owned()
        } else {
            m.permissions.names().join(", ")
        }
    );
    println!("may {}", describe(m.permissions));
    Ok(ExitCode::SUCCESS)
}

fn list(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--folder"])?;
    if !a.positional.is_empty() {
        return Err(USAGE.into());
    }
    let folders = fanimate_plugin::folders(&extra_folders(args));
    let found = fanimate_plugin::discover(&folders);
    if found.plugins.is_empty() {
        println!("no plugins found in:");
        for f in &folders {
            println!("  {}", f.display());
        }
    }
    for p in &found.plugins {
        let m = &p.manifest;
        let perms = if m.permissions == fanimate_api::Permissions::NONE {
            "none".to_owned()
        } else {
            m.permissions.names().join(", ")
        };
        println!("{} {}  [{perms}]  {}", m.name, m.version, p.dir.display());
        if !m.description.is_empty() {
            println!("    {}", m.description);
        }
    }
    for (dir, message) in &found.problems {
        eprintln!("fanimate-cli: {}: {message}", dir.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn new(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--lang", "-o"])?;
    let [name] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let lang = a.get::<String>("--lang")?.unwrap_or_else(|| "py".to_owned());
    let (entry, code) = match lang.as_str() {
        "py" | "python" => ("main.py", PYTHON),
        "js" | "javascript" => ("main.mjs", JAVASCRIPT),
        "ts" | "typescript" => ("main.ts", TYPESCRIPT),
        other => return Err(format!("--lang is py, js or ts, not {other}")),
    };
    let dir = a.get::<String>("-o")?.map_or_else(
        || PathBuf::from(fanimate_plugin::folder_name(name)),
        PathBuf::from,
    );
    if dir.exists() {
        return Err(format!("{} exists already", dir.display()));
    }
    let manifest = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "api": fanimate_api::API_VERSION.split('.').take(2).collect::<Vec<_>>().join("."),
        "description": "What this plugin does, in one sentence.",
        "entry": entry,
        "permissions": ["read", "edit"],
        "timeout_seconds": 60,
    });
    // check the name before writing anything
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fanimate_api::Manifest::parse(&text).map_err(|e| format!("{name}: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join(fanimate_plugin::MANIFEST_FILE), format!("{text}\n"))
        .and_then(|()| std::fs::write(dir.join(entry), code))
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    println!("made {} ({entry} and plugin.json)", dir.display());
    println!("try it:  fanimate-cli plugin test {}", dir.display());
    Ok(ExitCode::SUCCESS)
}

/// `run` edits the project and saves it when `--save` is given; `test` runs
/// the plugin on a copy and never saves, then says what changed.
fn run(args: &[String], real: bool) -> CmdResult {
    let a = Args::parse(args, &["--save", "--timeout", "--folder"])?;
    let (what, project) = match a.positional.as_slice() {
        [what] => (what, None),
        [what, project] => (what, Some(project.as_str())),
        _ => return Err(USAGE.into()),
    };
    let plugin = find(what, &extra_folders(args))?;
    let mut editor = script::open(project)?;
    let before = editor.doc.layers.len();
    let save_to = a.get::<String>("--save")?;
    if !real && save_to.is_some() {
        return Err("plugin test never saves: use plugin run --save".to_owned());
    }
    let options = RunOptions {
        timeout: a.get::<u64>("--timeout")?.map(Duration::from_secs),
        save_to: save_to.as_ref().map(PathBuf::from),
        ..RunOptions::default()
    };
    eprintln!(
        "fanimate-cli: {} {} may {}",
        plugin.manifest.name,
        plugin.manifest.version,
        describe(plugin.manifest.permissions)
    );
    let report = fanimate_plugin::run(&mut editor, &plugin, &options).map_err(plugin_error)?;
    if !report.stderr.trim().is_empty() {
        eprintln!("--- the plugin's error output ---\n{}", report.stderr.trim_end());
    }
    println!("{}", report.summary());
    println!(
        "{} call(s), {} failed, {} refused; layers {before} -> {}; {:.1} s",
        report.calls,
        report.failed,
        report.denied,
        editor.doc.layers.len(),
        report.elapsed.as_secs_f32()
    );
    if real && let Some(path) = &save_to {
        // a plugin that did not save leaves the project as the run left it
        if editor.dirty {
            fanimate_fnm::save(&editor.doc.mainline(), path).map_err(|e| e.to_string())?;
            println!("saved {path}");
        }
    }
    let failed = !report.succeeded() || matches!(report.ending, Ending::Exited(Some(c)) if c != 0);
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn pack(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["-o"])?;
    let [dir] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let plugin = Plugin::load(Path::new(dir)).map_err(plugin_error)?;
    let out = a.get::<String>("-o")?.map_or_else(
        || {
            PathBuf::from(format!(
                "{}-{}.{}",
                fanimate_plugin::folder_name(&plugin.manifest.name),
                plugin.manifest.version,
                fanimate_plugin::PACKAGE_EXTENSION
            ))
        },
        PathBuf::from,
    );
    let files = fanimate_plugin::pack(Path::new(dir), &out).map_err(plugin_error)?;
    println!("packed {files} file(s) into {}", out.display());
    Ok(ExitCode::SUCCESS)
}

fn install(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--folder"])?;
    let [file] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let folder = target_folder(args)?;
    let done = fanimate_plugin::install(Path::new(file), &folder).map_err(plugin_error)?;
    let m = &done.plugin.manifest;
    match &done.replaced {
        Some(old) => println!("updated {} from {old} to {}", m.name, m.version),
        None => println!("installed {} {}", m.name, m.version),
    }
    println!("in {} ({} file(s))", done.plugin.dir.display(), done.files);
    println!("it may {}", describe(m.permissions));
    Ok(ExitCode::SUCCESS)
}

const PYTHON: &str = r#""""A Fanimate plugin. Fanimate starts this file and talks to it over standard
input and output: one JSON request per line out, one JSON answer per line in.
The calls are listed by api.methods (see docs/scripting.md)."""
import json
import sys


def call(method, **params):
    request = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    sys.stdout.write(json.dumps(request) + "\n")
    sys.stdout.flush()
    answer = json.loads(sys.stdin.readline())
    if "error" in answer:
        raise RuntimeError(answer["error"]["message"])
    return answer["result"]


layer = call("add_layer", kind="vector", name="Hello from a plugin")
print("made layer", layer, file=sys.stderr)  # anything printed to stderr is shown to the user
"#;

const JAVASCRIPT: &str = r#"// A Fanimate plugin. Fanimate starts this file with Node and talks to it over
// standard input and output: one JSON request per line out, one answer per
// line in. The calls are listed by api.methods (see docs/scripting.md).
import readline from 'node:readline';

const lines = readline.createInterface({ input: process.stdin })[Symbol.asyncIterator]();

async function call(method, params = {}) {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) + '\n');
  const { value } = await lines.next();
  const answer = JSON.parse(value);
  if (answer.error) throw new Error(answer.error.message);
  return answer.result;
}

const layer = await call('add_layer', { kind: 'vector', name: 'Hello from a plugin' });
console.error('made layer', layer); // anything printed to stderr is shown to the user
process.exit(0);
"#;

const TYPESCRIPT: &str = r#"// A Fanimate plugin in TypeScript. It needs Node 22.6 or later, which strips the
// types when it runs the file. Fanimate talks to it over standard input and
// output: one JSON request per line out, one answer per line in.
import readline from 'node:readline';

const lines = readline.createInterface({ input: process.stdin })[Symbol.asyncIterator]();

async function call(method: string, params: Record<string, unknown> = {}): Promise<unknown> {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) + '\n');
  const { value } = await lines.next();
  const answer = JSON.parse(value as string);
  if (answer.error) throw new Error(answer.error.message);
  return answer.result;
}

const layer = await call('add_layer', { kind: 'vector', name: 'Hello from a plugin' });
console.error('made layer', layer); // anything printed to stderr is shown to the user
process.exit(0);
"#;
