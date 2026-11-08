//! The animation as one animated SVG: the same baking as the Lottie export,
//! written as SMIL `<animate>` elements that browsers play on their own,
//! with no script and no player library.
//!
//! Every frame of the range is evaluated like a render (bones, Smart Bones,
//! keys, text, the camera). Each fill and each stroke of each shape is one
//! `<path>` whose changing attributes (the path data, colour, opacity, stroke
//! width, and whether it is shown) are animated in discrete steps, one value
//! per run of frames in which the attribute does not change, so a still
//! shape costs no animation at all. Discrete steps are exact: the file shows
//! what the canvas shows on every frame, at the project's frame rate.
//!
//! The price is size, which grows with how much moves, so the report ends
//! with an estimate for a long, busy animation (use Lottie for those). What
//! SVG cannot say is counted in the [`Report`] as for a single frame, and
//! one more thing: SMIL is played by browsers but not by every editor
//! (Inkscape and Illustrator show the first frame only).

use std::collections::HashMap;
use std::fmt::Write;

use fanimate_core::eval::Evaluator;
use fanimate_core::{Affine2, Color, Document, Path, Vec2, camera_at};

use crate::report::Report;
use crate::svg::{num, paint, path_data};

/// The most frames one animated SVG bakes.
pub const MAX_FRAMES: usize = 5_000;
/// The most bytes it may grow to; a bigger one is refused with a message.
pub const MAX_BYTES: usize = 48 * 1024 * 1024;

const COMPOSITING_NOTE: &str = "masks, clips, blend modes and flattened groups drawn as plain layers";

/// What one track looks like on one frame.
#[derive(Clone, Debug, PartialEq)]
struct Look {
    d: String,
    color: String,
    alpha: String,
    /// A stroke's width.
    width: Option<String>,
}

struct Track {
    stroke: bool,
    round: bool,
    frames: Vec<Option<Look>>,
}

/// Bake `range` (the whole animation when `None`) into an animated SVG.
pub fn animated(doc: &Document, range: Option<(i32, i32)>) -> Result<(String, Report), String> {
    let (start, end) = range.unwrap_or((doc.start, doc.end));
    if end < start {
        return Err("The end frame comes before the start frame.".to_owned());
    }
    let count = (i64::from(end) - i64::from(start) + 1) as usize;
    if count > MAX_FRAMES {
        return Err(format!(
            "An animated SVG holds at most {MAX_FRAMES} frames; this has {count}. Export a shorter range, or use Lottie."
        ));
    }
    let mut report = Report::default();
    let mut tracks: Vec<Track> = Vec::new();
    let mut index: HashMap<(u32, usize, String, bool), usize> = HashMap::new();
    let mut evaluator = Evaluator::for_render();
    let mut items = Vec::new();

    for n in 0..count {
        let frame = start + n as i32;
        evaluator.evaluate_into(doc, frame as f32, &mut items);
        if evaluator.scopes.len() > 1 {
            if report.count(COMPOSITING_NOTE) == 0 {
                report.add(COMPOSITING_NOTE);
            }
            fanimate_core::eval::flatten_scopes(&mut items, &evaluator.scopes);
        }
        if doc.background.a > 0.0 {
            let camera = camera_at(doc, frame as f32).unwrap_or(Affine2::IDENTITY);
            let path = Path::rect(Vec2::ZERO, doc.size(), 0.0);
            let look = Look {
                d: path_data(&path, camera),
                color: hex(doc.background),
                alpha: num(doc.background.a),
                width: None,
            };
            put(
                &mut tracks,
                &mut index,
                count,
                n,
                (u32::MAX, 0, String::new(), false),
                false,
                false,
                look,
            );
        }
        for item in &items {
            if item.image.is_some() {
                report.add("pictures left out");
                continue;
            }
            let style = item.shape.style;
            let t = item.transform;
            if !t.is_finite() || item.shape.path.points().any(|p| !p.is_finite()) {
                report.add("shapes with unusable points left out");
                continue;
            }
            let part = format!("{:?}", item.part);
            let translucent = |c: Color| c.a * item.opacity < 0.999;
            if style.fill.is_some_and(translucent) || style.stroke.is_some_and(|s| translucent(s.color)) {
                report.add("translucent shapes blended in sRGB, so they can look different from the canvas");
            }
            if let Some(fill) = style.fill {
                let d = path_data(&item.shape.path, t);
                if !d.is_empty() {
                    let look = Look {
                        d,
                        color: hex(fill),
                        alpha: num((fill.a * item.opacity).clamp(0.0, 1.0)),
                        width: None,
                    };
                    let key = (item.layer.0, item.index, part.clone(), false);
                    put(&mut tracks, &mut index, count, n, key, false, false, look);
                }
            }
            if let Some(stroke) = style.stroke {
                let widths = item
                    .shape
                    .widths
                    .as_deref()
                    .filter(|w| w.len() == item.shape.path.on_curve_count());
                if widths.is_some() {
                    report.add("width profiles drawn as an even stroke");
                }
                let mut source = std::borrow::Cow::Borrowed(&item.shape.path);
                if let Some(flags) = item.shape.hidden_edges() {
                    source = std::borrow::Cow::Owned(item.shape.path.without_hidden_edges(flags, None).0);
                }
                if item.shape.trim != Vec2::new(0.0, 1.0) {
                    source =
                        std::borrow::Cow::Owned(source.trimmed(item.shape.trim.x, item.shape.trim.y, None).0);
                }
                let width = stroke.width * t.matrix2.determinant().abs().sqrt();
                let d = path_data(&source, t);
                if !d.is_empty() && width.is_finite() && width > 0.0 {
                    let look = Look {
                        d,
                        color: hex(stroke.color),
                        alpha: num((stroke.color.a * item.opacity).clamp(0.0, 1.0)),
                        width: Some(num(width)),
                    };
                    let key = (item.layer.0, item.index, part, true);
                    put(
                        &mut tracks,
                        &mut index,
                        count,
                        n,
                        key,
                        true,
                        widths.is_some(),
                        look,
                    );
                }
            }
        }
    }

    let seconds = count as f32 / doc.fps.max(1.0);
    let mut out = String::with_capacity(8192);
    let (w, h) = (doc.width.max(1.0), doc.height.max(1.0));
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        num(w),
        num(h),
        num(w),
        num(h)
    );
    let dur = format!("{}s", num(seconds.max(0.001)));
    for track in &tracks {
        write_track(&mut out, track, &dur, count);
        if out.len() > MAX_BYTES {
            return Err(format!(
                "This animation would make an SVG of more than {} MB. Export a shorter range, or use Lottie, which keeps files smaller.",
                MAX_BYTES / (1024 * 1024)
            ));
        }
    }
    out.push_str("</svg>\n");
    Ok((out, report))
}

