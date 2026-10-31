//! The JSON form of the API, the way a script host or a plugin process uses it.

use fanimate_api::rpc::{self, METHODS};
use fanimate_api::{API_VERSION, Api};
use fanimate_core::Document;
use fanimate_editor::Editor;
use serde_json::{Value, json};

fn editor() -> Editor {
    Editor::new(Document::new("t", 1280.0, 720.0, 24.0))
}

fn ask(ed: &mut Editor, line: &str) -> Value {
    let answer = rpc::handle_line(&mut Api::new(ed), line).expect("a request with an id is answered");
    serde_json::from_str(&answer).expect("the answer is JSON")
}

#[test]
fn a_request_gets_its_id_back_with_a_result() {
    let mut ed = editor();
    let answer = ask(&mut ed, r#"{"id": 7, "method": "document"}"#);
    assert_eq!(answer["id"], 7);
    assert_eq!(answer["result"]["name"], "t");
    assert_eq!(answer["result"]["api_version"], API_VERSION);
    assert!(answer.get("error").is_none());
    let version = ask(&mut ed, r#"{"id": "a", "method": "api.version"}"#);
    assert_eq!(version["id"], "a", "an id can be any JSON value");
    assert_eq!(version["result"], API_VERSION);
}

#[test]
fn a_scripted_title_is_built_changed_and_undone_through_json_alone() {
    let mut ed = editor();
    let made = ask(
        &mut ed,
        r#"{"id": 1, "method": "add_text_layer", "params": {"content": "Hello", "x": 100, "y": 80}}"#,
    );
    let id = made["result"].as_u64().expect("the new layer's id");
    let style = json!({"id": 2, "method": "set_text_style",
        "params": {"layer": id, "style": {"size": 96.0, "family": "JetBrains Mono"}}});
    assert!(ask(&mut ed, &style.to_string()).get("error").is_none());
    let animator = json!({"id": 3, "method": "add_text_animator", "params": {"layer": id, "preset": "wave"}});
    assert_eq!(ask(&mut ed, &animator.to_string())["result"], 0);
    let info = ask(
        &mut ed,
        &json!({"id": 4, "method": "text", "params": {"layer": id}}).to_string(),
    );
    assert_eq!(info["result"]["size"], 96.0);
    assert_eq!(info["result"]["family"], "JetBrains Mono");
    assert_eq!(info["result"]["animators"][0]["unit"], "character");
    // each call was one step: three undos empty the document, a fourth finds nothing
    for _ in 0..3 {
        assert_eq!(ask(&mut ed, r#"{"id": 5, "method": "undo"}"#)["result"], true);
    }
    assert_eq!(
        ask(&mut ed, r#"{"id": 6, "method": "layers"}"#)["result"],
        json!([])
    );
    assert_eq!(ask(&mut ed, r#"{"id": 7, "method": "undo"}"#)["result"], false);
    assert_eq!(ask(&mut ed, r#"{"id": 8, "method": "redo"}"#)["result"], true);
}

#[test]
fn every_failure_has_a_stable_code_and_the_host_carries_on() {
    let mut ed = editor();
    let cases = [
        ("not json at all", "invalid_argument"),
        (r#"[1, 2]"#, "invalid_argument"),
        (r#"{"id": 1}"#, "invalid_argument"),
        (r#"{"id": 1, "method": 5}"#, "invalid_argument"),
        (r#"{"id": 1, "method": "nothing_like_it"}"#, "not_found"),
        (
            r#"{"id": 1, "method": "layer", "params": {"id": 99}}"#,
            "not_found",
        ),
        (
            r#"{"id": 1, "method": "layer", "params": {}}"#,
            "invalid_argument",
        ),
        (
            r#"{"id": 1, "method": "layer", "params": {"id": "x"}}"#,
            "invalid_argument",
        ),
        (
            r#"{"id": 1, "method": "layer", "params": {"id": -1}}"#,
            "invalid_argument",
        ),
        (
            r#"{"id": 1, "method": "layer", "params": [1]}"#,
            "invalid_argument",
        ),
        (
            r#"{"id": 1, "method": "add_text_layer", "params": {"content": "x", "x": 1e999, "y": 0}}"#,
            "invalid_argument",
        ),
        (
            r#"{"id": 1, "method": "add_text_animator", "params": {"layer": 1, "preset": 4}}"#,
            "invalid_argument",
        ),
    ];
    for (line, code) in cases {
        let answer = ask(&mut ed, line);
        assert_eq!(answer["error"]["data"]["code"], code, "{line}: {answer}");
        assert!(
            answer["error"]["message"].as_str().is_some_and(|m| !m.is_empty()),
            "{line}"
        );
        assert!(answer.get("result").is_none());
    }
    // and the editor still works afterwards
    assert_eq!(
        ask(&mut ed, r#"{"id": 2, "method": "layers"}"#)["result"],
        json!([])
    );
}

#[test]
fn nothing_is_edited_while_the_animation_plays() {
    let mut ed = editor();
    ed.playing = true;
    let answer = ask(
        &mut ed,
        r#"{"id": 1, "method": "add_text_layer", "params": {"content": "x", "x": 0, "y": 0}}"#,
    );
    assert_eq!(answer["error"]["data"]["code"], "playing");
    assert_eq!(
        ask(&mut ed, r#"{"id": 2, "method": "undo"}"#)["error"]["data"]["code"],
        "playing"
    );
    // queries still answer
    assert!(
        ask(&mut ed, r#"{"id": 3, "method": "layers"}"#)
            .get("result")
            .is_some()
    );
}

#[test]
fn the_method_list_names_every_method_the_dispatcher_knows_and_nothing_else() {
    let mut ed = editor();
    let listed = ask(&mut ed, r#"{"id": 1, "method": "api.methods"}"#);
    let names: Vec<&str> = listed["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), METHODS.len());
    for method in METHODS {
        assert!(names.contains(&method.name));
        // calling it with nothing must be answered (a result or an error), never a panic,
        // and it must not be "no such method"
        let answer = ask(
            &mut ed,
            &json!({"id": 1, "method": method.name, "params": {}}).to_string(),
        );
        assert_ne!(
            answer["error"]["message"],
            format!("there is no method \"{}\"", method.name)
        );
        assert!(!method.about.is_empty() && !method.about.contains('\u{2014}'));
    }
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "no method is listed twice");
}

#[test]
fn hostile_values_never_panic() {
    let mut ed = editor();
    let id = ask(
        &mut ed,
        r#"{"id": 1, "method": "add_text_layer", "params": {"content": "x", "x": 0, "y": 0}}"#,
    )["result"]
        .as_u64()
        .unwrap();
    let hostile = [
        json!({"layer": id, "style": {"size": "big"}}),
        json!({"layer": id, "style": {"size": null, "width": 1e38}}),
        json!({"layer": id, "style": null}),
        json!({"layer": id, "style": {"family": 7}}),
        json!({"layer": u64::MAX, "style": {}}),
    ];
    for params in hostile {
        let line = json!({"id": 1, "method": "set_text_style", "params": params}).to_string();
        let answer = ask(&mut ed, &line);
        assert!(
            answer.get("result").is_some() || answer.get("error").is_some(),
            "{line}"
        );
    }
    let huge = "x".repeat(300);
    let rename = json!({"id": 1, "method": "rename_layer", "params": {"id": id, "name": huge}});
    assert_eq!(
        ask(&mut ed, &rename.to_string())["error"]["data"]["code"],
        "invalid_argument"
    );
}

#[test]
fn answers_are_json_rpc_two_with_the_spec_codes_and_the_apis_own_code_in_data() {
    let mut ed = editor();
    let ok = ask(&mut ed, r#"{"jsonrpc": "2.0", "id": 1, "method": "layers"}"#);
    assert_eq!(ok["jsonrpc"], "2.0");
    let unknown = ask(&mut ed, r#"{"id": 2, "method": "nothing_like_it"}"#);
    assert_eq!(
        (
            unknown["error"]["code"].as_i64(),
            unknown["error"]["data"]["code"].as_str()
        ),
        (Some(-32601), Some("not_found"))
    );
    let params = ask(&mut ed, r#"{"id": 3, "method": "layer", "params": {}}"#);
    assert_eq!(params["error"]["code"], -32602);
    let parse = ask(&mut ed, "{");
    assert_eq!(
        (parse["error"]["code"].as_i64(), parse["id"].is_null()),
        (Some(-32700), true)
    );
    let invalid = ask(&mut ed, r#"{"id": 4}"#);
    assert_eq!(invalid["error"]["code"], -32600);
    let gone = ask(&mut ed, r#"{"id": 5, "method": "layer", "params": {"id": 77}}"#);
    assert_eq!(
        (
            gone["error"]["code"].as_i64(),
            gone["error"]["data"]["code"].as_str()
        ),
        (Some(-32000), Some("not_found"))
    );
}

#[test]
fn a_notification_is_run_and_not_answered() {
    let mut ed = editor();
    let line =
        r#"{"jsonrpc": "2.0", "method": "add_text_layer", "params": {"content": "quiet", "x": 0, "y": 0}}"#;
    assert!(rpc::handle_line(&mut Api::new(&mut ed), line).is_none());
    assert_eq!(
        ask(&mut ed, r#"{"id": 1, "method": "layers"}"#)["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn captions_go_in_and_out_as_json_text() {
    let mut ed = editor();
    let content = "1\n00:00:01,000 --> 00:00:02,000\nHello\n";
    let made = ask(
        &mut ed,
        &json!({"id": 1, "method": "import_captions", "params": {"content": content}}).to_string(),
    );
    assert_eq!(made["result"]["captions"], 1, "{made}");
    let layer = made["result"]["layer"].as_u64().unwrap();
    let out = ask(
        &mut ed,
        &json!({"id": 2, "method": "export_captions", "params": {"layer": layer, "format": "srt"}})
            .to_string(),
    );
    assert!(out["result"].as_str().unwrap().contains("Hello"), "{out}");
}

#[test]
fn a_text_animator_value_goes_through_json_in_all_three_shapes() {
    let mut ed = editor();
    let layer = ask(
        &mut ed,
        r#"{"id": 1, "method": "add_text_layer", "params": {"content": "Hi", "x": 0, "y": 0}}"#,
    )["result"]
        .as_u64()
        .unwrap();
    let add = json!({"id": 2, "method": "add_text_animator", "params": {"layer": layer, "preset": "wave"}});
    ask(&mut ed, &add.to_string());
    for (property, value) in [
        ("tracking", json!(150)),
        ("position", json!([0, -20])),
        ("fill", json!("#00ff88")),
    ] {
        let call = json!({"id": 3, "method": "set_text_animator_value",
            "params": {"layer": layer, "index": 0, "property": property, "value": value}});
        let answer = ask(&mut ed, &call.to_string());
        assert!(answer.get("error").is_none(), "{property}: {answer}");
    }
    let wrong = json!({"id": 4, "method": "set_text_animator_value",
        "params": {"layer": layer, "index": 0, "property": "tracking", "value": "red"}});
    assert_eq!(
        ask(&mut ed, &wrong.to_string())["error"]["data"]["code"],
        "invalid_argument"
    );
}

#[test]
fn a_rig_and_its_spring_are_built_from_json_alone() {
    let mut ed = editor();
    let mut call = |method: &str, params: serde_json::Value| -> Value {
        let line = json!({"id": 1, "method": method, "params": params}).to_string();
        let answer = ask(&mut ed, &line);
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        answer["result"].clone()
    };
    let layer = call("add_bone_layer", json!({"name": "Tail"})).as_u64().unwrap();
    let root = call(
        "add_bone",
        json!({"layer": layer, "name": "root", "x": 0, "y": 0, "length": 50}),
    )
    .as_u64()
    .unwrap();
    let tip = call(
        "add_bone",
        json!({"layer": layer, "name": "tip", "parent": root, "x": 50, "y": 0, "length": 50}),
    )
    .as_u64()
    .unwrap();
    call(
        "set_bone_angle",
        json!({"layer": layer, "bone": root, "degrees": 0, "frame": 1}),
    );
    call(
        "set_bone_angle",
        json!({"layer": layer, "bone": root, "degrees": 90, "frame": 12}),
    );
    let keys = call(
        "bake_spring",
        json!({"layer": layer, "bones": [tip], "stiffness": 200, "damping": 6, "start": 1, "end": 60}),
    );
    assert_eq!(keys, 60);
}

#[test]
fn a_client_without_a_permission_is_refused_before_anything_runs() {
    use fanimate_api::{Permission, Permissions};
    let mut ed = editor();
    let as_client = |ed: &mut Editor, perms: Permissions, request: Value| -> Value {
        let answer = rpc::handle_line_as(&mut Api::new(ed), &request.to_string(), perms).expect("answered");
        serde_json::from_str(&answer).unwrap()
    };
    let add = json!({"id": 1, "method": "add_layer", "params": {"kind": "vector", "name": "Ink"}});

    // read only: queries work, an edit is refused and changes nothing
    let reader = Permissions::NONE.with(Permission::Read);
    assert_eq!(
        as_client(&mut ed, reader, json!({"id": 1, "method": "layers"}))["result"],
        json!([])
    );
    let refused = as_client(&mut ed, reader, add.clone());
    assert_eq!(refused["error"]["data"]["code"], "permission_denied");
    assert!(refused["error"]["message"].as_str().unwrap().contains("\"edit\""));
    assert!(ed.doc.layers.is_empty());
    // undo is an edit too
    let undo = as_client(&mut ed, reader, json!({"id": 2, "method": "undo"}));
    assert_eq!(undo["error"]["data"]["code"], "permission_denied");
    // saving is a permission of its own, even though only the host does it
    let save = as_client(
        &mut ed,
        reader.with(Permission::Edit),
        json!({"id": 3, "method": "save"}),
    );
    assert_eq!(save["error"]["data"]["code"], "permission_denied");

    // with edit, the same call runs
    let editing = reader.with(Permission::Edit);
    assert!(as_client(&mut ed, editing, add).get("error").is_none());
    assert_eq!(ed.doc.layers.len(), 1);

    // no permissions: even a query is refused, but a method that does not exist is still not_found
    let nothing = as_client(&mut ed, Permissions::NONE, json!({"id": 4, "method": "layers"}));
    assert_eq!(nothing["error"]["data"]["code"], "permission_denied");
    let unknown = as_client(&mut ed, Permissions::NONE, json!({"id": 5, "method": "no.such"}));
    assert_eq!(unknown["error"]["data"]["code"], "not_found");

    // a notification that is refused is still silent, and still does nothing
    let note = rpc::handle_line_as(
        &mut Api::new(&mut ed),
        &json!({"method": "delete_layer", "params": {"id": 1}}).to_string(),
        reader,
    );
    assert!(note.is_none());
    assert_eq!(ed.doc.layers.len(), 1);
}

#[test]
fn every_method_needs_a_permission_and_the_list_says_which() {
    use fanimate_api::Permission;
    let mut ed = editor();
    let listed = ask(&mut ed, r#"{"id": 1, "method": "api.methods"}"#);
    for m in listed["result"].as_array().unwrap() {
        let name = m["name"].as_str().unwrap();
        let edits = m["edits"].as_bool().unwrap();
        let needed = rpc::required(name).unwrap_or_else(|| panic!("{name} needs no permission"));
        assert_eq!(
            needed,
            if edits { Permission::Edit } else { Permission::Read },
            "{name}"
        );
        assert_eq!(m["permission"], needed.as_str(), "{name}");
    }
    assert_eq!(rpc::required("save"), Some(Permission::Save));
    assert_eq!(rpc::required("shutdown"), Some(Permission::Edit));
    assert_eq!(rpc::required("quit"), None);
    assert_eq!(rpc::required("auth"), None);
    assert_eq!(rpc::required("no.such"), None);
}

#[test]
fn bake_expression_is_a_json_call() {
    let mut ed = editor();
    let layer = ask(
        &mut ed,
        r#"{"id": 1, "method": "add_layer", "params": {"kind": "vector", "name": "Eye"}}"#,
    )["result"]
        .as_u64()
        .unwrap();
    let baked = ask(
        &mut ed,
        &json!({"id": 2, "method": "bake_expression", "params": {
            "layer": layer, "channel": "opacity", "component": 0,
            "expression": "frame * 10", "first": 1, "last": 5}})
        .to_string(),
    );
    assert_eq!(baked["result"], 5, "{baked}");
    let curves = ask(
        &mut ed,
        &json!({"id": 3, "method": "curves", "params": {"layer": layer}}).to_string(),
    );
    assert_eq!(curves["result"][0]["keys"][4]["value"], 50.0);
    // a missing parameter is invalid_argument, a bad expression is too
    let missing = ask(
        &mut ed,
        &json!({"id": 4, "method": "bake_expression", "params": {"layer": layer}}).to_string(),
    );
    assert_eq!(missing["error"]["data"]["code"], "invalid_argument");
    let bad = ask(
        &mut ed,
        &json!({"id": 5, "method": "bake_expression", "params": {
            "layer": layer, "channel": "opacity", "component": 0,
            "expression": "frame +", "first": 1, "last": 5}})
        .to_string(),
    );
    assert_eq!(bad["error"]["data"]["code"], "invalid_argument");
}

#[test]
fn import_ora_takes_the_file_as_base64() {
    // a tiny archive, built the way the API test builds one
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let stack = r#"<image w="4" h="4"><stack><layer name="L" src="data/l.png"/></stack></image>"#;
    let file = fanimate_fnm::zip::write(&[
        ("mimetype", b"image/openraster".as_slice()),
        ("stack.xml", stack.as_bytes()),
        ("data/l.png", &png),
    ])
    .unwrap();
    let encode = |bytes: &[u8]| {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
            for i in 0..=chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
            out.extend(std::iter::repeat_n('=', 3 - chunk.len()));
        }
        out
    };
    let mut ed = editor();
    let data = encode(&file);
    let made = ask(
        &mut ed,
        &json!({"id": 1, "method": "import_ora", "params": {"name": "a.ora", "data": data}}).to_string(),
    );
    assert!(made.get("error").is_none(), "{made}");
    let count = |ed: &Editor| {
        let mut n = 0;
        ed.doc.walk(|_| n += 1);
        n
    };
    assert_eq!(count(&ed), 2, "a group and its one layer");
    // white space in the text is ignored, and padding may be left off
    let spaced = format!("{}\n{}", &data[..8], data[8..].trim_end_matches('='));
    let again = ask(
        &mut ed,
        &json!({"id": 2, "method": "import_ora", "params": {"name": "b.ora", "data": spaced}}).to_string(),
    );
    assert!(again.get("error").is_none(), "{again}");
    assert_eq!(
        count(&ed),
        4,
        "the second file came in too, inside the selected group"
    );
    // what is not base64 is invalid_argument, and nothing is added
    for bad in ["@@@@", "QUJD=QUJD", "QUJDR", "QQ===", "é"] {
        let r = ask(
            &mut ed,
            &json!({"id": 3, "method": "import_ora", "params": {"name": "c.ora", "data": bad}}).to_string(),
        );
        assert_eq!(r["error"]["data"]["code"], "invalid_argument", "{bad}: {r}");
    }
    assert_eq!(count(&ed), 4);
}
