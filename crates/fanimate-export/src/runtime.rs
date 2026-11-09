//! The Fanimate runtime format (`.fnmr`, version 1): the animation as
//! slots, each one a shape with a path, paint and baked transform keys, for
//! a game engine or a web page to play with a small player of its own.
//!
//! Every frame is evaluated like a render (bones, Smart Bones, keys,
//! drivers, text, the camera) and written as what a player needs to draw it:
//! per slot a path in the shape's own space, its paint, and the matrix and
//! opacity that carry it to the picture, keyed only where they change. A
//! rigged character therefore stays small (its paths are written once and
//! only matrices move) and plays exactly as the canvas shows it, with no
//! bone solver, no easing code and no Smart Bone logic in the player. What
//! the runtime does **not** carry is how the animation was made: bones,
//! weights and drivers are gone, so a game cannot repose a character. The
//! specification is `docs/spec/runtime.md`.
//!
//! [`Player`] is the reference reader: the file's meaning in a few lines,
//! checked against the evaluator frame by frame. The JavaScript player in
//! `runtimes/web` is the same code in another language.

use std::collections::HashMap;

use fanimate_core::eval::Evaluator;
use fanimate_core::{Affine2, Color, Document, Path, Vec2, camera_at};
use serde_json::{Value, json};

use crate::report::Report;
use crate::svg::path_data;

/// The format's name and version, written to every file.
pub const FORMAT: &str = "fanimate-runtime";
pub const VERSION: u32 = 1;

/// The file inside the `.fnmr` archive.
pub const FILE: &str = "runtime.json";

/// The most frames one runtime file bakes.
pub const MAX_FRAMES: usize = 20_000;
/// The most keys it may hold.
pub const MAX_KEYS: usize = 3_000_000;

const COMPOSITING_NOTE: &str = "masks, clips, blend modes and flattened groups drawn as plain layers";

#[derive(Clone, PartialEq)]
struct Look {
    /// The farthest a point of the path is from the slot's origin: what a turn or a scale
    /// of the matrix costs in distance, for the tolerance of [`bake_with`].
    extent: f32,
    d: String,
    fill: Option<(String, f32)>,
    stroke: Option<(String, f32, f32)>,
    m: [f32; 6],
    opacity: f32,
}

struct Slot {
    name: String,
    frames: Vec<Option<Look>>,
}

/// What [`bake`] made.
pub struct Baked {
    pub json: Value,
    pub report: Report,
    pub keys: usize,
}

fn round4(v: f32) -> f32 {
    if v.is_finite() {
        (v * 10_000.0).round() / 10_000.0
    } else {
        0.0
    }
}

fn hex(c: Color) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", q(c.r), q(c.g), q(c.b))
}

/// The matrix with five decimals in its turn and scale and three in its move:
/// a point a thousand units from the origin is then off by less than 0.01.
fn matrix(t: Affine2) -> [f32; 6] {
    let c = t.to_cols_array();
    let fine = |v: f32| {
        if v.is_finite() {
            (v * 100_000.0).round() / 100_000.0
        } else {
            0.0
        }
    };
    let coarse = |v: f32| {
        if v.is_finite() {
            (v * 1_000.0).round() / 1_000.0
        } else {
            0.0
        }
    };
    [
        fine(c[0]),
        fine(c[1]),
        fine(c[2]),
        fine(c[3]),
        coarse(c[4]),
        coarse(c[5]),
    ]
}

/// Bake `range` (the whole animation when `None`) exactly: a key wherever a frame differs.
pub fn bake(doc: &Document, range: Option<(i32, i32)>) -> Result<Baked, String> {
    bake_with(doc, range, 0.0)
}

