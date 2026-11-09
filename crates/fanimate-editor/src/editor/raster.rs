//! Painting pixels: New Raster Layer, and the Brush and Eraser tools on it.
//!
//! A raster layer is a group marked with `Layer::raster` whose children are
//! one image layer per painted tile (see `fanimate_core::raster`). With one
//! selected, the same Brush (Freehand) and Eraser tools that draw and cut
//! vector shapes paint and rub out pixels, so there is no tool to switch to:
//! the tools follow the layer type, as everywhere in Fanimate. The brush maths
//! is in the core crate; this turns a gesture into dabs, keeps the tiles of
//! the stroke in memory while it lasts, and writes each changed tile back as a
//! PNG in the project's media store as it goes, so the canvas shows the paint
//! as it is laid. The whole gesture is one undo step, recorded when the first
//! pixel changes (a stroke that paints nothing, such as an eraser over empty
//! ground, adds none).

use std::io::Cursor;

use fanimate_core::raster::{self, Brush, Key, Painting, TILE, TILE_BYTES};
use fanimate_core::{Affine2, Color, Image, LayerId, LayerKind, MediaFile, Vec2};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};

use super::{Editor, StatusKind, labelled};

/// The most points one scripted stroke may have.
pub const MAX_PAINT_POINTS: usize = 20_000;

/// A gesture in progress.
pub(super) struct RasterStroke {
    layer: LayerId,
    painting: Painting,
    stroke: raster::Stroke,
    brush: Brush,
    /// Document units to layer units, for the brush's size.
    scale: f32,
    /// Document space to the layer's space.
    to_layer: Affine2,
    erase: bool,
    /// Whether the undo step has been recorded yet.
    recorded: bool,
    /// The tiles written so far.
    written: std::collections::BTreeSet<Key>,
}

impl RasterStroke {
    pub(super) fn erasing(&self) -> bool {
        self.erase
    }
}

/// Decode a tile's PNG to RGBA8, or a blank tile when it cannot be read.
fn decode(bytes: &[u8]) -> Vec<u8> {
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            if rgba.width() == TILE && rgba.height() == TILE {
                return rgba.into_raw();
            }
            // a tile of another size is placed at the corner, never read past its end
            let mut out = vec![0u8; TILE_BYTES];
            for y in 0..rgba.height().min(TILE) {
                for x in 0..rgba.width().min(TILE) {
                    let src = rgba.get_pixel(x, y).0;
                    let i = ((y * TILE + x) * 4) as usize;
                    out[i..i + 4].copy_from_slice(&src);
                }
            }
            out
        }
        Err(_) => vec![0; TILE_BYTES],
    }
}

/// A tile's pixels as the bytes of a PNG.
fn encode(pixels: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    PngEncoder::new_with_quality(Cursor::new(&mut out), CompressionType::Fast, FilterType::NoFilter)
        .write_image(pixels, TILE, TILE, ExtendedColorType::Rgba8)
        .ok()?;
    Some(out)
}

fn rgb8(c: Color) -> [u8; 3] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [q(c.r), q(c.g), q(c.b)]
}

impl Editor {
    /// The raster layer `id` is, or is a tile of.
    pub fn raster_group_of(&self, id: LayerId) -> Option<LayerId> {
        let layer = self.doc.find(id)?;
        if layer.raster.is_some() {
            return Some(id);
        }
        let (parent, _) = self.doc.locate(id)?;
        let parent = parent?;
        self.doc.find(parent)?.raster.is_some().then_some(parent)
    }

    /// The raster layer the brush and eraser would paint on: the one
    /// selected layer, when it is one (or one of its tiles), shown and not locked.
    pub(super) fn raster_target(&self) -> Option<LayerId> {
        let group = self.raster_group_of(self.primary()?)?;
        (self.shown(group) && !self.is_locked(group)).then_some(group)
    }

    /// New Raster Layer: an empty layer to paint pixels on.
    pub(super) fn new_raster_layer(&mut self) {
        self.checkpoint("New raster layer");
        let name = self.unused_name("Paint");
        let mut layer = labelled(self.doc.new_layer(name, LayerKind::Group { children: vec![] }));
        layer.raster = Some(raster::RasterInfo::default());
        let id = layer.id;
        let (parent, index) = self.insertion_point();
        self.doc.insert(parent, index, layer);
        self.selection = vec![id];
        self.anchor = Some(id);
        self.rename_request = Some(id);
    }

