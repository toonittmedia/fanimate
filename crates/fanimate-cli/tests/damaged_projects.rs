//! Damaged projects never crash Fanimate (Gate 1: "fuzzing the `.fnm`
//! format never produces a crash; the validator repairs or reports every
//! case"). Real projects are saved, their files damaged in thousands of
//! seeded ways, and every damaged copy is opened as the app opens one:
//! loaded, repaired, evaluated and tessellated, then given a new layer.
//! Most damage is valid JSON with values no project should hold (huge,
//! negative or tiny numbers), since that is what gets past the parser.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use fanimate_core::anim::Key;
use fanimate_core::frames::Cel;
use fanimate_core::{Document, LayerKind, Shape, evaluate, samples};
use fanimate_render::{Mesh, Tessellator};

type Files = BTreeMap<String, Vec<u8>>;

/// Values that are valid JSON but wrong for any field.
const BAD_NUMBERS: [&str; 16] = [
    "-1",
    "0",
    "-0",
    "0.5",
    "1e-40",
    "1e38",
    "-1e38",
    "1e39",
    "65536",
    "1000000000",
    "2147483647",
    "-2147483649",
    "4294967295",
    "4294967296",
    "18446744073709551615",
    "-9223372036854775809",
];

/// xorshift64*: the same damage on every run, so a failure can be replayed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Where each JSON number in `text` starts and ends.
fn numbers(text: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    while i < text.len() {
        let c = text[i];
        if in_string {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            i += 1;
        } else if c == b'-' || c.is_ascii_digit() {
            let start = i;
            while i < text.len() && matches!(text[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                i += 1;
            }
            found.push((start, i));
        } else {
            i += 1;
        }
    }
    found
}

/// One kind of damage to `text`, and what it was.
fn damage(rng: &mut Rng, text: &mut Vec<u8>) -> String {
    if text.is_empty() {
        return "empty".into();
    }
    match rng.below(8) {
        0 => {
            let at = rng.below(text.len());
            text[at] ^= 1 << rng.below(8);
            format!("flip byte {at}")
        }
        1 => {
            let at = rng.below(text.len());
            text.truncate(at);
            format!("truncate at {at}")
        }
        2 => {
            let at = rng.below(text.len());
            let len = 1 + rng.below(64);
            text.drain(at..(at + len).min(text.len()));
            format!("delete {len} at {at}")
        }
        3 => {
            let at = rng.below(text.len());
            let len = 1 + rng.below(64);
            let span: Vec<u8> = text[at..(at + len).min(text.len())].to_vec();
            let to = rng.below(text.len());
            text.splice(to..to, span);
            format!("copy {len} from {at} to {to}")
        }
        _ => {
            let spots = numbers(text);
            if spots.is_empty() {
                return "no numbers".into();
            }
            let (start, end) = spots[rng.below(spots.len())];
            let value = BAD_NUMBERS[rng.below(BAD_NUMBERS.len())];
            text.splice(start..end, value.bytes());
            format!("number at {start} = {value}")
        }
    }
}

/// Every file of a project folder, by its path inside the folder.
fn read_folder(dir: &Path) -> Files {
    fn walk(root: &Path, dir: &Path, files: &mut Files) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut files = Files::new();
    walk(dir, dir, &mut files);
    files
}

/// A project with every kind of layer the format holds that needs no
/// media: vector, group, bones, frame-by-frame and switch.
fn every_layer() -> Document {
    let mut doc = samples::hello();
    let mut shapes: Vec<Shape> = Vec::new();
    doc.walk(|l| {
        if let LayerKind::Vector { shapes: s } = &l.kind
            && shapes.is_empty()
        {
            shapes = s.clone();
        }
    });
    let mut cels = Vec::new();
    for (i, frame) in [0, 6, 12].into_iter().enumerate() {
        let mut cel = Cel::new(frame);
        cel.line = shapes.iter().take(i + 1).cloned().collect();
        cels.push(cel);
    }
    let frames = doc.new_layer("Drawn", LayerKind::Frames { cels });
    let open = doc.new_layer("Open", LayerKind::vector(shapes.clone()));
    let closed = doc.new_layer("Closed", LayerKind::vector(shapes));
    let switch = doc.new_layer(
        "Mouth",
        LayerKind::Switch {
            children: vec![open, closed],
            switch: vec![
                Key {
                    frame: 0,
                    value: "Open".into(),
                    interp: Default::default(),
                },
                Key {
                    frame: 10,
                    value: "Closed".into(),
                    interp: Default::default(),
                },
            ],
        },
    );
    let group = doc.new_layer(
        "Head",
        LayerKind::Group {
            children: vec![frames, switch],
        },
    );
    doc.layers.push(group);
    doc
}

/// Open a damaged project as the app would. Errors are fine; panics are not.
fn open(files: &Files, scratch: &Path, round_trip: bool) {
    let read = |rel: &str| {
        files
            .get(rel)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    };
    let Ok((doc, _report)) = fanimate_fnm::load_from("damaged.fnm", &read) else {
        return;
    };
    exercise(doc, scratch, round_trip);
}

/// What the app does with a project that opened: draw it, change it, save it.
fn exercise(mut doc: Document, scratch: &Path, round_trip: bool) {
    let mut tess = Tessellator::new();
    let mut mesh = Mesh::new();
    let (start, end) = (doc.start as f32, doc.end as f32);
    for frame in [start, end, (start + end) / 2.0, start + 0.5, 0.0, 7.25] {
        let items = evaluate(&doc, frame);
        tess.clear(&mut mesh);
        tess.canvas(&doc, &mut mesh);
        tess.draw_items(&items, &mut mesh);
    }
    // what the editor does next: add a layer, and save
    let layer = doc.new_layer("New", LayerKind::vector(Vec::new()));
    doc.layers.push(layer);
    if round_trip {
        let path = scratch.join("repaired.fnm");
        fanimate_fnm::save(&doc, &path).expect("a repaired project saves");
        fanimate_fnm::load(&path).expect("and opens again");
    }
}

#[test]
fn damaged_projects_never_crash() {
    let scratch = std::env::temp_dir().join(format!("fanimate-damaged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fanimate-fnm/tests/fixtures");
    let mut projects: Vec<(String, Files)> = Vec::new();
    for (name, doc) in [
        ("hello", samples::hello()),
        ("benchmark", samples::benchmark(12)),
        ("every-layer", every_layer()),
    ] {
        let dir = scratch.join(format!("{name}.fnm"));
        fanimate_fnm::save_folder(&doc, &dir).unwrap();
        projects.push((name.into(), read_folder(&dir)));
    }
    for name in ["hello-v0.fnm", "strokes-v1.fnm"] {
        projects.push((name.into(), read_folder(&fixtures.join(name))));
    }
    // the same project as version 5 wrote it: the loader shifts its frames up
    // by one, which must hold for any frame a damaged file can name
    let mut old = projects[2].1.clone();
    old.insert(
        "manifest.json".into(),
        br#"{"format":"fnm","version":5}"#.to_vec(),
    );
    projects.push(("every-layer-v5".into(), old));

    // a quiet hook while panics are caught, keeping only where they were
    static WHERE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        if let (Ok(mut w), Some(l)) = (WHERE.lock(), info.location()) {
            *w = format!("{}:{}", l.file(), l.line());
        }
    }));
    let mut rng = Rng(0x00f4_a71a_7e5e_ed01);
    let mut failures = Vec::new();
    let trials = 3000;
    for trial in 0..trials {
        let (name, original) = &projects[trial % projects.len()];
        let mut files = original.clone();
        let mut what = Vec::new();
        for _ in 0..1 + rng.below(3) {
            let keys: Vec<String> = files.keys().cloned().collect();
            let key = &keys[rng.below(keys.len())];
            let text = files.get_mut(key).unwrap();
            what.push(format!("{key}: {}", damage(&mut rng, text)));
        }
        let result = catch_unwind(AssertUnwindSafe(|| open(&files, &scratch, trial % 50 == 0)));
        if let Err(e) = result {
            let message = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            let at = WHERE.lock().map(|w| w.clone()).unwrap_or_default();
            failures.push(format!(
                "trial {trial} ({name}; {}): {message} at {at}",
                what.join("; ")
            ));
        }
    }
    std::panic::set_hook(hook);
    std::fs::remove_dir_all(&scratch).unwrap();
    assert!(
        failures.is_empty(),
        "{} of {trials} damaged projects crashed:\n{}",
        failures.len(),
        failures.iter().take(15).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The same, for the single-file form: a saved project packed into one
/// archive, then cut short, changed byte by byte, or given a damaged table
/// of contents. Opening it is an error or a project that draws and saves;
/// it is never a panic.
#[test]
fn damaged_single_files_never_crash() {
    let scratch = std::env::temp_dir().join(format!("fanimate-damaged-zip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();

    let mut archives: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, doc) in [
        ("hello", samples::hello()),
        ("benchmark", samples::benchmark(12)),
        ("every-layer", every_layer()),
    ] {
        let path = scratch.join(format!("{name}.fnm"));
        fanimate_fnm::save(&doc, &path).unwrap();
        archives.push((name.into(), std::fs::read(&path).unwrap()));
    }
    // with a preview picture in it
    let preview = [b"\x89PNG\r\n\x1a\n".as_slice(), &[7u8; 300]].concat();
    let path = scratch.join("with-preview.fnm");
    fanimate_fnm::save_with_thumbnail(&every_layer(), &path, Some(&preview)).unwrap();
    assert_eq!(fanimate_fnm::read_thumbnail(&path), Some(preview));
    archives.push(("with-preview".into(), std::fs::read(&path).unwrap()));

    static WHERE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        if let (Ok(mut w), Some(l)) = (WHERE.lock(), info.location()) {
            *w = format!("{}:{}", l.file(), l.line());
        }
    }));
    let mut rng = Rng(0x0a2c_41fe_5eed_0002);
    let mut failures = Vec::new();
    let trials = 1500;
    for trial in 0..trials {
        let (name, original) = &archives[trial % archives.len()];
        let mut bytes = original.clone();
        let what = match rng.below(4) {
            0 => {
                let cut = rng.below(bytes.len());
                bytes.truncate(cut);
                format!("cut at {cut}")
            }
            1 => {
                let n = 1 + rng.below(8);
                for _ in 0..n {
                    let at = rng.below(bytes.len());
                    bytes[at] ^= 1 + rng.below(255) as u8;
                }
                format!("{n} bytes changed")
            }
            // the table of contents is at the end: damage there
            2 => {
                let tail = bytes.len().saturating_sub(300);
                let at = tail + rng.below(bytes.len() - tail);
                bytes[at] = rng.below(256) as u8;
                format!("byte {at} near the end set")
            }
            _ => {
                let from = rng.below(bytes.len());
                let len = rng.below(64).min(bytes.len() - from);
                bytes.drain(from..from + len);
                format!("{len} bytes removed at {from}")
            }
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            if let Ok((doc, _report)) = fanimate_fnm::load_archive("damaged.fnm", &bytes) {
                exercise(doc, &scratch, trial % 50 == 0);
            }
            // the preview is read on its own, from the file, as the Home tab does
            if trial % 10 == 0 {
                let file = scratch.join("damaged-preview.fnm");
                std::fs::write(&file, &bytes).unwrap();
                let _ = fanimate_fnm::read_thumbnail(&file);
            }
        }));
        if let Err(e) = result {
            let message = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            let at = WHERE.lock().map(|w| w.clone()).unwrap_or_default();
            failures.push(format!("trial {trial} ({name}; {what}): {message} at {at}"));
        }
    }
    std::panic::set_hook(hook);
    std::fs::remove_dir_all(&scratch).unwrap();
    assert!(
        failures.is_empty(),
        "{} of {trials} damaged single files crashed:\n{}",
        failures.len(),
        failures.iter().take(15).cloned().collect::<Vec<_>>().join("\n")
    );
}
