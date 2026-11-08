//! What a tool call did, in a sentence an animator would say ("Set opacity on
//! Title at frame 12"), for the assistant's transcript. Queries say nothing:
//! only changes are worth a line.

use fanimate_api::rpc;
use serde_json::Value;

/// One change, for the transcript.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub text: String,
    /// The layer it was about, so the transcript can select it.
    pub layer: Option<u32>,
    /// The frame it was about.
    pub frame: Option<i32>,
}

/// The line for a call that worked, or `None` for a query. `name_of` gives a
/// layer's name from its id.
pub fn step(
    method: &str,
    params: &Value,
    result: &Value,
    name_of: &dyn Fn(u32) -> Option<String>,
) -> Option<Step> {
    let known = rpc::METHODS.iter().find(|m| m.name == method)?;
    if !known.edits {
        return None;
    }
    let number = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    let text_param = |key: &str| params.get(key).and_then(Value::as_str);
    let frame = params
        .get("frame")
        .and_then(Value::as_i64)
        .and_then(|f| i32::try_from(f).ok());
    let target = number("layer").or_else(|| number("id"));
    let on = |id: Option<u32>| match id {
        Some(id) => match name_of(id) {
            Some(name) => format!(" on \"{name}\""),
            None => format!(" on layer {id}"),
        },
        None => String::new(),
    };
    let at = |frame: Option<i32>| match frame {
        Some(0) | None => String::new(),
        Some(f) => format!(" at frame {f}"),
    };
    let made = result.as_u64().and_then(|n| u32::try_from(n).ok());
    let short = |s: &str| {
        let line: String = s.lines().next().unwrap_or("").chars().take(30).collect();
        if s.chars().count() > line.chars().count() {
            format!("{line}…")
        } else {
            line
        }
    };
    let (text, layer) = match method {
        "add_layer" => (
            format!("Added layer \"{}\"", text_param("name").unwrap_or("")),
            made,
        ),
        "add_shape" => {
            let kind = params
                .pointer("/shape/kind")
                .and_then(Value::as_str)
                .unwrap_or("shape");
            let article = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
                "an"
            } else {
                "a"
            };
            (format!("Drew {article} {kind}{}", on(target)), target)
        }
        "import_svg" => (
            format!("Imported artwork \"{}\"", text_param("name").unwrap_or("")),
            made,
        ),
        "set_layer_transform" => {
            let mut changed = Vec::new();
            if params.get("x").is_some() || params.get("y").is_some() {
                changed.push("position");
            }
            if params.get("rotation").is_some() {
                changed.push("rotation");
            }
            if params.get("scale_x").is_some() || params.get("scale_y").is_some() {
                changed.push("scale");
            }
            if params.get("opacity").is_some() {
                changed.push("opacity");
            }
            let what = if changed.is_empty() {
                "transform".to_owned()
            } else {
                changed.join(" and ")
            };
            (format!("Set {what}{}{}", on(target), at(frame)), target)
        }
        "set_key_interpolation" => (
            format!("Changed the easing of a key{}{}", on(target), at(frame)),
            target,
        ),
        "move_key" => (format!("Moved a key{}", on(target)), target),
        "set_bone_angle" => (format!("Posed a bone{}{}", on(target), at(frame)), target),
        "add_bone_layer" => (
            format!("Added bone layer \"{}\"", text_param("name").unwrap_or("")),
            made,
        ),
        "add_bone" => (
            format!(
                "Added bone \"{}\"{}",
                text_param("name").unwrap_or(""),
                on(target)
            ),
            target,
        ),
        "add_text_layer" => (
            format!("Added text \"{}\"", short(text_param("content").unwrap_or(""))),
            made,
        ),
        "set_text_content" => (format!("Changed the words{}", on(target)), target),
        "add_text_animator" => (
            format!(
                "Added a {} text animator{}",
                text_param("preset").unwrap_or("ready-made"),
                on(target)
            ),
            target,
        ),
        "rename_layer" => (
            format!("Renamed a layer to \"{}\"", text_param("name").unwrap_or("")),
            target,
        ),
        "delete_layer" => (format!("Deleted layer {}", target.unwrap_or(0)), None),
        "set_document" => ("Changed the project settings".to_owned(), None),
        "bake_spring" => (format!("Baked spring motion{}", on(target)), target),
        "bake_expression" => (format!("Baked an expression into keys{}", on(target)), target),
        "undo" => ("Undid a step".to_owned(), None),
        "redo" => ("Redid a step".to_owned(), None),
        other => (format!("{} ({other})", known.about), target),
    };
    Some(Step {
        text,
        layer,
        frame: frame.filter(|f| *f != 0),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn names(id: u32) -> Option<String> {
        match id {
            3 => Some("Title".into()),
            _ => None,
        }
    }

    fn step_of(method: &str, params: Value, result: Value) -> Option<Step> {
        step(method, &params, &result, &names)
    }

    #[test]
    fn queries_say_nothing_and_unknown_methods_too() {
        assert!(step_of("layers", json!({}), json!([])).is_none());
        assert!(step_of("document", json!({}), json!({})).is_none());
        assert!(step_of("no_such_method", json!({}), json!(null)).is_none());
    }

    #[test]
    fn a_key_is_described_with_the_layer_and_the_frame() {
        let s = step_of(
            "set_layer_transform",
            json!({"id": 3, "frame": 12, "opacity": 1.0, "x": 5}),
            json!(null),
        )
        .unwrap();
        assert_eq!(s.text, "Set position and opacity on \"Title\" at frame 12");
        assert_eq!((s.layer, s.frame), (Some(3), Some(12)));
        // on frame 0 it is the starting value, not a key
        let s = step_of(
            "set_layer_transform",
            json!({"id": 3, "opacity": 0.0}),
            json!(null),
        )
        .unwrap();
        assert_eq!(s.text, "Set opacity on \"Title\"");
        assert_eq!(s.frame, None);
    }

    #[test]
    fn made_things_point_at_what_they_made() {
        let s = step_of("add_layer", json!({"kind": "vector", "name": "Ball"}), json!(9)).unwrap();
        assert_eq!((s.text.as_str(), s.layer), ("Added layer \"Ball\"", Some(9)));
        let s = step_of(
            "add_shape",
            json!({"layer": 9, "shape": {"kind": "ellipse"}}),
            json!(0),
        )
        .unwrap();
        assert_eq!(s.text, "Drew an ellipse on layer 9");
        let s = step_of(
            "add_text_layer",
            json!({"content": "A very long title that goes on and on\nsecond line", "x": 0, "y": 0}),
            json!(4),
        )
        .unwrap();
        assert_eq!(s.text, "Added text \"A very long title that goes on…\"");
    }

    #[test]
    fn hostile_parameters_do_not_panic() {
        for params in [
            json!(null),
            json!([]),
            json!({"id": "x", "frame": 1e30, "layer": -1}),
            json!({"name": 5}),
        ] {
            for method in [
                "add_layer",
                "set_layer_transform",
                "add_shape",
                "rename_layer",
                "delete_layer",
            ] {
                let s = step(method, &params, &json!("odd"), &names);
                assert!(s.is_some(), "{method} {params}");
            }
        }
    }

    #[test]
    fn every_editing_method_has_a_sentence() {
        for m in rpc::METHODS.iter().filter(|m| m.edits) {
            let s = step_of(m.name, json!({}), json!(null)).unwrap_or_else(|| panic!("{}", m.name));
            assert!(!s.text.is_empty() && !s.text.contains('\u{2014}'), "{}", m.name);
        }
    }
}
