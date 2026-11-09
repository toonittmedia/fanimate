//! Raster layers: pixel painting on a layer that has no edges, kept as
//! square tiles.
//!
//! A raster layer is an ordinary group layer marked with [`RasterInfo`]
//! whose children are image layers, one per tile that has paint on it. A
//! tile is a 256 by 256 PNG in the project's media store, named for its
//! place (the child layer is called `x,y` and sits at `x * 256, y * 256`),
//! so a reader that knows nothing about raster layers still shows the
//! painting, as a group of pictures, and a tile that was never painted
//! costs nothing. This module is the pure part: the brush maths on tiles
//! held as straight-alpha RGBA8 (the PNG's own form), with no file or
//! window code, so the same dabs come out on every computer and in tests.
//! The editor turns tiles into PNGs and back (`fanimate_editor`).
//!
//! The brush is a round dab with a hardness (the size of its solid middle),
//! placed along the stroke at a spacing of a quarter of its size, each dab
//! laid over the pixels with `flow` as its strength; an eraser lowers
//! alpha instead. Blending is done in the pixels' own (sRGB) values, as
//! simple paint programs do, so a soft edge is as dark as the colour is.

use std::collections::{BTreeMap, BTreeSet};

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// The width and height of a tile, in pixels (and document units).
pub const TILE: u32 = 256;
/// Bytes in a tile: RGBA8.
pub const TILE_BYTES: usize = (TILE * TILE * 4) as usize;
/// The most tiles one raster layer may hold (a 16384 by 16384 area at most
/// if all were used), so a damaged project cannot ask for unbounded memory.
pub const MAX_TILES: usize = 4096;
/// The farthest a tile may be from the origin, in tiles, either way.
pub const MAX_TILE_COORD: i32 = 4096;

/// A tile's place: column and row, in tiles.
pub type Key = (i32, i32);

/// What marks a group layer as a raster layer. The tile size is written so
/// a later Fanimate that changes it can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RasterInfo {
    pub tile: u32,
}

impl Default for RasterInfo {
    fn default() -> Self {
        Self { tile: TILE }
    }
}

/// The name of the child layer that holds tile `key`.
pub fn tile_name(key: Key) -> String {
    format!("{},{}", key.0, key.1)
}

/// The place a child layer's name says it holds, if it is one, within limits.
pub fn parse_tile_name(name: &str) -> Option<Key> {
    let (x, y) = name.split_once(',')?;
    let (x, y): (i32, i32) = (x.trim().parse().ok()?, y.trim().parse().ok()?);
    (x.abs() <= MAX_TILE_COORD && y.abs() <= MAX_TILE_COORD).then_some((x, y))
}

/// Where tile `key` has its top-left corner, in layer space.
pub fn tile_origin(key: Key) -> Vec2 {
    Vec2::new(key.0 as f32 * TILE as f32, key.1 as f32 * TILE as f32)
}

/// The tile a layer-space point falls in, and its pixel there.
pub fn locate(p: Vec2) -> (Key, u32, u32) {
    let t = TILE as f32;
    let (cx, cy) = ((p.x / t).floor(), (p.y / t).floor());
    let (x, y) = (p.x - cx * t, p.y - cy * t);
    (
        (cx as i32, cy as i32),
        (x.floor() as i64).clamp(0, i64::from(TILE) - 1) as u32,
        (y.floor() as i64).clamp(0, i64::from(TILE) - 1) as u32,
    )
}

/// How a dab is laid down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    /// Straight sRGB colour of the paint.
    pub color: [u8; 3],
    /// Diameter in pixels, 1 to 2000.
    pub size: f32,
    /// How much of the radius is solid: 0 is all soft, 1 a hard edge.
    pub hardness: f32,
    /// The strength of each dab, 0 to 1.
    pub flow: f32,
    /// Take paint away (lower alpha) instead of adding it.
    pub erase: bool,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            color: [0, 0, 0],
            size: 12.0,
            hardness: 0.8,
            flow: 1.0,
            erase: false,
        }
    }
}

