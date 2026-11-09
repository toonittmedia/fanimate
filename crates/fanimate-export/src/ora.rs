//! OpenRaster (`.ora`) export: the pictures of a project as layers for
//! Krita, MyPaint, GIMP and other painting programs.
//!
//! What goes in is what a painting program can hold: **raster layers**
//! (each as one picture trimmed to its paint) and **picture layers**
//! (image layers at their own size), in the order they are stacked, with
//! each one's place, opacity and visibility as they are on the frame asked
//! for, groups flattened into the list. Everything else (vector shapes,
//! text, bones, frame-by-frame drawings, sounds, layers that are turned or
//! scaled, pictures bent by bones or stretched to another size, blend and
//! mask settings) is counted in the [`Report`], never dropped quietly; use
//! a normal export for those. Pictures are read straight from the project:
//! nothing is rendered, so this needs no graphics card and the pixels are
//! exactly the ones that were painted. The file also holds the flattened
//! `mergedimage.png`, so a file manager or viewer can show it.
//!
//! The archive follows the OpenRaster 0.0.3 layout: `mimetype` first, then
//! `stack.xml` (top layer first), `data/*.png` and `mergedimage.png`.

use std::io::Cursor;

use fanimate_core::raster::{self, TILE};
use fanimate_core::{Document, Frame, Layer, LayerKind, Vec2};
use image::{ImageEncoder, RgbaImage};

use crate::Report;

/// The most layers one file holds.
pub const MAX_LAYERS: usize = 500;
/// The most pixels in one layer or in the canvas.
pub const MAX_PIXELS: u64 = 100_000_000;
/// The widest or tallest canvas.
pub const MAX_SIDE: u32 = 16_384;

const MIMETYPE: &str = "image/openraster";

/// One layer on its way into the file: its picture and where it goes.
struct Piece {
    name: String,
    pixels: RgbaImage,
    x: i64,
    y: i64,
    opacity: f32,
    visible: bool,
}

/// What a walk down the layers carries with it.
#[derive(Clone, Copy)]
struct Inherited {
    offset: Vec2,
    opacity: f32,
    visible: bool,
}

/// The project's pictures as an OpenRaster file, as they are on `frame`,
/// and what was left out.
pub fn write(doc: &Document, frame: Frame) -> Result<(Vec<u8>, Report), String> {
    let (w, h) = (canvas_side(doc.width)?, canvas_side(doc.height)?);
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("The page is too large for an OpenRaster file (over 100 million pixels).".to_owned());
    }
    let mut report = Report::default();
    let mut pieces = Vec::new();
    let top = Inherited {
        offset: Vec2::ZERO,
        opacity: 1.0,
        visible: true,
    };
    collect(&doc.layers, doc, frame as f32, top, &mut pieces, &mut report)?;
    if pieces.is_empty() {
        return Err("There are no pictures to export: OpenRaster holds raster layers and picture layers. Add a raster layer, or use another export.".to_owned());
    }

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut stack = String::new();
    // the stack lists the top layer first; Fanimate's list runs bottom to top
    for (i, piece) in pieces.iter().enumerate().rev() {
        let src = format!("data/layer{i}.png");
        stack.push_str(&format!(
            "    <layer name=\"{}\" src=\"{src}\" x=\"{}\" y=\"{}\" opacity=\"{:.3}\" visibility=\"{}\" composite-op=\"svg:src-over\"/>\n",
            escape(&piece.name),
            piece.x,
            piece.y,
            piece.opacity,
            if piece.visible { "visible" } else { "hidden" }
        ));
        files.push((src, png(&piece.pixels)?));
    }
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<image version=\"0.0.3\" w=\"{w}\" h=\"{h}\">\n  <stack>\n{stack}  </stack>\n</image>\n"
    );
    files.push(("mergedimage.png".to_owned(), png(&merge(&pieces, w, h))?));

    let mut entries: Vec<(&str, &[u8])> =
        vec![("mimetype", MIMETYPE.as_bytes()), ("stack.xml", xml.as_bytes())];
    entries.extend(files.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
    let archive = fanimate_fnm::zip::write(&entries)?;
    Ok((archive, report))
}

fn canvas_side(v: f32) -> Result<u32, String> {
    if !v.is_finite() || v < 1.0 {
        return Err("The page has no size.".to_owned());
    }
    let side = v.round() as u32;
    if side > MAX_SIDE {
        return Err(format!("The page is wider or taller than {MAX_SIDE} pixels."));
    }
    Ok(side)
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 cannot hold most control characters at all
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(Cursor::new(&mut out))
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("a picture could not be written: {e}"))?;
    Ok(out)
}

