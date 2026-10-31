//! The SVG of a frame must look like the frame the canvas draws: it is
//! rasterised with resvg and compared with the GPU render of the same frame.
//! Skips when this computer has no graphics adapter.

use fanimate_core::{Color, Document, LayerKind, Path, Shape, Stroke, Style, Vec2};
use fanimate_render::{Gpu, render_to_rgba};

fn rasterise(svg: &str, w: u32, h: u32) -> Vec<u8> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).expect("the SVG parses");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    pixmap.data().to_vec()
}

fn scene() -> Document {
    let mut doc = Document::new("t", 200.0, 120.0, 24.0);
    doc.background = Color::rgb(0.9, 0.95, 1.0);
    let mut square = Shape::new(
        "sq",
        Path::rect(Vec2::new(20.0, 20.0), Vec2::new(80.0, 60.0), 8.0),
        Style::fill(Color::rgb(0.9, 0.2, 0.1)),
    );
    square.style.stroke = Some(Stroke {
        color: Color::rgb(0.0, 0.0, 0.4),
        width: 6.0,
    });
    let circle = Shape::new(
        "dot",
        Path::ellipse(Vec2::new(140.0, 60.0), Vec2::new(30.0, 30.0)),
        Style::fill(Color::rgb(0.1, 0.6, 0.2)),
    );
    let mut layer = doc.new_layer(
        "art",
        LayerKind::Vector {
            shapes: vec![square, circle],
        },
    );
    layer.transform.position.base = Vec2::new(5.0, 10.0);
    doc.layers.push(layer);
    doc
}

#[test]
fn the_svg_of_a_frame_matches_the_canvas_render() {
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("no graphics adapter; skipped");
        return;
    };
    let doc = scene();
    let (w, h) = (200, 120);
    let expected = render_to_rgba(&gpu, &doc, 1.0, w, h).expect("renders");
    let (svg, report) = fanimate_export::svg::frame(&doc, 1.0);
    assert!(report.is_empty(), "{report:?}");
    let got = rasterise(&svg, w, h);
    // anti-aliasing differs at edges, so compare the mean error, and the interior colours exactly-ish
    let total: u64 = expected
        .iter()
        .zip(&got)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    let mean = total as f64 / expected.len() as f64;
    assert!(mean < 1.5, "mean channel error {mean}");
    let at = |p: &[u8], x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [p[i], p[i + 1], p[i + 2]]
    };
    for (x, y) in [(40, 50), (145, 70), (2, 2)] {
        let (a, b) = (at(&expected, x, y), at(&got, x, y));
        for c in 0..3 {
            assert!(a[c].abs_diff(b[c]) <= 3, "({x},{y}) {a:?} vs {b:?}");
        }
    }
}

#[test]
fn see_through_shapes_are_reported_because_svg_blends_in_srgb() {
    let mut doc = scene();
    if let LayerKind::Vector { shapes } = &mut doc.layers[0].kind {
        shapes[1].style.fill = Some(Color::rgba(0.1, 0.6, 0.2, 0.6));
    }
    let (_, report) = fanimate_export::svg::frame(&doc, 1.0);
    assert_eq!(report.lines().len(), 1);
    assert!(
        report.summary().starts_with("1 translucent shapes"),
        "{}",
        report.summary()
    );
}
