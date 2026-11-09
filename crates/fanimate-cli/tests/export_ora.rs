//! `fanimate-cli export x.fnm -o x.ora` through the real program: paint on a
//! raster layer, save, export, and read the file back with the importer.

use std::process::Command;

use fanimate_api::Api;
use fanimate_core::Document;
use fanimate_editor::Editor;

fn painted_project(dir: &std::path::Path) -> String {
    let mut ed = Editor::new(Document::new("Painting", 400.0, 300.0, 24.0));
    let mut api = Api::new(&mut ed);
    let layer = api.add_raster_layer("Paint").unwrap();
    api.paint(
        layer,
        vec![[40.0, 40.0], [160.0, 60.0]],
        Some("#e63946".to_owned()),
        Some(16.0),
        None,
        None,
        false,
    )
    .unwrap();
    let path = dir.join("painting.fnm");
    fanimate_fnm::save(&ed.doc, &path).unwrap();
    path.to_string_lossy().into_owned()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fanimate-ora-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_painted_project_exports_to_an_openraster_file_that_reads_back() {
    let dir = scratch("ok");
    let project = painted_project(&dir);
    let out = dir.join("painting.ora");
    let run = Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .args(["export", &project, "-o", &out.to_string_lossy()])
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(run.status.success(), "{said}");
    let bytes = std::fs::read(&out).unwrap();
    let ora = fanimate_import::ora::parse(&bytes).unwrap();
    assert_eq!((ora.width, ora.height, ora.layers), (400, 300, 1));
    let fanimate_import::ora::OraNode::Layer(layer) = &ora.nodes[0] else {
        panic!("a layer is expected");
    };
    assert_eq!(layer.name, "Paint");
    // the stroke is trimmed to what was painted, near where it was drawn
    assert!(layer.width < 400 && layer.x >= 20 && layer.x < 60, "{layer:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_project_with_no_pictures_says_so_and_writes_nothing() {
    let dir = scratch("none");
    let project = dir.join("empty.fnm");
    fanimate_fnm::save(&Document::new("Empty", 100.0, 100.0, 24.0), &project).unwrap();
    let out = dir.join("empty.ora");
    let run = Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .args(["export", &project.to_string_lossy(), "-o", &out.to_string_lossy()])
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("no pictures"));
    assert!(!out.exists());
    std::fs::remove_dir_all(&dir).ok();
}
