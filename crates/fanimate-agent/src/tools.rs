//! The API as tools a model can call: one tool per method of the call table
//! (`fanimate_api::rpc::METHODS`), with a JSON Schema worked out from the
//! method's parameter list.
//!
//! The table stays the one source of truth. A method added to it appears here
//! without another list to keep, and its full parameter text is always in the
//! tool's description, so a schema that is looser than the text (the list is
//! written for people) never hides a parameter from the model.

use fanimate_api::rpc::{self, Method};
use serde_json::{Map, Value, json};

/// One tool: a name, what it does and the JSON Schema of its parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: &'static str,
    pub description: String,
    pub schema: Value,
    /// Whether it changes the project (and so needs the `edit` permission).
    pub edits: bool,
}

/// Methods that are not tools: the table's own listing, and the ones that
/// belong to the host (saving is the user's to do, not the assistant's).
fn is_tool(m: &Method) -> bool {
    !m.name.contains('.') && !matches!(m.name, "save" | "quit" | "shutdown" | "auth")
}

/// Every tool, in the order of the call table.
pub fn tools() -> Vec<Tool> {
    rpc::METHODS
        .iter()
        .filter(|m| is_tool(m))
        .map(|m| Tool {
            name: m.name,
            description: if m.params.is_empty() {
                m.about.to_owned()
            } else {
                format!("{}. Parameters: {}", m.about, m.params)
            },
            schema: schema_of(m.params),
            edits: m.edits,
        })
        .collect()
}

/// Whether `name` is a tool (the hosts refuse anything else a model invents).
pub fn is_tool_name(name: &str) -> bool {
    rpc::METHODS.iter().any(|m| m.name == name && is_tool(m))
}

/// The JSON Schema of a parameter list written like
/// `"id: number, name: string, frame: number (optional)"`.
///
/// Only what can be told for sure is said: a name with a plain type gets it,
/// a name marked `(optional)` or `?` is not required, and a part that is not
/// a plain `name: type` (a choice of words, an object's shape) is left to the
/// description. Extra properties are always allowed.
pub fn schema_of(params: &str) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for part in split_top_level(params) {
        let part = part.trim();
        let (head, rest) = match part.split_once(':') {
            Some((h, r)) => (h.trim(), Some(r.trim())),
            None => (part, None),
        };
        let optional_mark = head.ends_with('?');
        let name = head.trim_end_matches('?');
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let mut property = Map::new();
        let mut optional = optional_mark || rest.is_none();
        if let Some(rest) = rest {
            if rest.contains("(optional)") {
                optional = true;
            }
            if let Some(kind) = json_type(rest) {
                property.insert("type".into(), kind);
            }
            if let Some(items) = array_items(rest) {
                property.insert("items".into(), items);
            }
        }
        properties.insert(name.to_owned(), Value::Object(property));
        if !optional && properties[name].get("type").is_some() {
            required.push(Value::String(name.to_owned()));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": true,
    })
}

/// The JSON type a parameter's text starts with, when it is a plain one.
fn json_type(text: &str) -> Option<Value> {
    let first = text
        .split(|c: char| !c.is_ascii_alphabetic() && c != '[')
        .next()?;
    let kind = match first {
        "number" => "number",
        "string" => "string",
        "bool" | "boolean" => "boolean",
        "object" if !text.starts_with("object or") => "object",
        _ if text.starts_with('[') => "array",
        _ => return None,
    };
    // "number or null", "string or ...": a choice, so no single type
    if text.contains(" or ") && !text.contains("(optional)") {
        return None;
    }
    Some(Value::String(kind.into()))
}

fn array_items(text: &str) -> Option<Value> {
    let inner = text.strip_prefix('[')?.split(']').next()?.trim();
    match inner {
        "number" => Some(json!({"type": "number"})),
        "string" => Some(json!({"type": "string"})),
        _ => None,
    }
}

/// Split at commas that are not inside quotes, brackets, braces or
/// parentheses.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            '[' | '{' | '(' if !quoted => depth += 1,
            ']' | '}' | ')' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start <= text.len() {
        parts.push(&text[start..]);
    }
    parts.into_iter().filter(|p| !p.trim().is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(schema: &Value) -> &Map<String, Value> {
        schema["properties"].as_object().unwrap()
    }

    #[test]
    fn plain_types_and_required() {
        let s = schema_of("id: number, name: string");
        assert_eq!(props(&s)["id"]["type"], "number");
        assert_eq!(props(&s)["name"]["type"], "string");
        assert_eq!(s["required"], json!(["id", "name"]));
    }

    #[test]
    fn optional_marks_both_styles() {
        let s = schema_of("layer: number, blend?: string, frame: number (optional), unmasked?: bool");
        assert_eq!(s["required"], json!(["layer"]));
        assert_eq!(props(&s)["unmasked"]["type"], "boolean");
        assert_eq!(props(&s)["frame"]["type"], "number");
    }

    #[test]
    fn choices_and_objects_are_left_to_the_description() {
        let s =
            schema_of("layer: number, kind: \"slider\" or \"pad\", interp: \"smooth\" | {\"ease\": name}");
        // `kind` has no plain type, so it is listed without one and not required
        assert!(props(&s)["kind"].get("type").is_none());
        assert!(props(&s)["interp"].get("type").is_none());
        assert_eq!(s["required"], json!(["layer"]));
    }

    #[test]
    fn arrays_and_commas_inside_brackets() {
        let s = schema_of("layer: number, bones: [number], stiffness, damping (each optional)");
        assert_eq!(props(&s)["bones"]["type"], "array");
        assert_eq!(props(&s)["bones"]["items"]["type"], "number");
        assert!(props(&s).contains_key("stiffness"));
        let t = schema_of("layer: number, shape: object (kind, x, y, width)");
        assert_eq!(props(&t)["shape"]["type"], "object");
        assert!(
            !props(&t).contains_key("width"),
            "a list inside parentheses is one part"
        );
    }

    #[test]
    fn empty_list_is_an_empty_object() {
        let s = schema_of("");
        assert!(props(&s).is_empty());
        assert_eq!(s["type"], "object");
    }

    #[test]
    fn every_method_becomes_a_tool_with_a_valid_name() {
        let all = tools();
        assert!(all.len() > 50, "the table has many tools, got {}", all.len());
        for t in &all {
            assert!(
                t.name.len() <= 64 && t.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{} is not a name every provider accepts",
                t.name
            );
            assert_eq!(t.schema["type"], "object");
            assert!(t.description.starts_with(|c: char| c.is_alphabetic()));
        }
        // the host's own methods and the listing are not tools
        for hidden in ["save", "quit", "shutdown", "auth", "api.version", "api.methods"] {
            assert!(!is_tool_name(hidden), "{hidden}");
        }
        assert!(is_tool_name("add_layer") && is_tool_name("undo") && !is_tool_name("delete_everything"));
    }

    #[test]
    fn a_known_method_has_the_expected_schema() {
        let t = tools()
            .into_iter()
            .find(|t| t.name == "set_layer_transform")
            .unwrap();
        assert!(t.edits);
        assert_eq!(t.schema["required"], json!(["id"]));
        assert!(t.description.contains("keyed after frame 0"));
    }

    #[test]
    fn hostile_parameter_text_does_not_panic() {
        for text in [
            "",
            ",,,",
            "::",
            "a:",
            ":b",
            "\"",
            "[[[",
            "}}}",
            "x: [",
            "é: number",
            "a: number (optional",
        ] {
            let s = schema_of(text);
            assert_eq!(s["type"], "object", "{text:?}");
        }
    }
}
