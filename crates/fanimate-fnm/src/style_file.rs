//! Style files (`.fnstyle`): one saved style as a small JSON file, so brush looks can be shared
//! between people and projects.
//!
//! The file says what it is (`kind`) and which layout it uses (`version`) beside the style, so a
//! file of another kind, or from a later Fanimate, is refused by name instead of misread. What a
//! style holds is the same as in a project's `styles` list (see `docs/spec/fnm.md`).

use fanimate_core::StylePreset;

/// The longest style file read, in bytes: a style is a few hundred.
pub const MAX_STYLE_FILE: usize = 64 * 1024;

/// What a style file says about itself.
const KIND: &str = "fanimate-style";

/// The text of a style file for `style`.
pub fn to_text(style: &StylePreset) -> String {
    let value = serde_json::json!({ "kind": KIND, "version": 1, "style": style });
    serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
}

/// Read a style file. Anything that is not one, is too large, names no style or has numbers a
/// style cannot hold is an error that says which, never a panic. The name is tidied.
pub fn from_text(text: &str) -> Result<StylePreset, String> {
    if text.len() > MAX_STYLE_FILE {
        return Err("that file is too large to be a style".to_owned());
    }
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "that is not a Fanimate style file".to_owned())?;
    if value.get("kind").and_then(serde_json::Value::as_str) != Some(KIND) {
        return Err("that is not a Fanimate style file".to_owned());
    }
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(1) => {}
        Some(v) => {
            return Err(format!(
                "that style file is version {v}; this Fanimate reads version 1"
            ));
        }
        None => return Err("that style file has no version".to_owned()),
    }
    let style = value
        .get("style")
        .ok_or_else(|| "that style file holds no style".to_owned())?;
    let mut preset: StylePreset = serde_json::from_value(style.clone())
        .map_err(|e| format!("the style in that file is damaged: {e}"))?;
    preset.name = StylePreset::tidy_name(&preset.name);
    if preset.name.is_empty() {
        return Err("the style in that file has no name".to_owned());
    }
    if !preset.is_sound() {
        return Err("the style in that file has numbers a style cannot hold".to_owned());
    }
    Ok(preset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::Color;

    fn preset() -> StylePreset {
        StylePreset {
            name: "Ink".into(),
            line: Color::BLACK,
            width: 6.0,
            fill: None,
            outline: true,
            smoothing: 0.5,
            taper: 20.0,
            pressure: true,
            hardness: Some(0.2),
            flow: Some(0.4),
            spacing: None,
            flow_pressure: None,
        }
    }

    #[test]
    fn a_style_file_round_trips_and_every_kind_of_bad_file_is_refused_by_name() {
        let style = preset();
        let text = to_text(&style);
        assert_eq!(from_text(&text).unwrap(), style);
        // a name with spaces around it is tidied
        let spaced = text.replace("\"Ink\"", "\"  Ink  \"");
        assert_eq!(from_text(&spaced).unwrap().name, "Ink");
        for (bad, expect) in [
            ("", "not a Fanimate style file"),
            ("{", "not a Fanimate style file"),
            ("[1,2]", "not a Fanimate style file"),
            (r#"{"kind":"other","version":1}"#, "not a Fanimate style file"),
            (r#"{"kind":"fanimate-style"}"#, "no version"),
            (r#"{"kind":"fanimate-style","version":9,"style":{}}"#, "version 9"),
            (r#"{"kind":"fanimate-style","version":1}"#, "holds no style"),
            (
                r#"{"kind":"fanimate-style","version":1,"style":{"name":"x"}}"#,
                "damaged",
            ),
        ] {
            let e = from_text(bad).unwrap_err();
            assert!(e.contains(expect), "{bad}: {e}");
        }
        // no name, and numbers a style cannot hold
        let nameless = text.replace("\"Ink\"", "\"   \"");
        assert!(from_text(&nameless).unwrap_err().contains("no name"));
        let wide = text.replace("\"hardness\": 0.2", "\"hardness\": 7.0");
        assert!(from_text(&wide).unwrap_err().contains("cannot hold"));
        // far too large
        let huge = " ".repeat(MAX_STYLE_FILE + 1);
        assert!(from_text(&huge).unwrap_err().contains("too large"));
    }
}
