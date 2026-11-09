//! The classical assist tools through the API: line clean-up, inbetween
//! assist and auto-rig, called as a script or plugin would call them.

use fanimate_api::{Api, ErrorCode, Permissions, rpc};
use fanimate_core::frames::Cel;
use fanimate_core::{Color, Document, LayerKind, Path, Shape, Style, Vec2};
use fanimate_editor::Editor;
use serde_json::{Value, json};

fn editor() -> Editor {
    Editor::new(Document::new("t", 800.0, 800.0, 24.0))
}

fn style() -> Style {
    Style::default().with_stroke(Color::BLACK, 2.0)
}

fn rough_circle() -> Path {
    let mut p = Path::new();
    for i in 0..150 {
        let a = i as f32 / 150.0 * std::f32::consts::TAU;
        let r = 80.0 + ((i * 7919 % 13) as f32 - 6.0) * 0.08;
        let v = Vec2::new(200.0 + r * a.cos(), 200.0 + r * a.sin());
        p = if i == 0 { p.move_to(v) } else { p.line_to(v) };
    }
    p.close()
}

fn add_vector(ed: &mut Editor, name: &str, shapes: Vec<Shape>) -> u32 {
    let layer = ed.doc.new_layer(name, LayerKind::vector(shapes));
    let id = layer.id.0;
    ed.doc.layers.push(layer);
    id
}

fn call(ed: &mut Editor, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    let answer = rpc::handle_line(&mut Api::new(ed), &line).unwrap();
    serde_json::from_str(&answer).unwrap()
}

#[test]
fn clean_up_lines_over_json_rpc_reports_and_labels() {
    let mut ed = editor();
    let id = add_vector(
        &mut ed,
        "ink",
        vec![Shape::new("circle", rough_circle(), style())],
    );
    let answer = call(&mut ed, "clean_up_lines", json!({"layer": id}));
    let result = &answer["result"];
    assert_eq!(result["shapes"], 1, "{answer}");
    assert_eq!(result["points_before"], 150);
    assert!(result["points_after"].as_u64().unwrap() < 30);
    let layer = ed.doc.find(fanimate_core::LayerId(id)).unwrap();
    assert_eq!(
        layer.provenance.as_ref().map(|p| p.tool.as_str()),
        Some("line-clean-up")
    );
    // a looser tolerance keeps fewer points than a tight one on the same line
    let mut tight = editor();
    let id = add_vector(
        &mut tight,
        "ink",
        vec![Shape::new("circle", rough_circle(), style())],
    );
    let t = call(
        &mut tight,
        "clean_up_lines",
        json!({"layer": id, "tolerance": 0.05}),
    );
    assert!(t["result"]["points_after"].as_u64().unwrap() > result["points_after"].as_u64().unwrap());
}

#[test]
fn clean_up_lines_refuses_bad_arguments_with_the_stable_codes() {
    let mut ed = editor();
    let id = add_vector(
        &mut ed,
        "ink",
        vec![Shape::new("circle", rough_circle(), style())],
    );
    let code = |answer: &Value| answer["error"]["data"]["code"].as_str().map(str::to_owned);
    let a = call(&mut ed, "clean_up_lines", json!({"layer": id, "tolerance": 0.0}));
    assert_eq!(code(&a).as_deref(), Some(ErrorCode::InvalidArgument.as_str()));
    let a = call(
        &mut ed,
        "clean_up_lines",
        json!({"layer": id, "tolerance": 1000.0}),
    );
    assert_eq!(code(&a).as_deref(), Some(ErrorCode::InvalidArgument.as_str()));
    let a = call(&mut ed, "clean_up_lines", json!({"layer": 9999}));
    assert_eq!(code(&a).as_deref(), Some(ErrorCode::NotFound.as_str()));
    let a = call(&mut ed, "clean_up_lines", json!({}));
    assert_eq!(code(&a).as_deref(), Some(ErrorCode::InvalidArgument.as_str()));
    assert!(!ed.dirty, "a refused call changes nothing");
}

