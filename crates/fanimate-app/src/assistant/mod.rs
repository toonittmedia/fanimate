//! The Assistant: a model that works on the open project through the API, in
//! a panel beside the canvas, and the local socket other agents (Claude Code,
//! Codex, Antigravity) use to do the same.
//!
//! Everything funnels into one place. The Assistant's model runs on a thread
//! (`worker.rs`), a program on this computer connects to the socket
//! (`host.rs`), and both hand their requests to the UI thread as [`Job`]s.
//! [`pump`] runs them each frame through `fanimate_api::rpc` on the live
//! editor, so every call is a transaction (one undo step), is checked against
//! the same permissions a script gets, and labels what it creates as made by
//! a tool in the file. Nothing here edits a project from another thread.

mod host;
mod keys;
mod settings;
mod worker;

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use fanimate_agent::describe::{self, Step};
use fanimate_agent::prompt::Context;
use fanimate_agent::provider::{Message, ProviderError};
use fanimate_agent::{agent, tools};
use fanimate_api::{Api, Permission, Permissions, rpc};
use fanimate_core::{LayerId, Provenance};
use fanimate_editor::Editor;
use serde_json::{Value, json};

pub use host::{Job, Origin};
pub use keys::{Keys, Source};
pub use settings::{AiSettings, ProviderKind};

use crate::app::Shell;

/// Ollama's address for the OpenAI-style API, the default for the other provider.
pub const OLLAMA_URL: &str = "http://localhost:11434/v1";

/// One thing in the transcript.
pub enum Entry {
    /// What the user asked.
    You(String),
    /// What the model said.
    Reply(String),
    /// What was changed, with a way to take it back.
    Changes(Changes),
    /// A note from Fanimate (stopped, out of steps).
    Notice(String),
    /// Something went wrong, and what to do.
    Problem(String),
}

/// The changes one request made, as one card.
pub struct Changes {
    /// "Assistant" or "Another program".
    pub who: &'static str,
    pub lines: Vec<String>,
    /// The layers they were about, to select.
    pub layers: Vec<u32>,
    /// The first frame they were about, to go to.
    pub frame: Option<i32>,
    /// The history state before the first change and after the last, so the
    /// whole card is one Undo.
    pub before: usize,
    pub after: usize,
    pub undone: bool,
}

pub struct Assistant {
    pub entries: Vec<Entry>,
    pub input: String,
    /// Put the cursor in the message box on the next frame.
    pub focus_input: bool,
    pub settings_open: bool,
    /// The key being typed. Never saved anywhere but `keys.rs`, and cleared
    /// as soon as it is.
    pub key_draft: String,
    /// What saving or forgetting a key said.
    pub key_message: Option<String>,
    /// Scroll the transcript to its end on the next frame.
    pub scroll_to_end: bool,
    /// The conversation the model has seen.
    history: Vec<Message>,
    run: Option<worker::Run>,
    working: Option<String>,
    jobs_tx: Sender<Job>,
    jobs_rx: Receiver<Job>,
    host: Option<host::Host>,
    /// Why the socket for other programs could not start.
    pub host_error: Option<String>,
}

impl Default for Assistant {
    fn default() -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel();
        Self {
            entries: Vec::new(),
            input: String::new(),
            focus_input: false,
            settings_open: false,
            key_draft: String::new(),
            key_message: None,
            scroll_to_end: false,
            history: Vec::new(),
            run: None,
            working: None,
            jobs_tx,
            jobs_rx,
            host: None,
            host_error: None,
        }
    }
}

impl Assistant {
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// What it is doing now, for the line under the transcript.
    pub fn working(&self) -> Option<&str> {
        self.working.as_deref()
    }

    /// Whether other programs can connect now.
    pub fn linked(&self) -> bool {
        self.host.is_some()
    }