/// Bake with a `tolerance` in document units: a key that the player would draw
/// within that distance of the exact frame without it (every point of the slot's
/// path, and opacity at 1 percent to the unit) is left out. 0 keeps every key; a
/// walk cycle shrinks a lot at a couple of units and looks the same.
pub fn bake_with(doc: &Document, range: Option<(i32, i32)>, tolerance: f32) -> Result<Baked, String> {
    let tolerance = if tolerance.is_finite() {
        tolerance.clamp(0.0, 1000.0)
    } else {
        0.0
    };
    let (start, end) = range.unwrap_or((doc.start, doc.end));
    if end < start {
        return Err("The end frame comes before the start frame.".to_owned());
    }
    let count = (i64::from(end) - i64::from(start) + 1) as usize;
    if count > MAX_FRAMES {
        return Err(format!(
            "A runtime file holds at most {MAX_FRAMES} frames; this has {count}."
        ));
    }
    let mut report = Report::default();
    let mut slots: Vec<Slot> = Vec::new();
    let mut index: HashMap<(u32, usize, String), usize> = HashMap::new();
    let mut evaluator = Evaluator::for_render();
    let mut items = Vec::new();
    let mut put = |slots: &mut Vec<Slot>, key: (u32, usize, String), name: &str, n: usize, look: Look| {
        let at = *index.entry(key).or_insert_with(|| {
            slots.push(Slot {
                name: name.to_owned(),
                frames: vec![None; count],
            });
            slots.len() - 1
        });
        slots[at].frames[n] = Some(look);
    };

    for n in 0..count {
        let frame = start + n as i32;
        evaluator.evaluate_into(doc, frame as f32, &mut items);
        if evaluator.scopes.len() > 1 {
            if report.count(COMPOSITING_NOTE) == 0 {
                report.add(COMPOSITING_NOTE);
            }
            fanimate_core::eval::flatten_scopes(&mut items, &evaluator.scopes);
        }
        let camera = camera_at(doc, frame as f32).unwrap_or(Affine2::IDENTITY);
        if doc.background.a > 0.0 {
            let rect = Path::rect(Vec2::ZERO, doc.size(), 0.0);
            let look = Look {
                extent: doc.size().length(),
                d: path_data(&rect, Affine2::IDENTITY),
                fill: Some((hex(doc.background), round4(doc.background.a))),
                stroke: None,
                m: matrix(camera),
                opacity: 1.0,
            };
            put(&mut slots, (u32::MAX, 0, String::new()), "Background", n, look);
        }
        for item in &items {
            if item.image.is_some() {
                report.add("pictures left out");
                continue;
            }
            let style = item.shape.style;
            if style.fill.is_none() && style.stroke.is_none() {
                continue;
            }
            let t = camera * item.transform;
            if !t.is_finite() || item.shape.path.points().any(|p| !p.is_finite()) {
                report.add("shapes with unusable points left out");
                continue;
            }
            if item
                .shape
                .widths
                .as_deref()
                .is_some_and(|w| w.len() == item.shape.path.on_curve_count())
            {
                report.add("width profiles drawn as an even stroke");
            }
            if item.shape.hidden_edges().is_some() || item.shape.trim != Vec2::new(0.0, 1.0) {
                report.add("hidden edges and stroke exposure drawn as whole strokes");
            }
            if style.fill.is_some_and(|c| c.a * item.opacity < 0.999)
                || style.stroke.is_some_and(|s| s.color.a * item.opacity < 0.999)
            {
                report.add("translucent shapes blended in sRGB, so they can look different from the canvas");
            }
            let look = Look {
                extent: item.shape.path.points().map(|p| p.length()).fold(0.0, f32::max),
                d: path_data(&item.shape.path, Affine2::IDENTITY),
                fill: style.fill.map(|c| (hex(c), round4(c.a))),
                stroke: style
                    .stroke
                    .map(|s| (hex(s.color), round4(s.color.a), round4(s.width))),
                m: matrix(t),
                opacity: round4(item.opacity),
            };
            let part = format!("{:?}", item.part);
            put(
                &mut slots,
                (item.layer.0, item.index, part),
                &item.shape.name,
                n,
                look,
            );
        }
    }

    let mut keys = 0usize;
    let mut out = Vec::with_capacity(slots.len());
    for slot in &slots {
        out.push(write_slot(slot, start, &mut keys, tolerance));
        if keys > MAX_KEYS {
            return Err(format!(
                "This animation needs more than {MAX_KEYS} keys; export a shorter range."
            ));
        }
    }
    let background = (doc.background.a > 0.0).then(|| hex(doc.background));
    let json = json!({
        "format": FORMAT,
        "version": VERSION,
        "name": doc.name,
        "fps": round4(doc.fps),
        "width": round4(doc.width),
        "height": round4(doc.height),
        "first_frame": start,
        "last_frame": end,
        "background": background,
        "slots": out,
    });
    Ok(Baked { json, report, keys })
}

