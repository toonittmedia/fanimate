//! What every model provider has in common: a conversation of text and tool
//! calls, and one call that asks the model for its next turn.
//!
//! The provider adapters (`anthropic`, `openai`) only turn this conversation
//! into their service's request and the answer back. The conversation is
//! plain data, so the agent loop is tested with a model that is a script.

use std::sync::atomic::AtomicBool;

use serde_json::Value;

use crate::tools::Tool;

/// A piece of a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Text(String),
    /// The model asking for a tool.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// What a tool answered, sent back to the model.
    ToolResult {
        id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub blocks: Vec<Block>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            blocks: vec![Block::Text(text.into())],
        }
    }

    /// The text of the message, its text blocks joined.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Why the model's turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// It is done (or has nothing more to say).
    End,
    /// It wants tools run and their answers sent back.
    ToolUse,
    /// It ran out of room mid-answer.
    Length,
}

/// One turn from the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Completion {
    pub blocks: Vec<Block>,
    pub stop: Stop,
}

/// A request to a provider, everything it needs to give the next turn.
pub struct Request<'a> {
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [Tool],
    pub model: &'a str,
}

/// Why a provider call failed, in words that say what to do.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderError {
    /// The user stopped it.
    Cancelled,
    /// The service said no (a wrong key, no credit, an unknown model, a limit).
    Service { status: u16, message: String },
    /// The service could not be reached, or sent something unreadable.
    Transport(String),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "Stopped"),
            Self::Service { status, message } => write!(f, "{message} (the service answered {status})"),
            Self::Transport(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// A source of model turns.
pub trait Provider {
    /// A short name for the transcript and the file's provenance record.
    fn label(&self) -> String;

    /// Ask for the next turn. Returns early with [`ProviderError::Cancelled`]
    /// when `cancel` is set.
    fn complete(&self, request: &Request<'_>, cancel: &AtomicBool) -> Result<Completion, ProviderError>;
}

/// Pick the message out of a service's error body, which most services write
/// as `{"error": {"message": "..."}}` or `{"error": "..."}`.
pub fn error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let from_json = parsed.as_ref().and_then(|v| {
        let e = v.get("error")?;
        e.get("message")
            .and_then(Value::as_str)
            .or_else(|| e.as_str())
            .map(str::to_owned)
    });
    from_json.unwrap_or_else(|| {
        let text = body.trim();
        if text.is_empty() {
            "no reason was given".to_owned()
        } else {
            text.chars().take(300).collect()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_come_out_of_the_usual_shapes() {
        assert_eq!(error_message(r#"{"error":{"message":"bad key"}}"#), "bad key");
        assert_eq!(error_message(r#"{"error":"rate limited"}"#), "rate limited");
        assert_eq!(error_message("<html>oops</html>"), "<html>oops</html>");
        assert_eq!(error_message(""), "no reason was given");
        assert_eq!(error_message(&"x".repeat(1000)).len(), 300);
    }

    #[test]
    fn a_messages_text_joins_its_text_blocks() {
        let m = Message {
            role: Role::Assistant,
            blocks: vec![
                Block::Text("a".into()),
                Block::ToolUse {
                    id: "1".into(),
                    name: "layers".into(),
                    input: Value::Null,
                },
                Block::Text("b".into()),
            ],
        };
        assert_eq!(m.text(), "a\nb");
    }
}
