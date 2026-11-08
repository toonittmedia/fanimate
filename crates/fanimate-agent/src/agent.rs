//! The agent loop: send the conversation to a model, run the tools it asks
//! for, send the answers back, until it is done, the user stops it, or it has
//! used its steps.
//!
//! The loop never edits the project itself. Each tool call goes to a
//! [`ToolHost`], which in the app is the editor behind the API (so every call
//! is a transaction, one undo step), and everything the model says or does is
//! reported through `on_event` as it happens.

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::mcp::ToolError;
use crate::provider::{Block, Message, Provider, ProviderError, Request, Role, Stop};
use crate::tools::{self, Tool};

/// The most tool calls one message may make. A model that has not finished by
/// then is probably going round in circles.
pub const MAX_STEPS: usize = 40;
/// The longest a tool's answer is kept in the conversation, in characters.
const MAX_RESULT: usize = 20_000;

/// Where the loop's tool calls go.
pub trait ToolHost {
    fn call(&mut self, name: &str, input: &Value) -> Result<Value, ToolError>;
}

/// Something that happened, for the transcript.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The model said something.
    Said(String),
    /// A tool ran.
    Tool {
        name: String,
        input: Value,
        /// What it answered, or why it failed (`code: message`).
        result: Result<String, String>,
    },
}

/// How a message ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The model finished.
    Done,
    /// The model ran out of room in the middle of an answer.
    TooLong,
    /// It used all its steps without finishing.
    OutOfSteps,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// The model's last words.
    pub text: String,
    /// How many tools it called.
    pub calls: usize,
    pub ended: Ended,
}

pub struct Settings<'a> {
    pub system: &'a str,
    pub model: &'a str,
    pub tools: &'a [Tool],
    pub max_steps: usize,
}

/// Run one user message to its end. `history` is the conversation so far and
/// is extended with everything that happens, so the next message continues it.
pub fn run(
    provider: &dyn Provider,
    host: &mut dyn ToolHost,
    settings: &Settings<'_>,
    history: &mut Vec<Message>,
    user_text: &str,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(Event),
) -> Result<Outcome, ProviderError> {
    history.push(Message::user(user_text));
    let mut calls = 0;
    let mut last_text = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }
        let completion = provider.complete(
            &Request {
                system: settings.system,
                messages: history,
                tools: settings.tools,
                model: settings.model,
            },
            cancel,
        )?;
        let text = completion
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            on_event(Event::Said(text.clone()));
            last_text = text;
        }
        history.push(Message {
            role: Role::Assistant,
            blocks: completion.blocks.clone(),
        });
        match completion.stop {
            Stop::End => {
                return Ok(Outcome {
                    text: last_text,
                    calls,
                    ended: Ended::Done,
                });
            }
            Stop::Length => {
                return Ok(Outcome {
                    text: last_text,
                    calls,
                    ended: Ended::TooLong,
                });
            }
            Stop::ToolUse => {}
        }
        let mut results = Vec::new();
        for block in &completion.blocks {
            let Block::ToolUse { id, name, input } = block else {
                continue;
            };
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            calls += 1;
            let outcome = run_tool(host, name, input);
            on_event(Event::Tool {
                name: name.clone(),
                input: input.clone(),
                result: outcome.clone(),
            });
            let (content, is_error) = match outcome {
                Ok(text) => (text, false),
                Err(text) => (text, true),
            };
            results.push(Block::ToolResult {
                id: id.clone(),
                content: cut(content),
                is_error,
            });
        }
        history.push(Message {
            role: Role::User,
            blocks: results,
        });
        if calls >= settings.max_steps {
            return Ok(Outcome {
                text: last_text,
                calls,
                ended: Ended::OutOfSteps,
            });
        }
    }
}

/// One tool call, checked before it reaches the host: only real tools run,
/// and their input must be an object.
fn run_tool(host: &mut dyn ToolHost, name: &str, input: &Value) -> Result<String, String> {
    if !tools::is_tool_name(name) {
        return Err(format!(
            "there is no tool called \"{name}\"; use one from the list"
        ));
    }
    if !input.is_object() {
        return Err("the arguments must be a JSON object".to_owned());
    }
    match host.call(name, input) {
        Ok(value) => Ok(crate::mcp::text_of(&value)),
        Err(e) => Err(format!("{}: {}", e.code, e.message)),
    }
}

