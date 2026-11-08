//! The OpenAI-style chat completions API, which OpenRouter, OpenAI, Ollama,
//! LM Studio and many other services and local servers speak.

use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};

use crate::http::{Http, Post};
use crate::provider::{
    Block, Completion, Message, Provider, ProviderError, Request, Role, Stop, error_message,
};

pub const OPENROUTER: &str = "https://openrouter.ai/api/v1";
pub const OPENAI: &str = "https://api.openai.com/v1";
/// Ollama's own address for the same API.
pub const OLLAMA: &str = "http://localhost:11434/v1";

pub struct OpenAi<H: Http> {
    pub http: H,
    /// Everything before `/chat/completions`.
    pub base: String,
    /// Empty for a local server that needs none.
    pub key: String,
    pub headers: Vec<(String, String)>,
    pub name: String,
}

impl<H: Http> OpenAi<H> {
    pub fn new(http: H, name: impl Into<String>, base: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            http,
            base: base.into(),
            key: key.into(),
            headers: Vec::new(),
            name: name.into(),
        }
    }

    /// OpenRouter, which asks apps to say who they are.
    pub fn openrouter(http: H, key: impl Into<String>) -> Self {
        let mut p = Self::new(http, "OpenRouter", OPENROUTER, key);
        p.headers = vec![
            ("HTTP-Referer".into(), "https://fanimate.org".into()),
            ("X-Title".into(), "Fanimate".into()),
        ];
        p
    }
}

/// The JSON body for a request.
pub fn body(request: &Request<'_>) -> Value {
    let mut messages = vec![json!({"role": "system", "content": request.system})];
    for m in request.messages {
        messages.extend(message(m));
    }
    let mut body = json!({"model": request.model, "messages": messages});
    if !request.tools.is_empty() {
        body["tools"] = Value::Array(
            request
                .tools
                .iter()
                .map(|t| {
                    json!({"type": "function", "function": {
                        "name": t.name, "description": t.description, "parameters": t.schema}})
                })
                .collect(),
        );
    }
    body
}

/// One of our messages as the one or more this API wants: each tool result
/// is a message of its own.
fn message(m: &Message) -> Vec<Value> {
    match m.role {
        Role::User => {
            let mut out = Vec::new();
            let text = m.text();
            for b in &m.blocks {
                if let Block::ToolResult { id, content, .. } = b {
                    out.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
                }
            }
            if !text.is_empty() {
                out.push(json!({"role": "user", "content": text}));
            }
            out
        }
        Role::Assistant => {
            let calls: Vec<Value> = m
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::ToolUse { id, name, input } => Some(json!({
                        "id": id, "type": "function",
                        "function": {"name": name, "arguments": input.to_string()}
                    })),
                    _ => None,
                })
                .collect();
            let text = m.text();
            let mut msg = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::String(text) }});
            if !calls.is_empty() {
                msg["tool_calls"] = Value::Array(calls);
            }
            vec![msg]
        }
    }
}

/// Read a successful answer.
pub fn parse(answer: &Value) -> Result<Completion, ProviderError> {
    let Some(choice) = answer.get("choices").and_then(|c| c.get(0)) else {
        // some services answer 200 with an error in the body
        if answer.get("error").is_some() {
            return Err(ProviderError::Service {
                status: 200,
                message: error_message(&answer.to_string()),
            });
        }
        return Err(ProviderError::Transport("the answer had no choices".into()));
    };
    let message = choice.get("message").unwrap_or(&Value::Null);
    let mut blocks = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str)
        && !text.trim().is_empty()
    {
        blocks.push(Block::Text(text.to_owned()));
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let function = call.get("function").unwrap_or(&Value::Null);
            let raw = function.get("arguments").and_then(Value::as_str).unwrap_or("");
            let input = if raw.trim().is_empty() {
                json!({})
            } else {
                // an unreadable argument string is kept as a string, which the
                // agent turns into an error the model can read
                serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()))
            };
            blocks.push(Block::ToolUse {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                name: function
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                input,
            });
        }
    }
    let has_calls = blocks.iter().any(|b| matches!(b, Block::ToolUse { .. }));
    let stop = match choice.get("finish_reason").and_then(Value::as_str) {
        Some("length") => Stop::Length,
        _ if has_calls => Stop::ToolUse,
        _ => Stop::End,
    };
    Ok(Completion { blocks, stop })
}