/// The slot as JSON: constant parts once, parts that change as keys.
fn write_slot(slot: &Slot, start: i32, keys: &mut usize, tolerance: f32) -> Value {
    let first = slot
        .frames
        .iter()
        .flatten()
        .next()
        .expect("a slot has at least one frame");
    let frame_of = |n: usize| start + n as i32;
    // ranges of frames the slot is drawn on
    let mut present: Vec<[i32; 2]> = Vec::new();
    for (n, f) in slot.frames.iter().enumerate() {
        if f.is_some() {
            match present.last_mut() {
                Some(r) if r[1] == frame_of(n) - 1 => r[1] = frame_of(n),
                _ => present.push([frame_of(n), frame_of(n)]),
            }
        }
    }
    let mut value = json!({
        "name": slot.name,
        "d": first.d,
        "present": present,
    });
    let paint = |l: &Look| {
        json!({
            "fill": l.fill.as_ref().map(|(c, a)| json!({"color": c, "alpha": a})),
            "stroke": l.stroke.as_ref().map(|(c, a, w)| json!({"color": c, "alpha": a, "width": w})),
        })
    };
    // the values of a track on the frames it is drawn, with a key where it differs from a neighbour
    let pairs: Vec<(usize, &Look)> = slot
        .frames
        .iter()
        .enumerate()
        .filter_map(|(n, f)| f.as_ref().map(|l| (n, l)))
        .collect();
    let changes = |same: &dyn Fn(&Look, &Look) -> bool| -> Vec<usize> {
        let mut at = Vec::new();
        for (i, (n, look)) in pairs.iter().enumerate() {
            let prev = i.checked_sub(1).map(|p| pairs[p]);
            let next = pairs.get(i + 1);
            // the neighbour must be the next frame for a run to count as unchanged
            let differs_prev = prev.is_none_or(|(pn, pl)| pn + 1 != *n || !same(pl, look));
            // the last frame needs no key of its own: the player holds the last key
            let differs_next = next.is_some_and(|(nn, nl)| *nn != n + 1 || !same(look, nl));
            if differs_prev || differs_next {
                at.push(i);
            }
        }
        at
    };
    value["paint"] = paint(first);
    let paint_same = |a: &Look, b: &Look| a.fill == b.fill && a.stroke == b.stroke;
    let paint_at = changes(&paint_same);
    if paint_at.iter().any(|&i| !paint_same(first, pairs[i].1)) {
        // stepped: a paint holds until the next key
        let list: Vec<Value> = changes(&|a: &Look, b: &Look| paint_same(a, b))
            .into_iter()
            .filter(|&i| i == 0 || !paint_same(pairs[i - 1].1, pairs[i].1))
            .map(|i| {
                let mut p = paint(pairs[i].1);
                p["frame"] = json!(frame_of(pairs[i].0));
                p
            })
            .collect();
        *keys += list.len();
        value["paint_keys"] = json!(list);
    }
    let path_same = |a: &Look, b: &Look| a.d == b.d;
    if pairs.iter().any(|(_, l)| !path_same(first, l)) {
        let list: Vec<Value> = pairs
            .iter()
            .enumerate()
            .filter(|(i, _)| *i == 0 || !path_same(pairs[i - 1].1, pairs[*i].1))
            .map(|(_, (n, l))| json!({"frame": frame_of(*n), "d": l.d}))
            .collect();
        *keys += list.len();
        value["path_keys"] = json!(list);
    }
    let motion_same = |a: &Look, b: &Look| a.m == b.m && a.opacity == b.opacity;
    let mut at = changes(&motion_same);
    if tolerance > 0.0 {
        at = thin(&pairs, &at, tolerance);
    }
    let list: Vec<Value> = at
        .into_iter()
        .map(|i| {
            let (n, l) = pairs[i];
            json!({"frame": frame_of(n), "m": l.m, "opacity": l.opacity})
        })
        .collect();
    *keys += list.len();
    value["keys"] = json!(list);
    value
}