/// Keep a long conversation within bounds: when it has more than `keep`
/// messages, drop the oldest whole exchanges. A cut always lands on a
/// message the user typed (not a tool answer), so no tool call is left
/// without its answer or the other way round.
pub fn trim_history(history: &mut Vec<Message>, keep: usize) {
    if history.len() <= keep {
        return;
    }
    let first_kept = history.len() - keep;
    let starts_an_exchange =
        |m: &Message| m.role == Role::User && m.blocks.iter().all(|b| matches!(b, Block::Text(_)));
    if let Some(at) = (first_kept..history.len()).find(|i| starts_an_exchange(&history[*i])) {
        history.drain(..at);
    } else {
        // the last exchange alone is longer than the limit: keep it whole
        if let Some(at) = history.iter().rposition(starts_an_exchange) {
            history.drain(..at);
        }
    }
}

fn cut(mut text: String) -> String {
    if text.chars().count() > MAX_RESULT {
        text = text.chars().take(MAX_RESULT).collect();
        text.push_str("\n[the answer was cut here; ask for less at a time]");
    }
    text
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use serde_json::json;

    use super::*;
    use crate::provider::Completion;

    /// A model that is a script of turns.
    struct Script {
        turns: RefCell<Vec<Completion>>,
        seen: RefCell<Vec<usize>>,
    }

    impl Script {
        fn new(mut turns: Vec<Completion>) -> Self {
            turns.reverse();
            Self {
                turns: RefCell::new(turns),
                seen: RefCell::new(vec![]),
            }
        }
    }

    impl Provider for Script {
        fn label(&self) -> String {
            "script".into()
        }
        fn complete(&self, request: &Request<'_>, _: &AtomicBool) -> Result<Completion, ProviderError> {
            self.seen.borrow_mut().push(request.messages.len());
            self.turns
                .borrow_mut()
                .pop()
                .ok_or_else(|| ProviderError::Transport("the script ran out".into()))
        }
    }

    #[derive(Default)]
    struct Host {
        calls: Vec<(String, Value)>,
    }

    impl ToolHost for Host {
        fn call(&mut self, name: &str, input: &Value) -> Result<Value, ToolError> {
            self.calls.push((name.to_owned(), input.clone()));
            match name {
                "add_layer" => Ok(json!(3)),
                _ => Err(ToolError::new("not_found", "no such layer")),
            }
        }
    }

    fn call(id: &str, name: &str, input: Value) -> Block {
        Block::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    fn turn(blocks: Vec<Block>, stop: Stop) -> Completion {
        Completion { blocks, stop }
    }

    fn settings(tools: &[Tool]) -> Settings<'_> {
        Settings {
            system: "s",
            model: "m",
            tools,
            max_steps: MAX_STEPS,
        }
    }

    #[test]
    fn tools_are_run_and_answered_until_the_model_is_done() {
        let all = tools::tools();
        let model = Script::new(vec![
            turn(
                vec![
                    Block::Text("Adding it.".into()),
                    call("1", "add_layer", json!({"kind": "vector", "name": "Ball"})),
                ],
                Stop::ToolUse,
            ),
            turn(vec![Block::Text("Added Ball.".into())], Stop::End),
        ]);
        let mut host = Host::default();
        let mut history = vec![];
        let mut events = vec![];
        let outcome = run(
            &model,
            &mut host,
            &settings(&all),
            &mut history,
            "add a ball",
            &AtomicBool::new(false),
            &mut |e| events.push(e),
        )
        .unwrap();
        assert_eq!(
            (outcome.text.as_str(), outcome.calls, outcome.ended),
            ("Added Ball.", 1, Ended::Done)
        );
        assert_eq!(host.calls[0].0, "add_layer");
        // user, assistant (tool call), user (result), assistant (final)
        assert_eq!(history.len(), 4);
        assert!(
            matches!(&history[2].blocks[0], Block::ToolResult { content, is_error: false, .. } if content == "3")
        );
        assert_eq!(*model.seen.borrow(), vec![1, 3]);
        assert!(matches!(&events[0], Event::Said(t) if t == "Adding it."));
        assert!(matches!(&events[1], Event::Tool { result: Ok(r), .. } if r == "3"));
    }

    #[test]
    fn a_failing_tool_is_told_to_the_model_which_can_try_again() {
        let all = tools::tools();
        let model = Script::new(vec![
            turn(
                vec![call("1", "rename_layer", json!({"id": 9, "name": "x"}))],
                Stop::ToolUse,
            ),
            turn(vec![Block::Text("There is no layer 9.".into())], Stop::End),
        ]);
        let mut host = Host::default();
        let mut history = vec![];
        run(
            &model,
            &mut host,
            &settings(&all),
            &mut history,
            "go",
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert!(matches!(&history[2].blocks[0],
            Block::ToolResult { content, is_error: true, .. } if content == "not_found: no such layer"));
    }

    #[test]
    fn invented_tools_and_bad_arguments_never_reach_the_host() {
        let all = tools::tools();
        let model = Script::new(vec![
            turn(
                vec![
                    call("1", "delete_everything", json!({})),
                    call("2", "save", json!({})),
                    call("3", "add_layer", Value::String("{oops".into())),
                ],
                Stop::ToolUse,
            ),
            turn(vec![Block::Text("Sorry.".into())], Stop::End),
        ]);
        let mut host = Host::default();
        let mut history = vec![];
        run(
            &model,
            &mut host,
            &settings(&all),
            &mut history,
            "go",
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert!(host.calls.is_empty());
        let results = &history[2].blocks;
        assert_eq!(results.len(), 3);
        assert!(
            results
                .iter()
                .all(|b| matches!(b, Block::ToolResult { is_error: true, .. }))
        );
    }

    #[test]
    fn a_model_that_never_finishes_is_stopped_at_its_step_limit() {
        let all = tools::tools();
        let loopy: Vec<Completion> = (0..10)
            .map(|i| turn(vec![call(&i.to_string(), "layers", json!({}))], Stop::ToolUse))
            .collect();
        let model = Script::new(loopy);
        let mut host = Host::default();
        let mut history = vec![];
        let outcome = run(
            &model,
            &mut host,
            &Settings {
                max_steps: 3,
                ..settings(&all)
            },
            &mut history,
            "go",
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!((outcome.ended, outcome.calls), (Ended::OutOfSteps, 3));
    }

    #[test]
    fn stopping_ends_the_run_before_the_next_call() {
        let all = tools::tools();
        let model = Script::new(vec![turn(vec![call("1", "layers", json!({}))], Stop::ToolUse)]);
        let mut host = Host::default();
        let cancel = AtomicBool::new(true);
        let r = run(
            &model,
            &mut host,
            &settings(&all),
            &mut vec![],
            "go",
            &cancel,
            &mut |_| {},
        );
        assert_eq!(r, Err(ProviderError::Cancelled));
        assert!(host.calls.is_empty());
    }

    #[test]
    fn a_provider_error_comes_out_and_the_conversation_keeps_the_question() {
        let all = tools::tools();
        let model = Script::new(vec![]);
        let mut history = vec![];
        let r = run(
            &model,
            &mut Host::default(),
            &settings(&all),
            &mut history,
            "hi",
            &AtomicBool::new(false),
            &mut |_| {},
        );
        assert!(matches!(r, Err(ProviderError::Transport(_))));
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn a_long_conversation_is_cut_between_exchanges() {
        let mut history = Vec::new();
        for i in 0..6 {
            history.push(Message::user(format!("ask {i}")));
            history.push(Message {
                role: Role::Assistant,
                blocks: vec![call(&i.to_string(), "layers", json!({}))],
            });
            history.push(Message {
                role: Role::User,
                blocks: vec![Block::ToolResult {
                    id: i.to_string(),
                    content: "[]".into(),
                    is_error: false,
                }],
            });
            history.push(Message {
                role: Role::Assistant,
                blocks: vec![Block::Text("done".into())],
            });
        }
        assert_eq!(history.len(), 24);
        trim_history(&mut history, 10);
        // it starts with something the user typed, and keeps whole exchanges
        assert!(matches!(&history[0].blocks[0], Block::Text(t) if t.starts_with("ask")));
        assert!(history.len() <= 10 && history.len() % 4 == 0, "{}", history.len());
        // a short one is left alone
        let before = history.clone();
        trim_history(&mut history, 100);
        assert_eq!(history, before);
        // one huge exchange is kept whole rather than cut in half
        let mut one = vec![Message::user("go")];
        for i in 0..20 {
            one.push(Message {
                role: Role::Assistant,
                blocks: vec![call(&i.to_string(), "layers", json!({}))],
            });
            one.push(Message {
                role: Role::User,
                blocks: vec![Block::ToolResult {
                    id: i.to_string(),
                    content: "[]".into(),
                    is_error: false,
                }],
            });
        }
        let len = one.len();
        trim_history(&mut one, 6);
        assert_eq!(one.len(), len);
    }

    #[test]
    fn a_huge_answer_is_cut() {
        let long = "x".repeat(MAX_RESULT + 50);
        let cut = cut(long);
        assert!(cut.ends_with("ask for less at a time]"));
        assert!(cut.chars().count() < MAX_RESULT + 100);
    }
}