    /// Stop the message being worked on. Changes already made stay (and can
    /// be undone from their card); nothing more is made.
    pub fn stop(&mut self) {
        if let Some(run) = &self.run {
            run.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Start the conversation over.
    pub fn clear(&mut self) {
        self.stop();
        self.entries.clear();
        self.history.clear();
    }

    /// Note a change in the transcript, joining the card before it when it
    /// continues it (the same asker, nothing in between, not undone).
    fn note(&mut self, origin: Origin, change: Change) {
        let who = match origin {
            Origin::Assistant => "Assistant",
            Origin::Program => "Another program",
        };
        if let Some(Entry::Changes(card)) = self.entries.last_mut()
            && card.who == who
            && !card.undone
            && card.after == change.before
        {
            card.lines.push(change.step.text);
            card.after = change.after;
            card.layers
                .extend(change.step.layer.filter(|id| !card.layers.contains(id)));
            card.frame = card.frame.or(change.step.frame);
        } else {
            self.entries.push(Entry::Changes(Changes {
                who,
                lines: vec![change.step.text],
                layers: change.step.layer.into_iter().collect(),
                frame: change.step.frame,
                before: change.before,
                after: change.after,
                undone: false,
            }));
        }
        self.scroll_to_end = true;
    }
}

/// A change a request made.
pub struct Change {
    pub step: Step,
    pub before: usize,
    pub after: usize,
}

/// What running a request did.
pub struct Ran {
    pub answer: Option<String>,
    pub change: Option<Change>,
}

fn refusal(request: &Value, code: &str, message: &str) -> Option<String> {
    let id = request.get("id")?.clone();
    Some(
        json!({"jsonrpc": "2.0", "id": id,
               "error": {"code": -32000, "message": message, "data": {"code": code}}})
        .to_string(),
    )
}

/// The record put on every layer a request creates.
fn stamp(origin: Origin, model: &str) -> Provenance {
    let mut record = Provenance::new(match origin {
        Origin::Assistant => "assistant",
        Origin::Program => "external-agent",
    });
    if origin == Origin::Assistant {
        record.model = model.chars().take(200).collect();
    }
    record.version = env!("CARGO_PKG_VERSION").to_owned();
    record.date = fanimate_core::provenance::today();
    record
}

/// Run one request line on the editor. What it may do is what a script with
/// `read` and `edit` may do: never save, never quit.
pub fn run_request(editor: &mut Editor, line: &str, origin: Origin, model: &str) -> Ran {
    let request: Value = serde_json::from_str(line).unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let edits = rpc::required(&method) == Some(Permission::Edit);
    // an agent that is working pauses the playback it would otherwise be refused by
    if edits && editor.playing {
        editor.playing = false;
    }
    let before = editor.history.cursor();
    let permissions = Permissions::NONE.with(Permission::Read).with(Permission::Edit);
    let answer = rpc::handle_line_as(
        &mut Api::with_provenance(editor, stamp(origin, model)),
        line,
        permissions,
    );
    let after = editor.history.cursor();
    let change = (edits && after != before)
        .then(|| {
            let reply: Value = serde_json::from_str(answer.as_deref()?).ok()?;
            let result = reply.get("result")?;
            let doc = &editor.doc;
            let step = describe::step(&method, &params, result, &|id| {
                doc.find(LayerId(id)).map(|l| l.name.clone())
            })?;
            Some(Change { step, before, after })
        })
        .flatten();
    Ran { answer, change }
}

/// Why the Assistant cannot be used now, or nothing.
pub fn unavailable(shell: &Shell) -> Option<String> {
    if shell.ai.off {
        return Some("AI features are turned off. Turn them on in Preferences.".to_owned());
    }
    if !shell.policy().ai_allowed() {
        return Some("AI features are turned off by this computer's policy.".to_owned());
    }
    None
}

/// Where the user is, for the model and for the line under the message box.
pub fn context(shell: &Shell) -> Context {
    let doc = &shell.editor.doc;
    let mut layers = 0;
    doc.walk(|_| layers += 1);
    Context {
        project: doc.name.clone(),
        width: doc.width as u32,
        height: doc.height as u32,
        fps: doc.fps,
        start: doc.start,
        end: doc.end,
        layers,
        frame: shell.editor.current_frame(),
        selection: shell
            .editor
            .selection
            .iter()
            .filter_map(|id| doc.find(*id).map(|l| (id.0, l.name.clone())))
            .collect(),
    }
}

/// Everything a message needs, or what is missing, said so a person can fix it.
fn prepare(shell: &Shell) -> Result<worker::Config, String> {
    if let Some(why) = unavailable(shell) {
        return Err(why);
    }
    if shell.docs.home_front() {
        return Err("Open or create a project first.".to_owned());
    }
    let ai = &shell.ai;
    if !ai.is_local() && !shell.policy().network_allowed() {
        return Err(format!(
            "Fanimate is working offline, so it cannot reach {}. Turn off Work offline in Preferences, or use a local model.",
            ai.name()
        ));
    }
    if !ai.is_allowed() {
        return Err(format!(
            "Allow sending to {} first (the button above).",
            ai.destination()
        ));
    }
    if ai.model().trim().is_empty() {
        return Err("Choose a model in the settings.".to_owned());
    }
    let key = shell.keys.get(ai.provider).unwrap_or_default();
    if key.is_empty() && !ai.is_local() {
        return Err(format!("Add your {} key in the settings.", ai.provider.label()));
    }
    Ok(worker::Config {
        kind: ai.provider,
        model: ai.model().trim().to_owned(),
        name: ai.name(),
        url: ai.custom_url.trim().to_owned(),
        key,
    })
}

/// Send what is in the message box.
pub fn send(ctx: &egui::Context, shell: &mut Shell) {
    let text = shell.assistant.input.trim().to_owned();
    if text.is_empty() || shell.assistant.run.is_some() {
        return;
    }
    let config = match prepare(shell) {
        Ok(config) => config,
        Err(why) => {
            shell.assistant.entries.push(Entry::Notice(why));
            shell.assistant.scroll_to_end = true;
            return;
        }
    };
    let prompt = format!("{}\n\n{}", context(shell).to_text(), text);
    shell.assistant.input.clear();
    shell.assistant.entries.push(Entry::You(text));
    shell.assistant.scroll_to_end = true;
    shell.editor.playing = false;
    let history = std::mem::take(&mut shell.assistant.history);
    shell.assistant.run = Some(worker::start(
        ctx.clone(),
        config,
        history,
        prompt,
        shell.assistant.jobs_tx.clone(),
    ));
    shell.assistant.working = Some("Thinking".to_owned());
}

/// What the status line says while a tool runs.
fn working_label(tool: &str) -> &'static str {
    if tools::tools().iter().any(|t| t.name == tool && !t.edits) {
        "Looking at the project"
    } else {
        "Making changes"
    }
}

/// Take what the model thread has to say.
fn poll_run(shell: &mut Shell) {
    let Some(run) = &shell.assistant.run else {
        return;
    };
    let mut events = Vec::new();
    while let Ok(event) = run.events.try_recv() {
        events.push(event);
    }
    for event in events {
        let a = &mut shell.assistant;
        match event {
            worker::Event::Working(tool) => a.working = Some(working_label(&tool).to_owned()),
            worker::Event::Said(words) => {
                a.entries.push(Entry::Reply(words));
                a.scroll_to_end = true;
            }
            worker::Event::Done(result, history) => {
                a.run = None;
                a.working = None;
                a.history = history;
                a.scroll_to_end = true;
                match result {
                    Ok(done) => match done.ended {
                        agent::Ended::Done => {
                            if done.text.is_empty() && done.calls == 0 {
                                a.entries
                                    .push(Entry::Notice("The assistant had nothing to say.".into()));
                            }
                        }
                        agent::Ended::TooLong => a.entries.push(Entry::Notice(
                            "The answer ran out of room. Ask it to carry on.".into(),
                        )),
                        agent::Ended::OutOfSteps => a.entries.push(Entry::Notice(format!(
                            "It stopped after {} steps without finishing. Ask it to carry on.",
                            done.calls
                        ))),
                    },
                    Err(ProviderError::Cancelled) => a.entries.push(Entry::Notice("Stopped.".into())),
                    Err(error) => a.entries.push(Entry::Problem(explain(&error))),
                }
            }
        }
    }
}

/// A provider error in words that say what to do.
fn explain(error: &ProviderError) -> String {
    match error {
        ProviderError::Service {
            status: 401 | 403, ..
        } => "The service did not accept the key. Check it in the settings.".to_owned(),
        ProviderError::Service { status: 402, message } => format!("The account has no credit: {message}"),
        ProviderError::Service { status: 404, message } => {
            format!("The service does not know that model: {message}")
        }
        ProviderError::Service { status: 429, .. } => {
            "The service says too many requests. Wait a moment and try again.".to_owned()
        }
        other => other.to_string(),
    }
}

/// Run the requests that arrived, and start or stop the socket. Called once a frame.
pub fn pump(ctx: &egui::Context, shell: &mut Shell) {
    poll_run(shell);
    while let Ok(job) = shell.assistant.jobs_rx.try_recv() {
        if job.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            let request: Value = serde_json::from_str(&job.line).unwrap_or(Value::Null);
            let _ = job
                .reply
                .send(refusal(&request, "unavailable", "stopped by the user"));
            continue;
        }
        let request: Value = serde_json::from_str(&job.line).unwrap_or(Value::Null);
        let answer = if let Some(why) = unavailable(shell) {
            refusal(&request, "permission_denied", &why)
        } else if shell.docs.home_front() {
            refusal(
                &request,
                "not_found",
                "No project is open in Fanimate. Open or create one first.",
            )
        } else {
            let model = shell.ai.model().to_owned();
            let ran = run_request(&mut shell.editor, &job.line, job.origin, &model);
            if let Some(change) = ran.change {
                shell.assistant.note(job.origin, change);
            }
            ran.answer
        };
        let _ = job.reply.send(answer);
    }
    sync_host(ctx, shell);
}