/// How far the player would draw `mid` from the exact frame if only `a` and `b`
/// (`t` of the way from `a` to `b`) were keyed: the worst of the farthest point's
/// move and the opacity change (1 percent counts as a unit).
fn drift(a: &Look, b: &Look, mid: &Look, t: f32) -> f32 {
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let e = mid.extent.max(a.extent).max(b.extent);
    // a point at distance e from the origin moves by at most the translation error plus e
    // times the size (Frobenius norm) of the error of the other four numbers
    let m = |i: usize| lerp(a.m[i], b.m[i]) - mid.m[i];
    let moved = m(4).hypot(m(5)) + e * (m(0) * m(0) + m(1) * m(1) + m(2) * m(2) + m(3) * m(3)).sqrt();
    let faded = (lerp(a.opacity, b.opacity) - mid.opacity).abs() * 100.0;
    moved.max(faded)
}

/// The keys to keep out of `keys` (indices into `pairs`): the ends of every unbroken run
/// of frames, and every frame the player would otherwise draw more than `tolerance`
/// from the exact one (Ramer-Douglas-Peucker over the frames of the run).
fn thin(pairs: &[(usize, &Look)], keys: &[usize], tolerance: f32) -> Vec<usize> {
    // a slot that never moves has one key already, and no run to thin
    if keys.len() <= 1 {
        return keys.to_vec();
    }
    let mut kept: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < pairs.len() {
        // the run of consecutive frames from `i`
        let mut j = i;
        while j + 1 < pairs.len() && pairs[j + 1].0 == pairs[j].0 + 1 {
            j += 1;
        }
        kept.push(i);
        let mut stack = vec![(i, j)];
        while let Some((a, b)) = stack.pop() {
            if b <= a + 1 {
                continue;
            }
            let (fa, fb) = (pairs[a].0 as f32, pairs[b].0 as f32);
            let (mut worst, mut at) = (0.0f32, a);
            for k in a + 1..b {
                let t = (pairs[k].0 as f32 - fa) / (fb - fa);
                let d = drift(pairs[a].1, pairs[b].1, pairs[k].1, t);
                if d > worst {
                    (worst, at) = (d, k);
                }
            }
            if worst > tolerance {
                kept.push(at);
                stack.push((a, at));
                stack.push((at, b));
            }
        }
        if j > i {
            kept.push(j);
        }
        i = j + 1;
    }
    kept.sort_unstable();
    kept.dedup();
    kept
}

/// The runtime as a `.fnmr` file: a ZIP archive holding `runtime.json`.
pub fn fnmr(doc: &Document, range: Option<(i32, i32)>) -> Result<(Vec<u8>, Report, usize), String> {
    fnmr_with(doc, range, 0.0)
}

/// [`fnmr`] with the key tolerance of [`bake_with`].
pub fn fnmr_with(
    doc: &Document,
    range: Option<(i32, i32)>,
    tolerance: f32,
) -> Result<(Vec<u8>, Report, usize), String> {
    let baked = bake_with(doc, range, tolerance)?;
    let text = serde_json::to_vec(&baked.json).map_err(|e| e.to_string())?;
    let zip = fanimate_fnm::zip::write(&[(FILE, &text)])?;
    Ok((zip, baked.report, baked.keys))
}

// ---- the reference player ------------------------------------------------