fn collect(
    layers: &[Layer],
    doc: &Document,
    frame: f32,
    parent: Inherited,
    out: &mut Vec<Piece>,
    report: &mut Report,
) -> Result<(), String> {
    for layer in layers {
        let t = layer.transform.sample(frame);
        let here = Inherited {
            offset: parent.offset + t.position,
            opacity: parent.opacity * layer.opacity.sample(frame).clamp(0.0, 1.0),
            visible: parent.visible && layer.visible,
        };
        let straight = t.rotation == 0.0 && t.scale == Vec2::ONE;
        let is_picture =
            layer.raster.is_some() || matches!(&layer.kind, LayerKind::Image(_) | LayerKind::Group { .. });
        if !is_picture {
            report.add("layers that are not pictures left out");
            continue;
        }
        if !straight {
            report.add("turned or scaled layers left out");
            continue;
        }
        if !layer.composite.is_plain() {
            report.add("blend and mask settings not carried");
        }
        match &layer.kind {
            LayerKind::Group { children } if layer.raster.is_some() => {
                if let Some(piece) = raster_piece(layer, children, doc, here, report)? {
                    push(out, piece)?;
                }
            }
            LayerKind::Group { children } => collect(children, doc, frame, here, out, report)?,
            LayerKind::Image(img) => {
                if img.bone.is_some() || img.skin.is_some() || img.grid != [1, 1] {
                    report.add("pictures bent by bones left out");
                    continue;
                }
                let Some(file) = doc.media.get(&img.media) else {
                    report.add("pictures with no file left out");
                    continue;
                };
                let Ok(decoded) = image::load_from_memory(&file.bytes) else {
                    report.add("pictures that could not be read left out");
                    continue;
                };
                let rgba = decoded.to_rgba8();
                if u64::from(rgba.width()) * u64::from(rgba.height()) > MAX_PIXELS {
                    report.add("pictures too large left out");
                    continue;
                }
                if (img.width - rgba.width() as f32).abs() > 0.5
                    || (img.height - rgba.height() as f32).abs() > 0.5
                {
                    report.add("pictures not at their own size left out");
                    continue;
                }
                push(
                    out,
                    Piece {
                        name: layer.name.clone(),
                        x: here.offset.x.round() as i64,
                        y: here.offset.y.round() as i64,
                        pixels: rgba,
                        opacity: here.opacity,
                        visible: here.visible,
                    },
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn push(out: &mut Vec<Piece>, piece: Piece) -> Result<(), String> {
    if out.len() >= MAX_LAYERS {
        return Err(format!(
            "An OpenRaster file here holds at most {MAX_LAYERS} layers."
        ));
    }
    out.push(piece);
    Ok(())
}

/// A raster layer's tiles as one picture trimmed to the painted pixels, or
/// `None` when nothing is painted.
fn raster_piece(
    layer: &Layer,
    children: &[Layer],
    doc: &Document,
    here: Inherited,
    report: &mut Report,
) -> Result<Option<Piece>, String> {
    let mut tiles: Vec<(raster::Key, RgbaImage)> = Vec::new();
    for child in children {
        let (Some(key), LayerKind::Image(img)) = (raster::parse_tile_name(&child.name), &child.kind) else {
            report.add("pieces of a raster layer that are not tiles left out");
            continue;
        };
        let Some(file) = doc.media.get(&img.media) else {
            continue;
        };
        let Ok(decoded) = image::load_from_memory(&file.bytes) else {
            report.add("tiles that could not be read left out");
            continue;
        };
        let rgba = decoded.to_rgba8();
        if rgba.pixels().any(|p| p.0[3] != 0) {
            tiles.push((key, rgba));
        }
    }
    let Some(first) = tiles.first().map(|(k, _)| *k) else {
        return Ok(None);
    };
    let (mut lo, mut hi) = (first, first);
    for (k, _) in &tiles {
        lo = (lo.0.min(k.0), lo.1.min(k.1));
        hi = (hi.0.max(k.0), hi.1.max(k.1));
    }
    let tile = i64::from(TILE);
    let (w, h) = (
        (i64::from(hi.0) - i64::from(lo.0) + 1) * tile,
        (i64::from(hi.1) - i64::from(lo.1) + 1) * tile,
    );
    if w * h > MAX_PIXELS as i64 {
        report.add("raster layers too large left out");
        return Ok(None);
    }
    let mut full = RgbaImage::new(w as u32, h as u32);
    for (key, img) in &tiles {
        let (ox, oy) = (
            ((i64::from(key.0) - i64::from(lo.0)) * tile) as u32,
            ((i64::from(key.1) - i64::from(lo.1)) * tile) as u32,
        );
        for (x, y, p) in img.enumerate_pixels().filter(|(x, y, _)| *x < TILE && *y < TILE) {
            full.put_pixel(ox + x, oy + y, *p);
        }
    }
    // trim to the painted pixels, so a stroke in one corner is a small picture
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in full.enumerate_pixels() {
        if p.0[3] != 0 {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    let trimmed = image::imageops::crop_imm(&full, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image();
    Ok(Some(Piece {
        name: layer.name.clone(),
        x: here.offset.x.round() as i64 + i64::from(lo.0) * tile + i64::from(x0),
        y: here.offset.y.round() as i64 + i64::from(lo.1) * tile + i64::from(y0),
        pixels: trimmed,
        opacity: here.opacity,
        visible: here.visible,
    }))
}

/// The visible layers laid over each other (straight alpha, source over),
/// cut to the page.
fn merge(pieces: &[Piece], w: u32, h: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for piece in pieces.iter().filter(|p| p.visible && p.opacity > 0.0) {
        for (sx, sy, src) in piece.pixels.enumerate_pixels() {
            let (x, y) = (piece.x + i64::from(sx), piece.y + i64::from(sy));
            if x < 0 || y < 0 || x >= i64::from(w) || y >= i64::from(h) {
                continue;
            }
            let sa = f32::from(src.0[3]) / 255.0 * piece.opacity;
            if sa <= 0.0 {
                continue;
            }
            let dst = out.get_pixel_mut(x as u32, y as u32);
            let da = f32::from(dst.0[3]) / 255.0;
            let oa = sa + da * (1.0 - sa);
            for c in 0..3 {
                let s = f32::from(src.0[c]);
                let d = f32::from(dst.0[c]);
                dst.0[c] = ((s * sa + d * da * (1.0 - sa)) / oa).round().clamp(0.0, 255.0) as u8;
            }
            dst.0[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{Color, MediaFile, Path, Shape, Style};
    use std::sync::Arc;

    fn png_of(img: &RgbaImage) -> Vec<u8> {
        png(img).unwrap()
    }

    /// A raster layer with painted tiles, as the editor makes them.
    /// A tile's place, the colour of its one painted pixel, and that pixel's place in the tile.
    type Tile = ((i32, i32), [u8; 4], (u32, u32));

    fn painted(doc: &mut Document, name: &str, tiles: &[Tile]) {
        let mut children = Vec::new();
        for (key, colour, at) in tiles {
            let mut img = RgbaImage::new(TILE, TILE);
            img.put_pixel(at.0, at.1, image::Rgba(*colour));
            let bytes = png_of(&img);
            let media_name = MediaFile::name_for(&bytes, "png");
            doc.media
                .insert(media_name.clone(), Arc::new(MediaFile { bytes }));
            let mut child = doc.new_layer(
                raster::tile_name(*key),
                LayerKind::Image(fanimate_core::Image {
                    media: media_name,
                    width: TILE as f32,
                    height: TILE as f32,
                    grid: [1, 1],
                    bone: None,
                    skin: None,
                }),
            );
            child.transform.position.base = raster::tile_origin(*key);
            children.push(child);
        }
        let mut layer = doc.new_layer(name, LayerKind::Group { children });
        layer.raster = Some(raster::RasterInfo::default());
        doc.layers.push(layer);
    }

    fn entries(bytes: &[u8]) -> (Vec<String>, String, RgbaImage) {
        let zip = fanimate_fnm::zip::Archive::read(bytes).unwrap();
        let names: Vec<String> = zip.names().map(str::to_owned).collect();
        let stack = String::from_utf8(zip.get_checked("stack.xml").unwrap().unwrap()).unwrap();
        let merged = image::load_from_memory(&zip.get_checked("mergedimage.png").unwrap().unwrap())
            .unwrap()
            .to_rgba8();
        (names, stack, merged)
    }

    #[test]
    fn a_raster_layer_becomes_one_trimmed_layer_at_its_place() {
        let mut doc = Document::new("p", 600.0, 400.0, 24.0);
        // one pixel in tile (1, 0) at (10, 20): page position 266, 20
        painted(&mut doc, "Paint", &[((1, 0), [255, 0, 0, 255], (10, 20))]);
        let (bytes, report) = write(&doc, 1).unwrap();
        assert!(report.is_empty(), "{}", report.summary());
        let (names, stack, merged) = entries(&bytes);
        assert!(names.contains(&"mimetype".to_owned()));
        assert!(names.contains(&"data/layer0.png".to_owned()));
        assert!(stack.contains("name=\"Paint\""), "{stack}");
        assert!(stack.contains("x=\"266\" y=\"20\""), "{stack}");
        assert!(stack.contains("w=\"600\" h=\"400\""));
        assert_eq!(merged.dimensions(), (600, 400));
        assert_eq!(merged.get_pixel(266, 20).0, [255, 0, 0, 255]);
        assert_eq!(merged.get_pixel(265, 20).0[3], 0);
        // the mimetype is the first entry and stored (method 0), as the format requires
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[8..10], [0, 0]);
        assert_eq!(&bytes[38..54], b"image/openraster");
    }

    #[test]
    fn the_file_reads_back_through_the_importer() {
        let mut doc = Document::new("p", 600.0, 400.0, 24.0);
        painted(&mut doc, "Under", &[((0, 0), [0, 0, 255, 255], (5, 5))]);
        painted(&mut doc, "Over", &[((0, 0), [0, 255, 0, 128], (5, 5))]);
        doc.layers[1].opacity.base = 0.5;
        doc.layers[1].visible = true;
        let (bytes, _) = write(&doc, 1).unwrap();
        let ora = fanimate_import::ora::parse(&bytes).unwrap();
        // the top layer is listed first, as the format says
        let (_, stack, merged) = entries(&bytes);
        assert!(
            stack.find("Over").unwrap() < stack.find("Under").unwrap(),
            "{stack}"
        );
        assert!(stack.contains("opacity=\"0.500\""));
        assert_eq!((ora.width, ora.height, ora.layers), (600, 400, 2));
        let names: Vec<(&str, f32)> = ora
            .nodes
            .iter()
            .filter_map(|n| match n {
                fanimate_import::ora::OraNode::Layer(l) => Some((l.name.as_str(), l.opacity)),
                _ => None,
            })
            .collect();
        assert_eq!(names, [("Under", 1.0), ("Over", 0.5)], "bottom first");
        // green at 128/255 * 0.5 over opaque blue: mostly blue
        let p = merged.get_pixel(5, 5).0;
        assert_eq!(p[3], 255);
        assert!(p[2] > 180 && p[1] > 40 && p[1] < 90, "{p:?}");
    }

    #[test]
    fn what_a_painting_program_cannot_hold_is_counted() {
        let mut doc = Document::new("p", 100.0, 100.0, 24.0);
        painted(&mut doc, "Paint", &[((0, 0), [1, 2, 3, 255], (0, 0))]);
        let vector = doc.new_layer(
            "Shapes",
            LayerKind::vector(vec![Shape::new(
                "r",
                Path::rect(Vec2::ZERO, Vec2::new(10.0, 10.0), 0.0),
                Style::fill(Color::BLACK),
            )]),
        );
        doc.layers.push(vector);
        let mut turned = doc.new_layer("Turned", LayerKind::Group { children: vec![] });
        turned.transform.rotation.base = 0.5;
        doc.layers.push(turned);
        let (_, report) = write(&doc, 1).unwrap();
        assert_eq!(report.count("layers that are not pictures left out"), 1);
        assert_eq!(report.count("turned or scaled layers left out"), 1);
    }

    #[test]
    fn nothing_to_write_and_hostile_pages_are_errors_not_panics() {
        let doc = Document::new("p", 100.0, 100.0, 24.0);
        assert!(write(&doc, 1).unwrap_err().contains("no pictures"));
        for (w, h) in [(0.0, 10.0), (f32::NAN, 10.0), (20_000.0, 10.0), (1.0e9, 1.0e9)] {
            let mut doc = Document::new("p", w, h, 24.0);
            painted(&mut doc, "Paint", &[((0, 0), [1, 1, 1, 255], (0, 0))]);
            assert!(write(&doc, 1).is_err(), "{w} x {h}");
        }
        // a damaged tile (not a picture) is counted, and an empty painting writes nothing
        let mut doc = Document::new("p", 100.0, 100.0, 24.0);
        painted(&mut doc, "Paint", &[((0, 0), [0, 0, 0, 0], (0, 0))]);
        assert!(write(&doc, 1).is_err());
        painted(&mut doc, "Second", &[((0, 0), [9, 9, 9, 255], (1, 1))]);
        for media in doc.media.values_mut() {
            *media = Arc::new(MediaFile {
                bytes: b"not a picture".to_vec(),
            });
        }
        assert!(write(&doc, 1).is_err());
    }

    #[test]
    fn names_are_escaped_in_the_stack() {
        let mut doc = Document::new("p", 100.0, 100.0, 24.0);
        painted(
            &mut doc,
            "a <b> & \"c\"\u{1}",
            &[((0, 0), [1, 2, 3, 255], (0, 0))],
        );
        let (bytes, _) = write(&doc, 1).unwrap();
        let (_, stack, _) = entries(&bytes);
        assert!(stack.contains("a &lt;b&gt; &amp; &quot;c&quot;\""), "{stack}");
    }
}
