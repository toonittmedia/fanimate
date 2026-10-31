//! The `fonts` call and text set in an installed font. The font provider is
//! global to the process, so this has a test binary of its own.

use fanimate_api::rpc;
use fanimate_api::{Api, TextStyle};
use fanimate_core::Document;
use fanimate_editor::Editor;
use fanimate_text::fonts::{self, SystemFont, SystemFonts};
use serde_json::{Value, json};

const FILE: &[u8] = include_bytes!("../../../brand/fonts/JetBrainsMono-Regular.ttf");

/// A computer with one installed font, "Stand-in Mono".
struct Computer;

impl SystemFonts for Computer {
    fn families(&self) -> Vec<String> {
        vec!["Stand-in Mono".to_owned()]
    }

    fn load(&self, family: &str, _bold: bool) -> Option<SystemFont> {
        (family == "Stand-in Mono").then(|| SystemFont {
            data: FILE.to_vec(),
            index: 0,
        })
    }
}

#[test]
fn fonts_lists_the_shipped_and_installed_families_and_text_can_use_an_installed_one() {
    let mut ed = Editor::new(Document::new("t", 640.0, 360.0, 24.0));
    let ask = |ed: &mut Editor, request: Value| -> Value {
        let answer = rpc::handle_line(&mut Api::new(ed), &request.to_string()).expect("answered");
        serde_json::from_str(&answer).expect("JSON")
    };

    // before the host installs the computer's fonts, only the shipped ones
    let before = ask(&mut ed, json!({"id": 1, "method": "fonts"}));
    assert_eq!(before["result"]["bundled"][0], "Inter");
    assert_eq!(before["result"]["installed"], json!([]));
    assert!(rpc::METHODS.iter().any(|m| m.name == "fonts" && !m.edits));

    assert!(fonts::install_system_fonts(Box::new(Computer)));
    let after = ask(&mut ed, json!({"id": 2, "method": "fonts"}));
    assert_eq!(after["result"]["installed"], json!(["Stand-in Mono"]));
    assert_eq!(after["result"]["bundled"], before["result"]["bundled"]);

    // a text set in it keeps the name and is laid out in that font
    let mut api = Api::new(&mut ed);
    let layer = api.add_text_layer("Hello", 10.0, 10.0).unwrap();
    api.set_text_style(
        layer,
        TextStyle {
            family: Some("Stand-in Mono".to_owned()),
            ..TextStyle::default()
        },
    )
    .unwrap();
    assert_eq!(api.text(layer).unwrap().family, "Stand-in Mono");
    assert!(
        ed.text(fanimate_core::LayerId(layer))
            .unwrap()
            .look()
            .font_is_available()
    );
}