/// Open the socket for other programs when it is switched on and AI is
/// allowed, close it when not.
fn sync_host(ctx: &egui::Context, shell: &mut Shell) {
    let want = shell.ai.programs && unavailable(shell).is_none();
    match (want, shell.assistant.host.is_some()) {
        (true, false) => match host::Host::start(shell.assistant.jobs_tx.clone(), ctx.clone()) {
            Ok(host) => {
                shell.assistant.host = Some(host);
                shell.assistant.host_error = None;
            }
            Err(error) => {
                shell.assistant.host_error = Some(error);
                shell.ai.programs = false;
            }
        },
        (false, true) => shell.assistant.host = None,
        _ => {}
    }
}

/// Take a card's changes back: the project goes to the state before them,
/// as long as nothing has been done since.
pub fn undo_card(shell: &mut Shell, index: usize) {
    let Some(Entry::Changes(card)) = shell.assistant.entries.get_mut(index) else {
        return;
    };
    if card.undone || shell.editor.history.cursor() != card.after {
        return;
    }
    card.undone = true;
    shell
        .pending
        .push(fanimate_editor::Action::GoToHistory(card.before));
}

/// Whether a card can still be undone as a whole.
pub fn card_undoable(shell: &Shell, card: &Changes) -> bool {
    !card.undone && shell.editor.history.cursor() == card.after
}