impl Brush {
    /// The settings kept within what the maths holds: finite, in range.
    pub fn sane(self) -> Self {
        let fix =
            |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        Self {
            size: fix(self.size, 1.0, 2000.0, 12.0),
            hardness: fix(self.hardness, 0.0, 1.0, 0.8),
            flow: fix(self.flow, 0.0, 1.0, 1.0),
            ..self
        }
    }
}

/// Tiles being painted on, with the ones the paint reached marked.
#[derive(Default)]
pub struct Painting {
    tiles: BTreeMap<Key, Vec<u8>>,
    dirty: BTreeSet<Key>,
}

impl Painting {
    /// The tiles that were changed, in order.
    pub fn dirty(&self) -> impl Iterator<Item = Key> + '_ {
        self.dirty.iter().copied()
    }

    pub fn forget_dirty(&mut self) {
        self.dirty.clear();
    }

    /// The pixels of tile `key` (RGBA8), if the painting has it.
    pub fn tile(&self, key: Key) -> Option<&[u8]> {
        self.tiles.get(&key).map(Vec::as_slice)
    }

    /// Whether every pixel of a tile is fully transparent: such a tile need
    /// not be kept.
    pub fn is_empty(tile: &[u8]) -> bool {
        tile.iter().skip(3).step_by(4).all(|a| *a == 0)
    }

    /// Lay one dab centred at `at` (layer space). `load` gives the pixels
    /// of a tile the painting does not hold yet (or a transparent tile). A
    /// dab past the limits of the layer is ignored.
    pub fn dab(&mut self, at: Vec2, brush: &Brush, scale: f32, load: &mut dyn FnMut(Key) -> Vec<u8>) {
        let brush = brush.sane();
        if !(at.x.is_finite() && at.y.is_finite()) {
            return;
        }
        let radius = (brush.size * scale.clamp(0.0, 4.0) / 2.0).max(0.5);
        let lim = (MAX_TILE_COORD as f32) * TILE as f32;
        if at.x.abs() > lim || at.y.abs() > lim {
            return;
        }
        let solid = radius * brush.hardness;
        let (x0, y0) = ((at.x - radius).floor() as i64, (at.y - radius).floor() as i64);
        let (x1, y1) = ((at.x + radius).ceil() as i64, (at.y + radius).ceil() as i64);
        let t = i64::from(TILE);
        for ty in y0.div_euclid(t)..=y1.div_euclid(t) {
            for tx in x0.div_euclid(t)..=x1.div_euclid(t) {
                let key = (tx as i32, ty as i32);
                if !self.tiles.contains_key(&key) {
                    if self.tiles.len() >= MAX_TILES {
                        continue;
                    }
                    let mut px = load(key);
                    px.resize(TILE_BYTES, 0);
                    self.tiles.insert(key, px);
                }
                let px = self.tiles.get_mut(&key).expect("inserted above");
                let mut touched = false;
                for y in y0.max(ty * t)..=y1.min(ty * t + t - 1) {
                    for x in x0.max(tx * t)..=x1.min(tx * t + t - 1) {
                        // the pixel's centre against the dab's
                        let (dx, dy) = (x as f32 + 0.5 - at.x, y as f32 + 0.5 - at.y);
                        let d = (dx * dx + dy * dy).sqrt();
                        if d > radius {
                            continue;
                        }
                        let falloff = if d <= solid || radius <= solid {
                            1.0
                        } else {
                            let u = 1.0 - (d - solid) / (radius - solid);
                            u * u * (3.0 - 2.0 * u)
                        };
                        let alpha = falloff * brush.flow;
                        if alpha <= 0.0 {
                            continue;
                        }
                        let i = (((y - ty * t) * t + (x - tx * t)) * 4) as usize;
                        let before = [px[i], px[i + 1], px[i + 2], px[i + 3]];
                        blend(&mut px[i..i + 4], brush.color, alpha, brush.erase);
                        // only a pixel that really changed makes the tile dirty: rubbing out
                        // empty ground, or painting a colour already there, changes nothing
                        touched |= before != [px[i], px[i + 1], px[i + 2], px[i + 3]];
                    }
                }
                if touched {
                    self.dirty.insert(key);
                }
            }
        }
    }
}

