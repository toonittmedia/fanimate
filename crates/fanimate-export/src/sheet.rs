//! Sprite sheets for games: the frames of named clips packed into one PNG,
//! with an atlas that says where each frame is and which frames make a clip.
//!
//! Each frame is rendered like an export, its empty margin is trimmed away
//! (the atlas keeps the offset, so the engine puts it back where it was), a
//! frame identical to an earlier one shares that frame's place on the sheet
//! (a held pose costs nothing), and the rest are packed in shelves. The atlas
//! is TexturePacker's "JSON hash" layout, which Phaser, PixiJS, Godot and
//! most engines read as it is, with a `clips` list added for the clips.

use std::collections::HashMap;

use fanimate_core::Document;
use fanimate_render::{Gpu, render_to_rgba};
use serde_json::{Value, json};

use crate::report::Report;

/// The widest and tallest a sheet may be.
pub const MAX_SIDE: u32 = 8192;
/// The most frames one sheet holds.
pub const MAX_FRAMES: usize = 5_000;

/// A run of frames with a name, such as `walk` or `idle`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub name: String,
    /// First and last frame, inclusive.
    pub start: i32,
    pub end: i32,
}

/// How to make the sheet.
#[derive(Clone, Debug)]
pub struct Options {
    /// The size of every frame before it is trimmed.
    pub width: u32,
    pub height: u32,
    /// Empty pixels left round every packed frame, so filtering does not
    /// bleed one frame into its neighbour.
    pub padding: u32,
    /// The widest the sheet may be; frames are packed in shelves up to it.
    pub max_width: u32,
    /// Cut the empty margin off each frame.
    pub trim: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: 256,
            height: 256,
            padding: 2,
            max_width: 2048,
            trim: true,
        }
    }
}

/// A frame as rendered: its pixels and its place in its clip.
pub struct Cell {
    pub clip: usize,
    pub frame: i32,
    pub rgba: Vec<u8>,
}

/// The finished sheet.
#[derive(Debug)]
pub struct Sheet {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// The atlas, with `meta.image` set to `image_name`.
    pub atlas: Value,
    /// What could not be carried (nothing, for a sheet today).
    pub report: Report,
    /// Frames that were the same as an earlier one and share its picture.
    pub shared: usize,
}

/// Parse `walk=1-24,idle=25-48` into clips. A frame range may be one number.
pub fn parse_clips(text: &str) -> Result<Vec<Clip>, String> {
    let mut clips = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, range) = part
            .split_once('=')
            .ok_or_else(|| format!("\"{part}\" should look like name=first-last"))?;
        let name = name.trim();
        if name.is_empty() {
            return Err(format!("\"{part}\" has no clip name"));
        }
        let number = |s: &str| {
            s.trim()
                .parse::<i32>()
                .map_err(|_| format!("\"{s}\" is not a frame number"))
        };
        let (start, end) = match range.split_once('-') {
            Some((a, b)) if !a.trim().is_empty() => (number(a)?, number(b)?),
            _ => {
                let n = number(range)?;
                (n, n)
            }
        };
        if end < start {
            return Err(format!("clip \"{name}\" ends before it starts"));
        }
        if clips.iter().any(|c: &Clip| c.name == name) {
            return Err(format!("there are two clips called \"{name}\""));
        }
        clips.push(Clip {
            name: name.to_owned(),
            start,
            end,
        });
    }
    if clips.is_empty() {
        return Err("no clips given".to_owned());
    }
    Ok(clips)
}

/// Render every frame of every clip and build the sheet.
pub fn render(doc: &Document, clips: &[Clip], opts: &Options, image_name: &str) -> Result<Sheet, String> {
    let total: usize = clips.iter().map(|c| (c.end - c.start + 1).max(0) as usize).sum();
    if total == 0 || total > MAX_FRAMES {
        return Err(format!(
            "A sheet holds 1 to {MAX_FRAMES} frames; these clips have {total}."
        ));
    }
    if opts.width == 0 || opts.height == 0 || opts.width > MAX_SIDE || opts.height > MAX_SIDE {
        return Err(format!("A frame is 1 to {MAX_SIDE} pixels each way."));
    }
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let mut cells = Vec::with_capacity(total);
    for (i, clip) in clips.iter().enumerate() {
        for frame in clip.start..=clip.end {
            let rgba = render_to_rgba(&gpu, doc, frame as f32, opts.width, opts.height)
                .map_err(|e| e.to_string())?;
            cells.push(Cell { clip: i, frame, rgba });
        }
    }
    build(clips, &cells, doc.fps, opts, image_name)
}