/// Select what a card was about and go to its frame.
pub fn show_card(shell: &mut Shell, index: usize) {
    let Some(Entry::Changes(card)) = shell.assistant.entries.get(index) else {
        return;
    };
    let (layers, frame) = (card.layers.clone(), card.frame);
    for (i, id) in layers.into_iter().enumerate() {
        let id = LayerId(id);
        if shell.editor.doc.find(id).is_some() {
            let mode = if i == 0 {
                fanimate_editor::SelectMode::Replace
            } else {
                fanimate_editor::SelectMode::Toggle
            };
            shell.editor.select(id, mode);
        }
    }
    if let Some(frame) = frame {
        shell.editor.go_to(frame as _);
    }
}

/// The path of `fanimate-cli`, which is installed beside Fanimate.
pub fn cli_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "fanimate-cli.exe"
    } else {
        "fanimate-cli"
    };
    let path = exe.parent()?.join(name);
    path.exists().then_some(path)
}

/// The command that connects an agent to Fanimate, to copy and run once.
pub fn connect_command(agent: &str, cli: &str) -> String {
    let quoted = if cli.contains(' ') {
        format!("\"{cli}\"")
    } else {
        cli.to_owned()
    };
    match agent {
        "claude" => format!("claude mcp add --scope user fanimate -- {quoted} mcp --app"),
        "codex" => format!("codex mcp add fanimate -- {quoted} mcp --app"),
        _ => format!("agy mcp add fanimate {quoted} mcp --app"),
    }
}

