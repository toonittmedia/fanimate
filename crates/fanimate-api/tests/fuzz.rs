//! Fuzzing the call table: every method is called with parameters made up
//! from its own signature and then mangled (wrong types, huge, negative and
//! non-finite numbers, deep nesting, missing and extra keys), and every
//! malformed line a client could send is thrown at the dispatcher. Whatever
//! comes back, the host must answer (an error is fine), never panic, never
//! hang, and a refused call must leave the project exactly as it was.
//!
//! The generator is a fixed-seed xorshift, so a failure names the request
//! that caused it and reruns the same way.

use fanimate_api::{Api, Permission, Permissions, rpc};
use fanimate_core::{Document, samples};
use fanimate_editor::Editor;
use serde_json::{Value, json};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn deep(n: usize) -> Value {
    let mut v = json!(1);
    for _ in 0..n {
        v = json!([v]);
    }
    v
}

/// A value of the kind the signature names, or (one time in three) a wrong one.
fn value(rng: &mut Rng, hint: &str) -> Value {
    let nasty: [Value; 14] = [
        Value::Null,
        json!(true),
        json!(-1),
        json!(0),
        json!(1.0e38),
        json!(-1.0e38),
        json!(4_294_967_296_u64),
        json!(u64::MAX),
        json!(""),
        json!("x".repeat(70_000)),
        json!("\u{0}\u{202e}\u{1F600}"),
        json!([]),
        json!({}),
        deep(60),
    ];
    if rng.below(3) == 0 {
        return rng.pick(&nasty).clone();
    }
    let hint = hint.trim();
    if hint.starts_with("number") {
        return match rng.below(6) {
            0 => json!(0),
            1 => json!(1),
            2 => json!(rng.below(20)),
            3 => json!(rng.below(1000) as f64 / 7.0),
            4 => json!(-(rng.below(500) as f64)),
            _ => json!(rng.below(100_000)),
        };
    }
    if hint.starts_with("string") {
        return json!(["a", "vector", "#ff8800", "Hello", "arm L", "linear"][rng.below(6)]);
    }
    if hint.starts_with("boolean") {
        return json!(rng.below(2) == 0);
    }
    if hint.starts_with('[') {
        return json!((0..rng.below(4)).map(|i| i).collect::<Vec<_>>());
    }
    rng.pick(&nasty).clone()
}

/// `(name, type hint)` for each parameter in a method's signature text.
fn signature(params: &str) -> Vec<(String, String)> {
    params
        .split(", ")
        .filter_map(|p| {
            let (name, hint) = p.split_once(':')?;
            Some((name.trim().to_owned(), hint.trim().to_owned()))
        })
        .collect()
}

fn params_for(rng: &mut Rng, signature: &[(String, String)]) -> Value {
    let mut map = serde_json::Map::new();
    for (name, hint) in signature {
        // leave some out, now and then
        if rng.below(12) == 0 {
            continue;
        }
        map.insert(name.clone(), value(rng, hint));
    }
    if rng.below(8) == 0 {
        map.insert("surprise".to_owned(), value(rng, "?"));
    }
    Value::Object(map)
}

fn editor() -> Editor {
    Editor::new(samples::walk_cycle())
}

