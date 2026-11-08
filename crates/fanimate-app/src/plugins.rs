//! Plugins in the app: Window > Plugins lists the plugins found in the
//! plugins folder, asks the user before one runs, and runs it as a separate
//! program (`fanimate-plugin`) on a worker thread. Each request line the
//! plugin writes is handed to the UI thread and answered in [`pump`], so the
//! plugin changes the project through the same API, permissions and editor as
//! a script does and never from another thread. The whole run is one undo
//! step, and Stop ends it.
//!
//! What a user grants is exactly what the plugin's manifest declares: the
//! prompt lists each permission in words, and "Always allow" remembers that
//! name, version and permission list (a new version, or a manifest that asks
//! for more, asks again). The grants file is `plugin-grants.json` in the
//! user's data folder. A plugin is a program and is **not sandboxed from the
//! computer**; the prompt says so.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use eframe::egui::{self, RichText, vec2};
use fanimate_api::{Api, ApiError, ErrorCode, Permission, Permissions, rpc};
use fanimate_editor::StatusKind;
use fanimate_plugin::{Handled, Plugin, RunOptions, RunReport};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::Shell;
use crate::kit::{self, Backdrop};

const GRANTS_FILE: &str = "plugin-grants.json";
/// How long a plugin's request waits for the UI thread to answer it.
const ANSWER_WITHIN: Duration = Duration::from_secs(120);

/// One request line from a running plugin, and where its answer goes.
struct Job {
    line: String,
    reply: Sender<Handled>,
}

/// A run in progress.
struct Running {
    name: String,
    permissions: Permissions,
    cancel: Arc<AtomicBool>,
    done: Receiver<Result<RunReport, String>>,
    /// The undo history when the run began, to make the run one step.
    mark: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Grant {
    name: String,
    version: String,
    permissions: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct GrantsFile {
    #[serde(default)]
    allowed: Vec<Grant>,
}

/// What the app remembers about plugins.
pub struct Plugins {
    pub found: Vec<Plugin>,
    pub problems: Vec<(PathBuf, String)>,
    /// A plugin waiting for the user's yes.
    asking: Option<Plugin>,
    /// A plugin to start on the next frame (it needs the frame's context).
    start_next: Option<Plugin>,
    grants: GrantsFile,
    running: Option<Running>,
    jobs_tx: Sender<Job>,
    jobs_rx: Receiver<Job>,
    loaded: bool,
}

impl Default for Plugins {
    fn default() -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel();
        Self {
            found: Vec::new(),
            problems: Vec::new(),
            asking: None,
            start_next: None,
            grants: GrantsFile::default(),
            running: None,
            jobs_tx,
            jobs_rx,
            loaded: false,
        }
    }
}

fn grants_path() -> Option<PathBuf> {
    eframe::storage_dir("Fanimate").map(|d| d.join(GRANTS_FILE))
}

fn names(p: Permissions) -> Vec<String> {
    p.names().into_iter().map(str::to_owned).collect()
}

impl Plugins {
    /// Read the plugins folder, and the grants the first time.
    pub fn refresh(&mut self) {
        let found = fanimate_plugin::discover(&fanimate_plugin::folders(&[]));
        self.found = found.plugins;
        self.problems = found.problems;
        if !self.loaded {
            self.loaded = true;
            self.grants = grants_path()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// Whether the user said "always" to exactly what this manifest asks for.
    fn allowed(&self, plugin: &Plugin) -> bool {
        let m = &plugin.manifest;
        let wanted = names(m.permissions);
        self.grants
            .allowed
            .iter()
            .any(|g| g.name == m.name && g.version == m.version && g.permissions == wanted)
    }

    fn remember(&mut self, plugin: &Plugin) {
        let m = &plugin.manifest;
        self.grants.allowed.retain(|g| g.name != m.name);
        self.grants.allowed.push(Grant {
            name: m.name.clone(),
            version: m.version.clone(),
            permissions: names(m.permissions),
        });
        if let Some(path) = grants_path() {
            let part = path.with_extension("json.part");
            let text = serde_json::to_string_pretty(&self.grants).unwrap_or_default();
            // a grant that could not be written is asked again next time, which is safe
            let _ = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&part, text))
                .and_then(|()| std::fs::rename(&part, &path));
        }
    }