fn hex(c: Color) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", q(c.r), q(c.g), q(c.b))
}

#[allow(clippy::too_many_arguments)]
fn put(
    tracks: &mut Vec<Track>,
    index: &mut HashMap<(u32, usize, String, bool), usize>,
    count: usize,
    n: usize,
    key: (u32, usize, String, bool),
    stroke: bool,
    round: bool,
    look: Look,
) {
    let at = *index.entry(key).or_insert_with(|| {
        tracks.push(Track {
            stroke,
            round,
            frames: vec![None; count],
        });
        tracks.len() - 1
    });
    tracks[at].frames[n] = Some(look);
}

/// Runs of equal values: each run's first frame and its value. A frame with
/// no value takes the one before it (what it shows does not matter, the
/// track is hidden then), so it never starts a run.
fn runs<T: PartialEq + Clone>(frames: &[Option<T>], pick: impl Fn(&T) -> String) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (n, f) in frames.iter().enumerate() {
        let value = match f {
            Some(look) => pick(look),
            None => match out.last() {
                Some((_, v)) => v.clone(),
                None => continue,
            },
        };
        match out.last() {
            Some((_, v)) if *v == value => {}
            _ => out.push((n, value)),
        }
    }
    out
}

fn animate(out: &mut String, attr: &str, runs: &[(usize, String)], dur: &str, count: usize) {
    if runs.len() < 2 {
        return;
    }
    let times: Vec<String> = runs
        .iter()
        .map(|(n, _)| {
            let t = *n as f64 / count as f64;
            let s = format!("{t:.6}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s.is_empty() {
                "0".to_owned()
            } else {
                s.to_owned()
            }
        })
        .collect();
    let values: Vec<&str> = runs.iter().map(|(_, v)| v.as_str()).collect();
    // keyTimes must start at 0: the first run starts on the first frame the track has a value
    let mut times = times;
    times[0] = "0".to_owned();
    let _ = writeln!(
        out,
        "<animate attributeName=\"{attr}\" dur=\"{dur}\" repeatCount=\"indefinite\" calcMode=\"discrete\" keyTimes=\"{}\" values=\"{}\"/>",
        times.join(";"),
        values.join(";")
    );
}