    /// The pixels of tile `key` of raster layer `group`: what is painted, or blank.
    fn tile_pixels(&self, group: LayerId, key: Key) -> Vec<u8> {
        let name = raster::tile_name(key);
        let bytes = self.doc.find(group).and_then(|g| match &g.kind {
            LayerKind::Group { children } => children.iter().find(|c| c.name == name),
            _ => None,
        });
        let media = bytes.and_then(|c| match &c.kind {
            LayerKind::Image(img) => self.doc.media.get(&img.media),
            _ => None,
        });
        media.map_or_else(|| vec![0; TILE_BYTES], |m| decode(&m.bytes))
    }

    /// Whether any layer still shows the picture `name`.
    fn media_in_use(&self, name: &str) -> bool {
        let mut used = false;
        self.doc.walk(|l| {
            if matches!(&l.kind, LayerKind::Image(i) if i.media == name) {
                used = true;
            }
        });
        used
    }

    /// Begin painting (or, with `erase`, rubbing out) on raster layer `layer`.
    pub(super) fn begin_raster_stroke(
        &mut self,
        layer: LayerId,
        at: Vec2,
        pressure: f32,
        erase: bool,
    ) -> bool {
        let to_layer = self.world(layer).inverse();
        if !to_layer.is_finite() {
            self.set_status(
                StatusKind::Warning,
                "That layer is scaled to zero, so it can't be painted on.",
            );
            return false;
        }
        let scale = to_layer.matrix2.determinant().abs().sqrt();
        let d = &self.draw;
        let brush = Brush {
            color: rgb8(d.line),
            size: if erase { d.eraser } else { d.width },
            hardness: if erase { 1.0 } else { d.hardness },
            flow: if erase { 1.0 } else { d.line.a * d.brush_flow },
            erase,
            spacing: d.brush_spacing,
        };
        self.start_raster_stroke(layer, brush, scale, to_layer);
        self.continue_raster_stroke(at, pressure);
        true
    }

    fn start_raster_stroke(&mut self, layer: LayerId, brush: Brush, scale: f32, to_layer: Affine2) {
        self.raster_stroke = Some(RasterStroke {
            layer,
            painting: Painting::default(),
            stroke: raster::Stroke::default(),
            erase: brush.erase,
            brush,
            scale,
            to_layer,
            recorded: false,
            written: Default::default(),
        });
    }

