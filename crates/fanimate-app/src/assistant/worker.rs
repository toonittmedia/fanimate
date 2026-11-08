//! The Assistant's own model, run on a thread so the window never waits for
//! the network. The thread talks to the provider and runs the agent loop; its
//! tool calls go back to the UI thread as [`Job`]s, so the editor is only ever
//! touched where the rest of the app touches it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use eframe::egui;
use fanimate_agent::agent::{self, Event as AgentEvent, Outcome, Settings, ToolHost};
use fanimate_agent::anthropic::Anthropic;
use fanimate_agent::http::Curl;
use fanimate_agent::mcp::ToolError;
use fanimate_agent::openai::OpenAi;
use fanimate_agent::prompt::GUIDE;
use fanimate_agent::provider::{Message, Provider, ProviderError};
use fanimate_agent::{link, tools};
use serde_json::{Value, json};

use super::ProviderKind;
use super::host::{Job, Origin};

/// How long one tool call may wait for the window to run it.
const TOOL_WITHIN: Duration = Duration::from_secs(120);

/// Everything a run needs to reach a provider.
#[derive(Clone)]
pub struct Config {
    pub kind: ProviderKind,
    pub model: String,
    pub name: String,
    pub url: String,
    pub key: String,
}

/// Something the thread tells the window.
pub enum Event {
    /// The tool it is on, for the status line.
    Working(String),
    /// Words from the model.
    Said(String),
    /// The end, with the conversation so far for the next message.
    Done(Result<Outcome, ProviderError>, Vec<Message>),
}

/// A message being worked on.
pub struct Run {
    pub cancel: Arc<AtomicBool>,
    pub events: Receiver<Event>,
}

fn provider(config: &Config) -> Box<dyn Provider + Send> {
    match config.kind {
        ProviderKind::Anthropic => Box::new(Anthropic::new(Curl::default(), config.key.clone())),
        ProviderKind::OpenRouter => Box::new(OpenAi::openrouter(Curl::default(), config.key.clone())),
        ProviderKind::Custom => Box::new(OpenAi::new(
            Curl::default(),
            config.name.clone(),
            config.url.clone(),
            config.key.clone(),
        )),
    }
}

/// Start the model on a message. `history` is the conversation so far.
pub fn start(
    ctx: egui::Context,
    config: Config,
    history: Vec<Message>,
    text: String,
    jobs: Sender<Job>,
) -> Run {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, events) = mpsc::channel();
    let flag = cancel.clone();
    let spawned = std::thread::Builder::new()
        .name("fanimate-assistant".into())
        .spawn({
            let tx = tx.clone();
            let ctx = ctx.clone();
            move || {
                let send = |event: Event| {
                    let _ = tx.send(event);
                    ctx.request_repaint();
                };
                let provider = provider(&config);
                let all = tools::tools();
                let mut history = history;
                let mut host = Window {
                    jobs,
                    ctx: ctx.clone(),
                    cancel: flag.clone(),
                    next: 1,
                };
                let settings = Settings {
                    system: GUIDE,
                    model: &config.model,
                    tools: &all,
                    max_steps: agent::MAX_STEPS,
                };
                let result = agent::run(
                    provider.as_ref(),
                    &mut host,
                    &settings,
                    &mut history,
                    &text,
                    &flag,
                    &mut |event| match event {
                        AgentEvent::Said(words) => send(Event::Said(words)),
                        AgentEvent::Tool { name, .. } => send(Event::Working(name)),
                    },
                );
                agent::trim_history(&mut history, 80);
                send(Event::Done(result, history));
            }
        });
    if spawned.is_err() {
        let _ = tx.send(Event::Done(
            Err(ProviderError::Transport("could not start the assistant".into())),
            Vec::new(),
        ));
    }
    Run { cancel, events }
}

/// The tools, which run in the window: each call is a request line the UI
/// thread answers.
struct Window {
    jobs: Sender<Job>,
    ctx: egui::Context,
    cancel: Arc<AtomicBool>,
    next: u64,
}

impl ToolHost for Window {
    fn call(&mut self, name: &str, input: &Value) -> Result<Value, ToolError> {
        let id = self.next;
        self.next += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": name, "params": input}).to_string();
        let (reply, answer) = mpsc::channel();
        self.jobs
            .send(Job {
                line,
                origin: Origin::Assistant,
                reply,
                cancel: Some(self.cancel.clone()),
            })
            .map_err(|_| ToolError::new("unavailable", "Fanimate is closing"))?;
        self.ctx.request_repaint();
        let started = Instant::now();
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(ToolError::new("cancelled", "stopped by the user"));
            }
            match answer.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(text)) => {
                    let value: Value = serde_json::from_str(&text)
                        .map_err(|_| ToolError::new("error", "the answer was not readable"))?;
                    return link::answer_of(&value);
                }
                Ok(None) => return Ok(Value::Null),
                Err(RecvTimeoutError::Timeout) if started.elapsed() < TOOL_WITHIN => {}
                Err(_) => return Err(ToolError::new("unavailable", "the window did not answer")),
            }
        }
    }
}