fn write_track(out: &mut String, track: &Track, dur: &str, count: usize) {
    let Some(first) = track.frames.iter().flatten().next() else {
        return;
    };
    let colour_attr = if track.stroke { "stroke" } else { "fill" };
    let opacity_attr = if track.stroke {
        "stroke-opacity"
    } else {
        "fill-opacity"
    };
    let hidden_first = track.frames[0].is_none();
    let _ = write!(out, "<path d=\"{}\"", first.d);
    if track.stroke {
        let _ = write!(
            out,
            " fill=\"none\"{}",
            paint("stroke", color_of(&first.color), 1.0)
        );
        let _ = write!(out, " stroke-width=\"{}\"", first.width.as_deref().unwrap_or("1"));
        if track.round {
            out.push_str(" stroke-linecap=\"round\" stroke-linejoin=\"round\"");
        }
    } else {
        let _ = write!(
            out,
            "{} fill-rule=\"nonzero\"",
            paint("fill", color_of(&first.color), 1.0)
        );
    }
    if first.alpha != "1" {
        let _ = write!(out, " {opacity_attr}=\"{}\"", first.alpha);
    }
    if hidden_first {
        out.push_str(" display=\"none\"");
    }
    out.push_str(">\n");
    animate(out, "d", &runs(&track.frames, |l| l.d.clone()), dur, count);
    animate(
        out,
        colour_attr,
        &runs(&track.frames, |l| l.color.clone()),
        dur,
        count,
    );
    animate(
        out,
        opacity_attr,
        &runs(&track.frames, |l| l.alpha.clone()),
        dur,
        count,
    );
    if track.stroke {
        animate(
            out,
            "stroke-width",
            &runs(&track.frames, |l| l.width.clone().unwrap_or_else(|| "1".into())),
            dur,
            count,
        );
    }
    if track.frames.iter().any(Option::is_none) {
        // shown or hidden, frame by frame (a track that starts hidden says so above)
        let shown: Vec<Option<()>> = track.frames.iter().map(|f| f.as_ref().map(|_| ())).collect();
        let mut visibility: Vec<(usize, String)> = Vec::new();
        for (n, f) in shown.iter().enumerate() {
            let v = if f.is_some() { "inline" } else { "none" };
            if visibility.last().is_none_or(|(_, last)| last != v) {
                visibility.push((n, v.to_owned()));
            }
        }
        animate(out, "display", &visibility, dur, count);
    }
    out.push_str("</path>\n");
}