    /// Paint `points` (document space) on raster layer `layer` with `brush`, as one
    /// undo step; the script and plugin form of the Brush and Eraser tools. Returns
    /// how many tiles the paint changed. Nothing changes when `layer` is not a raster
    /// layer (or a tile of one), is locked, or no pixel would change.
    pub fn paint_raster(&mut self, layer: LayerId, points: &[Vec2], brush: Brush) -> Result<usize, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to paint.".to_owned());
        }
        let Some(group) = self.raster_group_of(layer) else {
            return Err("That is not a raster layer: add one with add_raster_layer".to_owned());
        };
        if self.is_locked(group) {
            return Err("The layer is locked".to_owned());
        }
        if points.is_empty() || points.len() > MAX_PAINT_POINTS {
            return Err(format!("A stroke has 1 to {MAX_PAINT_POINTS} points"));
        }
        let to_layer = self.world(group).inverse();
        if !to_layer.is_finite() {
            return Err("That layer is scaled to zero, so it can't be painted on.".to_owned());
        }
        let scale = to_layer.matrix2.determinant().abs().sqrt();
        self.start_raster_stroke(group, brush.sane(), scale, to_layer);
        for p in points {
            self.continue_raster_stroke(*p, 1.0);
        }
        self.flush_paint();
        let written = self.raster_stroke.as_ref().map_or(0, |s| s.written.len());
        self.finish_raster_stroke();
        Ok(written)
    }

    /// Paint Bucket on a raster layer: fill the pixels joined to the point `at`
    /// (document space) that look like the one there (within `tolerance`, 0 to 1)
    /// with `color`, spreading only over the page, as one undo step. The edge
    /// of a soft line is covered behind it (see `Painting::flood_fill`). Returns
    /// how many pixels changed; nothing changes, and no undo step is made, when
    /// none would (the point is off the page, or already that colour).
    pub fn fill_raster(
        &mut self,
        layer: LayerId,
        at: Vec2,
        color: Color,
        tolerance: f32,
    ) -> Result<usize, String> {
        self.fill_raster_closing(layer, at, color, tolerance, 0)
    }

    /// [`fill_raster`](Self::fill_raster) that does not leak through gaps in a
    /// line up to about twice `gap` pixels wide (`gap` is at most 8; see
    /// `Painting::flood_fill_closing`).
    pub fn fill_raster_closing(
        &mut self,
        layer: LayerId,
        at: Vec2,
        color: Color,
        tolerance: f32,
        gap: u32,
    ) -> Result<usize, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to fill.".to_owned());
        }
        let Some(group) = self.raster_group_of(layer) else {
            return Err("That is not a raster layer: add one with add_raster_layer".to_owned());
        };
        if self.is_locked(group) {
            return Err("The layer is locked".to_owned());
        }
        let to_layer = self.world(group).inverse();
        if !to_layer.is_finite() {
            return Err("That layer is scaled to zero, so it can't be filled.".to_owned());
        }
        // the page as the rectangle (in layer pixels) around its four corners
        let (w, h) = (self.doc.width, self.doc.height);
        let corners = [Vec2::ZERO, Vec2::new(w, 0.0), Vec2::new(0.0, h), Vec2::new(w, h)]
            .map(|c| to_layer.transform_point2(c));
        let lo = corners.iter().fold(Vec2::splat(f32::INFINITY), |m, c| m.min(*c));
        let hi = corners
            .iter()
            .fold(Vec2::splat(f32::NEG_INFINITY), |m, c| m.max(*c));
        let seed = to_layer.transform_point2(at);
        if ![lo.x, lo.y, hi.x, hi.y, seed.x, seed.y]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err("That point is not on the page.".to_owned());
        }
        let bounds = (
            lo.x.floor() as i64,
            lo.y.floor() as i64,
            hi.x.ceil() as i64,
            hi.y.ceil() as i64,
        );
        let mut painting = Painting::default();
        let changed = {
            let view = &*self;
            let mut load = |key: Key| view.tile_pixels(group, key);
            painting.flood_fill_closing(
                (seed.x.floor() as i64, seed.y.floor() as i64),
                bounds,
                rgb8(color),
                tolerance,
                1,
                gap,
                &mut load,
            )
        };
        if changed == 0 {
            return Ok(0);
        }
        self.checkpoint("Fill");
        let dirty: Vec<Key> = painting.dirty().collect();
        for key in dirty {
            if let Some(pixels) = painting.tile(key) {
                self.write_tile(group, key, pixels);
            }
        }
        Ok(changed)
    }

    /// Eyedropper on a raster layer: take the colour of the painted pixel under `at` (document
    /// space) as the line colour, and as the fill colour when fill is on. Changes no document, so
    /// there is no undo step. Returns false (with a status message) when nothing is painted there.
    pub fn pick_raster_colour(&mut self, layer: LayerId, at: Vec2) -> bool {
        let Some(group) = self.raster_group_of(layer) else {
            return false;
        };
        let to_layer = self.world(group).inverse();
        if !to_layer.is_finite() {
            return false;
        }
        let p = to_layer.transform_point2(at);
        if !(p.x.is_finite() && p.y.is_finite()) {
            return false;
        }
        let (key, x, y) = raster::locate(p);
        let pixels = self.tile_pixels(group, key);
        let i = ((y * TILE + x) * 4) as usize;
        let Some(px) = pixels.get(i..i + 4) else {
            return false;
        };
        if px[3] == 0 {
            self.set_status(StatusKind::Info, "Nothing is painted there");
            return false;
        }
        let colour = Color::rgb(
            f32::from(px[0]) / 255.0,
            f32::from(px[1]) / 255.0,
            f32::from(px[2]) / 255.0,
        );
        self.draw.line = colour;
        if self.draw.fill.is_some() {
            self.draw.fill = Some(colour);
        }
        self.set_status(
            StatusKind::Info,
            format!("Picked #{:02X}{:02X}{:02X}", px[0], px[1], px[2]),
        );
        true
    }

    /// Move the brush to `at` (document space), laying paint along the way.
    pub(super) fn continue_raster_stroke(&mut self, at: Vec2, pressure: f32) {
        let Some(mut s) = self.raster_stroke.take() else {
            return;
        };
        let pressure = if self.draw.pressure && !s.erase {
            pressure
        } else {
            1.0
        };
        let to = s.to_layer.transform_point2(at);
        let mut brush = s.brush;
        brush.size *= s.scale;
        {
            let doc_view = &*self;
            let group = s.layer;
            let mut load = |key: Key| doc_view.tile_pixels(group, key);
            s.stroke.line(&mut s.painting, &brush, to, pressure, &mut load);
        }
        self.raster_stroke = Some(s);
    }

    /// Write the tiles the paint reached back into the project, so the canvas shows it. The
    /// canvas calls this once a frame, after the pointer moves of that frame, not for each move:
    /// encoding a tile is the costly part of painting. Also done when the stroke ends.
    pub fn flush_paint(&mut self) {
        let Some(mut s) = self.raster_stroke.take() else {
            return;
        };
        let dirty: Vec<Key> = s.painting.dirty().collect();
        if !dirty.is_empty() && !s.recorded {
            self.checkpoint(if s.erase { "Erase pixels" } else { "Paint" });
            s.recorded = true;
        }
        for key in dirty {
            let Some(pixels) = s.painting.tile(key).map(<[u8]>::to_vec) else {
                continue;
            };
            self.write_tile(s.layer, key, &pixels);
            s.written.insert(key);
        }
        s.painting.forget_dirty();
        self.raster_stroke = Some(s);
    }

    /// Put `pixels` in as tile `key` of `group`: a fresh PNG named for its
    /// content, the tile's image layer pointed at it (made when the tile is
    /// new, removed when the tile is empty), and the old picture dropped from
    /// the store unless something else still shows it.
    fn write_tile(&mut self, group: LayerId, key: Key, pixels: &[u8]) {
        let name = raster::tile_name(key);
        let empty = Painting::is_empty(pixels);
        let existing = self.doc.find(group).and_then(|g| match &g.kind {
            LayerKind::Group { children } => children.iter().position(|c| c.name == name),
            _ => None,
        });
        let old = existing.and_then(|i| match &self.doc.find(group)?.kind {
            LayerKind::Group { children } => match &children[i].kind {
                LayerKind::Image(img) => Some(img.media.clone()),
                _ => None,
            },
            _ => None,
        });
        if empty {
            // an erased tile goes away entirely
            if let (Some(i), Some(g)) = (existing, self.doc.find_mut(group))
                && let LayerKind::Group { children } = &mut g.kind
            {
                children.remove(i);
            }
        } else {
            let Some(png) = encode(pixels) else {
                return;
            };
            let media = MediaFile::name_for(&png, "png");
            self.doc
                .media
                .insert(media.clone(), std::sync::Arc::new(MediaFile { bytes: png }));
            match existing {
                Some(i) => {
                    if let Some(g) = self.doc.find_mut(group)
                        && let LayerKind::Group { children } = &mut g.kind
                        && let LayerKind::Image(img) = &mut children[i].kind
                    {
                        img.media = media;
                    }
                }
                None => {
                    let img = Image::new(media, TILE as f32, TILE as f32);
                    let mut tile = self.doc.new_layer(name, LayerKind::Image(img));
                    tile.transform.position.base = raster::tile_origin(key);
                    if let Some(g) = self.doc.find_mut(group)
                        && let LayerKind::Group { children } = &mut g.kind
                    {
                        children.push(tile);
                    }
                }
            }
        }
        if let Some(old) = old
            && !self.media_in_use(&old)
        {
            self.doc.media.remove(&old);
        }
        self.dirty = true;
    }

    /// Lift the brush: the gesture is done. Its tiles are already in the project.
    pub(super) fn finish_raster_stroke(&mut self) {
        self.flush_paint();
        self.raster_stroke = None;
    }
}

#[cfg(test)]
#[path = "raster_tests.rs"]
mod tests;
