//! Glyph outlines as paths.
//!
//! A font draws each glyph as contours of straight lines and curves. Fanimate
//! draws text as ordinary shapes, so an outline becomes a list of path
//! segments scaled to the text size, with its origin at the glyph's own
//! origin (the left end of its baseline) and y pointing down like the
//! document's. Curves are cubic: the quadratic curves most fonts use are
//! raised to cubics exactly.

use glam::Vec2;
use rustybuzz::Face;
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder, Tag};

use crate::fonts::Font;

/// One piece of an outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Vec2),
    Line(Vec2),
    /// Two control points, then the end point.
    Cubic(Vec2, Vec2, Vec2),
    Close,
}

/// Draws glyphs of one font at one weight and size.
pub struct Outliner {
    face: Face<'static>,
    scale: f32,
}

impl Outliner {
    /// `None` if the font cannot be read (the bundled ones can).
    pub fn new(font: &'static Font, weight: f32, size: f32) -> Option<Self> {
        let mut face = Face::from_slice(font.data, 0)?;
        if font.variable && weight.is_finite() {
            let _ = face.set_variation(Tag::from_bytes(b"wght"), weight.clamp(1.0, 1000.0));
        }
        let size = if size.is_finite() {
            size.clamp(1.0, 4096.0)
        } else {
            48.0
        };
        let scale = size / face.units_per_em().max(1) as f32;
        Some(Self { face, scale })
    }

    /// The outline of glyph `id`; empty for a glyph with no ink, such as a
    /// space, or one the font does not have.
    pub fn glyph(&self, id: u16) -> Vec<Seg> {
        let mut builder = Builder {
            segs: Vec::new(),
            scale: self.scale,
            at: Vec2::ZERO,
        };
        let _ = self.face.outline_glyph(GlyphId(id), &mut builder);
        builder.segs
    }
}

struct Builder {
    segs: Vec<Seg>,
    scale: f32,
    /// The pen, to raise quadratic curves.
    at: Vec2,
}

impl Builder {
    fn point(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x * self.scale, -y * self.scale)
    }
}

impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.at = self.point(x, y);
        self.segs.push(Seg::Move(self.at));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.at = self.point(x, y);
        self.segs.push(Seg::Line(self.at));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (from, control, to) = (self.at, self.point(x1, y1), self.point(x, y));
        // a quadratic curve is the cubic whose controls are two thirds of the
        // way from each end to the quadratic's control
        self.segs.push(Seg::Cubic(
            from + (control - from) * (2.0 / 3.0),
            to + (control - to) * (2.0 / 3.0),
            to,
        ));
        self.at = to;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.at = self.point(x, y);
        self.segs
            .push(Seg::Cubic(self.point(x1, y1), self.point(x2, y2), self.at));
    }

    fn close(&mut self) {
        self.segs.push(Seg::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::find_or_default;

    fn glyph_id(font: &'static Font, c: char) -> u16 {
        Face::from_slice(font.data, 0)
            .unwrap()
            .glyph_index(c)
            .expect("the font has the glyph")
            .0
    }

    fn bounds(segs: &[Seg]) -> (Vec2, Vec2) {
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for s in segs {
            let points: &[Vec2] = match s {
                Seg::Move(p) | Seg::Line(p) => std::slice::from_ref(p),
                Seg::Cubic(a, b, c) => &[*a, *b, *c],
                Seg::Close => &[],
            };
            for p in points {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        (lo, hi)
    }

    #[test]
    fn a_capital_is_drawn_above_its_baseline_in_y_down_space() {
        let font = find_or_default("Inter");
        let o = Outliner::new(font, 400.0, 100.0).unwrap();
        let segs = o.glyph(glyph_id(font, 'I'));
        assert!(!segs.is_empty());
        let (lo, hi) = bounds(&segs);
        // above the baseline is negative y; Inter's capitals are about 0.73 em tall
        assert!(hi.y.abs() < 1.0, "sits on the baseline: {hi}");
        assert!((-80.0..-65.0).contains(&lo.y), "{lo}");
        assert!(matches!(segs[0], Seg::Move(_)) && segs.last() == Some(&Seg::Close));
    }

    #[test]
    fn letters_with_holes_have_one_contour_per_hole_and_a_space_has_none() {
        let font = find_or_default("Inter");
        let o = Outliner::new(font, 400.0, 100.0).unwrap();
        let contours = |c| {
            o.glyph(glyph_id(font, c))
                .iter()
                .filter(|s| matches!(s, Seg::Move(_)))
                .count()
        };
        assert_eq!(contours('I'), 1);
        assert_eq!(contours('O'), 2, "the outside and the hole");
        assert_eq!(contours('B'), 3);
        assert!(o.glyph(glyph_id(font, ' ')).is_empty());
        // a glyph the font does not have draws nothing and does not fail
        assert!(o.glyph(u16::MAX).is_empty());
    }

    #[test]
    fn the_outline_scales_with_the_size_and_the_weight_changes_the_shape() {
        let font = find_or_default("Inter");
        let id = glyph_id(font, 'H');
        let small = bounds(&Outliner::new(font, 400.0, 50.0).unwrap().glyph(id));
        let big = bounds(&Outliner::new(font, 400.0, 100.0).unwrap().glyph(id));
        assert!(
            ((big.1 - big.0) / (small.1 - small.0) - Vec2::splat(2.0))
                .abs()
                .max_element()
                < 0.01
        );
        let thin = bounds(&Outliner::new(font, 100.0, 100.0).unwrap().glyph(id));
        let black = bounds(&Outliner::new(font, 900.0, 100.0).unwrap().glyph(id));
        assert!(
            (black.1.x - black.0.x) > (thin.1.x - thin.0.x) + 5.0,
            "heavier is wider"
        );
    }

    #[test]
    fn quadratic_curves_become_cubics_that_pass_through_the_same_ends() {
        let mut b = Builder {
            segs: Vec::new(),
            scale: 1.0,
            at: Vec2::ZERO,
        };
        b.move_to(0.0, 0.0);
        b.quad_to(50.0, 100.0, 100.0, 0.0);
        let Seg::Cubic(c1, c2, end) = b.segs[1] else {
            unreachable!()
        };
        assert_eq!(end, Vec2::new(100.0, 0.0));
        // the midpoint of the quadratic (50, -50 in y-down) is that of the cubic
        let mid = (Vec2::ZERO + 3.0 * c1 + 3.0 * c2 + end) / 8.0;
        assert!((mid - Vec2::new(50.0, -50.0)).length() < 1e-4, "{mid}");
    }
}