    /// The user picked `plugin`: run it if it was allowed before, else ask.
    fn request(&mut self, plugin: Plugin) {
        if self.running.is_some() {
            return;
        }
        if self.allowed(&plugin) {
            self.start_next = Some(plugin);
        } else {
            self.asking = Some(plugin);
        }
    }
}

/// Window > Plugins.
pub fn menu(ui: &mut egui::Ui, shell: &mut Shell) {
    if !shell.plugins.loaded {
        shell.plugins.refresh();
    }
    let running = shell.plugins.is_running();
    if shell.plugins.found.is_empty() {
        ui.add_enabled(false, egui::Button::new("No plugins installed"));
    }
    let no_project = shell.docs.home_front();
    let mut chosen = None;
    for (i, plugin) in shell.plugins.found.iter().enumerate() {
        let m = &plugin.manifest;
        let can = !running && m.entry.is_some() && !no_project;
        let mut r = ui.add_enabled(can, egui::Button::new(format!("{} {}", m.name, m.version)));
        if !m.description.is_empty() {
            r = r.on_hover_text(&m.description);
        }
        if r.clicked() {
            chosen = Some(i);
            ui.close();
        }
    }
    if let Some(i) = chosen {
        let plugin = shell.plugins.found[i].clone();
        shell.plugins.request(plugin);
    }
    ui.separator();
    if running && ui.button("Stop Plugin").clicked() {
        stop(shell);
        ui.close();
    }
    if ui.button("Reload Plugin List").clicked() {
        shell.plugins.refresh();
    }
    if ui.button("Open Plugins Folder").clicked() {
        if let Some(dir) = fanimate_plugin::user_folder() {
            let _ = std::fs::create_dir_all(&dir);
            crate::links::open(&dir.to_string_lossy());
        }
        ui.close();
    }
    for (dir, why) in &shell.plugins.problems {
        let name = dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        ui.add_enabled(false, egui::Button::new(format!("{name}: {why}")));
    }
}

/// Ask the running plugin to end.
pub fn stop(shell: &mut Shell) {
    if let Some(run) = &shell.plugins.running {
        run.cancel.store(true, Ordering::Relaxed);
    }
}

/// The answer to one request line, from the UI thread, for a plugin that has
/// `permissions`. `quit` and `shutdown` end the run; `save` is the user's
/// (File > Save): a plugin does not write the project from the app.
fn answer_line(editor: &mut fanimate_editor::Editor, line: &str, permissions: Permissions) -> Handled {
    let request: Option<Value> = serde_json::from_str(line).ok();
    let method = request
        .as_ref()
        .and_then(|r| r.get("method"))
        .and_then(Value::as_str);
    let notification = request.as_ref().is_some_and(|r| r.get("id").is_none());
    let id = request
        .as_ref()
        .and_then(|r| r.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    match method {
        Some("quit" | "shutdown") => ((!notification).then(|| rpc::reply(id, Value::Null)), true),
        Some("save") => {
            let e = ApiError::new(
                ErrorCode::InvalidArgument,
                "a plugin run from Fanimate does not save: use File > Save when it has finished",
            );
            ((!notification).then(|| rpc::failure(id, e)), false)
        }
        _ => (
            rpc::handle_line_as(&mut Api::new(editor), line, permissions),
            false,
        ),
    }
}

fn refusal(line: &str, message: &str) -> String {
    let id = serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|r| r.get("id").cloned())
        .unwrap_or(Value::Null);
    rpc::failure(id, ApiError::new(ErrorCode::NotFound, message))
}

fn start(ctx: &egui::Context, shell: &mut Shell, plugin: Plugin) {
    if shell.plugins.running.is_some() {
        return;
    }
    // nothing is edited while the animation plays, so a plugin starts with it paused
    shell.editor.playing = false;
    let cancel = Arc::new(AtomicBool::new(false));
    let options = RunOptions {
        cancel: Some(cancel.clone()),
        ..RunOptions::default()
    };
    let (done_tx, done) = mpsc::channel();
    let (jobs, repaint, worker) = (shell.plugins.jobs_tx.clone(), ctx.clone(), plugin.clone());
    let spawned = std::thread::Builder::new()
        .name("fanimate-plugin".into())
        .spawn(move || {
            let result = fanimate_plugin::run_with(&worker, &options, &mut |line, _| {
                let (reply, answer) = mpsc::channel();
                let job = Job {
                    line: line.to_owned(),
                    reply,
                };
                if jobs.send(job).is_err() {
                    return (None, true);
                }
                repaint.request_repaint();
                answer.recv_timeout(ANSWER_WITHIN).unwrap_or((None, true))
            })
            .map_err(|e| e.to_string());
            let _ = done_tx.send(result);
            repaint.request_repaint();
        });
    if let Err(e) = spawned {
        shell
            .editor
            .set_status(StatusKind::Error, format!("The plugin could not be started: {e}"));
        return;
    }
    let name = plugin.manifest.name.clone();
    shell.plugins.running = Some(Running {
        name: name.clone(),
        permissions: plugin.manifest.permissions,
        cancel,
        done,
        mark: shell.editor.history_mark(),
    });
    shell.editor.set_status(
        StatusKind::Info,
        format!("Running {name}… (Window > Plugins > Stop Plugin ends it)"),
    );
}

/// Each frame: start a plugin that was chosen, answer the requests of the
/// one that runs, and report when it ends.
pub fn pump(ctx: &egui::Context, shell: &mut Shell) {
    if let Some(plugin) = shell.plugins.start_next.take() {
        start(ctx, shell, plugin);
    }
    while let Ok(job) = shell.plugins.jobs_rx.try_recv() {
        let handled = match &shell.plugins.running {
            None => (Some(refusal(&job.line, "the plugin has ended")), true),
            Some(_) if shell.docs.home_front() => {
                (Some(refusal(&job.line, "no project is open in Fanimate")), false)
            }
            Some(run) => answer_line(&mut shell.editor, &job.line, run.permissions),
        };
        let _ = job.reply.send(handled);
    }
    let Some(run) = &shell.plugins.running else {
        return;
    };
    let outcome = match run.done.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        Err(TryRecvError::Disconnected) => Err("the plugin stopped unexpectedly".to_owned()),
    };
    let Some(run) = shell.plugins.running.take() else {
        return;
    };
    // everything the run did is one undo step
    let merged = shell
        .editor
        .merge_steps_since(run.mark, &format!("Plugin {}", run.name));
    let undo = if merged || shell.editor.history_mark() > run.mark {
        " Undo takes back all of it."
    } else {
        ""
    };
    match outcome {
        Ok(report) if report.succeeded() => shell.editor.set_status(
            StatusKind::Success,
            format!("{}: {}.{undo}", run.name, report.summary()),
        ),
        Ok(report) => {
            let first = report.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            let detail = if first.is_empty() {
                String::new()
            } else {
                format!(" ({first})")
            };
            shell.editor.set_status(
                StatusKind::Warning,
                format!("{}: {}{detail}.{undo}", run.name, report.summary()),
            );
        }
        Err(e) => shell
            .editor
            .set_status(StatusKind::Error, format!("{} could not run: {e}", run.name)),
    }
}