fn drawing(frame: i32, x: f32) -> Cel {
    let mut cel = Cel::new(frame);
    cel.line.push(Shape::new(
        "line",
        Path::line(Vec2::new(x, 0.0), Vec2::new(x + 100.0, 0.0)),
        style(),
    ));
    cel
}

#[test]
fn suggest_inbetween_over_json_rpc_adds_a_labelled_drawing() {
    let mut ed = editor();
    let layer = ed.doc.new_layer(
        "anim",
        LayerKind::Frames {
            cels: vec![drawing(1, 0.0), drawing(11, 100.0)],
        },
    );
    let id = layer.id.0;
    ed.doc.layers.push(layer);
    ed.go_to(1);
    let answer = call(&mut ed, "suggest_inbetween", json!({"layer": id}));
    assert_eq!(answer["result"]["frame"], 6, "{answer}");
    assert_eq!(answer["result"]["blended"], 1);
    let layer = ed.doc.find(fanimate_core::LayerId(id)).unwrap();
    let LayerKind::Frames { cels } = &layer.kind else {
        panic!()
    };
    assert_eq!(cels.len(), 3);
    assert_eq!(
        layer.provenance.as_ref().map(|p| p.tool.as_str()),
        Some("inbetween-assist")
    );
    // there is nothing after the last drawing
    ed.go_to(11);
    let again = call(&mut ed, "suggest_inbetween", json!({"layer": id}));
    assert!(
        again["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no later drawing"),
        "{again}"
    );
}

#[test]
fn auto_rig_over_json_rpc_makes_a_bone_layer_and_names_what_it_left() {
    let mut ed = editor();
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
        Shape::new(
            "r",
            Path::rect(Vec2::new(x0, y0), Vec2::new(x1 - x0, y1 - y0), 0.0),
            Style::fill(Color::BLACK),
        )
    };
    let bg = add_vector(&mut ed, "background", vec![rect(0.0, 0.0, 800.0, 800.0)]);
    let torso = add_vector(&mut ed, "torso", vec![rect(350.0, 300.0, 450.0, 520.0)]);
    let head = add_vector(&mut ed, "head", vec![rect(360.0, 180.0, 440.0, 300.0)]);
    let answer = call(&mut ed, "auto_rig", json!({"layers": [bg, torso, head]}));
    let result = &answer["result"];
    assert_eq!(
        (result["bones"].as_u64(), result["bound"].as_u64()),
        (Some(2), Some(2)),
        "{answer}"
    );
    assert_eq!(result["left_out"], json!(["background"]));
    // the rig is a bone layer, marked as made by the tool
    let rig = ed
        .doc
        .find(fanimate_core::LayerId(result["layer"].as_u64().unwrap() as u32))
        .unwrap();
    assert!(matches!(rig.kind, LayerKind::Bone { .. }));
    assert_eq!(rig.provenance.as_ref().map(|p| p.tool.as_str()), Some("auto-rig"));
    // no torso: refused, nothing changes
    let mut other = editor();
    let only_head = add_vector(&mut other, "head", vec![rect(0.0, 0.0, 10.0, 10.0)]);
    let refused = call(&mut other, "auto_rig", json!({"layers": [only_head]}));
    assert!(
        refused["error"]["message"].as_str().unwrap().contains("torso"),
        "{refused}"
    );
    assert!(!other.dirty);
}

#[test]
fn the_assist_calls_need_the_edit_permission() {
    for method in ["clean_up_lines", "suggest_inbetween", "auto_rig"] {
        assert_eq!(
            rpc::required(method),
            Some(fanimate_api::Permission::Edit),
            "{method}"
        );
        let mut ed = editor();
        let line = json!({"id": 1, "method": method, "params": {"layer": 1, "layers": [1]}}).to_string();
        let read_only = Permissions::NONE.with(fanimate_api::Permission::Read);
        let answer = rpc::handle_line_as(&mut Api::new(&mut ed), &line, read_only).unwrap();
        assert!(answer.contains("permission_denied"), "{method}: {answer}");
    }
}