/// The colour a `#rrggbb` string names, for reuse of the one-frame writer.
fn color_of(hex: &str) -> Color {
    Color::from_hex(hex).unwrap_or(Color::BLACK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{Interpolation, LayerKind, Shape, Style};

    fn doc_with(shapes: Vec<Shape>) -> (Document, fanimate_core::LayerId) {
        let mut doc = Document::new("anim", 200.0, 100.0, 10.0);
        doc.start = 1;
        doc.end = 5;
        let layer = doc.new_layer("art", LayerKind::Vector { shapes });
        let id = layer.id;
        doc.layers.push(layer);
        (doc, id)
    }

    fn square() -> Shape {
        Shape::new(
            "sq",
            Path::rect(Vec2::new(10.0, 10.0), Vec2::new(20.0, 20.0), 0.0),
            Style::fill(Color::rgb(1.0, 0.0, 0.0)),
        )
    }

    fn move_x(doc: &mut Document, id: fanimate_core::LayerId, keys: &[(i32, f32)]) {
        let layer = doc.find_mut(id).unwrap();
        let mut position = layer.transform.position.clone();
        for (frame, x) in keys {
            position = position.with_key(*frame, Vec2::new(*x, 0.0), Interpolation::Linear);
        }
        layer.transform.position = position;
    }

    /// The `(keyTimes, values)` of the first `<animate>` for `attr`.
    fn animation(svg: &str, attr: &str) -> Option<(Vec<String>, Vec<String>)> {
        let tag = format!("<animate attributeName=\"{attr}\"");
        let at = svg.find(&tag)?;
        let line = &svg[at..svg[at..].find("/>")? + at];
        let get = |name: &str| {
            let start = line.find(&format!("{name}=\""))? + name.len() + 2;
            let end = line[start..].find('"')? + start;
            Some(line[start..end].split(';').map(str::to_owned).collect::<Vec<_>>())
        };
        Some((get("keyTimes")?, get("values")?))
    }

    #[test]
    fn a_still_animation_has_paths_and_no_animate_elements() {
        let (doc, _) = doc_with(vec![square()]);
        let (svg, report) = animated(&doc, None).unwrap();
        assert!(report.is_empty());
        assert!(!svg.contains("<animate"), "{svg}");
        assert!(svg.contains("M10 10L30 10L30 30L10 30Z"));
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn a_moving_shape_steps_its_path_every_frame_at_the_frame_rate() {
        let (mut doc, id) = doc_with(vec![square()]);
        move_x(&mut doc, id, &[(1, 0.0), (5, 40.0)]);
        let (svg, _) = animated(&doc, None).unwrap();
        let (times, values) = animation(&svg, "d").expect("the path is animated");
        assert_eq!(times.len(), 5);
        assert_eq!(times.len(), values.len());
        assert_eq!(times[0], "0");
        assert_eq!(times[1], "0.2");
        // frame 1 at x 10, frame 5 at x 50
        assert!(values[0].starts_with("M10 10"), "{}", values[0]);
        assert!(values[4].starts_with("M50 10"), "{}", values[4]);
        // 5 frames at 10 per second last half a second
        assert!(svg.contains("dur=\"0.5s\""), "{svg}");
        // the colour never changes, so it is not animated
        assert!(animation(&svg, "fill").is_none());
    }

    #[test]
    fn a_range_bakes_only_those_frames_and_a_bad_range_is_refused() {
        let (mut doc, id) = doc_with(vec![square()]);
        move_x(&mut doc, id, &[(1, 0.0), (5, 40.0)]);
        let (svg, _) = animated(&doc, Some((2, 3))).unwrap();
        let (times, values) = animation(&svg, "d").unwrap();
        assert_eq!((times.len(), values.len()), (2, 2));
        assert!(animated(&doc, Some((5, 1))).is_err());
        assert!(
            animated(&doc, Some((1, 100_000)))
                .unwrap_err()
                .contains("at most")
        );
    }

    #[test]
    fn a_colour_that_changes_is_animated_with_the_runs_collapsed() {
        let mut s = square();
        s.fill_keys = vec![
            fanimate_core::Key {
                frame: 1,
                value: Color::rgb(1.0, 0.0, 0.0),
                interp: Interpolation::Step,
            },
            fanimate_core::Key {
                frame: 4,
                value: Color::rgb(0.0, 0.0, 1.0),
                interp: Interpolation::Step,
            },
        ];
        let (doc, _) = doc_with(vec![s]);
        let (svg, _) = animated(&doc, None).unwrap();
        let (times, values) = animation(&svg, "fill").expect("the fill is animated");
        assert_eq!(values, ["#ff0000", "#0000ff"]);
        assert_eq!(times, ["0", "0.6"]);
    }

    #[test]
    fn a_shape_that_is_not_there_on_every_frame_is_shown_and_hidden() {
        let (mut doc, _) = doc_with(vec![]);
        // a switch layer would be the usual way; a frame-by-frame layer shows its drawing from frame 3
        let mut cel = fanimate_core::Cel::new(3);
        cel.line.push(square());
        let layer = doc.new_layer("anim", LayerKind::Frames { cels: vec![cel] });
        doc.layers.push(layer);
        let (svg, _) = animated(&doc, None).unwrap();
        assert!(svg.contains("display=\"none\""), "{svg}");
        let (times, values) = animation(&svg, "display").expect("visibility is animated");
        assert_eq!(values, ["none", "inline"]);
        assert_eq!(times, ["0", "0.4"]);
    }

    #[test]
    fn strokes_and_the_losses_are_reported_like_a_single_frame() {
        let mut brush = Shape::new(
            "ink",
            Path::line(Vec2::new(0.0, 0.0), Vec2::new(50.0, 0.0)),
            Style::default().with_stroke(Color::rgba(0.0, 0.0, 1.0, 0.5), 4.0),
        );
        brush.widths = Some(vec![1.0; brush.path.on_curve_count()]);
        let (doc, _) = doc_with(vec![brush]);
        let (svg, report) = animated(&doc, None).unwrap();
        assert!(
            svg.contains("stroke-width=\"4\"") && svg.contains("stroke-linecap=\"round\""),
            "{svg}"
        );
        assert!(svg.contains("stroke-opacity=\"0.5\""));
        assert!(report.count("width profiles drawn as an even stroke") > 0);
        assert_eq!(
            report.count("translucent shapes blended in sRGB, so they can look different from the canvas"),
            5
        );
    }

    #[test]
    fn damaged_points_never_reach_the_file() {
        let mut bad = square();
        bad.path
            .cmds
            .push(fanimate_core::PathCmd::LineTo(Vec2::new(f32::NAN, 0.0)));
        let (doc, _) = doc_with(vec![bad]);
        let (svg, report) = animated(&doc, None).unwrap();
        assert!(!svg.contains("NaN") && !svg.contains("inf"));
        assert!(report.count("shapes with unusable points left out") > 0);
    }
}
