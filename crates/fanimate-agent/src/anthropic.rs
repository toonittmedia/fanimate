//! The Anthropic Messages API: the request for a conversation with tools and
//! the reading of its answer.

use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};

use crate::http::{Http, Post};
use crate::provider::{
    Block, Completion, Message, Provider, ProviderError, Request, Role, Stop, error_message,
};

pub const DEFAULT_URL: &str = "https://api.anthropic.com/v1/messages";
const VERSION: &str = "2023-06-01";
/// Room for one turn's answer. Tool calls are short; this is for the words.
const MAX_TOKENS: u32 = 8192;

pub struct Anthropic<H: Http> {
    pub http: H,
    pub url: String,
    pub key: String,
}

impl<H: Http> Anthropic<H> {
    pub fn new(http: H, key: impl Into<String>) -> Self {
        Self {
            http,
            url: DEFAULT_URL.to_owned(),
            key: key.into(),
        }
    }
}

/// The JSON body for a request.
pub fn body(request: &Request<'_>) -> Value {
    json!({
        "model": request.model,
        "max_tokens": MAX_TOKENS,
        "system": request.system,
        "tools": request.tools.iter().map(|t| json!({
            "name": t.name,
            "description": t.description,
            "input_schema": t.schema,
        })).collect::<Vec<_>>(),
        "messages": request.messages.iter().map(message).collect::<Vec<_>>(),
    })
}

fn message(m: &Message) -> Value {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<Value> = m
        .blocks
        .iter()
        .map(|b| match b {
            Block::Text(t) => json!({"type": "text", "text": t}),
            Block::ToolUse { id, name, input } => {
                json!({"type": "tool_use", "id": id, "name": name, "input": input})
            }
            Block::ToolResult {
                id,
                content,
                is_error,
            } => {
                json!({"type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error})
            }
        })
        .collect();
    json!({"role": role, "content": content})
}

/// Read a successful answer.
pub fn parse(answer: &Value) -> Result<Completion, ProviderError> {
    let content = answer
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::Transport("the answer had no content".into()))?;
    let mut blocks = Vec::new();
    for part in content {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = part.get("text").and_then(Value::as_str)
                    && !t.is_empty()
                {
                    blocks.push(Block::Text(t.to_owned()));
                }
            }
            Some("tool_use") => blocks.push(Block::ToolUse {
                id: part
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                name: part
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                input: part.get("input").cloned().unwrap_or_else(|| json!({})),
            }),
            // thinking blocks and the like are not shown or sent back
            _ => {}
        }
    }
    let stop = match answer.get("stop_reason").and_then(Value::as_str) {
        Some("tool_use") => Stop::ToolUse,
        Some("max_tokens") => Stop::Length,
        _ => Stop::End,
    };
    // a turn that says "tool" but has no call is a finished turn
    let stop = if stop == Stop::ToolUse && !blocks.iter().any(|b| matches!(b, Block::ToolUse { .. })) {
        Stop::End
    } else {
        stop
    };
    Ok(Completion { blocks, stop })
}

impl<H: Http> Provider for Anthropic<H> {
    fn label(&self) -> String {
        "Anthropic".to_owned()
    }

    fn complete(&self, request: &Request<'_>, cancel: &AtomicBool) -> Result<Completion, ProviderError> {
        let answer = self.http.post(
            &Post {
                url: &self.url,
                headers: &[
                    ("x-api-key".into(), self.key.clone()),
                    ("anthropic-version".into(), VERSION.into()),
                ],
                body: &body(request),
            },
            cancel,
        )?;
        if !(200..300).contains(&answer.status) {
            return Err(ProviderError::Service {
                status: answer.status,
                message: error_message(&answer.body),
            });
        }
        let value: Value = serde_json::from_str(&answer.body)
            .map_err(|e| ProviderError::Transport(format!("the answer was not JSON: {e}")))?;
        parse(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::tools;

    #[test]
    fn a_request_carries_tools_and_the_conversation() {
        let all = tools();
        let messages = vec![
            Message::user("fade the title in"),
            Message {
                role: Role::Assistant,
                blocks: vec![Block::ToolUse {
                    id: "t1".into(),
                    name: "layers".into(),
                    input: json!({}),
                }],
            },
            Message {
                role: Role::User,
                blocks: vec![Block::ToolResult {
                    id: "t1".into(),
                    content: "[]".into(),
                    is_error: false,
                }],
            },
        ];
        let b = body(&Request {
            system: "be brief",
            messages: &messages,
            tools: &all,
            model: "claude-sonnet-5-5",
        });
        assert_eq!(b["model"], "claude-sonnet-5-5");
        assert_eq!(b["system"], "be brief");
        assert_eq!(b["tools"].as_array().unwrap().len(), all.len());
        assert_eq!(b["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(b["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(b["messages"][2]["content"][0]["tool_use_id"], "t1");
        assert_eq!(b["messages"][2]["content"][0]["is_error"], false);
    }

    #[test]
    fn a_tool_turn_is_read() {
        let c = parse(&json!({
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "text", "text": "Adding a layer."},
                {"type": "tool_use", "id": "toolu_1", "name": "add_layer", "input": {"kind": "vector", "name": "Ball"}}
            ],
            "stop_reason": "tool_use"
        }))
        .unwrap();
        assert_eq!(c.stop, Stop::ToolUse);
        assert_eq!(c.blocks.len(), 2);
        assert!(
            matches!(&c.blocks[1], Block::ToolUse { name, input, .. } if name == "add_layer" && input["name"] == "Ball")
        );
    }

    #[test]
    fn the_end_of_a_turn_and_a_cut_off_one() {
        let c = parse(&json!({"content": [{"type": "text", "text": "Done."}], "stop_reason": "end_turn"}))
            .unwrap();
        assert_eq!((c.stop, c.blocks.len()), (Stop::End, 1));
        let c = parse(&json!({"content": [{"type": "text", "text": "Sor"}], "stop_reason": "max_tokens"}))
            .unwrap();
        assert_eq!(c.stop, Stop::Length);
        // says tool_use but made no call: finished
        let c =
            parse(&json!({"content": [{"type": "text", "text": "ok"}], "stop_reason": "tool_use"})).unwrap();
        assert_eq!(c.stop, Stop::End);
    }

    #[test]
    fn a_damaged_answer_is_an_error_not_a_panic() {
        assert!(parse(&json!({})).is_err());
        assert!(parse(&json!({"content": "text"})).is_err());
        let c = parse(&json!({"content": [{"type": "tool_use"}]})).unwrap();
        assert!(matches!(&c.blocks[0], Block::ToolUse { name, .. } if name.is_empty()));
    }
}