#[cfg(test)]
mod tests {
    use fanimate_core::Document;

    use super::*;

    fn editor() -> Editor {
        Editor::new(Document::new("Scene", 1920.0, 1080.0, 24.0))
    }

    fn request(id: u32, method: &str, params: Value) -> String {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
    }

    #[test]
    fn a_change_is_recorded_with_its_history_span_and_the_layer_is_labelled() {
        let mut ed = editor();
        let before = ed.history.cursor();
        let ran = run_request(
            &mut ed,
            &request(1, "add_layer", json!({"kind": "vector", "name": "Ball"})),
            Origin::Assistant,
            "claude-sonnet-5-5",
        );
        let change = ran.change.expect("a change");
        assert_eq!(change.step.text, "Added layer \"Ball\"");
        assert_eq!((change.before, change.after), (before, before + 1));
        let layer = ed.doc.layers.last().unwrap();
        let record = layer.provenance.as_ref().expect("labelled");
        assert_eq!(record.tool, "assistant");
        assert_eq!(record.model, "claude-sonnet-5-5");
        assert!(record.is_sound(), "{record:?}");
        // another program is labelled as such, with no model named
        run_request(
            &mut ed,
            &request(2, "add_layer", json!({"kind": "vector", "name": "Other"})),
            Origin::Program,
            "ignored",
        );
        let other = ed.doc.layers.last().unwrap().provenance.as_ref().unwrap();
        assert_eq!(
            (other.tool.as_str(), other.model.as_str()),
            ("external-agent", "")
        );
    }

    #[test]
    fn looking_at_the_project_is_not_a_change() {
        let mut ed = editor();
        let ran = run_request(&mut ed, &request(1, "layers", json!({})), Origin::Assistant, "m");
        assert!(ran.change.is_none());
        assert!(ran.answer.unwrap().contains("\"result\""));
        assert!(!ed.dirty);
    }

    #[test]
    fn saving_and_quitting_are_not_something_an_agent_can_do() {
        let mut ed = editor();
        for method in ["save", "shutdown", "quit", "format_disk"] {
            let ran = run_request(
                &mut ed,
                &request(1, method, json!({"path": "x.fnm"})),
                Origin::Program,
                "m",
            );
            let reply: Value = serde_json::from_str(&ran.answer.unwrap()).unwrap();
            assert!(reply.get("error").is_some(), "{method}: {reply}");
        }
    }

    #[test]
    fn a_failed_request_changes_nothing_and_leaves_no_card() {
        let mut ed = editor();
        let ran = run_request(
            &mut ed,
            &request(1, "rename_layer", json!({"id": 99, "name": "x"})),
            Origin::Assistant,
            "m",
        );
        assert!(ran.change.is_none());
        let reply: Value = serde_json::from_str(&ran.answer.unwrap()).unwrap();
        assert_eq!(reply["error"]["data"]["code"], "not_found");
    }