/// The smallest box holding every pixel that is not fully transparent, as
/// (x, y, width, height); a frame with nothing in it is one transparent pixel.
fn trim_box(rgba: &[u8], w: u32, h: u32) -> (u32, u32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if rgba[((y * w + x) * 4 + 3) as usize] != 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        (0, 0, 1, 1)
    } else {
        (x0, y0, x1 - x0, y1 - y0)
    }
}

/// Where each packed rectangle sits, and the size of the sheet.
pub type Packing = (Vec<(u32, u32)>, (u32, u32));

/// Shelf-pack rectangles `sizes` (each already including its padding) into
/// rows no wider than `max_width`. Returns each one's top left corner and the
/// size of the sheet, or `None` when they do not fit in `MAX_SIDE` high.
pub fn pack(sizes: &[(u32, u32)], max_width: u32) -> Option<Packing> {
    if sizes.iter().any(|(w, _)| *w > max_width) {
        return None;
    }
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    // tallest first, so each shelf is about as tall as everything on it
    order.sort_by_key(|&i| std::cmp::Reverse((sizes[i].1, sizes[i].0)));
    let mut at = vec![(0, 0); sizes.len()];
    let (mut x, mut y, mut shelf, mut width) = (0u32, 0u32, 0u32, 0u32);
    for i in order {
        let (w, h) = sizes[i];
        if x + w > max_width {
            y += shelf;
            x = 0;
            shelf = 0;
        }
        at[i] = (x, y);
        x += w;
        shelf = shelf.max(h);
        width = width.max(x);
    }
    let height = y + shelf;
    (height <= MAX_SIDE).then_some((at, (width.max(1), height.max(1))))
}