/// The permission prompt.
pub fn window(ctx: &egui::Context, shell: &mut Shell) {
    let Some(plugin) = shell.plugins.asking.clone() else {
        return;
    };
    let p = shell.palette;
    let m = &plugin.manifest;
    let (mut once, mut always, mut cancel) = (false, false, false);
    let closed = kit::modal(
        ctx,
        &p,
        "plugin-prompt",
        "Run this plugin?",
        vec2(460.0, 270.0),
        Backdrop::Ignored,
        |ui| {
            ui.label(RichText::new(format!("{} {}", m.name, m.version)).strong());
            if !m.description.is_empty() {
                ui.label(RichText::new(&m.description).color(p.text_muted));
            }
            ui.add_space(8.0);
            ui.label("It asks to be allowed to:");
            let mut any = false;
            for permission in Permission::ALL {
                if m.permissions.contains(permission) {
                    any = true;
                    ui.label(format!("  •  {}", permission.about()));
                }
            }
            if !any {
                ui.label("  •  do nothing to the project (it asks for no permissions)");
            }
            ui.add_space(8.0);
            kit::note(
                ui,
                &p,
                "A plugin is a program. Fanimate limits what it can do to your project, not what it can do to your computer: run only plugins you trust. Everything it does to the project is one undo step.",
            );
            kit::footer(ui, |ui| {
                if ui.add(kit::primary(&p, "Run")).clicked() {
                    once = true;
                }
                if ui.button("Always allow and run").clicked() {
                    always = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        },
    );
    if once || always {
        shell.plugins.asking = None;
        if always {
            shell.plugins.remember(&plugin);
        }
        shell.plugins.start_next = Some(plugin);
    } else if closed || cancel {
        shell.plugins.asking = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::Document;
    use fanimate_editor::Editor;
    use serde_json::json;

    fn editor() -> Editor {
        Editor::new(Document::new("t", 800.0, 600.0, 24.0))
    }

    fn plugin(version: &str, permissions: &str) -> Plugin {
        let text = format!(
            r#"{{"name": "Tool", "version": "{version}", "api": "0.1", "entry": "main.py", "permissions": [{permissions}]}}"#
        );
        Plugin::from_text(&text, PathBuf::from(".")).unwrap()
    }

    #[test]
    fn a_grant_covers_exactly_the_name_version_and_permissions_it_was_given_for() {
        let mut plugins = Plugins::default();
        let one = plugin("1.0", r#""read", "edit""#);
        assert!(!plugins.allowed(&one));
        plugins.grants.allowed.push(Grant {
            name: "Tool".into(),
            version: "1.0".into(),
            permissions: names(one.manifest.permissions),
        });
        assert!(plugins.allowed(&one));
        // a new version, or a manifest that now asks for more, asks again
        assert!(!plugins.allowed(&plugin("1.1", r#""read", "edit""#)));
        assert!(!plugins.allowed(&plugin("1.0", r#""read", "edit", "save""#)));
        // a plugin already allowed starts at once, another is asked about
        plugins.request(one.clone());
        assert!(plugins.start_next.is_some() && plugins.asking.is_none());
        let mut fresh = Plugins::default();
        fresh.request(one);
        assert!(fresh.start_next.is_none() && fresh.asking.is_some());
    }

    #[test]
    fn requests_are_answered_with_the_plugins_permissions_and_the_hosts_own_rules() {
        let mut ed = editor();
        let all = Permissions::all();
        let read_only = Permissions::NONE.with(Permission::Read);
        let add = json!({"jsonrpc": "2.0", "id": 1, "method": "add_layer", "params": {"kind": "vector", "name": "x"}}).to_string();
        // refused without edit, and nothing changes
        let (answer, quit) = answer_line(&mut ed, &add, read_only);
        assert!(answer.unwrap().contains("permission_denied") && !quit);
        assert!(ed.doc.layers.is_empty());
        // allowed with it
        let (answer, _) = answer_line(&mut ed, &add, all);
        assert!(answer.unwrap().contains("result"));
        assert_eq!(ed.doc.layers.len(), 1);
        // save belongs to the user; quit ends the run
        let save = json!({"id": 2, "method": "save"}).to_string();
        let (answer, quit) = answer_line(&mut ed, &save, all);
        assert!(answer.unwrap().contains("does not save") && !quit);
        let (answer, quit) = answer_line(
            &mut ed,
            &json!({"id": 3, "method": "quit"}).to_string(),
            read_only,
        );
        assert!(answer.is_some() && quit);
        // a notification gets no answer, and nonsense gets an error, not a panic
        let (answer, _) = answer_line(&mut ed, &json!({"method": "quit"}).to_string(), all);
        assert!(answer.is_none());
        let (answer, quit) = answer_line(&mut ed, "not json", all);
        assert!(answer.unwrap().contains("error") && !quit);
    }

    #[test]
    fn a_refusal_keeps_the_requests_id() {
        let text = refusal(r#"{"id": 7, "method": "layers"}"#, "no project");
        assert!(text.contains("\"id\":7") && text.contains("no project"), "{text}");
        assert!(refusal("garbage", "x").contains("\"id\":null"));
    }
}