    #[test]
    fn an_agent_pauses_playback_instead_of_being_refused() {
        let mut ed = editor();
        ed.playing = true;
        let ran = run_request(
            &mut ed,
            &request(1, "add_layer", json!({"kind": "vector", "name": "A"})),
            Origin::Assistant,
            "m",
        );
        assert!(!ed.playing);
        assert!(ran.change.is_some());
    }

    #[test]
    fn steps_of_one_request_join_one_card_that_is_one_undo() {
        let mut ed = editor();
        let mut a = Assistant::default();
        let start = ed.history.cursor();
        for (i, name) in ["One", "Two", "Three"].into_iter().enumerate() {
            let ran = run_request(
                &mut ed,
                &request(i as u32, "add_layer", json!({"kind": "vector", "name": name})),
                Origin::Assistant,
                "m",
            );
            a.note(Origin::Assistant, ran.change.unwrap());
        }
        assert_eq!(a.entries.len(), 1);
        let Entry::Changes(card) = &a.entries[0] else {
            panic!("a card")
        };
        assert_eq!(card.lines.len(), 3);
        assert_eq!((card.before, card.after), (start, start + 3));
        // going to the card's `before` takes all three away at once
        ed.run(fanimate_editor::Action::GoToHistory(card.before));
        assert!(ed.doc.layers.is_empty());
    }

    #[test]
    fn a_change_by_someone_else_or_after_the_user_edited_starts_a_new_card() {
        let mut ed = editor();
        let mut a = Assistant::default();
        let one = run_request(
            &mut ed,
            &request(1, "add_layer", json!({"kind": "vector", "name": "A"})),
            Origin::Assistant,
            "m",
        );
        a.note(Origin::Assistant, one.change.unwrap());
        // another program's change is its own card
        let two = run_request(
            &mut ed,
            &request(2, "add_layer", json!({"kind": "vector", "name": "B"})),
            Origin::Program,
            "m",
        );
        a.note(Origin::Program, two.change.unwrap());
        assert_eq!(a.entries.len(), 2);
        // the user edits in between: the Assistant's next change is not joined to the older card
        ed.checkpoint("User edit");
        let three = run_request(
            &mut ed,
            &request(3, "add_layer", json!({"kind": "vector", "name": "C"})),
            Origin::Program,
            "m",
        );
        a.note(Origin::Program, three.change.unwrap());
        assert_eq!(a.entries.len(), 3);
    }

    #[test]
    fn the_connect_commands_quote_a_path_with_spaces() {
        assert_eq!(
            connect_command("claude", "C:\\Program Files\\Fanimate\\fanimate-cli.exe"),
            "claude mcp add --scope user fanimate -- \"C:\\Program Files\\Fanimate\\fanimate-cli.exe\" mcp --app"
        );
        assert_eq!(
            connect_command("codex", "/opt/fanimate/fanimate-cli"),
            "codex mcp add fanimate -- /opt/fanimate/fanimate-cli mcp --app"
        );
        assert_eq!(
            connect_command("agy", "/opt/f/fanimate-cli"),
            "agy mcp add fanimate /opt/f/fanimate-cli mcp --app"
        );
    }

    #[test]
    fn provider_errors_say_what_to_do() {
        let key = explain(&ProviderError::Service {
            status: 401,
            message: "bad".into(),
        });
        assert!(key.contains("key"));
        assert!(
            explain(&ProviderError::Service {
                status: 429,
                message: String::new()
            })
            .contains("too many")
        );
        assert!(explain(&ProviderError::Transport("no route".into())).contains("no route"));
    }

    #[test]
    fn the_status_line_tells_looking_from_making() {
        assert_eq!(working_label("layers"), "Looking at the project");
        assert_eq!(working_label("add_layer"), "Making changes");
    }
}