/// Build the sheet from rendered frames. Pure (no GPU), so it is tested alone.
pub fn build(
    clips: &[Clip],
    cells: &[Cell],
    fps: f32,
    opts: &Options,
    image_name: &str,
) -> Result<Sheet, String> {
    let (fw, fh) = (opts.width, opts.height);
    let mut shared = 0;
    // each distinct picture once: a frame equal to an earlier one shares it
    struct Picture {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        pixels: Vec<u8>,
    }
    let mut pictures: Vec<Picture> = Vec::new();
    let mut by_content: HashMap<(u32, u32, u32, u32, Vec<u8>), usize> = HashMap::new();
    let mut which: Vec<usize> = Vec::with_capacity(cells.len());
    for cell in cells {
        if cell.rgba.len() != (fw * fh * 4) as usize {
            return Err("A frame is not the size of the sheet's frames.".to_owned());
        }
        let (x, y, w, h) = if opts.trim {
            trim_box(&cell.rgba, fw, fh)
        } else {
            (0, 0, fw, fh)
        };
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for row in y..y + h {
            let from = ((row * fw + x) * 4) as usize;
            pixels.extend_from_slice(&cell.rgba[from..from + (w * 4) as usize]);
        }
        let key = (x, y, w, h, pixels);
        let n = match by_content.get(&key) {
            Some(n) => {
                shared += 1;
                *n
            }
            None => {
                pictures.push(Picture {
                    x,
                    y,
                    w,
                    h,
                    pixels: key.4.clone(),
                });
                by_content.insert(key, pictures.len() - 1);
                pictures.len() - 1
            }
        };
        which.push(n);
    }
    let pad = opts.padding;
    let sizes: Vec<(u32, u32)> = pictures.iter().map(|p| (p.w + pad, p.h + pad)).collect();
    let (spots, (sw, sh)) = pack(&sizes, opts.max_width.min(MAX_SIDE)).ok_or_else(|| {
        format!(
            "The frames do not fit in a sheet {} wide; make them smaller or allow a wider sheet.",
            opts.max_width
        )
    })?;
    let mut rgba = vec![0u8; (sw * sh * 4) as usize];
    for (p, (px, py)) in pictures.iter().zip(&spots) {
        for row in 0..p.h {
            let from = (row * p.w * 4) as usize;
            let to = (((py + row) * sw + px) * 4) as usize;
            rgba[to..to + (p.w * 4) as usize].copy_from_slice(&p.pixels[from..from + (p.w * 4) as usize]);
        }
    }
    let mut frames = serde_json::Map::new();
    let mut clip_list: Vec<Value> = clips
        .iter()
        .map(|c| json!({ "name": c.name, "fps": fps, "frames": [] }))
        .collect();
    for (cell, &n) in cells.iter().zip(&which) {
        let p = &pictures[n];
        let (px, py) = spots[n];
        let name = format!("{}_{:04}", clips[cell.clip].name, cell.frame);
        frames.insert(
            name.clone(),
            json!({
                "frame": { "x": px, "y": py, "w": p.w, "h": p.h },
                "rotated": false,
                "trimmed": p.w != fw || p.h != fh,
                "spriteSourceSize": { "x": p.x, "y": p.y, "w": p.w, "h": p.h },
                "sourceSize": { "w": fw, "h": fh },
            }),
        );
        if let Some(list) = clip_list[cell.clip]["frames"].as_array_mut() {
            list.push(json!(name));
        }
    }
    let atlas = json!({
        "frames": frames,
        "clips": clip_list,
        "meta": {
            "app": "Fanimate",
            "image": image_name,
            "format": "RGBA8888",
            "size": { "w": sw, "h": sh },
            "scale": "1",
        },
    });
    Ok(Sheet {
        width: sw,
        height: sh,
        rgba,
        atlas,
        report: Report::default(),
        shared,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(clip: usize, frame: i32, w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Cell {
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend(f(x, y));
            }
        }
        Cell { clip, frame, rgba }
    }

    fn clip(name: &str, start: i32, end: i32) -> Clip {
        Clip {
            name: name.into(),
            start,
            end,
        }
    }

    #[test]
    fn clips_are_parsed_from_text() {
        let c = parse_clips("walk=1-24, idle=25-48,pose=3").unwrap();
        assert_eq!(c, [clip("walk", 1, 24), clip("idle", 25, 48), clip("pose", 3, 3)]);
        for bad in ["", "walk", "=1-2", "walk=a-b", "walk=5-1", "a=1-2,a=3-4"] {
            assert!(parse_clips(bad).is_err(), "{bad}");
        }
        assert!(parse_clips("neg=-3-2").is_err(), "frames are not negative here");
    }

    #[test]
    fn the_empty_margin_is_found() {
        let c = cell(0, 1, 8, 8, |x, y| {
            if (2..5).contains(&x) && (3..4).contains(&y) {
                [9, 9, 9, 255]
            } else {
                [0; 4]
            }
        });
        assert_eq!(trim_box(&c.rgba, 8, 8), (2, 3, 3, 1));
        let empty = cell(0, 1, 8, 8, |_, _| [0; 4]);
        assert_eq!(trim_box(&empty.rgba, 8, 8), (0, 0, 1, 1));
    }

    #[test]
    fn rectangles_pack_in_shelves_without_overlapping() {
        let sizes = [(30, 20), (30, 20), (30, 10), (60, 15), (10, 10)];
        let (at, (w, h)) = pack(&sizes, 70).unwrap();
        assert!(w <= 70);
        for (i, a) in at.iter().enumerate() {
            assert!(a.0 + sizes[i].0 <= w && a.1 + sizes[i].1 <= h, "inside the sheet");
            for (j, b) in at.iter().enumerate().skip(i + 1) {
                let apart = a.0 + sizes[i].0 <= b.0
                    || b.0 + sizes[j].0 <= a.0
                    || a.1 + sizes[i].1 <= b.1
                    || b.1 + sizes[j].1 <= a.1;
                assert!(apart, "{i} and {j} overlap");
            }
        }
        assert!(pack(&[(80, 10)], 70).is_none(), "too wide for any shelf");
        assert!(pack(&[(100, 6000), (100, 6000)], 100).is_none(), "too tall");
    }

    #[test]
    fn a_sheet_holds_the_trimmed_pictures_and_an_atlas_that_finds_them() {
        let dot = |x0: u32| {
            move |x: u32, y: u32| {
                if x == x0 && y == 2 {
                    [200, 0, 0, 255]
                } else {
                    [0; 4]
                }
            }
        };
        let cells = [
            cell(0, 1, 6, 6, dot(1)),
            cell(0, 2, 6, 6, dot(4)),
            cell(1, 5, 6, 6, dot(1)),
        ];
        let clips = [clip("a", 1, 2), clip("b", 5, 5)];
        let opts = Options {
            width: 6,
            height: 6,
            padding: 1,
            max_width: 64,
            trim: true,
        };
        let sheet = build(&clips, &cells, 12.0, &opts, "s.png").unwrap();
        let frames = sheet.atlas["frames"].as_object().unwrap();
        assert_eq!(frames.len(), 3);
        // every frame trims to the one red pixel; its offset is where it sat
        assert_eq!(
            frames["a_0001"]["spriteSourceSize"],
            json!({ "x": 1, "y": 2, "w": 1, "h": 1 })
        );
        assert_eq!(
            frames["a_0002"]["spriteSourceSize"],
            json!({ "x": 4, "y": 2, "w": 1, "h": 1 })
        );
        assert_eq!(frames["a_0001"]["sourceSize"], json!({ "w": 6, "h": 6 }));
        assert_eq!(frames["a_0001"]["trimmed"], true);
        // the third frame equals the first, so it shares its place
        assert_eq!(frames["b_0005"]["frame"], frames["a_0001"]["frame"]);
        assert_eq!(sheet.shared, 1);
        assert!(sheet.report.is_empty());
        // the pixel is on the sheet where the atlas says
        let f = &frames["a_0002"]["frame"];
        let (x, y) = (f["x"].as_u64().unwrap() as u32, f["y"].as_u64().unwrap() as u32);
        assert_eq!(
            &sheet.rgba[((y * sheet.width + x) * 4) as usize..][..4],
            [200, 0, 0, 255]
        );
        assert_eq!(sheet.atlas["meta"]["image"], "s.png");
        assert_eq!(sheet.atlas["clips"][0]["frames"], json!(["a_0001", "a_0002"]));
        assert_eq!(sheet.atlas["clips"][1]["frames"], json!(["b_0005"]));
        assert_eq!(sheet.atlas["clips"][0]["fps"], 12.0);
    }

    #[test]
    fn without_trimming_every_frame_is_whole_and_a_wrong_size_is_refused() {
        let cells = [cell(0, 1, 4, 4, |x, _| [x as u8 * 50, 0, 0, 255])];
        let opts = Options {
            width: 4,
            height: 4,
            padding: 0,
            max_width: 16,
            trim: false,
        };
        let sheet = build(&[clip("a", 1, 1)], &cells, 24.0, &opts, "s.png").unwrap();
        assert_eq!((sheet.width, sheet.height), (4, 4));
        assert_eq!(sheet.atlas["frames"]["a_0001"]["trimmed"], false);
        assert_eq!(sheet.rgba, cells[0].rgba);
        let wrong = Options { width: 5, ..opts };
        assert!(build(&[clip("a", 1, 1)], &cells, 24.0, &wrong, "s.png").is_err());
    }

    #[test]
    fn a_rendered_clip_is_trimmed_to_what_is_drawn_and_a_held_pose_is_stored_once() {
        use fanimate_core::{Color, Interpolation, LayerKind, Path, Shape, Style, Vec2};
        if Gpu::headless().is_err() {
            eprintln!("no graphics adapter; skipped");
            return;
        }
        let mut doc = Document::new("t", 64.0, 64.0, 12.0);
        doc.background = Color::TRANSPARENT;
        let square = Shape::new(
            "sq",
            Path::rect(Vec2::new(8.0, 8.0), Vec2::new(16.0, 16.0), 0.0),
            Style::fill(Color::rgb(1.0, 0.0, 0.0)),
        );
        let mut layer = doc.new_layer("art", LayerKind::Vector { shapes: vec![square] });
        // still on frames 1 to 3, then it jumps 20 pixels right and stays
        layer.transform.position = layer
            .transform
            .position
            .clone()
            .with_key(1, Vec2::ZERO, Interpolation::Step)
            .with_key(4, Vec2::new(20.0, 0.0), Interpolation::Step);
        doc.layers.push(layer);
        let opts = Options {
            width: 64,
            height: 64,
            padding: 0,
            max_width: 256,
            trim: true,
        };
        let sheet = render(&doc, &[clip("hop", 1, 6)], &opts, "hop.png").unwrap();
        let frames = sheet.atlas["frames"].as_object().unwrap();
        assert_eq!(frames.len(), 6);
        assert_eq!(
            frames["hop_0001"]["frame"], frames["hop_0003"]["frame"],
            "held frames share"
        );
        assert_eq!(frames["hop_0004"]["frame"], frames["hop_0006"]["frame"]);
        assert_ne!(frames["hop_0001"]["frame"], frames["hop_0004"]["frame"]);
        // 16 by 16 pixels, trimmed from 64 by 64, offset where the square was drawn
        let f = &frames["hop_0001"];
        assert_eq!(
            (f["frame"]["w"].as_u64(), f["frame"]["h"].as_u64()),
            (Some(16), Some(16))
        );
        assert_eq!(f["spriteSourceSize"]["x"], 8);
        assert_eq!(frames["hop_0004"]["spriteSourceSize"]["x"], 28);
        assert_eq!(sheet.shared, 4);
        assert_eq!((sheet.width, sheet.height), (32, 16), "two pictures side by side");
    }

    #[test]
    fn frames_that_do_not_fit_are_refused_with_a_message() {
        let cells = [cell(0, 1, 40, 4, |_, _| [1, 1, 1, 255])];
        let opts = Options {
            width: 40,
            height: 4,
            padding: 0,
            max_width: 16,
            trim: false,
        };
        let e = build(&[clip("a", 1, 1)], &cells, 24.0, &opts, "s.png").unwrap_err();
        assert!(e.contains("do not fit"), "{e}");
    }
}