#[test]
fn raster_layers_are_made_and_painted_over_json_rpc() {
    let mut ed = editor();
    let made = call(&mut ed, "add_raster_layer", json!({"name": "Paint"}));
    let id = made["result"].as_u64().unwrap_or_else(|| panic!("{made}"));
    let a = call(
        &mut ed,
        "paint",
        json!({"layer": id, "points": [[20, 20], [120, 40]], "color": "#e63946", "size": 16}),
    );
    assert_eq!(a["result"], 1, "{a}");
    // a stroke over a tile edge changes two tiles; rubbing out changes the same ones
    let wide = call(
        &mut ed,
        "paint",
        json!({"layer": id, "points": [[240, 100], [280, 100]], "size": 30}),
    );
    assert_eq!(wide["result"], 2, "{wide}");
    let rub = call(
        &mut ed,
        "paint",
        json!({"layer": id, "points": [[240, 100], [280, 100]], "size": 60, "hardness": 1, "erase": true}),
    );
    assert_eq!(rub["result"], 2, "{rub}");
    // bad arguments are refused with the stable codes and change nothing
    let code = |a: &Value| a["error"]["data"]["code"].as_str().map(str::to_owned);
    let before = ed.doc.clone();
    for bad in [
        json!({"layer": id, "points": []}),
        json!({"layer": id, "points": [[1, 1]], "color": "red"}),
        json!({"layer": 9999, "points": [[1, 1]]}),
    ] {
        let answer = call(&mut ed, "paint", bad);
        assert!(code(&answer).is_some(), "{answer}");
    }
    assert!(ed.doc == before);
    // a vector layer is not painted on
    let vector = call(&mut ed, "add_layer", json!({"kind": "vector", "name": "v"}));
    let refused = call(
        &mut ed,
        "paint",
        json!({"layer": vector["result"], "points": [[1, 1]]}),
    );
    assert_eq!(code(&refused).as_deref(), Some("invalid_argument"), "{refused}");
    assert_eq!(rpc::required("paint"), Some(fanimate_api::Permission::Edit));
}

#[test]
fn the_palette_is_listed_and_a_colour_is_replaced_everywhere_in_one_step() {
    let mut ed = editor();
    let red = Shape::new(
        "r",
        Path::rect(Vec2::ZERO, Vec2::new(10.0, 10.0), 0.0),
        Style::fill(Color::rgb(1.0, 0.0, 0.0)),
    );
    let id = add_vector(&mut ed, "art", vec![red.clone(), red]);
    let _ = id;
    let listed = call(&mut ed, "palette", json!({}));
    let first = &listed["result"][0];
    assert_eq!(
        (first["color"].as_str(), first["count"].as_u64()),
        (Some("#FF0000"), Some(2)),
        "{listed}"
    );
    let done = call(
        &mut ed,
        "replace_colour",
        json!({"from": "#ff0000", "to": "#0088ff"}),
    );
    assert_eq!(done["result"], 2, "{done}");
    let after = call(&mut ed, "palette", json!({}));
    assert_eq!(after["result"][0]["color"], "#0088FF");
    // a colour that is not used changes nothing and leaves no undo step
    let steps = ed.history.cursor();
    let none = call(
        &mut ed,
        "replace_colour",
        json!({"from": "#123456", "to": "#abcdef"}),
    );
    assert_eq!(none["result"], 0);
    assert_eq!(ed.history.cursor(), steps);
    let bad = call(&mut ed, "replace_colour", json!({"from": "red", "to": "#abcdef"}));
    assert_eq!(bad["error"]["data"]["code"], "invalid_argument", "{bad}");
    assert_eq!(
        rpc::required("replace_colour"),
        Some(fanimate_api::Permission::Edit)
    );
    assert_eq!(rpc::required("palette"), Some(fanimate_api::Permission::Read));
}