impl<H: Http> Provider for OpenAi<H> {
    fn label(&self) -> String {
        self.name.clone()
    }

    fn complete(&self, request: &Request<'_>, cancel: &AtomicBool) -> Result<Completion, ProviderError> {
        let mut headers = self.headers.clone();
        if !self.key.is_empty() {
            headers.push(("Authorization".into(), format!("Bearer {}", self.key)));
        }
        let url = format!("{}/chat/completions", self.base.trim_end_matches('/'));
        let answer = self.http.post(
            &Post {
                url: &url,
                headers: &headers,
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
    fn a_conversation_becomes_chat_messages_with_one_message_per_tool_result() {
        let all = tools();
        let messages = vec![
            Message::user("hello"),
            Message {
                role: Role::Assistant,
                blocks: vec![
                    Block::Text("Looking.".into()),
                    Block::ToolUse {
                        id: "c1".into(),
                        name: "layers".into(),
                        input: json!({}),
                    },
                    Block::ToolUse {
                        id: "c2".into(),
                        name: "document".into(),
                        input: json!({}),
                    },
                ],
            },
            Message {
                role: Role::User,
                blocks: vec![
                    Block::ToolResult {
                        id: "c1".into(),
                        content: "[]".into(),
                        is_error: false,
                    },
                    Block::ToolResult {
                        id: "c2".into(),
                        content: "{}".into(),
                        is_error: false,
                    },
                ],
            },
        ];
        let b = body(&Request {
            system: "sys",
            messages: &messages,
            tools: &all,
            model: "anthropic/claude-sonnet-5-5",
        });
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m[0]["role"], "system");
        assert_eq!(m[1]["role"], "user");
        assert_eq!(m[2]["tool_calls"].as_array().unwrap().len(), 2);
        assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], "{}");
        assert_eq!(
            (m[3]["role"].as_str(), m[3]["tool_call_id"].as_str()),
            (Some("tool"), Some("c1"))
        );
        assert_eq!(m[4]["tool_call_id"], "c2");
        assert_eq!(m.len(), 5);
        assert_eq!(b["tools"][0]["type"], "function");
        assert_eq!(b["tools"][0]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn no_tools_means_no_tools_field() {
        let b = body(&Request {
            system: "s",
            messages: &[Message::user("x")],
            tools: &[],
            model: "m",
        });
        assert!(b.get("tools").is_none());
    }

    #[test]
    fn tool_calls_are_read_and_bad_arguments_are_kept_as_text() {
        let c = parse(&json!({"choices": [{"finish_reason": "tool_calls", "message": {
            "content": null,
            "tool_calls": [
                {"id": "a", "function": {"name": "add_layer", "arguments": "{\"kind\":\"vector\",\"name\":\"B\"}"}},
                {"id": "b", "function": {"name": "layers", "arguments": ""}},
                {"id": "c", "function": {"name": "rename_layer", "arguments": "{oops"}}
            ]}}]}))
        .unwrap();
        assert_eq!(c.stop, Stop::ToolUse);
        assert!(matches!(&c.blocks[0], Block::ToolUse { input, .. } if input["name"] == "B"));
        assert!(
            matches!(&c.blocks[1], Block::ToolUse { input, .. } if input.as_object().unwrap().is_empty())
        );
        assert!(matches!(&c.blocks[2], Block::ToolUse { input, .. } if input == "{oops"));
    }

    #[test]
    fn text_end_and_length_stops() {
        let c =
            parse(&json!({"choices": [{"finish_reason": "stop", "message": {"content": "Done."}}]})).unwrap();
        assert_eq!((c.stop, c.blocks.len()), (Stop::End, 1));
        let c =
            parse(&json!({"choices": [{"finish_reason": "length", "message": {"content": "Do"}}]})).unwrap();
        assert_eq!(c.stop, Stop::Length);
    }

    #[test]
    fn an_error_inside_a_200_and_a_damaged_answer() {
        let e = parse(&json!({"error": {"message": "No credits", "code": 402}})).unwrap_err();
        assert!(matches!(e, ProviderError::Service { message, .. } if message == "No credits"));
        assert!(parse(&json!({})).is_err());
        assert!(parse(&json!({"choices": []})).is_err());
        let c = parse(&json!({"choices": [{"message": {}}]})).unwrap();
        assert!(c.blocks.is_empty());
    }
}