/// One slot as a player draws it on a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub name: String,
    /// The path in the slot's own space (SVG path data).
    pub d: String,
    pub fill: Option<(String, f32)>,
    /// Colour, alpha and width of the stroke.
    pub stroke: Option<(String, f32, f32)>,
    /// Slot space to the picture.
    pub transform: Affine2,
    pub opacity: f32,
}

/// The reference reader of a runtime file.
#[derive(Debug)]
pub struct Player {
    pub name: String,
    pub fps: f32,
    pub width: f32,
    pub height: f32,
    pub first_frame: i32,
    pub last_frame: i32,
    root: Value,
}

impl Player {
    /// Read a `.fnmr` archive, or the bare JSON of one.
    pub fn open(bytes: &[u8]) -> Result<Self, String> {
        let text: Vec<u8> = if fanimate_fnm::zip::is_zip(bytes) {
            let zip = fanimate_fnm::zip::Archive::read(bytes)?;
            zip.get_checked(FILE).ok_or("the archive has no runtime.json")??
        } else {
            bytes.to_vec()
        };
        let root: Value = serde_json::from_slice(&text).map_err(|e| format!("not a runtime file: {e}"))?;
        if root["format"] != FORMAT {
            return Err("not a Fanimate runtime file".to_owned());
        }
        let version = root["version"].as_u64().unwrap_or(0);
        if version != u64::from(VERSION) {
            return Err(format!(
                "runtime version {version} is not one this reader knows ({VERSION})"
            ));
        }
        let num = |k: &str| root[k].as_f64().unwrap_or(0.0) as f32;
        Ok(Self {
            name: root["name"].as_str().unwrap_or("").to_owned(),
            fps: num("fps"),
            width: num("width"),
            height: num("height"),
            first_frame: root["first_frame"].as_i64().unwrap_or(0) as i32,
            last_frame: root["last_frame"].as_i64().unwrap_or(0) as i32,
            root,
        })
    }

    /// What is drawn on `frame` (a fraction is between two frames), back to front.
    pub fn sample(&self, frame: f32) -> Vec<Drawn> {
        let whole = frame.floor() as i32;
        let mut out = Vec::new();
        for slot in self.root["slots"].as_array().into_iter().flatten() {
            let shown = slot["present"].as_array().is_some_and(|rs| {
                rs.iter().any(|r| {
                    let (a, b) = (
                        r[0].as_i64().unwrap_or(0) as i32,
                        r[1].as_i64().unwrap_or(-1) as i32,
                    );
                    (a..=b).contains(&whole)
                })
            });
            if !shown {
                continue;
            }
            // the last stepped key at or before the frame
            let stepped = |list: &str| {
                slot[list].as_array().and_then(|keys| {
                    keys.iter()
                        .rfind(|k| k["frame"].as_i64().unwrap_or(0) as i32 <= whole)
                })
            };
            let d = stepped("path_keys")
                .map_or(&slot["d"], |k| &k["d"])
                .as_str()
                .unwrap_or("")
                .to_owned();
            let paint = stepped("paint_keys").unwrap_or(&slot["paint"]);
            let fill = paint["fill"].is_object().then(|| {
                (
                    paint["fill"]["color"].as_str().unwrap_or("#000000").to_owned(),
                    paint["fill"]["alpha"].as_f64().unwrap_or(1.0) as f32,
                )
            });
            let stroke = paint["stroke"].is_object().then(|| {
                (
                    paint["stroke"]["color"].as_str().unwrap_or("#000000").to_owned(),
                    paint["stroke"]["alpha"].as_f64().unwrap_or(1.0) as f32,
                    paint["stroke"]["width"].as_f64().unwrap_or(1.0) as f32,
                )
            });
            let (m, opacity) = motion(slot["keys"].as_array().map_or(&[][..], Vec::as_slice), frame);
            out.push(Drawn {
                name: slot["name"].as_str().unwrap_or("").to_owned(),
                d,
                fill,
                stroke,
                transform: Affine2::from_cols_array(&m),
                opacity,
            });
        }
        out
    }
}

