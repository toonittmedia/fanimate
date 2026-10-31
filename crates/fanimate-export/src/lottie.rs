//! Lottie and dotLottie: the animation as vector shapes the web and phones
//! play with the Lottie libraries.
//!
//! Every frame is evaluated the way a render evaluates it (bones, Smart
//! Bones, keys, text, the camera) and written as keys, so the Lottie shows
//! what the canvas shows frame by frame. That is the "baked" rung of the
//! fidelity ladder: nothing about how the animation was built (bones,
//! drivers, easing curves) survives, only its result, and a run of frames
//! that do not change costs no keys. What Lottie cannot carry is counted in
//! the [`Report`]: pictures are left out, a brush stroke's width profile is
//! drawn as an even stroke, and a see-through shape blends in sRGB.

use std::collections::HashMap;

use fanimate_core::eval::Evaluator;
use fanimate_core::{Affine2, Document, Path, PathCmd, Vec2, camera_at};
use serde_json::{Value, json};

use crate::report::Report;

/// The most frames one export bakes.
pub const MAX_FRAMES: usize = 20_000;
/// The most keys one export writes, so a damaged or enormous project fails
/// with a message rather than filling the disk.
pub const MAX_KEYS: usize = 3_000_000;

/// What was written and what it cost.
#[derive(Debug)]
pub struct Lottie {
    /// The animation, as the JSON Lottie players read.
    pub json: Value,
    pub report: Report,
    /// How many keys were written (a size estimate: about 40 bytes each).
    pub keys: usize,
}

/// One closed or open run of vertices with its bezier handles, in Lottie's
/// form: `i` and `o` are handles relative to their vertex.
#[derive(Clone, Debug, PartialEq)]
struct Poly {
    closed: bool,
    v: Vec<[f32; 2]>,
    i: Vec<[f32; 2]>,
    o: Vec<[f32; 2]>,
}

/// What one track (a fill, or a stroke, of one shape) looks like on a frame.
#[derive(Clone, Debug, PartialEq)]
struct State {
    polys: Vec<Poly>,
    color: [f32; 3],
    alpha: f32,
    /// A stroke's width and whether its ends and corners are round.
    stroke: Option<(f32, bool)>,
}

struct Track {
    name: String,
    frames: Vec<Option<State>>,
}

