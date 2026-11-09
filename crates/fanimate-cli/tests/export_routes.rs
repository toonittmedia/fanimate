//! The vector, game and interchange exports through the real program:
//! animated SVG, the runtime file and an OpenTimelineIO timeline.

use std::path::Path;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn said(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fanimate-export-routes-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn project(dir: &Path) -> String {
    let file = dir.join("hello.fnm").to_string_lossy().into_owned();
    let out = cli(&["sample", "hello", &file]);
    assert!(out.status.success(), "{}", said(&out));
    file
}

#[test]
fn an_animated_svg_is_written_with_smil_steps() {
    let dir = temp("svg");
    let file = project(&dir);
    let svg = dir.join("hello.svg").to_string_lossy().into_owned();
    let out = cli(&["export", &file, "-o", &svg, "--start", "1", "--end", "12"]);
    assert!(out.status.success(), "{}", said(&out));
    let text = std::fs::read_to_string(&svg).unwrap();
    assert!(text.starts_with("<svg ") && text.trim_end().ends_with("</svg>"));
    assert!(
        text.contains("<animate attributeName="),
        "something in the sample moves"
    );
    assert!(text.contains("calcMode=\"discrete\""));
    // a reversed range is refused with a message
    let bad = cli(&["export", &file, "-o", &svg, "--start", "9", "--end", "2"]);
    assert!(!bad.status.success());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_runtime_file_is_a_zip_with_a_runtime_json_that_names_its_format() {
    let dir = temp("fnmr");
    let file = project(&dir);
    let out_file = dir.join("hello.fnmr").to_string_lossy().into_owned();
    let out = cli(&["export", &file, "-o", &out_file]);
    assert!(out.status.success(), "{}", said(&out));
    let bytes = std::fs::read(&out_file).unwrap();
    assert!(fanimate_fnm::zip::is_zip(&bytes));
    let zip = fanimate_fnm::zip::Archive::read(&bytes).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&zip.get_checked("runtime.json").unwrap().unwrap()).unwrap();
    assert_eq!(json["format"], "fanimate-runtime");
    assert_eq!(json["version"], 1);
    assert!(!json["slots"].as_array().unwrap().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_timeline_names_the_movie_it_is_given_and_lists_what_it_does_not_carry() {
    let dir = temp("otio");
    let file = project(&dir);
    let out_file = dir.join("hello.otio").to_string_lossy().into_owned();
    let out = cli(&["export", &file, "-o", &out_file, "--video", "hello.mp4"]);
    assert!(out.status.success(), "{}", said(&out));
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out_file).unwrap()).unwrap();
    assert_eq!(json["OTIO_SCHEMA"], "Timeline.1");
    let clip = &json["tracks"]["children"][0]["children"][0];
    assert_eq!(clip["media_reference"]["target_url"], "hello.mp4");
    assert!(said(&out).contains("not carried"), "{}", said(&out));
    // without --video the clip's media is missing, to be linked in the editing program
    let bare = dir.join("bare.otio").to_string_lossy().into_owned();
    assert!(cli(&["export", &file, "-o", &bare]).status.success());
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&bare).unwrap()).unwrap();
    assert_eq!(
        json["tracks"]["children"][0]["children"][0]["media_reference"]["OTIO_SCHEMA"],
        "MissingReference.1"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_estimate_says_how_big_and_writes_nothing_and_a_tolerance_makes_the_runtime_smaller() {
    let dir = temp("estimate");
    let file = project(&dir);
    let out_file = dir.join("hello.fnmr").to_string_lossy().into_owned();
    let out = cli(&["export", &file, "-o", &out_file, "--estimate"]);
    assert!(out.status.success(), "{}", said(&out));
    assert!(
        said(&out).contains("would write") && said(&out).contains("nothing written"),
        "{}",
        said(&out)
    );
    assert!(
        !std::path::Path::new(&out_file).exists(),
        "an estimate writes no file"
    );
    let lottie = dir.join("hello.json").to_string_lossy().into_owned();
    let out = cli(&["export", &file, "-o", &lottie, "--estimate"]);
    assert!(
        said(&out).contains("keys") && !std::path::Path::new(&lottie).exists(),
        "{}",
        said(&out)
    );
    // a tolerance never makes the file larger
    let exact = cli(&["export", &file, "-o", &out_file]);
    assert!(exact.status.success(), "{}", said(&exact));
    let exact_size = std::fs::metadata(&out_file).unwrap().len();
    let loose = cli(&["export", &file, "-o", &out_file, "--tolerance", "5"]);
    assert!(loose.status.success(), "{}", said(&loose));
    assert!(std::fs::metadata(&out_file).unwrap().len() <= exact_size);
    // a tolerance that is not a number is refused
    assert!(
        !cli(&["export", &file, "-o", &out_file, "--tolerance", "lots"])
            .status
            .success()
    );
    std::fs::remove_dir_all(&dir).ok();
}