/// Linear interpolation of the keyed matrix and opacity at `frame`, holding the ends.
fn motion(keys: &[Value], frame: f32) -> ([f32; 6], f32) {
    let read = |k: &Value| {
        let mut m = [0.0f32; 6];
        for (i, v) in k["m"].as_array().into_iter().flatten().take(6).enumerate() {
            m[i] = v.as_f64().unwrap_or(0.0) as f32;
        }
        (
            k["frame"].as_f64().unwrap_or(0.0) as f32,
            m,
            k["opacity"].as_f64().unwrap_or(1.0) as f32,
        )
    };
    let Some(first) = keys.first() else {
        return ([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 1.0);
    };
    let after = keys
        .iter()
        .position(|k| k["frame"].as_f64().unwrap_or(0.0) as f32 > frame);
    match after {
        None => {
            let (_, m, o) = read(&keys[keys.len() - 1]);
            (m, o)
        }
        Some(0) => {
            let (_, m, o) = read(first);
            (m, o)
        }
        Some(i) => {
            let (f0, m0, o0) = read(&keys[i - 1]);
            let (f1, m1, o1) = read(&keys[i]);
            let t = if f1 > f0 { (frame - f0) / (f1 - f0) } else { 0.0 };
            let mut m = [0.0; 6];
            for k in 0..6 {
                m[k] = m0[k] + (m1[k] - m0[k]) * t;
            }
            (m, o0 + (o1 - o0) * t)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{Interpolation, LayerKind, Shape, Style, samples};

    fn square_doc() -> (Document, fanimate_core::LayerId) {
        let mut doc = Document::new("rt", 200.0, 100.0, 24.0);
        doc.start = 1;
        doc.end = 5;
        let shape = Shape::new(
            "sq",
            Path::rect(Vec2::new(10.0, 10.0), Vec2::new(20.0, 20.0), 0.0),
            Style::fill(Color::rgb(1.0, 0.0, 0.0)),
        );
        let layer = doc.new_layer("art", LayerKind::Vector { shapes: vec![shape] });
        let id = layer.id;
        doc.layers.push(layer);
        (doc, id)
    }

    #[test]
    fn a_still_slot_is_one_key_and_its_path_is_written_once() {
        let (doc, _) = square_doc();
        let b = bake(&doc, None).unwrap();
        let slots = b.json["slots"].as_array().unwrap();
        // the background and the square
        assert_eq!(slots.len(), 2);
        let sq = &slots[1];
        assert_eq!(sq["name"], "sq");
        assert_eq!(sq["d"], "M10 10L30 10L30 30L10 30Z");
        assert_eq!(sq["present"], json!([[1, 5]]));
        assert_eq!(sq["keys"].as_array().unwrap().len(), 1);
        assert!(sq.get("path_keys").is_none() && sq.get("paint_keys").is_none());
        assert_eq!(b.json["format"], FORMAT);
        assert!(b.report.is_empty());
    }

    #[test]
    fn a_moving_slot_has_keys_where_it_moves_and_the_player_interpolates_between_them() {
        let (mut doc, id) = square_doc();
        let layer = doc.find_mut(id).unwrap();
        layer.transform.position = layer
            .transform
            .position
            .clone()
            .with_key(1, Vec2::ZERO, Interpolation::Linear)
            .with_key(5, Vec2::new(40.0, 0.0), Interpolation::Linear);
        let (bytes, _, keys) = fnmr(&doc, None).unwrap();
        assert!(fanimate_fnm::zip::is_zip(&bytes));
        assert!(keys >= 5);
        let player = Player::open(&bytes).unwrap();
        for frame in 1..=5 {
            let drawn = player.sample(frame as f32);
            let sq = drawn.iter().find(|d| d.name == "sq").unwrap();
            assert!(
                (sq.transform.translation.x - 10.0 * (frame - 1) as f32).abs() < 1.0e-3,
                "frame {frame}"
            );
        }
        // half a frame between is halfway
        let half = player.sample(2.5);
        let sq = half.iter().find(|d| d.name == "sq").unwrap();
        assert!((sq.transform.translation.x - 15.0).abs() < 1.0e-3);
        // outside the range nothing of it is drawn
        assert!(player.sample(9.0).iter().all(|d| d.name != "sq"));
    }

    #[test]
    fn a_tolerance_leaves_out_keys_the_player_would_not_miss_and_never_strays_further_than_it() {
        let doc = samples::walk_cycle();
        let exact = bake(&doc, None).unwrap();
        let loose = bake_with(&doc, None, 2.0).unwrap();
        // a rig that swings its limbs through big angles needs most of its keys at a couple of
        // units: rotation is not linear in the matrix. It never needs more
        assert!(
            loose.keys <= exact.keys,
            "{} keys at 2 against {} exact",
            loose.keys,
            exact.keys
        );
        // a tolerance of 0 is the exact bake, and nonsense is the exact bake too
        assert_eq!(bake_with(&doc, None, 0.0).unwrap().keys, exact.keys);
        assert_eq!(bake_with(&doc, None, f32::NAN).unwrap().keys, exact.keys);
        // the player draws every frame within the tolerance of the exact one
        let make = |t: f32| {
            let (bytes, _, _) = fnmr_with(&doc, None, t).unwrap();
            Player::open(&bytes).unwrap()
        };
        let (exact_player, loose_player) = (make(0.0), make(2.0));
        let mut worst = 0.0f32;
        for frame in doc.start..=doc.end {
            let (a, b) = (
                exact_player.sample(frame as f32),
                loose_player.sample(frame as f32),
            );
            assert_eq!(a.len(), b.len(), "frame {frame}");
            for (x, y) in a.iter().zip(&b) {
                assert_eq!(x.d, y.d);
                // the farthest corner of a 200 unit box stands in for the whole path
                for corner in [
                    Vec2::ZERO,
                    Vec2::new(200.0, 0.0),
                    Vec2::new(0.0, 200.0),
                    Vec2::new(200.0, 200.0),
                ] {
                    let (p, q) = (
                        x.transform.transform_point2(corner),
                        y.transform.transform_point2(corner),
                    );
                    worst = worst.max(p.distance(q));
                }
            }
        }
        // the corners sit at about the slot's own extent, where the bound is tightest
        assert!(worst < 2.0, "the loosest frame is {worst} units off");
    }

    #[test]
    fn a_straight_move_needs_two_keys_and_an_eased_one_far_fewer_than_one_a_frame() {
        let mut doc = Document::new("slide", 800.0, 100.0, 24.0);
        doc.start = 1;
        doc.end = 49;
        let shape = Shape::new(
            "box",
            Path::rect(Vec2::ZERO, Vec2::new(20.0, 20.0), 0.0),
            Style::fill(Color::BLACK),
        );
        let layer = doc.new_layer("art", LayerKind::Vector { shapes: vec![shape] });
        let id = layer.id;
        doc.layers.push(layer);
        let keys_for = |interp: Interpolation, tolerance: f32, doc: &mut Document| {
            let l = doc.find_mut(id).unwrap();
            l.transform.position = fanimate_core::Channel::new(Vec2::ZERO)
                .with_key(1, Vec2::ZERO, interp)
                .with_key(49, Vec2::new(480.0, 0.0), interp);
            bake_with(doc, None, tolerance).unwrap().keys
        };
        // the background has a key of its own, and every key above that belongs to the box
        let exact = keys_for(Interpolation::Linear, 0.0, &mut doc);
        let straight = keys_for(Interpolation::Linear, 0.25, &mut doc);
        assert!(
            exact >= 49 && straight <= 4,
            "{straight} keys for a straight move, {exact} exact"
        );
        let eased = keys_for(Interpolation::Smooth, 4.0, &mut doc);
        assert!(
            eased < exact / 2 && eased > straight,
            "{eased} eased keys against {exact} exact"
        );
        // and the player stays within the tolerance at every frame
        let l = doc.find_mut(id).unwrap();
        l.transform.position = fanimate_core::Channel::new(Vec2::ZERO)
            .with_key(1, Vec2::ZERO, Interpolation::Smooth)
            .with_key(49, Vec2::new(480.0, 0.0), Interpolation::Smooth);
        let (bytes, _, _) = fnmr_with(&doc, None, 4.0).unwrap();
        let (exact_bytes, _, _) = fnmr(&doc, None).unwrap();
        let (loose, exact) = (Player::open(&bytes).unwrap(), Player::open(&exact_bytes).unwrap());
        for frame in 1..=49 {
            let x = |p: &Player| {
                p.sample(frame as f32)
                    .iter()
                    .find(|d| d.name == "box")
                    .unwrap()
                    .transform
                    .translation
                    .x
            };
            assert!((x(&loose) - x(&exact)).abs() <= 4.0 + 0.01, "frame {frame}");
        }
    }

    #[test]
    fn a_player_refuses_what_it_cannot_read_without_panicking() {
        assert!(Player::open(b"not a file").is_err());
        assert!(Player::open(br#"{"format": "other"}"#).is_err());
        let future = br#"{"format": "fanimate-runtime", "version": 99}"#;
        assert!(Player::open(future).unwrap_err().contains("version 99"));
        // a damaged but valid file draws nothing and does not panic
        let hollow =
            br#"{"format": "fanimate-runtime", "version": 1, "slots": [{"present": "x", "keys": 3}]}"#;
        assert!(Player::open(hollow).unwrap().sample(1.0).is_empty());
        assert!(bake(&square_doc().0, Some((5, 1))).is_err());
        assert!(bake(&square_doc().0, Some((1, 1_000_000))).is_err());
    }

    #[test]
    fn the_losses_are_reported() {
        let (mut doc, id) = square_doc();
        if let LayerKind::Vector { shapes } = &mut doc.find_mut(id).unwrap().kind {
            shapes[0].trim = Vec2::new(0.0, 0.5);
            shapes[0].style = Style::default().with_stroke(Color::rgba(0.0, 0.0, 1.0, 0.5), 2.0);
        }
        let b = bake(&doc, None).unwrap();
        assert_eq!(
            b.report
                .count("hidden edges and stroke exposure drawn as whole strokes"),
            5
        );
    }

    /// The walk cycle is rigged with bones and keys: the player's drawing of every frame
    /// must match the evaluator's, point for point.
    #[test]
    fn a_rigged_walk_cycle_plays_exactly_as_the_evaluator_draws_it() {
        let doc = samples::walk_cycle();
        let (bytes, report, _) = fnmr(&doc, None).unwrap();
        assert!(report.is_empty(), "{:?}", report.lines());
        let player = Player::open(&bytes).unwrap();
        let mut evaluator = Evaluator::for_render();
        let mut items = Vec::new();
        let step = ((doc.end - doc.start) / 24).max(1) as usize;
        let mut worst = 0.0f32;
        for frame in (doc.start..=doc.end).step_by(step) {
            evaluator.evaluate_into(&doc, frame as f32, &mut items);
            let camera = camera_at(&doc, frame as f32).unwrap_or(Affine2::IDENTITY);
            let drawn = player.sample(frame as f32);
            // the player also draws the background first
            let bg = usize::from(doc.background.a > 0.0);
            let shapes: Vec<_> = items.iter().filter(|i| i.image.is_none()).collect();
            assert_eq!(drawn.len(), shapes.len() + bg, "slots on frame {frame}");
            for (item, got) in shapes.iter().zip(drawn.iter().skip(bg)) {
                let want = camera * item.transform;
                let svg = path_data(&item.shape.path, Affine2::IDENTITY);
                assert_eq!(got.d, svg, "path of {} on frame {frame}", item.shape.name);
                for p in item.shape.path.points() {
                    let a = want.transform_point2(p);
                    let b = got.transform.transform_point2(p);
                    worst = worst.max(a.distance(b));
                }
                assert!((got.opacity - item.opacity).abs() < 1.0e-3);
            }
        }
        assert!(worst < 0.02, "the worst point is {worst} units off");
    }
}
