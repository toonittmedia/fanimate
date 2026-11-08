//! Hostile projects through the vector, game and interchange exporters: a
//! project with numbers that are not numbers, absurd sizes and rates, ranges
//! that are huge or backwards, and shapes with damaged points must make a
//! file that is valid or fail with a message. It must never panic, hang or
//! write NaN into a file.

use fanimate_core::{Color, Document, LayerKind, Path, Shape, Style, Vec2, samples};
use fanimate_export::{otio, runtime, svg, svg_animated};

fn damaged() -> Vec<(&'static str, Document)> {
    let mut out: Vec<(&'static str, Document)> = Vec::new();
    let base = samples::walk_cycle;

    let mut d = base();
    d.fps = 0.0;
    out.push(("zero frame rate", d));
    let mut d = base();
    d.fps = f32::NAN;
    out.push(("NaN frame rate", d));
    let mut d = base();
    d.fps = -24.0;
    out.push(("negative frame rate", d));
    let mut d = base();
    d.width = 0.0;
    d.height = f32::INFINITY;
    out.push(("zero and infinite size", d));
    let mut d = base();
    d.background = Color::rgba(f32::NAN, 2.0, -1.0, f32::NAN);
    out.push(("background that is not a colour", d));
    let mut d = base();
    d.start = 10;
    d.end = 2;
    out.push(("backwards range", d));
    let mut d = base();
    d.start = i32::MIN / 2;
    d.end = i32::MAX / 2;
    out.push(("huge range", d));
    let mut d = base();
    for layer in &mut d.layers {
        layer.transform.position.base = Vec2::new(f32::NAN, f32::INFINITY);
        layer.transform.scale.base = Vec2::new(f32::MAX, 0.0);
        layer.opacity.base = f32::NAN;
    }
    out.push(("layers with damaged transforms", d));

    let mut d = Document::new("points", 200.0, 100.0, 24.0);
    let mut bad = Shape::new(
        "bad",
        Path::rect(Vec2::ZERO, Vec2::new(10.0, 10.0), 0.0),
        Style::fill(Color::BLACK).with_stroke(Color::BLACK, f32::NAN),
    );
    bad.path.cmds.push(fanimate_core::PathCmd::CubicTo(
        Vec2::new(f32::NAN, 0.0),
        Vec2::new(1.0e30, -1.0e30),
        Vec2::new(f32::INFINITY, 1.0),
    ));
    bad.widths = Some(vec![f32::NAN; 2]);
    bad.hidden_edge_flags = vec![true; 1000];
    bad.trim = Vec2::new(f32::NAN, 3.0);
    let layer = d.new_layer("art", LayerKind::vector(vec![bad]));
    d.layers.push(layer);
    out.push(("shapes with damaged points", d));
    out
}

fn assert_clean(text: &str, what: &str) {
    for bad in ["NaN", "nan", "inf", "Infinity"] {
        assert!(!text.contains(bad), "{what}: the output contains {bad}");
    }
}

#[test]
fn every_exporter_makes_a_valid_file_or_says_why_not() {
    for (what, doc) in damaged() {
        // a range a person could ask for, and the project's own
        for range in [None, Some((1, 3)), Some((5, 1)), Some((1, 4_000_000))] {
            if let Ok((text, _)) = svg_animated::animated(&doc, range) {
                assert_clean(&text, what);
            }
            if let Ok(baked) = runtime::bake(&doc, range) {
                assert_clean(&baked.json.to_string(), what);
            }
            if let Ok(l) = fanimate_export::lottie::lottie(&doc, range) {
                assert_clean(&l.json.to_string(), what);
            }
        }
        for frame in [0.0, 1.0, 1.5, -3.0, f32::NAN, 1.0e9] {
            let (text, _) = svg::frame(&doc, frame);
            assert_clean(&text, what);
        }
        let made = otio::timeline(&doc, &otio::Options::default());
        assert_clean(&made.json.to_string(), what);
    }
}

#[test]
fn a_runtime_file_made_from_a_damaged_project_is_read_back_without_a_panic() {
    for (what, doc) in damaged() {
        let Ok((bytes, _, _)) = runtime::fnmr(&doc, Some((1, 5))) else {
            continue;
        };
        let player = runtime::Player::open(&bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
        for frame in [0.0, 1.0, 2.5, 99.0, f32::NAN] {
            let _ = player.sample(frame);
        }
    }
}
