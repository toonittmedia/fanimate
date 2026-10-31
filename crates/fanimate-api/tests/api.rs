//! The API through an editor, the way a script would use it.

use fanimate_api::{API_VERSION, Api, ErrorCode, MAX_NAME_CHARS, TextStyle};
use fanimate_core::{Document, LayerKind, Provenance, samples};
use fanimate_editor::{Action, Editor};

fn three() -> Editor {
    let mut doc = Document::new("t", 100.0, 100.0, 24.0);
    for n in ["A", "B", "C"] {
        let l = doc.new_layer(n, LayerKind::vector(vec![]));
        doc.layers.push(l);
    }
    Editor::new(doc)
}

#[test]
fn the_document_and_its_layers_are_described() {
    let mut ed = three();
    let api = Api::new(&mut ed);
    let doc = api.document();
    assert_eq!((doc.name.as_str(), doc.layers, doc.fps), ("t", 3, 24.0));
    assert_eq!(doc.format_version, fanimate_fnm::FORMAT_VERSION);
    assert_eq!(doc.api_version, API_VERSION);
    let layers = api.layers();
    assert_eq!(
        layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert!(
        layers
            .iter()
            .all(|l| l.kind == "vector" && l.parent.is_none() && l.visible)
    );
    let b = api.layer(layers[1].id).unwrap();
    assert_eq!(b.name, "B");
}

#[test]
fn nested_layers_come_after_their_parent_and_name_it() {
    let mut ed = Editor::new(samples::walk_cycle());
    let api = Api::new(&mut ed);
    let layers = api.layers();
    assert!(
        layers.iter().any(|l| l.parent.is_some()),
        "the walk cycle has layers inside layers"
    );
    for (i, l) in layers.iter().enumerate() {
        if let Some(parent) = l.parent {
            let at = layers
                .iter()
                .position(|p| p.id == parent)
                .expect("the parent is listed");
            assert!(at < i, "{} comes before its child {}", layers[at].name, l.name);
            assert!(layers[at].children > 0);
        }
    }
    let ids: std::collections::BTreeSet<_> = layers.iter().map(|l| l.id).collect();
    assert_eq!(ids.len(), layers.len(), "ids are unique");
}

#[test]
fn a_missing_layer_is_not_found() {
    let mut ed = three();
    let mut api = Api::new(&mut ed);
    assert_eq!(api.layer(999).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(api.rename_layer(999, "x").unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        api.set_layer_visible(999, false).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.set_layer_provenance(999, None).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(
        !ed.dirty && ed.history.undo_label().is_none(),
        "a failed call leaves no trace"
    );
}

#[test]
fn a_command_is_one_undo_step_and_undo_takes_it_back() {
    let mut ed = three();
    let id = Api::new(&mut ed).layers()[1].id;
    let mut api = Api::new(&mut ed);
    api.rename_layer(id, "  Arm  ").unwrap();
    api.set_layer_visible(id, false).unwrap();
    assert_eq!(api.layer(id).unwrap().name, "Arm", "the name is trimmed");
    assert!(!api.layer(id).unwrap().visible);
    assert!(ed.dirty);
    assert_eq!(ed.history.undo_label(), Some("Hide layer"));
    ed.run(Action::Undo);
    assert!(Api::new(&mut ed).layer(id).unwrap().visible);
    ed.run(Action::Undo);
    assert_eq!(Api::new(&mut ed).layer(id).unwrap().name, "B");
    assert_eq!(ed.history.undo_label(), None);
}

#[test]
fn a_call_that_changes_nothing_records_nothing() {
    let mut ed = three();
    let id = Api::new(&mut ed).layers()[0].id;
    let mut api = Api::new(&mut ed);
    api.rename_layer(id, "A").unwrap();
    api.set_layer_visible(id, true).unwrap();
    api.set_layer_provenance(id, None).unwrap();
    assert!(!ed.dirty);
    assert_eq!(ed.history.undo_label(), None);
}

#[test]
fn unusable_arguments_are_refused_before_anything_changes() {
    let mut ed = three();
    let id = Api::new(&mut ed).layers()[0].id;
    let mut api = Api::new(&mut ed);
    for name in ["", "   ", &"x".repeat(MAX_NAME_CHARS + 1)] {
        assert_eq!(
            api.rename_layer(id, name).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    assert!(api.rename_layer(id, &"x".repeat(MAX_NAME_CHARS)).is_ok());
    for bad in [
        Provenance::new(""),
        Provenance {
            date: "yesterday".into(),
            ..Provenance::new("t")
        },
        Provenance {
            model: "m".repeat(500),
            ..Provenance::new("t")
        },
    ] {
        let err = api.set_layer_provenance(id, Some(bad)).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
        assert!(err.to_string().starts_with("invalid_argument: "), "{err}");
    }
    assert!(api.layer(id).unwrap().provenance.is_none());
}

#[test]
fn nothing_is_edited_while_the_animation_plays() {
    let mut ed = three();
    let id = Api::new(&mut ed).layers()[0].id;
    ed.playing = true;
    let mut api = Api::new(&mut ed);
    assert_eq!(api.rename_layer(id, "x").unwrap_err().code, ErrorCode::Playing);
    assert_eq!(
        api.set_layer_visible(id, false).unwrap_err().code,
        ErrorCode::Playing
    );
    // reading is always fine
    assert_eq!(api.layer(id).unwrap().name, "A");
    assert!(!ed.dirty);
}

#[test]
fn error_codes_are_stable() {
    // scripts match on these strings: changing one is a breaking change
    assert_eq!(ErrorCode::NotFound.as_str(), "not_found");
    assert_eq!(ErrorCode::InvalidArgument.as_str(), "invalid_argument");
    assert_eq!(ErrorCode::Playing.as_str(), "playing");
    assert_eq!(ErrorCode::Io.as_str(), "io_error");
    assert_eq!(ErrorCode::Unauthorized.as_str(), "unauthorized");
    assert_eq!(ErrorCode::PermissionDenied.as_str(), "permission_denied");
}

#[test]
fn the_data_serialises_to_the_json_a_script_sees() {
    let mut ed = three();
    let mut api = Api::new(&mut ed);
    let id = api.layers()[0].id;
    let plain = serde_json::to_value(api.layer(id).unwrap()).unwrap();
    assert_eq!(
        plain,
        serde_json::json!({
            "id": id, "name": "A", "kind": "vector", "parent": null,
            "visible": true, "locked": false, "children": 0, "provenance": null
        })
    );
    let mut record = Provenance::new("line-clean-up");
    record.date = "2026-10-30".into();
    api.set_layer_provenance(id, Some(record)).unwrap();
    let marked = serde_json::to_value(api.layer(id).unwrap()).unwrap();
    assert_eq!(
        marked["provenance"],
        serde_json::json!({"tool": "line-clean-up", "date": "2026-10-30"})
    );
    let info = serde_json::to_value(api.document()).unwrap();
    assert_eq!(info["api_version"], API_VERSION);
    assert_eq!(info["layers"], 3);
}

/// The exit check for this slice: a stub tool changes a document through the
/// API, the change is one undo step, and it survives a save, a reload and the
/// feature check, with the file saying a tool made the layer.
#[test]
fn a_tool_marks_a_layer_through_the_api_and_the_file_keeps_it() {
    let dir = std::env::temp_dir().join(format!("fanimate-api-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut ed = three();
    let id = Api::new(&mut ed).layers()[2].id;
    let mut record = Provenance::new("auto-rig");
    record.model = "rig-net".into();
    record.version = "0.1".into();
    record.date = "2026-10-30".into();
    Api::new(&mut ed)
        .set_layer_provenance(id, Some(record.clone()))
        .unwrap();
    assert_eq!(
        fanimate_fnm::required_features(&ed.doc),
        [fanimate_fnm::FEATURE_PROVENANCE]
    );

    let path = dir.join("marked.fnm");
    fanimate_fnm::save(&ed.doc, &path).unwrap();
    let (loaded, report) = fanimate_fnm::load(&path).unwrap();
    assert!(report.is_clean(), "{report}");
    let mut reloaded = Editor::new(loaded);
    assert_eq!(
        Api::new(&mut reloaded).layer(id).unwrap().provenance,
        Some(record)
    );

    ed.run(Action::Undo);
    assert!(Api::new(&mut ed).layer(id).unwrap().provenance.is_none());
    assert!(fanimate_fnm::required_features(&ed.doc).is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bones_are_named_by_ids_that_survive_reparenting() {
    let mut ed = Editor::new(samples::walk_cycle());
    let rig = Api::new(&mut ed)
        .layers()
        .into_iter()
        .find(|l| l.kind == "bone")
        .expect("the walk cycle has a bone layer")
        .id;
    let before = Api::new(&mut ed).bones(rig).unwrap();
    assert!(before.len() >= 3);
    let mut ids: Vec<u32> = before.iter().map(|b| b.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), before.len(), "ids are unique");
    for b in &before {
        if let Some(p) = b.parent {
            let at = before
                .iter()
                .position(|x| x.id == p)
                .expect("the parent is listed");
            assert!(
                at < before.iter().position(|x| x.id == b.id).unwrap(),
                "parents first"
            );
        }
    }

    // the root goes under the last bone, which becomes a root itself: the list is reordered
    let root = before[0].clone();
    let last = fanimate_editor::BoneRef {
        layer: fanimate_core::LayerId(rig),
        bone: before.len() - 1,
    };
    assert!(ed.set_bone_parent(last, None));
    assert!(ed.set_bone_parent(
        fanimate_editor::BoneRef {
            layer: last.layer,
            bone: 0
        },
        Some(last.bone)
    ));
    let after = Api::new(&mut ed).bones(rig).unwrap();
    assert_ne!(after[0].id, root.id, "the list was reordered");
    Api::new(&mut ed).rename_bone(rig, root.id, "  Hip  ").unwrap();
    let renamed = Api::new(&mut ed).bones(rig).unwrap();
    assert_eq!(renamed.iter().find(|b| b.id == root.id).unwrap().name, "Hip");
    assert_eq!(renamed.iter().filter(|b| b.name == "Hip").count(), 1);

    // one undo step, and the errors
    ed.run(Action::Undo);
    assert_eq!(
        Api::new(&mut ed)
            .bones(rig)
            .unwrap()
            .iter()
            .find(|b| b.id == root.id)
            .unwrap()
            .name,
        root.name
    );
    let mut api = Api::new(&mut ed);
    assert_eq!(
        api.rename_bone(rig, 9999, "x").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.rename_bone(rig, root.id, " ").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    let art = api.layers().into_iter().find(|l| l.kind != "bone").unwrap().id;
    assert_eq!(
        api.bones(art).unwrap_err().code,
        ErrorCode::NotFound,
        "not a bone layer"
    );
    assert_eq!(api.bones(9999).unwrap_err().code, ErrorCode::NotFound);
}

fn titled() -> (Editor, u32) {
    let mut ed = Editor::new(Document::new("t", 1280.0, 720.0, 24.0));
    let id = Api::new(&mut ed)
        .add_text_layer("Hello, API", 100.0, 80.0)
        .unwrap();
    (ed, id)
}

#[test]
fn a_script_adds_a_text_layer_reads_it_and_it_is_one_undo_step() {
    let (mut ed, id) = titled();
    let api = Api::new(&mut ed);
    let layer = api.layer(id).unwrap();
    assert_eq!((layer.kind, layer.name.as_str()), ("text", "Text 1"));
    let info = api.text(id).unwrap();
    assert_eq!(info.content, "Hello, API");
    assert_eq!((info.characters, info.words, info.lines), (10, 2, 1));
    assert_eq!(
        (info.family.as_str(), info.size, info.width),
        ("Inter", 72.0, None)
    );
    assert!(info.animators.is_empty());
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["content"], "Hello, API");
    assert_eq!(json["width"], serde_json::Value::Null);
    assert_eq!(json["animators"], serde_json::json!([]));
    ed.run(Action::Undo);
    assert!(Api::new(&mut ed).layers().is_empty());
}

#[test]
fn text_content_and_style_change_through_the_api_and_numbers_that_are_not_numbers_are_refused() {
    use fanimate_api::TextStyle;
    let (mut ed, id) = titled();
    let mut api = Api::new(&mut ed);
    api.set_text_content(id, "One two three").unwrap();
    api.set_text_style(
        id,
        TextStyle {
            size: Some(120.0),
            weight: Some(700.0),
            width: Some(400.0),
            ..TextStyle::default()
        },
    )
    .unwrap();
    let info = api.text(id).unwrap();
    assert_eq!(
        (info.content.as_str(), info.size, info.weight, info.width),
        ("One two three", 120.0, 700.0, Some(400.0))
    );
    assert_eq!(info.words, 3);
    // a style can be read from JSON the way a script sends it, and turn wrapping off
    let style: TextStyle = serde_json::from_str(r#"{"size": 48, "wrap": false}"#).unwrap();
    assert_eq!((style.wrap, style.width), (Some(false), None));
    api.set_text_style(id, style).unwrap();
    let info = api.text(id).unwrap();
    assert_eq!((info.size, info.width, info.weight), (48.0, None, 700.0));
    // refused, not silently held
    for bad in [f32::NAN, f32::INFINITY] {
        let err = api
            .set_text_style(
                id,
                TextStyle {
                    size: Some(bad),
                    ..TextStyle::default()
                },
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
    }
    // out of range is held in range
    api.set_text_style(
        id,
        TextStyle {
            size: Some(1.0e9),
            ..TextStyle::default()
        },
    )
    .unwrap();
    assert_eq!(api.text(id).unwrap().size, 4096.0);
    // each call was one step
    ed.run(Action::Undo);
    assert_eq!(Api::new(&mut ed).text(id).unwrap().size, 48.0);
}

#[test]
fn animators_are_added_by_preset_name() {
    let (mut ed, id) = titled();
    let mut api = Api::new(&mut ed);
    assert_eq!(api.add_text_animator(id, "typewriter").unwrap(), 0);
    assert_eq!(api.add_text_animator(id, "rise_in").unwrap(), 1);
    let info = api.text(id).unwrap();
    assert_eq!(
        info.animators
            .iter()
            .map(|a| (a.name.as_str(), a.unit))
            .collect::<Vec<_>>(),
        [("Typewriter", "character"), ("Rise in by word", "word")]
    );
    assert_eq!(
        api.add_text_animator(id, "spin").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    for preset in ["fade_in", "pop_in", "wave"] {
        assert!(api.add_text_animator(id, preset).is_ok(), "{preset}");
    }
    ed.run(Action::Undo);
    assert_eq!(Api::new(&mut ed).text(id).unwrap().animators.len(), 4);
}

#[test]
fn text_calls_name_what_went_wrong() {
    let (mut ed, id) = titled();
    let vector = ed.doc.new_layer("v", LayerKind::vector(vec![]));
    let vid = vector.id.0;
    ed.doc.layers.push(vector);
    let mut api = Api::new(&mut ed);
    assert_eq!(api.text(vid).unwrap_err().code, ErrorCode::NotFound);
    assert!(api.text(vid).unwrap_err().message.contains("not a text layer"));
    assert_eq!(api.text(9999).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        api.set_text_content(vid, "x").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.add_text_animator(vid, "wave").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.add_text_layer("x", f32::NAN, 0.0).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    // nothing is changed while the animation plays
    ed.playing = true;
    let mut api = Api::new(&mut ed);
    assert_eq!(
        api.add_text_layer("x", 0.0, 0.0).unwrap_err().code,
        ErrorCode::Playing
    );
    assert_eq!(
        api.set_text_content(id, "x").unwrap_err().code,
        ErrorCode::Playing
    );
    assert_eq!(api.text(id).unwrap().content, "Hello, API");
}

/// The exit check for text: a script makes a title with an animator, the
/// project saves and reloads (needing `text@1`), and it draws the same.
#[test]
fn a_scripted_title_saves_reloads_and_draws_the_same() {
    let dir = std::env::temp_dir().join(format!("fanimate-api-text-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (mut ed, id) = titled();
    {
        let mut api = Api::new(&mut ed);
        api.set_text_content(id, "A scripted title").unwrap();
        api.add_text_animator(id, "fade_in").unwrap();
    }
    assert_eq!(
        fanimate_fnm::required_features(&ed.doc),
        [fanimate_fnm::FEATURE_TEXT]
    );
    let path = dir.join("title.fnm");
    fanimate_fnm::save(&ed.doc, &path).unwrap();
    let (loaded, report) = fanimate_fnm::load(&path).unwrap();
    assert!(report.is_clean(), "{report}");
    assert_eq!(loaded, ed.doc);
    for frame in [1.0, 10.0, 30.0] {
        let a = fanimate_core::evaluate(&ed.doc, frame);
        let b = fanimate_core::evaluate(&loaded, frame);
        assert_eq!(a.len(), b.len(), "frame {frame}");
        assert!(
            a.iter()
                .zip(&b)
                .all(|(x, y)| x.transform == y.transform && x.opacity == y.opacity)
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_script_makes_text_follow_a_shape_moves_it_along_and_sets_it_straight() {
    let (mut ed, id) = titled();
    let curve = fanimate_core::Path::new()
        .move_to(fanimate_core::Vec2::new(100.0, 400.0))
        .line_to(fanimate_core::Vec2::new(900.0, 400.0));
    let shape = fanimate_core::Shape::new(
        "c",
        curve,
        fanimate_core::Style::fill(fanimate_core::Color::BLACK),
    );
    let layer = ed.doc.new_layer("Curve", LayerKind::vector(vec![shape]));
    let curve_id = layer.id.0;
    ed.doc.layers.push(layer);
    let empty = ed.doc.new_layer("Empty", LayerKind::vector(vec![]));
    let empty_id = empty.id.0;
    ed.doc.layers.push(empty);

    let mut api = Api::new(&mut ed);
    assert!(!api.text(id).unwrap().on_path);
    api.set_text_path(id, curve_id).unwrap();
    assert!(api.text(id).unwrap().on_path);
    api.set_text_path_offset(id, 25.0).unwrap();
    api.clear_text_path(id).unwrap();
    assert!(!api.text(id).unwrap().on_path);
    // clearing what is not there is fine; asking for the impossible is named
    api.clear_text_path(id).unwrap();
    assert_eq!(
        api.set_text_path(id, empty_id).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(api.set_text_path(id, 9999).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        api.set_text_path(curve_id, id).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.set_text_path_offset(id, f32::NAN).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    // each call was one undo step: undoing the clear brings the path back
    ed.run(Action::Undo);
    assert!(Api::new(&mut ed).text(id).unwrap().on_path);
}

const CAPTIONS: &str =
    "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n2\n00:00:03,000 --> 00:00:04,500\nSecond\n";

#[test]
fn captions_are_imported_exported_and_each_is_one_undo_step() {
    let mut ed = Editor::new(Document::new("t", 1280.0, 720.0, 24.0));
    let mut api = Api::new(&mut ed);
    let made = api.import_captions(CAPTIONS, "Dialogue").unwrap();
    assert_eq!((made.captions, made.skipped, made.cut_off), (2, 0, 0));
    let layer = api.layer(made.layer).unwrap();
    assert_eq!(
        (layer.kind, layer.name.as_str(), layer.children),
        ("switch", "Dialogue", 3)
    );
    let srt = api.export_captions(made.layer, "srt").unwrap();
    assert!(
        srt.starts_with("1\n00:00:01,000 --> 00:00:02,000\nHello\n"),
        "{srt}"
    );
    assert!(
        api.export_captions(made.layer, "vtt")
            .unwrap()
            .starts_with("WEBVTT")
    );
    assert!(api.undo().unwrap());
    assert!(api.layers().is_empty());
}

#[test]
fn restyling_captions_copies_one_look_to_the_set_in_one_undo_step() {
    let mut ed = Editor::new(Document::new("t", 1280.0, 720.0, 24.0));
    let mut api = Api::new(&mut ed);
    let made = api.import_captions(CAPTIONS, "Dialogue").unwrap();
    let texts: Vec<u32> = api
        .layers()
        .iter()
        .filter(|l| l.kind == "text")
        .map(|l| l.id)
        .collect();
    assert_eq!(texts.len(), 2);
    api.set_text_style(
        texts[0],
        TextStyle {
            size: Some(77.0),
            ..TextStyle::default()
        },
    )
    .unwrap();
    assert_eq!(api.restyle_captions(texts[0]).unwrap(), 1);
    assert_eq!(api.text(texts[1]).unwrap().size, 77.0);
    assert_eq!(api.text(texts[1]).unwrap().content, "Second");
    // alike now: nothing changes
    assert_eq!(api.restyle_captions(texts[0]).unwrap(), 0);
    // one undo step
    assert!(api.undo().unwrap());
    assert_ne!(api.text(texts[1]).unwrap().size, 77.0);
    // not a text layer, and not a caption
    assert_eq!(
        api.restyle_captions(made.layer).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(api.restyle_captions(999).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn caption_calls_name_what_went_wrong() {
    let (mut ed, text) = titled();
    let mut api = Api::new(&mut ed);
    assert_eq!(
        api.import_captions("not captions", "x").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.import_captions(CAPTIONS, &"n".repeat(300)).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.export_captions(text, "srt").unwrap_err().code,
        ErrorCode::InvalidArgument,
        "a text layer is not captions"
    );
    assert_eq!(
        api.export_captions(text, "ass").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.export_captions(9999, "srt").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(api.layers().len(), 1, "nothing was added");
}

fn fresh() -> Editor {
    Editor::new(Document::new("t", 1280.0, 720.0, 24.0))
}

#[test]
fn layers_are_added_nested_deleted_and_each_is_one_undo_step() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let group = api.add_layer("group", "Scene", None).unwrap();
    let inner = api.add_layer("vector", "Ball", Some(group)).unwrap();
    let switch = api.add_layer("switch", "Mouths", Some(group)).unwrap();
    let all = api.layers();
    assert_eq!(
        all.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["Scene", "Ball", "Mouths"]
    );
    assert_eq!(api.layer(inner).unwrap().parent, Some(group));
    assert_eq!(api.layer(group).unwrap().children, 2);
    // not into a layer that cannot hold layers, a missing one, or as a kind that does not exist
    assert_eq!(
        api.add_layer("vector", "x", Some(inner)).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.add_layer("vector", "x", Some(999)).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.add_layer("bone", "x", None).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.add_layer("vector", "  ", None).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    api.delete_layer(group).unwrap();
    assert!(api.layers().is_empty(), "the layers inside went with it");
    assert_eq!(api.delete_layer(group).unwrap_err().code, ErrorCode::NotFound);
    assert!(api.undo().unwrap());
    assert_eq!(api.layers().len(), 3);
    let _ = switch;
}

#[test]
fn project_settings_change_together_or_not_at_all() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    api.set_document(fanimate_api::DocumentSettings {
        name: Some("Film".into()),
        width: Some(1920.0),
        height: Some(1080.0),
        fps: Some(30.0),
        end: Some(300),
        ..Default::default()
    })
    .unwrap();
    let doc = api.document();
    assert_eq!(
        (
            doc.name.as_str(),
            doc.width,
            doc.height,
            doc.fps,
            doc.start,
            doc.end
        ),
        ("Film", 1920.0, 1080.0, 30.0, 1, 300)
    );
    let bad = [
        fanimate_api::DocumentSettings {
            width: Some(0.0),
            ..Default::default()
        },
        fanimate_api::DocumentSettings {
            width: Some(f32::NAN),
            ..Default::default()
        },
        fanimate_api::DocumentSettings {
            height: Some(1.0e9),
            ..Default::default()
        },
        fanimate_api::DocumentSettings {
            fps: Some(500.0),
            ..Default::default()
        },
        fanimate_api::DocumentSettings {
            start: Some(0),
            ..Default::default()
        },
        fanimate_api::DocumentSettings {
            start: Some(50),
            end: Some(10),
            ..Default::default()
        },
        fanimate_api::DocumentSettings {
            end: Some(i32::MAX),
            ..Default::default()
        },
        fanimate_api::DocumentSettings {
            name: Some(String::new()),
            ..Default::default()
        },
    ];
    for settings in bad {
        assert_eq!(
            api.set_document(settings.clone()).unwrap_err().code,
            ErrorCode::InvalidArgument,
            "{settings:?}"
        );
    }
    assert_eq!(api.document().width, 1920.0, "a refused call changes nothing");
    assert!(api.undo().unwrap());
    assert_eq!(api.document().name, "t");
}

#[test]
fn a_transform_follows_the_keying_rule_and_takes_degrees() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_layer("vector", "Ball", None).unwrap();
    let rest = fanimate_api::TransformChange {
        x: Some(100.0),
        y: Some(50.0),
        ..Default::default()
    };
    api.set_layer_transform(layer, rest).unwrap();
    let later = fanimate_api::TransformChange {
        frame: Some(25),
        x: Some(500.0),
        rotation: Some(90.0),
        opacity: Some(0.5),
        ..Default::default()
    };
    api.set_layer_transform(layer, later).unwrap();
    let t = &ed.doc.find(fanimate_core::LayerId(layer)).unwrap().transform;
    assert_eq!(
        t.position.base,
        fanimate_core::Vec2::new(100.0, 50.0),
        "frame 0 changes the value itself"
    );
    assert_eq!(
        t.position.keys.len(),
        2,
        "a later frame sets a key and seeds the rest pose"
    );
    assert_eq!(
        t.position.sample(25.0),
        fanimate_core::Vec2::new(500.0, 50.0),
        "y was left alone"
    );
    assert!((t.rotation.sample(25.0) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert_eq!(
        ed.doc
            .find(fanimate_core::LayerId(layer))
            .unwrap()
            .opacity
            .sample(25.0),
        0.5
    );
}

#[test]
fn a_transform_that_is_not_usable_is_refused() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_layer("vector", "Ball", None).unwrap();
    let nan = fanimate_api::TransformChange {
        x: Some(f32::NAN),
        ..Default::default()
    };
    let negative = fanimate_api::TransformChange {
        frame: Some(-1),
        x: Some(1.0),
        ..Default::default()
    };
    let huge = fanimate_api::TransformChange {
        frame: Some(i32::MAX),
        x: Some(1.0),
        ..Default::default()
    };
    for change in [nan, negative, huge] {
        assert_eq!(
            api.set_layer_transform(layer, change).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    let any = fanimate_api::TransformChange {
        x: Some(1.0),
        ..Default::default()
    };
    assert_eq!(
        api.set_layer_transform(999, any).unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn shapes_are_drawn_with_colours_and_the_bad_ones_are_named() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_layer("vector", "Art", None).unwrap();
    let rect = fanimate_api::ShapeSpec {
        kind: "rectangle".into(),
        x: 10.0,
        y: 20.0,
        width: 300.0,
        height: 200.0,
        radius: Some(12.0),
        fill: Some("#ff8800".into()),
        stroke: Some("#000000".into()),
        ..Default::default()
    };
    assert_eq!(api.add_shape(layer, rect.clone()).unwrap(), 0);
    let ellipse = fanimate_api::ShapeSpec {
        kind: "ellipse".into(),
        fill: None,
        stroke: None,
        ..rect.clone()
    };
    assert_eq!(api.add_shape(layer, ellipse).unwrap(), 1);
    let fanimate_core::LayerKind::Vector { shapes } =
        &ed.doc.find(fanimate_core::LayerId(layer)).unwrap().kind
    else {
        panic!("vector");
    };
    assert_eq!(shapes.len(), 2);
    assert_eq!(
        shapes[0].style.fill,
        Some(fanimate_core::Color::from_hex("#ff8800").unwrap())
    );
    assert_eq!(shapes[0].style.stroke.map(|s| s.width), Some(4.0));
    assert_eq!(
        shapes[1].style.fill,
        Some(fanimate_core::Color::BLACK),
        "no colour at all is black"
    );
    let mut api = Api::new(&mut ed);
    let bad = [
        fanimate_api::ShapeSpec {
            fill: Some("orange".into()),
            ..rect.clone()
        },
        fanimate_api::ShapeSpec {
            kind: "star".into(),
            ..rect.clone()
        },
        fanimate_api::ShapeSpec {
            width: 0.0,
            ..rect.clone()
        },
        fanimate_api::ShapeSpec {
            x: f32::INFINITY,
            ..rect.clone()
        },
        fanimate_api::ShapeSpec {
            stroke_width: Some(-1.0),
            ..rect.clone()
        },
        fanimate_api::ShapeSpec {
            height: 1.0e12,
            ..rect.clone()
        },
    ];
    for spec in bad {
        assert_eq!(
            api.add_shape(layer, spec.clone()).unwrap_err().code,
            ErrorCode::InvalidArgument,
            "{spec:?}"
        );
    }
    let text = api.add_text_layer("t", 0.0, 0.0).unwrap();
    assert_eq!(
        api.add_shape(text, rect.clone()).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(api.add_shape(9999, rect).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn text_animator_values_are_set_by_name_in_degrees_and_hex_and_keyed_after_frame_zero() {
    let (mut ed, id) = titled();
    let mut api = Api::new(&mut ed);
    api.add_text_animator(id, "fade_in").unwrap();
    let n = |v: f32| fanimate_api::AnimatorValue::Number(v);
    api.set_text_animator_value(id, 0, "tracking", n(200.0), None)
        .unwrap();
    api.set_text_animator_value(id, 0, "rotation", n(90.0), None)
        .unwrap();
    api.set_text_animator_value(
        id,
        0,
        "stroke_color",
        fanimate_api::AnimatorValue::Colour("#ff0000".into()),
        None,
    )
    .unwrap();
    api.set_text_animator_value(
        id,
        0,
        "anchor",
        fanimate_api::AnimatorValue::Pair([4.0, 5.0]),
        None,
    )
    .unwrap();
    api.set_text_animator_value(id, 0, "tracking", n(500.0), Some(12))
        .unwrap();
    let a = &ed.text(fanimate_core::LayerId(id)).unwrap().animators[0];
    assert_eq!(
        a.tracking.keys.len(),
        2,
        "a later frame is keyed and the rest pose seeded"
    );
    assert_eq!((a.tracking.sample(0.0), a.tracking.sample(12.0)), (200.0, 500.0));
    assert!((a.rotation.base - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert_eq!(
        a.stroke_color.as_ref().unwrap().base,
        fanimate_core::Color::from_hex("#ff0000").unwrap()
    );
    assert_eq!(a.anchor.base, fanimate_core::Vec2::new(4.0, 5.0));
}

#[test]
fn a_text_animator_value_that_does_not_fit_is_named_and_changes_nothing() {
    let (mut ed, id) = titled();
    let mut api = Api::new(&mut ed);
    api.add_text_animator(id, "wave").unwrap();
    let n = |v: f32| fanimate_api::AnimatorValue::Number(v);
    let pair = fanimate_api::AnimatorValue::Pair([1.0, 2.0]);
    let colour = |s: &str| fanimate_api::AnimatorValue::Colour(s.into());
    let cases = [
        ("tracking", pair.clone(), None),
        ("position", n(1.0), None),
        ("fill", n(1.0), None),
        ("fill", colour("blue"), None),
        ("tracking", n(f32::NAN), None),
        (
            "position",
            fanimate_api::AnimatorValue::Pair([f32::INFINITY, 0.0]),
            None,
        ),
        ("tracking", n(1.0), Some(-3)),
        ("tracking", n(1.0), Some(i32::MAX)),
        ("no_such_thing", n(1.0), None),
    ];
    for (property, value, frame) in cases {
        let e = api
            .set_text_animator_value(id, 0, property, value.clone(), frame)
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgument, "{property} {value:?}");
    }
    assert_eq!(
        api.set_text_animator_value(id, 7, "tracking", n(1.0), None)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.set_text_animator_value(9999, 0, "tracking", n(1.0), None)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let a = &ed.text(fanimate_core::LayerId(id)).unwrap().animators[0];
    assert_eq!(a.tracking.base, 0.0);
}

/// A three bone tail on a bone layer, the root swinging, built through the API.
fn tail(api: &mut Api) -> (u32, Vec<u32>) {
    let layer = api.add_bone_layer("Tail").unwrap();
    let root = api.add_bone(layer, "root", None, 0.0, 0.0, 60.0).unwrap();
    let mid = api.add_bone(layer, "mid", Some(root), 60.0, 0.0, 60.0).unwrap();
    let tip = api.add_bone(layer, "tip", Some(mid), 60.0, 0.0, 60.0).unwrap();
    api.set_bone_angle(layer, root, 0.0, Some(1)).unwrap();
    api.set_bone_angle(layer, root, 70.0, Some(10)).unwrap();
    api.set_bone_angle(layer, root, 70.0, Some(40)).unwrap();
    (layer, vec![root, mid, tip])
}

#[test]
fn a_rig_is_built_posed_and_given_secondary_motion_through_the_api() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let (layer, bones) = tail(&mut api);
    let listed = api.bones(layer).unwrap();
    assert_eq!(
        listed.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
        ["root", "mid", "tip"]
    );
    assert_eq!(listed[2].parent, Some(bones[1]));
    let keys = api
        .bake_spring(
            layer,
            &bones[1..],
            fanimate_api::SpringSettings::default(),
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        keys,
        2 * 96,
        "a key a frame on two bones over the default 96 frames"
    );
    let swing = ed.skeleton(fanimate_core::LayerId(layer)).unwrap().bones[1]
        .angle
        .keys
        .iter()
        .map(|k| k.value.abs())
        .fold(0.0f32, f32::max);
    assert!(swing > 0.01, "the middle bone swings: {swing}");
    // one undo step takes the baking back
    assert!(Api::new(&mut ed).undo().unwrap());
    assert!(
        ed.skeleton(fanimate_core::LayerId(layer)).unwrap().bones[1]
            .angle
            .keys
            .is_empty()
    );
}

#[test]
fn rig_calls_that_cannot_be_done_are_named_and_change_nothing() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let (layer, bones) = tail(&mut api);
    let art = api.add_layer("vector", "Art", None).unwrap();
    assert_eq!(
        api.add_bone(art, "x", None, 0.0, 0.0, 1.0).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.add_bone(layer, "x", Some(999), 0.0, 0.0, 1.0)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    for (x, length) in [(f32::NAN, 1.0), (0.0, -5.0), (0.0, f32::INFINITY), (1.0e9, 1.0)] {
        assert_eq!(
            api.add_bone(layer, "x", None, x, 0.0, length).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    assert_eq!(
        api.add_bone(layer, " ", None, 0.0, 0.0, 1.0).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.set_bone_angle(layer, 999, 10.0, None).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.set_bone_angle(layer, bones[0], f32::NAN, None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.set_bone_angle(layer, bones[0], 1.0, Some(-1))
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    let default = fanimate_api::SpringSettings::default();
    assert_eq!(
        api.bake_spring(art, &bones, default.clone(), None, None)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.bake_spring(layer, &[], default.clone(), None, None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.bake_spring(layer, &[777], default.clone(), None, None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.bake_spring(layer, &bones, default.clone(), Some(50), Some(10))
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    let wild = fanimate_api::SpringSettings {
        stiffness: Some(1.0e9),
        ..Default::default()
    };
    assert_eq!(
        api.bake_spring(layer, &bones, wild, None, None).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    let nan = fanimate_api::SpringSettings {
        damping: Some(f32::NAN),
        ..Default::default()
    };
    assert_eq!(
        api.bake_spring(layer, &bones, nan, None, None).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert!(
        ed.skeleton(fanimate_core::LayerId(layer)).unwrap().bones[1]
            .angle
            .keys
            .is_empty()
    );
}

#[test]
fn a_pose_is_saved_mirrored_and_put_back_through_the_api() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_bone_layer("Body").unwrap();
    let spine = api.add_bone(layer, "spine", None, 0.0, 0.0, 80.0).unwrap();
    let left = api.add_bone(layer, "arm L", Some(spine), 0.0, 0.0, 60.0).unwrap();
    let right = api.add_bone(layer, "arm R", Some(spine), 0.0, 0.0, 60.0).unwrap();
    api.set_bone_angle(layer, left, 70.0, Some(10)).unwrap();
    let saved = api.save_pose(layer, Some("Reach L"), Some(10)).unwrap();
    let mirrored = api.mirror_pose(layer, saved).unwrap();
    let listed = api.actions(layer).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|a| (a.name.as_str(), a.kind))
            .collect::<Vec<_>>(),
        [("Reach L", "pose"), ("Reach R", "pose")]
    );
    api.insert_action(layer, mirrored, 30).unwrap();
    let bones = ed.skeleton(fanimate_core::LayerId(layer)).unwrap();
    let right_index = bones.index_of(right).unwrap();
    assert!((bones.bones[right_index].angle.sample(30.0) + 70.0f32.to_radians()).abs() < 1e-4);
    // each call was one undo step
    let mut api = Api::new(&mut ed);
    assert!(api.undo().unwrap());
    assert_eq!(api.actions(layer).unwrap().len(), 2);
}

#[test]
fn pose_calls_that_cannot_be_done_are_named() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_bone_layer("Body").unwrap();
    assert_eq!(
        api.save_pose(layer, None, None).unwrap_err().code,
        ErrorCode::InvalidArgument,
        "no bones yet"
    );
    api.add_bone(layer, "spine", None, 0.0, 0.0, 80.0).unwrap();
    assert_eq!(
        api.save_pose(layer, Some(" "), None).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.save_pose(layer, None, Some(-5)).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    let pose = api.save_pose(layer, None, None).unwrap();
    assert_eq!(api.actions(layer).unwrap()[pose].name, "Pose 1");
    assert_eq!(
        api.mirror_pose(layer, 7).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.insert_action(layer, 7, 5).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        api.insert_action(layer, pose, -1).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.save_pose(9999, None, None).unwrap_err().code,
        ErrorCode::NotFound
    );
    let art = api.add_layer("vector", "Art", None).unwrap();
    assert_eq!(api.actions(art).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn svg_text_is_imported_as_text_layers_through_the_api() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="200">
        <rect width="600" height="200" fill="#eeeeee"/>
        <text x="40" y="120" font-size="64" font-weight="bold" fill="#cc0000">Imported</text></svg>"##;
    let id = api.import_svg(svg, "Poster").unwrap();
    let layers = api.layers();
    assert_eq!(
        layers.iter().map(|l| l.kind).collect::<Vec<_>>(),
        ["group", "vector", "text"]
    );
    assert_eq!(api.layer(id).unwrap().name, "Poster");
    let text_id = layers.iter().find(|l| l.kind == "text").unwrap().id;
    let info = api.text(text_id).unwrap();
    assert_eq!(
        (info.content.as_str(), info.size, info.weight),
        ("Imported", 64.0, 700.0)
    );
    assert_eq!(
        api.import_svg("<svg", "Broken").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.import_svg(svg, " ").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert!(api.undo().unwrap());
    assert!(api.layers().is_empty());
}

#[test]
fn gravity_and_wind_reach_the_bake_through_the_api_and_are_range_checked() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_bone_layer("Tail").unwrap();
    let bone = api.add_bone(layer, "tail", None, 0.0, 0.0, 80.0).unwrap();
    let settings = fanimate_api::SpringSettings {
        stiffness: Some(100.0),
        damping: Some(12.0),
        inertia: Some(0.0),
        gravity: Some(30.0),
        ..Default::default()
    };
    api.bake_spring(layer, &[bone], settings, Some(1), Some(200))
        .unwrap();
    let sag = ed.skeleton(fanimate_core::LayerId(layer)).unwrap().bones[0]
        .angle
        .sample(200.0);
    assert!(
        sag > 0.2 && sag < 0.32,
        "a sideways bone sags under gravity: {sag}"
    );
    let mut api = Api::new(&mut ed);
    for bad in [
        fanimate_api::SpringSettings {
            gravity: Some(-1.0),
            ..Default::default()
        },
        fanimate_api::SpringSettings {
            wind: Some(1.0e6),
            ..Default::default()
        },
        fanimate_api::SpringSettings {
            wind: Some(f32::NAN),
            ..Default::default()
        },
    ] {
        assert_eq!(
            api.bake_spring(layer, &[bone], bad, None, None).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
}

/// A rig with a jaw and an "open" action that turns it a quarter turn, built through the API.
fn mouth_rig(api: &mut Api) -> (u32, u32) {
    let layer = api.add_bone_layer("Face").unwrap();
    let jaw = api.add_bone(layer, "jaw", None, 0.0, 0.0, 40.0).unwrap();
    api.set_bone_angle(layer, jaw, 0.0, Some(1)).unwrap();
    api.set_bone_angle(layer, jaw, 90.0, Some(10)).unwrap();
    let open = api.save_pose(layer, Some("open"), Some(10)).unwrap();
    (layer, open as u32)
}

#[test]
fn a_slider_made_through_the_api_plays_an_action_and_is_keyed_by_the_keying_rule() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let (layer, open) = mouth_rig(&mut api);
    let slider = api.add_controller(layer, "Mouth", "slider").unwrap();
    api.link_controller(layer, slider, open as usize, "x", 0.0, 1.0)
        .unwrap();
    api.set_controller_value(layer, slider, "x", 0.5, None).unwrap();
    api.set_controller_value(layer, slider, "x", 1.0, Some(24))
        .unwrap();
    let listed = api.controllers(layer, Some(12)).unwrap();
    assert_eq!(
        (listed[0].name.as_str(), listed[0].kind, listed[0].links.len()),
        ("Mouth", "slider", 1)
    );
    assert_eq!(
        api.controllers(layer, Some(0)).unwrap()[0].x,
        0.5,
        "the rest pose holds the first value"
    );
    assert_eq!(api.controllers(layer, Some(24)).unwrap()[0].x, 1.0);
    assert!(api.unlink_controller(layer, slider, open as usize).unwrap());
    assert!(!api.unlink_controller(layer, slider, open as usize).unwrap());
    api.remove_controller(layer, slider).unwrap();
    assert!(api.controllers(layer, None).unwrap().is_empty());
    assert!(api.undo().unwrap());
    assert_eq!(api.controllers(layer, None).unwrap().len(), 1);
}

#[test]
fn controller_calls_that_cannot_be_done_are_named() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let (layer, open) = mouth_rig(&mut api);
    assert_eq!(
        api.add_controller(layer, "x", "dial").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    let slider = api.add_controller(layer, "S", "slider").unwrap();
    api.rename_controller(layer, slider, "  Smile ").unwrap();
    assert_eq!(api.controllers(layer, None).unwrap()[slider].name, "Smile");
    for bad in ["", "   ", &"n".repeat(300)] {
        assert_eq!(
            api.rename_controller(layer, slider, bad).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    assert_eq!(
        api.rename_controller(layer, 9, "x").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert!(api.undo().unwrap());
    assert_eq!(api.controllers(layer, None).unwrap()[slider].name, "S");
    for (axis, from, to) in [
        ("z", 0.0, 1.0),
        ("y", 0.0, 1.0),
        ("x", 0.5, 0.5),
        ("x", f32::NAN, 1.0),
    ] {
        assert_eq!(
            api.link_controller(layer, slider, open as usize, axis, from, to)
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument,
            "{axis} {from} {to}"
        );
    }
    assert_eq!(
        api.link_controller(layer, slider, 99, "x", 0.0, 1.0)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.set_controller_value(layer, slider, "x", f32::NAN, None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.set_controller_value(layer, slider, "x", 0.5, Some(-1))
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.set_controller_value(layer, 9, "x", 0.5, None)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    let art = api.add_layer("vector", "Art", None).unwrap();
    assert_eq!(api.controllers(art, None).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        api.add_controller(9999, "x", "slider").unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn curves_are_read_and_a_key_is_moved_and_reshaped_through_the_api() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_layer("vector", "Ball", None).unwrap();
    for (frame, x, rotation) in [(1, 0.0, 0.0), (12, 100.0, 90.0), (24, 0.0, 0.0)] {
        let change = fanimate_api::TransformChange {
            frame: Some(frame),
            x: Some(x),
            rotation: Some(rotation),
            ..Default::default()
        };
        api.set_layer_transform(layer, change).unwrap();
    }
    let curves = api.curves(layer).unwrap();
    let names: Vec<(&str, usize)> = curves.iter().map(|c| (c.channel.as_str(), c.component)).collect();
    // x and y of the position, and the rotation, each keyed on the same frames
    assert!(names.contains(&("position", 0)) && names.contains(&("position", 1)));
    assert!(names.contains(&("rotation", 0)));
    let rotation = curves.iter().find(|c| c.channel == "rotation").unwrap();
    let key = rotation.keys.iter().find(|k| k.frame == 12).unwrap();
    assert!(
        (key.value - 90.0).abs() < 1e-3,
        "angles are in degrees: {}",
        key.value
    );
    // move the middle key to frame 15 and 45 degrees
    assert_eq!(api.move_key(layer, "rotation", 0, 12, 15, 45.0).unwrap(), 15);
    let rotation = api
        .curves(layer)
        .unwrap()
        .into_iter()
        .find(|c| c.channel == "rotation")
        .unwrap();
    let moved = rotation.keys.iter().find(|k| k.frame == 15).unwrap();
    assert!((moved.value - 45.0).abs() < 1e-3);
    assert!(rotation.keys.iter().all(|k| k.frame != 12));
    // the key stays between its neighbours
    assert_eq!(api.move_key(layer, "rotation", 0, 15, 99, 45.0).unwrap(), 23);
    // position and rotation are separate properties: the position key is still on frame 12
    let curve = serde_json::json!({"bezier": [250, 0, 750, 1000]});
    assert!(
        api.set_key_interpolation(layer, "position", 12, curve.clone())
            .unwrap()
    );
    let position = api
        .curves(layer)
        .unwrap()
        .into_iter()
        .find(|c| c.channel == "position")
        .unwrap();
    assert_eq!(
        position.keys.iter().find(|k| k.frame == 12).unwrap().interp,
        curve
    );
}

#[test]
fn key_edits_that_cannot_be_made_are_refused_and_undo_brings_a_move_back() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_layer("vector", "Ball", None).unwrap();
    for frame in [1, 10] {
        let change = fanimate_api::TransformChange {
            frame: Some(frame),
            opacity: Some(if frame == 1 { 1.0 } else { 0.2 }),
            ..Default::default()
        };
        api.set_layer_transform(layer, change).unwrap();
    }
    let bad = |r: Result<i32, fanimate_api::ApiError>| r.unwrap_err().code;
    assert_eq!(
        bad(api.move_key(layer, "wobble", 0, 1, 2, 1.0)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        bad(api.move_key(layer, "opacity", 0, 5, 6, 1.0)),
        ErrorCode::InvalidArgument,
        "no key there"
    );
    assert_eq!(
        bad(api.move_key(layer, "opacity", 3, 1, 2, 1.0)),
        ErrorCode::InvalidArgument,
        "no such curve"
    );
    assert_eq!(
        bad(api.move_key(layer, "opacity", 0, 1, 2, f32::NAN)),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        bad(api.move_key(layer, "bone:1", 0, 1, 2, 1.0)),
        ErrorCode::NotFound,
        "not a bone layer"
    );
    assert_eq!(
        bad(api.move_key(9999, "opacity", 0, 1, 2, 1.0)),
        ErrorCode::NotFound
    );
    let junk = api.set_key_interpolation(layer, "opacity", 1, serde_json::json!("wobbly"));
    assert_eq!(junk.unwrap_err().code, ErrorCode::InvalidArgument);
    let outside = api.set_key_interpolation(
        layer,
        "opacity",
        1,
        serde_json::json!({"bezier": [2000, 0, 0, 0]}),
    );
    assert_eq!(outside.unwrap_err().code, ErrorCode::InvalidArgument);
    // opacity is shown as a percentage
    assert_eq!(api.move_key(layer, "opacity", 0, 10, 10, 50.0).unwrap(), 10);
    let opacity = api
        .curves(layer)
        .unwrap()
        .into_iter()
        .find(|c| c.channel == "opacity")
        .unwrap();
    assert!((opacity.keys.iter().find(|k| k.frame == 10).unwrap().value - 50.0).abs() < 1e-3);
    assert!(
        api.set_key_interpolation(layer, "opacity", 1, serde_json::json!("step"))
            .unwrap()
    );
    assert!(
        !api.set_key_interpolation(layer, "opacity", 1, serde_json::json!("step"))
            .unwrap()
    );
    assert!(api.undo().unwrap());
    assert!(api.undo().unwrap());
    let opacity = api
        .curves(layer)
        .unwrap()
        .into_iter()
        .find(|c| c.channel == "opacity")
        .unwrap();
    assert!(
        (opacity.keys.iter().find(|k| k.frame == 10).unwrap().value - 20.0).abs() < 1e-3,
        "undo put the key's value back"
    );
}

#[test]
fn an_action_is_retargeted_to_another_rig_through_the_api() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let tall = api.add_bone_layer("Tall").unwrap();
    let t_body = api.add_bone(tall, "body", None, 0.0, 0.0, 80.0).unwrap();
    let t_arm = api.add_bone(tall, "arm L", Some(t_body), 0.0, 0.0, 50.0).unwrap();
    api.set_bone_angle(tall, t_arm, 60.0, Some(10)).unwrap();
    let small = api.add_bone_layer("Small").unwrap();
    let s_body = api.add_bone(small, "Body", None, 0.0, 0.0, 40.0).unwrap();
    api.add_bone(small, "Arm_L", Some(s_body), 0.0, 0.0, 25.0)
        .unwrap();
    let pose = api.save_pose(tall, Some("Reach"), Some(10)).unwrap();
    assert_eq!(api.retarget_action(tall, pose, small).unwrap(), 0);
    let actions = api.actions(small).unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].name, "Reach");
    for (from, action, to) in [
        (tall, pose, tall),
        (tall, 9, small),
        (tall, pose, 9999),
        (9999, pose, small),
    ] {
        assert!(
            api.retarget_action(from, action, to).is_err(),
            "{from} {action} {to}"
        );
    }
    assert!(api.undo().unwrap());
    assert!(api.actions(small).unwrap().is_empty());
}

#[test]
fn an_expression_is_baked_into_keys_in_one_undo_step_and_a_bad_one_changes_nothing() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let layer = api.add_layer("vector", "Eye", None).unwrap();

    // a blink: opacity drops to 0 for 3 frames in every 24, the same on every run
    let baked = api
        .bake_expression(layer, "opacity", 0, "100 - blink(frame, 7, 24, 3) * 100", 1, 72)
        .unwrap();
    assert_eq!(baked, 72);
    let curves = api.curves(layer).unwrap();
    let opacity = curves.iter().find(|c| c.channel == "opacity").unwrap();
    assert_eq!(opacity.keys.len(), 72);
    assert_eq!(
        opacity.keys.iter().filter(|k| k.value < 50.0).count(),
        9,
        "3 closed frames in each of 3 periods: {:?}",
        opacity.keys.iter().map(|k| k.value).collect::<Vec<_>>()
    );
    // a wobble on a rotation uses degrees, and `value` is the curve it had
    api.bake_expression(layer, "rotation", 0, "value + 10 * sin(frame)", 1, 10)
        .unwrap();
    let curves = api.curves(layer).unwrap();
    let rotation = curves.iter().find(|c| c.channel == "rotation").unwrap();
    let k = rotation.keys.iter().find(|k| k.frame == 3).unwrap();
    assert!(
        (f64::from(k.value) - 10.0 * 3f64.sin()).abs() < 1e-2,
        "{}",
        k.value
    );

    // the whole bake is one undo step
    assert!(api.undo().unwrap());
    assert!(api.curves(layer).unwrap().iter().all(|c| c.channel != "rotation"));
    assert!(api.curves(layer).unwrap().iter().any(|c| c.channel == "opacity"));

    // what is wrong is named, and nothing changes
    let before = api.curves(layer).unwrap().len();
    let bad = |r: Result<u32, fanimate_api::ApiError>| r.unwrap_err();
    let e = bad(api.bake_expression(layer, "opacity", 0, "10 / (frame - 2)", 1, 5));
    assert_eq!(e.code, ErrorCode::InvalidArgument);
    assert!(
        e.message.contains("Frame 2") && e.message.contains("zero"),
        "{}",
        e.message
    );
    assert_eq!(
        bad(api.bake_expression(layer, "opacity", 0, "nope", 1, 5)).code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        bad(api.bake_expression(layer, "wobble", 0, "1", 1, 5)).code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        bad(api.bake_expression(layer, "opacity", 5, "1", 1, 5)).code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        bad(api.bake_expression(layer, "opacity", 0, "1", 9_999_999, 9_999_999)).code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        bad(api.bake_expression(999, "opacity", 0, "1", 1, 5)).code,
        ErrorCode::NotFound
    );
    assert_eq!(api.curves(layer).unwrap().len(), before);
}

/// A file of one-colour PNG layers in an OpenRaster archive.
fn ora_bytes() -> Vec<u8> {
    let png = |w: u32, h: u32| {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    };
    let stack = r#"<image w="32" h="16"><stack>
        <layer name="Top" src="data/top.png" x="2" y="3"/>
        <layer name="Bottom" src="data/bottom.png"/>
    </stack></image>"#;
    let (top, bottom) = (png(8, 8), png(32, 16));
    fanimate_fnm::zip::write(&[
        ("mimetype", b"image/openraster".as_slice()),
        ("stack.xml", stack.as_bytes()),
        ("data/top.png", &top),
        ("data/bottom.png", &bottom),
    ])
    .unwrap()
}

#[test]
fn an_openraster_file_is_imported_as_a_group_of_image_layers_in_one_undo_step() {
    let mut ed = fresh();
    let mut api = Api::new(&mut ed);
    let group = api.import_ora("sketch.ora", &ora_bytes()).unwrap();
    let layers = api.layers();
    let kinds: Vec<(&str, &str)> = layers.iter().map(|l| (l.kind, l.name.as_str())).collect();
    // depth first: the group, then its layers bottom first
    assert_eq!(
        kinds,
        [("group", "sketch"), ("image", "Bottom"), ("image", "Top")]
    );
    assert_eq!(layers[0].id, group);
    assert!(api.undo().unwrap());
    assert!(api.layers().is_empty());
    // not an OpenRaster file, an empty one, and a name that is too long
    assert_eq!(
        api.import_ora("x.ora", b"not a zip").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.import_ora("x.ora", b"").unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        api.import_ora(&"n".repeat(300), &ora_bytes()).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert!(api.layers().is_empty(), "nothing was added");
}