/// Bake `start..=end` of `doc` (the whole animation when `None`).
pub fn lottie(doc: &Document, range: Option<(i32, i32)>) -> Result<Lottie, String> {
    let (start, end) = range.unwrap_or((doc.start, doc.end));
    if end < start {
        return Err("The end frame comes before the start frame.".to_owned());
    }
    let count = (i64::from(end) - i64::from(start) + 1) as usize;
    if count > MAX_FRAMES {
        return Err(format!(
            "A Lottie holds at most {MAX_FRAMES} frames; this has {count}."
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
        // the canvas itself, carried through the camera
        if doc.background.a > 0.0 {
            let camera = camera_at(doc, frame as f32).unwrap_or(Affine2::IDENTITY);
            let path = Path::rect(Vec2::ZERO, doc.size(), 0.0);
            let state = State {
                polys: polys(&path, camera),
                color: [doc.background.r, doc.background.g, doc.background.b].map(round3),
                alpha: round3(doc.background.a),
                stroke: None,
            };
            put(
                &mut tracks,
                &mut index,
                count,
                n,
                (u32::MAX, 0, String::new(), false),
                "Background",
                state,
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
            let name = if item.shape.name.is_empty() {
                "Shape".to_owned()
            } else {
                item.shape.name.clone()
            };
            if let Some(fill) = style.fill {
                let state = State {
                    polys: polys(&item.shape.path, t),
                    color: [fill.r, fill.g, fill.b].map(round3),
                    alpha: round3(fill.a * item.opacity),
                    stroke: None,
                };
                if state.alpha < 0.999 {
                    report.add(
                        "translucent shapes blended in sRGB, so they can look different from the canvas",
                    );
                }
                put(
                    &mut tracks,
                    &mut index,
                    count,
                    n,
                    (item.layer.0, item.index, part.clone(), false),
                    &name,
                    state,
                );
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
                if width.is_finite() && width > 0.0 {
                    let state = State {
                        polys: polys(&source, t),
                        color: [stroke.color.r, stroke.color.g, stroke.color.b].map(round3),
                        alpha: round3(stroke.color.a * item.opacity),
                        stroke: Some((round3(width), widths.is_some())),
                    };
                    if state.alpha < 0.999 {
                        report.add(
                            "translucent shapes blended in sRGB, so they can look different from the canvas",
                        );
                    }
                    put(
                        &mut tracks,
                        &mut index,
                        count,
                        n,
                        (item.layer.0, item.index, part, true),
                        &name,
                        state,
                    );
                }
            }
        }
    }

    let mut keys = 0usize;
    // Lottie lists the top layer first; the tracks are back to front
    let mut layers = Vec::with_capacity(tracks.len());
    for (n, track) in tracks.iter().enumerate().rev() {
        layers.push(layer(track, n + 1, &mut keys));
        if keys > MAX_KEYS {
            return Err(format!(
                "This animation needs more than {MAX_KEYS} keys; export a shorter range."
            ));
        }
    }
    let json = json!({
        "v": "5.7.0",
        "fr": round3(doc.fps),
        "ip": 0,
        "op": count,
        "w": doc.width.round().max(1.0) as i64,
        "h": doc.height.round().max(1.0) as i64,
        "nm": doc.name,
        "ddd": 0,
        "assets": [],
        "layers": layers,
    });
    Ok(Lottie { json, report, keys })
}

/// The Lottie as a dotLottie file: a ZIP with a manifest and the animation.
pub fn dotlottie(doc: &Document, range: Option<(i32, i32)>) -> Result<(Vec<u8>, Report), String> {
    let l = lottie(doc, range)?;
    let animation = serde_json::to_vec(&l.json).map_err(|e| e.to_string())?;
    let manifest = serde_json::to_vec(&json!({
        "version": "1",
        "generator": "Fanimate",
        "animations": [{ "id": "animation", "autoplay": true, "loop": true }],
    }))
    .map_err(|e| e.to_string())?;
    let bytes = fanimate_fnm::zip::write(&[
        ("manifest.json", manifest.as_slice()),
        ("animations/animation.json", animation.as_slice()),
    ])?;
    Ok((bytes, l.report))
}

/// Record `state` for the track `key` on frame `n`, making the track the
/// first time it is seen.
fn put(
    tracks: &mut Vec<Track>,
    index: &mut HashMap<(u32, usize, String, bool), usize>,
    count: usize,
    n: usize,
    key: (u32, usize, String, bool),
    name: &str,
    state: State,
) {
    let at = *index.entry(key).or_insert_with(|| {
        tracks.push(Track {
            name: name.to_owned(),
            frames: vec![None; count],
        });
        tracks.len() - 1
    });
    tracks[at].frames[n] = Some(state);
}

fn round2(v: f32) -> f32 {
    if v.is_finite() {
        (v * 100.0).round() / 100.0
    } else {
        0.0
    }
}

fn round3(v: f32) -> f32 {
    if v.is_finite() {
        (v * 1000.0).round() / 1000.0
    } else {
        0.0
    }
}

/// The subpaths of `path`, carried through `t`, as Lottie vertex lists.
fn polys(path: &Path, t: Affine2) -> Vec<Poly> {
    let mut out: Vec<Poly> = Vec::new();
    let mut cur: Option<Poly> = None;
    let mut pen = Vec2::ZERO;
    let pt = |p: Vec2| t.transform_point2(p);
    let rel = |from: Vec2, to: Vec2| {
        let d = to - from;
        [round2(d.x), round2(d.y)]
    };
    let finish = |poly: Option<Poly>, out: &mut Vec<Poly>| {
        if let Some(p) = poly
            && p.v.len() > 1
        {
            out.push(p);
        }
    };
    for cmd in &path.cmds {
        match cmd {
            PathCmd::MoveTo(a) => {
                finish(cur.take(), &mut out);
                let p = pt(*a);
                pen = p;
                cur = Some(Poly {
                    closed: false,
                    v: vec![[round2(p.x), round2(p.y)]],
                    i: vec![[0.0, 0.0]],
                    o: vec![[0.0, 0.0]],
                });
            }
            PathCmd::LineTo(a) => {
                if let Some(c) = cur.as_mut() {
                    let p = pt(*a);
                    c.v.push([round2(p.x), round2(p.y)]);
                    c.i.push([0.0, 0.0]);
                    c.o.push([0.0, 0.0]);
                    pen = p;
                }
            }
            PathCmd::CubicTo(a, b, e) => {
                if let Some(c) = cur.as_mut() {
                    let (c1, c2, p) = (pt(*a), pt(*b), pt(*e));
                    if let Some(last) = c.o.last_mut() {
                        *last = rel(pen, c1);
                    }
                    c.v.push([round2(p.x), round2(p.y)]);
                    c.i.push(rel(p, c2));
                    c.o.push([0.0, 0.0]);
                    pen = p;
                }
            }
            PathCmd::Close => {
                if let Some(mut c) = cur.take() {
                    c.closed = true;
                    // a curve that ends where the path began is the closing
                    // segment: fold its last vertex into the first
                    if c.v.len() > 1 && c.v.last() == c.v.first() {
                        c.v.pop();
                        if let (Some(last_in), Some(first_in)) = (c.i.pop(), c.i.first_mut()) {
                            *first_in = last_in;
                        }
                        c.o.pop();
                    }
                    finish(Some(c), &mut out);
                }
            }
        }
    }
    finish(cur, &mut out);
    out
}

fn poly_json(p: &Poly) -> Value {
    json!({ "c": p.closed, "v": p.v, "i": p.i, "o": p.o })
}

/// An animated property from one value per frame: a plain value when it never
/// changes, otherwise keys. `linear` lets the player interpolate between
/// keys (and drops keys inside a run of equal values); without it every key
/// holds until the next. `at` is the comp frame of the first value. `key`
/// writes a value as a key's start, `plain` as an unkeyed value (Lottie
/// writes a number plainly and as a one-item list in a key). Returns the
/// property and how many keys it has.
fn property<T: PartialEq>(
    values: &[T],
    at: usize,
    key: impl Fn(&T) -> Value,
    plain: impl Fn(&T) -> Value,
    linear: bool,
) -> (Value, usize) {
    let n = values.len();
    if n == 0 || values.iter().all(|v| *v == values[0]) {
        let v = values.first().map_or(Value::Null, &plain);
        return (json!({ "a": 0, "k": v }), 0);
    }
    let key_fn = &key;
    let mut keys = Vec::new();
    for i in 0..n {
        let first = i == 0;
        let last = i + 1 == n;
        let same_before = i > 0 && values[i - 1] == values[i];
        let same_after = !last && values[i + 1] == values[i];
        // with interpolation a value equal to both neighbours adds nothing;
        // holding, a value equal to the one before adds nothing
        let skip = if linear {
            same_before && same_after
        } else {
            same_before
        };
        if !first && !last && skip {
            continue;
        }
        let mut key = serde_json::Map::new();
        key.insert("t".into(), json!(at + i));
        key.insert("s".into(), key_fn(&values[i]));
        if !last {
            if linear {
                key.insert("i".into(), json!({ "x": [1], "y": [1] }));
                key.insert("o".into(), json!({ "x": [0], "y": [0] }));
            } else {
                key.insert("h".into(), json!(1));
            }
        }
        keys.push(Value::Object(key));
    }
    let count = keys.len();
    (json!({ "a": 1, "k": keys }), count)
}

/// One shape layer for a track.
fn layer(track: &Track, ind: usize, keys: &mut usize) -> Value {
    // a track is there from its first frame to its last; where it is missing
    // in between it keeps the look of the frame before and is made see-through
    let first = track.frames.iter().position(Option::is_some).unwrap_or(0);
    let last = track.frames.iter().rposition(Option::is_some).unwrap_or(0);
    let mut held: Option<&State> = None;
    let mut shown: Vec<(&State, bool)> = Vec::with_capacity(last - first + 1);
    for f in first..=last {
        match (&track.frames[f], held) {
            (Some(s), _) => {
                held = Some(s);
                shown.push((s, true));
            }
            (None, Some(h)) => shown.push((h, false)),
            (None, None) => {}
        }
    }
    let template = shown[0].0;
    let stroke = template.stroke;
    // the paths hold their shape between keys only while the vertex count
    // stays the same; otherwise each key holds
    let same_shape = shown.iter().all(|(s, _)| {
        s.polys.len() == template.polys.len()
            && s.polys
                .iter()
                .zip(&template.polys)
                .all(|(a, b)| a.v.len() == b.v.len() && a.closed == b.closed)
    });
    let mut items = Vec::new();
    let mut local = 0usize;
    for p in 0..template.polys.len() {
        let values: Vec<Poly> = shown
            .iter()
            .map(|(s, _)| {
                s.polys
                    .get(p)
                    .cloned()
                    .unwrap_or_else(|| template.polys[p].clone())
            })
            .collect();
        let (ks, k) = property(
            &values,
            first,
            |poly| json!([poly_json(poly)]),
            poly_json,
            same_shape,
        );
        local += k;
        items.push(json!({ "ty": "sh", "nm": "Path", "ks": ks }));
    }
    let colors: Vec<[f32; 3]> = shown.iter().map(|(s, _)| s.color).collect();
    let colour = |c: &[f32; 3]| json!([c[0], c[1], c[2], 1]);
    let (c, k) = property(&colors, first, colour, colour, true);
    local += k;
    let alphas: Vec<f32> = shown.iter().map(|(s, _)| round3(s.alpha * 100.0)).collect();
    let (o, k) = property(&alphas, first, |a| json!([a]), |a| json!(a), true);
    local += k;
    if let Some((_, round)) = stroke {
        let widths: Vec<f32> = shown.iter().map(|(s, _)| s.stroke.map_or(0.0, |s| s.0)).collect();
        let (w, k) = property(&widths, first, |w| json!([w]), |w| json!(w), true);
        local += k;
        items.push(json!({
            "ty": "st", "nm": "Stroke", "c": c, "o": o, "w": w,
            "lc": if round { 2 } else { 1 }, "lj": if round { 2 } else { 1 }, "ml": 4,
        }));
    } else {
        items.push(json!({ "ty": "fl", "nm": "Fill", "c": c, "o": o, "r": 1 }));
    }
    items.push(json!({
        "ty": "tr",
        "p": { "a": 0, "k": [0, 0] }, "a": { "a": 0, "k": [0, 0] },
        "s": { "a": 0, "k": [100, 100] }, "r": { "a": 0, "k": 0 },
        "o": { "a": 0, "k": 100 }, "sk": { "a": 0, "k": 0 }, "sa": { "a": 0, "k": 0 },
    }));
    // the layer's own opacity hides the track where it is missing
    let presence: Vec<f32> = shown
        .iter()
        .map(|(_, here)| if *here { 100.0 } else { 0.0 })
        .collect();
    let (lo, k) = property(&presence, first, |p| json!([p]), |p| json!(p), false);
    local += k;
    *keys += local;
    json!({
        "ddd": 0, "ind": ind, "ty": 4, "nm": track.name, "sr": 1,
        "ks": {
            "o": lo,
            "r": { "a": 0, "k": 0 },
            "p": { "a": 0, "k": [0, 0, 0] },
            "a": { "a": 0, "k": [0, 0, 0] },
            "s": { "a": 0, "k": [100, 100, 100] },
        },
        "ao": 0,
        "shapes": [{ "ty": "gr", "nm": "Group", "it": items }],
        "ip": first,
        "op": last + 1,
        "st": 0,
        "bm": 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{Color, Interpolation, LayerKind, Shape, Stroke, Style};

    fn doc_with(shapes: Vec<Shape>) -> (Document, fanimate_core::LayerId) {
        let mut doc = Document::new("anim", 200.0, 100.0, 24.0);
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

    fn layers(l: &Lottie) -> &Vec<Value> {
        l.json["layers"].as_array().unwrap()
    }

    fn move_x(doc: &mut Document, id: fanimate_core::LayerId, keys: &[(i32, f32)], interp: Interpolation) {
        let layer = doc.find_mut(id).unwrap();
        let mut position = layer.transform.position.clone();
        for (frame, x) in keys {
            position = position.with_key(*frame, Vec2::new(*x, 0.0), interp);
        }
        layer.transform.position = position;
    }

    #[test]
    fn a_still_shape_is_one_layer_with_plain_values_and_no_keys() {
        let (doc, _) = doc_with(vec![square()]);
        let l = lottie(&doc, None).unwrap();
        assert_eq!(l.json["fr"], 24.0);
        assert_eq!((l.json["ip"].as_i64(), l.json["op"].as_i64()), (Some(0), Some(5)));
        assert_eq!(
            (l.json["w"].as_i64(), l.json["h"].as_i64()),
            (Some(200), Some(100))
        );
        // the top layer is listed first: the square, then the background
        let ls = layers(&l);
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[0]["nm"], "sq");
        assert_eq!(ls[1]["nm"], "Background");
        assert_eq!(l.keys, 0, "nothing moves, nothing is keyed");
        let group = &ls[0]["shapes"][0]["it"];
        assert_eq!(group[0]["ty"], "sh");
        assert_eq!(group[0]["ks"]["a"], 0);
        assert_eq!(group[0]["ks"]["k"]["c"], true);
        assert_eq!(group[0]["ks"]["k"]["v"].as_array().unwrap().len(), 4);
        assert_eq!(group[1]["ty"], "fl");
        assert_eq!(group[1]["c"]["k"], json!([1.0, 0.0, 0.0, 1]));
        assert_eq!(group[1]["o"]["k"], 100.0, "an unkeyed opacity is a number");
        assert!(l.report.is_empty());
    }

    #[test]
    fn a_moving_shape_is_baked_to_keys_at_the_frames_it_changes() {
        let (mut doc, id) = doc_with(vec![square()]);
        move_x(&mut doc, id, &[(1, 0.0), (5, 100.0)], Interpolation::Linear);
        let l = lottie(&doc, None).unwrap();
        let path = &layers(&l)[0]["shapes"][0]["it"][0]["ks"];
        assert_eq!(path["a"], 1);
        let keys = path["k"].as_array().unwrap();
        // linear motion changes every frame, so every frame is a key
        let times: Vec<i64> = keys.iter().map(|k| k["t"].as_i64().unwrap()).collect();
        assert_eq!(times, [0, 1, 2, 3, 4]);
        let x_at = |k: &Value| k["s"][0]["v"][0][0].as_f64().unwrap();
        assert_eq!(x_at(&keys[0]), 10.0);
        assert_eq!(x_at(&keys[4]), 110.0);
        assert!(keys[4].get("i").is_none(), "the last key has no handles");
        assert_eq!(keys[0]["i"]["x"], json!([1]));
        assert!(l.keys >= 5);
    }

    #[test]
    fn a_run_of_equal_frames_costs_no_keys_between_its_ends() {
        let (mut doc, id) = doc_with(vec![square()]);
        doc.end = 9;
        move_x(&mut doc, id, &[(1, 0.0), (7, 50.0)], Interpolation::Step);
        let l = lottie(&doc, None).unwrap();
        let keys = layers(&l)[0]["shapes"][0]["it"][0]["ks"]["k"]
            .as_array()
            .unwrap()
            .clone();
        let times: Vec<i64> = keys.iter().map(|k| k["t"].as_i64().unwrap()).collect();
        // still on frames 1 to 6 (times 0 to 5), then 50 from frame 7 (time 6) on
        assert_eq!(times, [0, 5, 6, 8], "{times:?}");
    }

    #[test]
    fn colour_and_opacity_animate() {
        let mut s = square();
        s.fill_keys = vec![
            fanimate_core::Key {
                frame: 1,
                value: Color::rgb(1.0, 0.0, 0.0),
                interp: Interpolation::Linear,
            },
            fanimate_core::Key {
                frame: 5,
                value: Color::rgb(0.0, 0.0, 1.0),
                interp: Interpolation::Linear,
            },
        ];
        let (mut doc, id) = doc_with(vec![s]);
        doc.find_mut(id).unwrap().opacity = fanimate_core::Channel::new(1.0)
            .with_key(1, 1.0, Interpolation::Linear)
            .with_key(5, 0.5, Interpolation::Linear);
        let l = lottie(&doc, None).unwrap();
        let fill = &layers(&l)[0]["shapes"][0]["it"][1];
        assert_eq!(fill["c"]["a"], 1, "the colour is keyed");
        assert_eq!(fill["c"]["k"][0]["s"], json!([1.0, 0.0, 0.0, 1]));
        assert_eq!(fill["o"]["a"], 1, "so is the opacity");
        assert_eq!(fill["o"]["k"][4]["s"], json!([50.0]));
        assert!(
            l.report
                .count("translucent shapes blended in sRGB, so they can look different from the canvas")
                > 0
        );
    }

    #[test]
    fn a_track_missing_for_some_frames_is_hidden_there_and_lives_from_first_to_last() {
        let state = State {
            polys: polys(
                &Path::rect(Vec2::ZERO, Vec2::new(5.0, 5.0), 0.0),
                Affine2::IDENTITY,
            ),
            color: [0.0; 3],
            alpha: 1.0,
            stroke: None,
        };
        let track = Track {
            name: "blink".into(),
            frames: vec![
                None,
                Some(state.clone()),
                None,
                Some(state.clone()),
                Some(state),
                None,
            ],
        };
        let mut keys = 0;
        let layer = layer(&track, 1, &mut keys);
        assert_eq!((layer["ip"].as_i64(), layer["op"].as_i64()), (Some(1), Some(5)));
        let o = &layer["ks"]["o"];
        assert_eq!(o["a"], 1);
        let held: Vec<(i64, f64)> = o["k"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| (k["t"].as_i64().unwrap(), k["s"][0].as_f64().unwrap()))
            .collect();
        // shown on 1, hidden on 2, shown on 3 and 4 (the run's end is a key)
        assert_eq!(held, [(1, 100.0), (2, 0.0), (3, 100.0), (4, 100.0)]);
        assert_eq!(o["k"][0]["h"], 1, "visibility holds, it does not fade");
    }

    #[test]
    fn strokes_curves_and_caps_are_written() {
        let mut s = Shape::new(
            "ring",
            Path::ellipse(Vec2::new(50.0, 50.0), Vec2::new(20.0, 20.0)),
            Style {
                fill: None,
                stroke: Some(Stroke {
                    color: Color::rgb(0.0, 0.5, 0.0),
                    width: 3.0,
                }),
            },
        );
        s.widths = Some(vec![1.0; s.path.on_curve_count()]);
        let (doc, _) = doc_with(vec![s]);
        let l = lottie(&doc, None).unwrap();
        let group = &layers(&l)[0]["shapes"][0]["it"];
        assert_eq!(group[1]["ty"], "st");
        assert_eq!(group[1]["w"]["k"], 3.0);
        assert_eq!(group[1]["lc"], 2, "brush strokes are round");
        let path = &group[0]["ks"]["k"];
        assert_eq!(
            path["v"].as_array().unwrap().len(),
            4,
            "a circle is four vertices"
        );
        assert_eq!(path["c"], true);
        assert_ne!(path["o"][0], json!([0.0, 0.0]), "curves carry handles");
        assert_eq!(l.report.count("width profiles drawn as an even stroke"), 5);
    }

    #[test]
    fn an_open_line_stays_open_and_a_damaged_point_is_left_out() {
        let line = Shape::new(
            "line",
            Path::new()
                .move_to(Vec2::new(0.0, 0.0))
                .line_to(Vec2::new(10.0, 5.0)),
            Style {
                fill: None,
                stroke: Some(Stroke {
                    color: Color::BLACK,
                    width: 1.0,
                }),
            },
        );
        let mut bad = square();
        bad.path.cmds.push(PathCmd::LineTo(Vec2::new(f32::NAN, 0.0)));
        let (doc, _) = doc_with(vec![line, bad]);
        let l = lottie(&doc, None).unwrap();
        let ls = layers(&l);
        assert_eq!(ls.len(), 2, "the background and the line");
        assert_eq!(ls[0]["shapes"][0]["it"][0]["ks"]["k"]["c"], false);
        assert_eq!(
            l.report.count("shapes with unusable points left out"),
            5,
            "once a frame"
        );
    }

    #[test]
    fn ranges_and_limits_are_checked() {
        let (doc, _) = doc_with(vec![square()]);
        assert!(lottie(&doc, Some((5, 1))).is_err());
        let too_many = lottie(&doc, Some((1, MAX_FRAMES as i32 + 10))).unwrap_err();
        assert!(too_many.contains("at most"), "{too_many}");
        let part = lottie(&doc, Some((2, 3))).unwrap();
        assert_eq!(part.json["op"], 2);
    }

    #[test]
    fn a_dotlottie_is_a_zip_with_a_manifest_and_the_animation() {
        let (doc, _) = doc_with(vec![square()]);
        let (bytes, report) = dotlottie(&doc, None).unwrap();
        assert!(report.is_empty());
        assert!(fanimate_fnm::zip::is_zip(&bytes));
        let zip = fanimate_fnm::zip::Archive::read(&bytes).unwrap();
        let manifest: Value = serde_json::from_slice(&zip.get("manifest.json").unwrap().unwrap()).unwrap();
        assert_eq!(manifest["animations"][0]["id"], "animation");
        let animation: Value =
            serde_json::from_slice(&zip.get("animations/animation.json").unwrap().unwrap()).unwrap();
        assert_eq!(animation["nm"], "anim");
    }

    #[test]
    fn closed_curves_fold_their_last_vertex_into_the_first() {
        let c = polys(
            &Path::ellipse(Vec2::ZERO, Vec2::new(10.0, 10.0)),
            Affine2::IDENTITY,
        );
        assert_eq!(c.len(), 1);
        assert!(c[0].closed);
        assert_eq!(c[0].v.len(), 4);
        assert!(c[0].i.iter().chain(&c[0].o).all(|h| *h != [0.0, 0.0]));
        let r = polys(
            &Path::rect(Vec2::ZERO, Vec2::new(4.0, 4.0), 0.0),
            Affine2::IDENTITY,
        );
        assert!(r[0].closed && r[0].i.iter().all(|h| *h == [0.0, 0.0]));
    }
}