/// Lay `color` over one RGBA pixel with strength `alpha`, or take that much
/// alpha away when `erase`.
fn blend(px: &mut [u8], color: [u8; 3], alpha: f32, erase: bool) {
    let da = f32::from(px[3]) / 255.0;
    if erase {
        px[3] = ((da * (1.0 - alpha)) * 255.0).round() as u8;
        return;
    }
    let out_a = alpha + da * (1.0 - alpha);
    if out_a <= 0.0 {
        return;
    }
    for c in 0..3 {
        let (s, d) = (f32::from(color[c]), f32::from(px[c]));
        // premultiplied sum, back to straight
        let v = (s * alpha + d * da * (1.0 - alpha)) / out_a;
        px[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    px[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// A stroke being painted: where the last dab went, so the next one is a
/// spacing further on, however the pointer moves.
#[derive(Default)]
pub struct Stroke {
    last: Option<(Vec2, f32)>,
    /// How far along the line the next dab is due.
    carry: f32,
}

impl Stroke {
    /// Move the brush to `to` with `pressure` (0 to 1) and lay the dabs
    /// between the last position and it. The first call lays one dab.
    pub fn line(
        &mut self,
        painting: &mut Painting,
        brush: &Brush,
        to: Vec2,
        pressure: f32,
        load: &mut dyn FnMut(Key) -> Vec<u8>,
    ) {
        let pressure = if pressure.is_finite() {
            pressure.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let spacing = (brush.sane().size * 0.25).max(1.0);
        let Some((from, from_pressure)) = self.last else {
            painting.dab(to, brush, 0.25 + 0.75 * pressure, load);
            self.last = Some((to, pressure));
            return;
        };
        let length = from.distance(to);
        if !length.is_finite() {
            return;
        }
        // an absurd jump (a damaged pointer) is spread over at most this many dabs
        let mut at = (spacing - self.carry).max(0.0);
        let mut dabs = 0;
        while at <= length && dabs < 4000 {
            let t = if length > 0.0 { at / length } else { 1.0 };
            let p = from_pressure + (pressure - from_pressure) * t;
            painting.dab(from.lerp(to, t), brush, 0.25 + 0.75 * p, load);
            at += spacing;
            dabs += 1;
        }
        self.carry = (self.carry + length) % spacing;
        self.last = Some((to, pressure));
    }
}

/// The most pixels one fill looks at: a 4096 by 4096 area.
pub const MAX_FILL_PIXELS: i64 = 4096 * 4096;

/// The rectangle a fill may spread in, in layer pixels: left and top
/// (included), right and bottom (not included).
pub type FillBounds = (i64, i64, i64, i64);

impl Painting {
    /// Paint Bucket: fill the pixels joined to `seed` that look like the
    /// pixel there (every channel within `tolerance`, 0 to 1, of it; fully
    /// clear pixels all look alike) with the opaque `color`, spreading no
    /// farther than `bounds`. Then `grow` more pixels are covered **behind**
    /// the paint already there, so the soft edge of a drawn line keeps its
    /// look instead of leaving a pale halo between line and fill. Returns how
    /// many pixels changed; nothing is touched when that is none, when the
    /// seed is outside the bounds, or when the bounds are larger than
    /// [`MAX_FILL_PIXELS`] (they are cut to fit first).
    pub fn flood_fill(
        &mut self,
        seed: (i64, i64),
        bounds: FillBounds,
        color: [u8; 3],
        tolerance: f32,
        grow: u32,
        load: &mut dyn FnMut(Key) -> Vec<u8>,
    ) -> usize {
        let lim = i64::from(MAX_TILE_COORD) * i64::from(TILE);
        let (x0, y0) = (bounds.0.max(-lim), bounds.1.max(-lim));
        let (x1, y1) = (bounds.2.min(lim), bounds.3.min(lim));
        if x1 <= x0 || y1 <= y0 || seed.0 < x0 || seed.0 >= x1 || seed.1 < y0 || seed.1 >= y1 {
            return 0;
        }
        let (w, h) = (x1 - x0, y1 - y0);
        if w.saturating_mul(h) > MAX_FILL_PIXELS {
            return 0;
        }
        let (w, h) = (w as usize, h as usize);

        // read the area into one buffer, from the tiles held or the project's
        let mut buf = vec![0u8; w * h * 4];
        let t = i64::from(TILE);
        for ty in y0.div_euclid(t)..=(y1 - 1).div_euclid(t) {
            for tx in x0.div_euclid(t)..=(x1 - 1).div_euclid(t) {
                let key = (tx as i32, ty as i32);
                let tile = match self.tiles.get(&key) {
                    Some(held) => held.clone(),
                    None => {
                        let mut px = load(key);
                        px.resize(TILE_BYTES, 0);
                        px
                    }
                };
                for y in y0.max(ty * t)..y1.min(ty * t + t) {
                    for x in x0.max(tx * t)..x1.min(tx * t + t) {
                        let from = (((y - ty * t) * t + (x - tx * t)) * 4) as usize;
                        let to = (((y - y0) * w as i64 + (x - x0)) * 4) as usize;
                        buf[to..to + 4].copy_from_slice(&tile[from..from + 4]);
                    }
                }
            }
        }

        let at = |x: usize, y: usize| (y * w + x) * 4;
        let s = at((seed.0 - x0) as usize, (seed.1 - y0) as usize);
        let seed_px = [buf[s], buf[s + 1], buf[s + 2], buf[s + 3]];
        let tol = if tolerance.is_finite() {
            (tolerance.clamp(0.0, 1.0) * 255.0).round() as i32
        } else {
            0
        };
        let alike = |p: &[u8]| {
            if seed_px[3] == 0 && p[3] == 0 {
                return true;
            }
            (0..4).all(|c| (i32::from(p[c]) - i32::from(seed_px[c])).abs() <= tol)
        };

        // scanline fill: spans of alike pixels, and the rows above and below them
        let mut mask = vec![false; w * h];
        let mut stack = vec![((seed.0 - x0) as usize, (seed.1 - y0) as usize)];
        while let Some((sx, sy)) = stack.pop() {
            if mask[sy * w + sx] || !alike(&buf[at(sx, sy)..at(sx, sy) + 4]) {
                continue;
            }
            let (mut l, mut r) = (sx, sx);
            while l > 0 && !mask[sy * w + l - 1] && alike(&buf[at(l - 1, sy)..at(l - 1, sy) + 4]) {
                l -= 1;
            }
            while r + 1 < w && !mask[sy * w + r + 1] && alike(&buf[at(r + 1, sy)..at(r + 1, sy) + 4]) {
                r += 1;
            }
            for x in l..=r {
                mask[sy * w + x] = true;
            }
            for ny in [sy.checked_sub(1), (sy + 1 < h).then_some(sy + 1)]
                .into_iter()
                .flatten()
            {
                let mut open = false;
                for x in l..=r {
                    let free = !mask[ny * w + x] && alike(&buf[at(x, ny)..at(x, ny) + 4]);
                    if free && !open {
                        stack.push((x, ny));
                    }
                    open = free;
                }
            }
        }

        // the rim: pixels within `grow` steps of the fill that are not part of it
        let mut rim = vec![false; w * h];
        let mut frontier: Vec<usize> = (0..w * h).filter(|i| mask[*i]).collect();
        let mut reached = mask.clone();
        for _ in 0..grow.min(8) {
            let mut next = Vec::new();
            for i in frontier {
                let (x, y) = (i % w, i / w);
                let around = [
                    x.checked_sub(1).map(|x| (x, y)),
                    (x + 1 < w).then_some((x + 1, y)),
                    y.checked_sub(1).map(|y| (x, y)),
                    (y + 1 < h).then_some((x, y + 1)),
                ];
                for (nx, ny) in around.into_iter().flatten() {
                    let j = ny * w + nx;
                    if !reached[j] {
                        reached[j] = true;
                        rim[j] = true;
                        next.push(j);
                    }
                }
            }
            frontier = next;
        }

        // lay the paint: over the matched pixels, behind the rim's
        let mut changed = 0;
        for i in 0..w * h {
            if !(mask[i] || rim[i]) {
                continue;
            }
            let o = i * 4;
            let old = [buf[o], buf[o + 1], buf[o + 2], buf[o + 3]];
            let new = if mask[i] {
                [color[0], color[1], color[2], 255]
            } else {
                let a = f32::from(old[3]) / 255.0;
                let mix = |c: usize| (f32::from(old[c]) * a + f32::from(color[c]) * (1.0 - a)).round() as u8;
                [mix(0), mix(1), mix(2), 255]
            };
            if new == old {
                continue;
            }
            let (x, y) = (x0 + (i % w) as i64, y0 + (i / w) as i64);
            let key = (x.div_euclid(t) as i32, y.div_euclid(t) as i32);
            if !self.tiles.contains_key(&key) {
                if self.tiles.len() >= MAX_TILES {
                    continue;
                }
                let mut px = load(key);
                px.resize(TILE_BYTES, 0);
                self.tiles.insert(key, px);
            }
            let px = self.tiles.get_mut(&key).expect("inserted above");
            let at = (((y - i64::from(key.1) * t) * t + (x - i64::from(key.0) * t)) * 4) as usize;
            px[at..at + 4].copy_from_slice(&new);
            self.dirty.insert(key);
            changed += 1;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty(_: Key) -> Vec<u8> {
        vec![0; TILE_BYTES]
    }

    fn pixel(painting: &Painting, x: f32, y: f32) -> [u8; 4] {
        let (key, px, py) = locate(Vec2::new(x, y));
        let i = ((py * TILE + px) * 4) as usize;
        let t = painting.tile(key).expect("tile exists");
        [t[i], t[i + 1], t[i + 2], t[i + 3]]
    }

    #[test]
    fn tiles_have_names_places_and_a_point_finds_its_tile_and_pixel() {
        assert_eq!(tile_name((-2, 3)), "-2,3");
        assert_eq!(parse_tile_name("-2,3"), Some((-2, 3)));
        assert_eq!(parse_tile_name(" 5 , 6 "), Some((5, 6)));
        for bad in ["", "1", "a,b", "1,2,3", "99999,0", "0,-99999"] {
            assert_eq!(parse_tile_name(bad), None, "{bad}");
        }
        assert_eq!(tile_origin((2, -1)), Vec2::new(512.0, -256.0));
        assert_eq!(locate(Vec2::new(0.5, 0.5)), ((0, 0), 0, 0));
        assert_eq!(locate(Vec2::new(256.0, 255.9)), ((1, 0), 0, 255));
        // negative coordinates fall in negative tiles
        assert_eq!(locate(Vec2::new(-0.5, -256.5)), ((-1, -2), 255, 255));
    }

    #[test]
    fn a_hard_dab_is_a_solid_disc_and_a_soft_one_fades_to_its_edge() {
        let mut painting = Painting::default();
        let hard = Brush {
            size: 20.0,
            hardness: 1.0,
            color: [255, 0, 0],
            ..Brush::default()
        };
        painting.dab(Vec2::new(100.0, 100.0), &hard, 1.0, &mut empty);
        assert_eq!(pixel(&painting, 100.0, 100.0), [255, 0, 0, 255]);
        assert_eq!(pixel(&painting, 108.0, 100.0)[3], 255);
        assert_eq!(pixel(&painting, 120.0, 100.0)[3], 0, "outside the radius");
        let soft = Brush {
            hardness: 0.0,
            ..hard
        };
        let mut painting = Painting::default();
        painting.dab(Vec2::new(100.0, 100.0), &soft, 1.0, &mut empty);
        let (centre, middle, rim) = (
            pixel(&painting, 100.0, 100.0)[3],
            pixel(&painting, 105.0, 100.0)[3],
            pixel(&painting, 109.0, 100.0)[3],
        );
        assert!(
            centre > middle && middle > rim && rim > 0,
            "{centre} {middle} {rim}"
        );
        // the colour of a soft edge is the paint's, only its strength changes
        assert_eq!(&pixel(&painting, 105.0, 100.0)[..3], &[255, 0, 0]);
    }

    #[test]
    fn paint_over_paint_mixes_by_alpha_and_flow_is_the_strength() {
        let mut painting = Painting::default();
        let half = Brush {
            size: 10.0,
            hardness: 1.0,
            flow: 0.5,
            color: [0, 0, 255],
            ..Brush::default()
        };
        painting.dab(Vec2::new(50.0, 50.0), &half, 1.0, &mut empty);
        let once = pixel(&painting, 50.0, 50.0);
        assert!((127..=129).contains(&once[3]), "{once:?}");
        painting.dab(Vec2::new(50.0, 50.0), &half, 1.0, &mut empty);
        let twice = pixel(&painting, 50.0, 50.0);
        assert!((190..=192).contains(&twice[3]), "{twice:?}");
        // new colour over old: the mix leans toward the new one by its strength
        let red = Brush {
            color: [255, 0, 0],
            flow: 1.0,
            ..half
        };
        painting.dab(Vec2::new(50.0, 50.0), &red, 1.0, &mut empty);
        assert_eq!(pixel(&painting, 50.0, 50.0), [255, 0, 0, 255]);
    }

    #[test]
    fn the_eraser_takes_alpha_away_and_a_dab_across_tiles_touches_each() {
        let mut painting = Painting::default();
        let paint = Brush {
            size: 40.0,
            hardness: 1.0,
            color: [0, 128, 0],
            ..Brush::default()
        };
        // centred on a corner of four tiles
        painting.dab(Vec2::new(256.0, 256.0), &paint, 1.0, &mut empty);
        let touched: Vec<Key> = painting.dirty().collect();
        assert_eq!(touched, [(0, 0), (0, 1), (1, 0), (1, 1)]);
        painting.forget_dirty();
        assert_eq!(painting.dirty().count(), 0);
        let rub = Brush {
            erase: true,
            size: 10.0,
            hardness: 1.0,
            ..paint
        };
        painting.dab(Vec2::new(256.0, 256.0), &rub, 1.0, &mut empty);
        assert_eq!(pixel(&painting, 256.0, 256.0)[3], 0);
        assert_eq!(pixel(&painting, 270.0, 256.0)[3], 255, "beyond the eraser");
        assert!(!Painting::is_empty(painting.tile((0, 0)).unwrap()));
    }

    #[test]
    fn a_stroke_lays_dabs_along_the_line_at_an_even_spacing_whatever_the_pointer_does() {
        let brush = Brush {
            size: 8.0,
            hardness: 1.0,
            color: [0, 0, 0],
            ..Brush::default()
        };
        let line = |moves: &[Vec2]| {
            let mut painting = Painting::default();
            let mut stroke = Stroke::default();
            for m in moves {
                stroke.line(&mut painting, &brush, *m, 1.0, &mut empty);
            }
            painting
        };
        // one long move and many short ones paint the same pixels
        let long = line(&[Vec2::new(10.0, 20.0), Vec2::new(110.0, 20.0)]);
        let short_moves: Vec<Vec2> = (0..=100).map(|i| Vec2::new(10.0 + i as f32, 20.0)).collect();
        let short = line(&short_moves);
        assert_eq!(long.tile((0, 0)), short.tile((0, 0)));
        // continuous: every pixel along the line has paint, none past its ends
        for x in 10..=110 {
            assert_eq!(pixel(&long, x as f32, 20.0)[3], 255, "gap at {x}");
        }
        assert_eq!(pixel(&long, 200.0, 20.0)[3], 0);
        // a tap is one dab
        let tap = line(&[Vec2::new(30.0, 30.0)]);
        assert_eq!(pixel(&tap, 30.0, 30.0)[3], 255);
    }

    #[test]
    fn pressure_scales_the_dab_and_hostile_input_changes_nothing_or_stays_bounded() {
        let brush = Brush {
            size: 40.0,
            hardness: 1.0,
            ..Brush::default()
        };
        let (mut light, mut full) = (Painting::default(), Painting::default());
        Stroke::default().line(&mut light, &brush, Vec2::new(100.0, 100.0), 0.0, &mut empty);
        Stroke::default().line(&mut full, &brush, Vec2::new(100.0, 100.0), 1.0, &mut empty);
        assert_eq!(pixel(&full, 118.0, 100.0)[3], 255);
        assert_eq!(
            pixel(&light, 118.0, 100.0)[3],
            0,
            "a feather touch makes a small dab"
        );
        let mut painting = Painting::default();
        let mut stroke = Stroke::default();
        for to in [
            Vec2::new(f32::NAN, 1.0),
            Vec2::new(1.0e30, 1.0e30),
            Vec2::new(f32::INFINITY, 0.0),
            Vec2::new(5.0, 5.0),
        ] {
            stroke.line(&mut painting, &brush, to, f32::NAN, &mut empty);
        }
        painting.dab(
            Vec2::NAN,
            &Brush {
                size: f32::NAN,
                hardness: f32::NAN,
                flow: f32::NAN,
                ..brush
            },
            1.0,
            &mut empty,
        );
        assert!(painting.dirty().count() <= MAX_TILES);
        // a tile a loader returns short is padded, never read past its end
        let mut short = Painting::default();
        short.dab(Vec2::new(5.0, 5.0), &brush, 1.0, &mut |_| vec![1, 2, 3]);
        assert_eq!(short.tile((0, 0)).unwrap().len(), TILE_BYTES);
    }

    /// A painting with a hollow square of black one pixel thick, from (10, 10) to (30, 30).
    fn square() -> Painting {
        let mut p = Painting::default();
        let black = [0, 0, 0, 255];
        let load = |_: Key| vec![0u8; TILE_BYTES];
        for i in 10..=30 {
            for (x, y) in [(i, 10), (i, 30), (10, i), (30, i)] {
                let (key, px, py) = locate(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
                p.tiles.entry(key).or_insert_with(|| load(key)).splice(
                    ((py * TILE + px) * 4) as usize..((py * TILE + px) * 4 + 4) as usize,
                    black,
                );
            }
        }
        p
    }

    const PAGE: FillBounds = (0, 0, 100, 100);

    #[test]
    fn a_fill_stops_at_a_closed_line_and_leaves_the_outside_alone() {
        let mut p = square();
        let n = p.flood_fill((20, 20), PAGE, [255, 0, 0], 0.0, 0, &mut empty);
        // the inside of the square is 19 by 19 pixels
        assert_eq!(n, 19 * 19);
        assert_eq!(pixel(&p, 20.5, 20.5), [255, 0, 0, 255]);
        assert_eq!(pixel(&p, 10.5, 10.5), [0, 0, 0, 255], "the line is untouched");
        assert_eq!(pixel(&p, 5.5, 5.5)[3], 0, "the outside is untouched");
        // filling the outside covers the page minus the square's whole footprint
        let mut q = square();
        let outside = q.flood_fill((2, 2), PAGE, [0, 255, 0], 0.0, 0, &mut empty);
        assert_eq!(outside, 100 * 100 - 21 * 21);
    }

    #[test]
    fn filling_the_same_colour_again_changes_nothing_and_leaves_the_tiles_clean() {
        let mut p = square();
        p.flood_fill((20, 20), PAGE, [255, 0, 0], 0.0, 0, &mut empty);
        p.forget_dirty();
        assert_eq!(p.flood_fill((20, 20), PAGE, [255, 0, 0], 0.0, 0, &mut empty), 0);
        assert_eq!(p.dirty().count(), 0);
    }

    #[test]
    fn a_line_with_a_gap_lets_the_fill_out_and_a_tolerance_spreads_over_near_colours() {
        let mut p = square();
        // open a gap in the top edge
        let (key, px, py) = locate(Vec2::new(20.5, 10.5));
        let i = ((py * TILE + px) * 4) as usize;
        p.tiles.get_mut(&key).unwrap()[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
        let n = p.flood_fill((20, 20), PAGE, [255, 0, 0], 0.0, 0, &mut empty);
        assert_eq!(n, 100 * 100 - 79, "the whole page but the line");
        // near colours: a pale grey page filled with a tolerance covers the slightly darker pixel
        let mut q = Painting::default();
        let mut load = |_: Key| {
            let mut t = vec![200u8; TILE_BYTES];
            for a in t.iter_mut().skip(3).step_by(4) {
                *a = 255;
            }
            t
        };
        q.tiles.insert((0, 0), load((0, 0)));
        q.tiles.get_mut(&(0, 0)).unwrap()[(5 * TILE * 4) as usize..(5 * TILE * 4 + 3) as usize]
            .copy_from_slice(&[190, 190, 190]);
        let strict = q.flood_fill((50, 50), PAGE, [0, 0, 255], 0.0, 0, &mut load);
        assert_eq!(strict, 100 * 100 - 1);
        let mut r = Painting::default();
        r.tiles.insert((0, 0), load((0, 0)));
        r.tiles.get_mut(&(0, 0)).unwrap()[(5 * TILE * 4) as usize..(5 * TILE * 4 + 3) as usize]
            .copy_from_slice(&[190, 190, 190]);
        assert_eq!(
            r.flood_fill((50, 50), PAGE, [0, 0, 255], 0.1, 0, &mut load),
            100 * 100
        );
    }

    #[test]
    fn grow_covers_the_soft_edge_behind_the_line_without_hiding_it() {
        let mut p = Painting::default();
        // a half see-through grey line down x = 20, a clear page either side
        let (key, _, _) = locate(Vec2::new(20.5, 5.5));
        let mut tile = vec![0u8; TILE_BYTES];
        for y in 0..100usize {
            let i = (y * TILE as usize + 20) * 4;
            tile[i..i + 4].copy_from_slice(&[100, 100, 100, 128]);
        }
        p.tiles.insert(key, tile);
        let n = p.flood_fill((5, 50), (0, 0, 30, 100), [255, 255, 255], 0.0, 1, &mut empty);
        // 20 columns by 100 rows are the page up to the line; the line's own column is the rim
        assert_eq!(n, 20 * 100 + 100);
        let line = pixel(&p, 20.5, 50.5);
        assert_eq!(line[3], 255);
        // grey at half strength over white: lighter than the grey, darker than white
        assert!(line[0] > 100 && line[0] < 255, "{line:?}");
        // the far side of the line was never reached
        assert_eq!(pixel(&p, 25.5, 50.5)[3], 0);
    }

    #[test]
    fn hostile_fills_do_nothing() {
        let mut p = Painting::default();
        // seed outside the bounds, empty or backwards bounds, bounds too large, NaN tolerance
        assert_eq!(p.flood_fill((200, 200), PAGE, [1, 1, 1], 0.1, 1, &mut empty), 0);
        assert_eq!(
            p.flood_fill((5, 5), (10, 10, 10, 10), [1, 1, 1], 0.1, 1, &mut empty),
            0
        );
        assert_eq!(
            p.flood_fill((5, 5), (50, 50, 0, 0), [1, 1, 1], 0.1, 1, &mut empty),
            0
        );
        assert_eq!(
            p.flood_fill((5, 5), (0, 0, 100_000, 100_000), [1, 1, 1], 0.1, 1, &mut empty),
            0
        );
        assert_eq!(
            p.flood_fill(
                (5, 5),
                (i64::MIN, i64::MIN, i64::MAX, i64::MAX),
                [1, 1, 1],
                0.1,
                1,
                &mut empty
            ),
            0
        );
        assert!(p.flood_fill((5, 5), PAGE, [1, 1, 1], f32::NAN, u32::MAX, &mut empty) > 0);
        assert!(p.dirty().count() <= MAX_TILES);
        // a loader that returns a short tile is padded, never read past its end
        let mut q = Painting::default();
        assert!(q.flood_fill((5, 5), PAGE, [9, 9, 9], 0.0, 0, &mut |_| vec![1, 2]) > 0);
    }
}