#[test]
fn every_method_survives_mangled_parameters_and_refusals_change_nothing() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut ed = editor();
    let mut answered = 0usize;
    for method in rpc::METHODS {
        // calls that leave the project or end the host are not what is fuzzed here
        if matches!(method.name, "quit" | "shutdown" | "save" | "import_ora") {
            continue;
        }
        let sig = signature(method.params);
        for round in 0..60 {
            let params = params_for(&mut rng, &sig);
            let request = json!({"jsonrpc": "2.0", "id": round, "method": method.name, "params": params});
            let line = request.to_string();
            let before = ed.doc.clone();
            let answer = rpc::handle_line(&mut Api::new(&mut ed), &line)
                .unwrap_or_else(|| panic!("no answer to {}", &line[..line.len().min(300)]));
            let parsed: Value = serde_json::from_str(&answer)
                .unwrap_or_else(|e| panic!("answer is not JSON ({e}) to {}", &line[..line.len().min(300)]));
            assert_eq!(parsed["id"], round, "{}", &line[..line.len().min(300)]);
            assert!(
                parsed.get("result").is_some() ^ parsed.get("error").is_some(),
                "exactly one of result and error: {answer}"
            );
            if parsed.get("error").is_some() {
                // a call that failed (every method is all or nothing) changed nothing
                assert!(
                    ed.doc == before,
                    "{} failed but the project changed: {}",
                    method.name,
                    &line[..line.len().min(300)]
                );
            }
            answered += 1;
        }
        // the project is still one that can be written and read back
        if method.edits {
            let doc = ed.doc.mainline().into_owned();
            let file = std::env::temp_dir().join(format!("fanimate-fuzz-{}.fnm", std::process::id()));
            assert!(
                fanimate_fnm::save(&doc, &file).is_ok() && fanimate_fnm::load(&file).is_ok(),
                "{} left a project that cannot be saved and opened",
                method.name
            );
            std::fs::remove_file(&file).ok();
        }
        // keep the document a sensible size for the next method
        if ed.doc.layers.len() > 200 {
            ed = editor();
        }
    }
    assert!(answered > 1000, "only {answered} calls were made");
}

#[test]
fn malformed_lines_get_json_rpc_errors() {
    let mut ed = editor();
    let lines: Vec<String> = vec![
        String::new(),
        " ".into(),
        "{".into(),
        "null".into(),
        "[]".into(),
        "[[], {}, 3]".into(),
        "42".into(),
        "\"layers\"".into(),
        "{}".into(),
        r#"{"method": 5}"#.into(),
        r#"{"method": null, "id": 1}"#.into(),
        r#"{"method": "layers", "params": 5, "id": 2}"#.into(),
        r#"{"method": "layers", "params": [1, 2], "id": 3}"#.into(),
        r#"{"id": {"a": 1}, "method": "nope"}"#.into(),
        r#"{"id": "x", "method": "layer", "params": {"id": "not a number"}}"#.into(),
        format!(r#"{{"id": 1, "method": "{}"}}"#, "m".repeat(100_000)),
        format!(
            r#"{{"id": 1, "method": "layers", "params": {{"x": {}}}}}"#,
            deep(400)
        ),
        "\u{0}\u{1}\u{2}".into(),
        "{\"id\": 1, \"method\": \"layers\"} trailing".into(),
    ];
    for line in lines {
        // none of them may panic; those that are requests are answered
        let shown = line.chars().take(60).collect::<String>();
        if let Some(answer) = rpc::handle_line(&mut Api::new(&mut ed), &line) {
            let parsed: Value = serde_json::from_str(&answer).unwrap_or_else(|e| panic!("{e}: {shown}"));
            assert_eq!(parsed["jsonrpc"], "2.0", "{shown}");
        }
    }
    // the document is untouched by all of it
    assert!(ed.doc == editor().doc);
}

#[test]
fn a_client_with_no_permissions_changes_nothing_through_any_method() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut ed = editor();
    let before = ed.doc.clone();
    let none = Permissions::NONE;
    let read_only = Permissions::NONE.with(Permission::Read);
    for method in rpc::METHODS {
        if matches!(method.name, "auth" | "quit") {
            continue;
        }
        let sig = signature(method.params);
        for _ in 0..4 {
            let params = params_for(&mut rng, &sig);
            let line = json!({"id": 1, "method": method.name, "params": params}).to_string();
            for permissions in [none, read_only] {
                if let Some(answer) = rpc::handle_line_as(&mut Api::new(&mut ed), &line, permissions) {
                    let parsed: Value = serde_json::from_str(&answer).unwrap();
                    if method.edits {
                        let code = parsed["error"]["data"]["code"].as_str();
                        assert_eq!(code, Some("permission_denied"), "{}: {answer}", method.name);
                    }
                }
            }
        }
    }
    assert!(
        ed.doc == before,
        "a client without the edit permission changed the project"
    );
    let _ = Document::new("unused", 1.0, 1.0, 1.0);
}
