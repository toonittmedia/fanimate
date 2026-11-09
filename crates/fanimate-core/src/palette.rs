//! The palette of a project: every colour its artwork uses, and replacing
//! one colour with another everywhere.
//!
//! This is "Extract palette" and a project-wide recolour. A colour here is
//! what a person would call one colour: two paints are the same when they
//! show the same 8-bit red, green, blue and alpha (the hex code). Replacing
//! a colour changes every fill, stroke and colour key of every shape on
//! vector layers and on the drawings of frame-by-frame layers, and the
//! project's background. Text layers and the pictures of image layers are not
//! touched: a text keeps the look set in its own panel, and pixels are not paint.
//!
//! Colours stay literal in the file (no palette is stored): nothing about the
//! format changes, and what this finds is always what is drawn. Palette
//! entries that shapes refer to, and variants as overrides, need a format
//! change and are not built.

use crate::color::Color;
use crate::frames::Part;
use crate::path::Shape;
use crate::scene::{Document, LayerKind};

/// A colour of the project and how many paints use it.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteEntry {
    /// The first exact colour found that has this hex code.
    pub color: Color,
    /// Fills, strokes and colour keys that show it, and one for the background.
    pub count: usize,
}

/// The colours the artwork uses, the most used first (equal counts in the order found).
pub fn colours_used(doc: &Document) -> Vec<PaletteEntry> {
    let mut found: Vec<(String, PaletteEntry)> = Vec::new();
    let mut note = |c: Color| {
        let key = c.to_hex();
        match found.iter_mut().find(|(k, _)| *k == key) {
            Some((_, e)) => e.count += 1,
            None => found.push((key, PaletteEntry { color: c, count: 1 })),
        }
    };
    if doc.background.a > 0.0 {
        note(doc.background);
    }
    let mut shapes = |shape: &Shape| {
        if let Some(c) = shape.style.fill {
            note(c);
        }
        if let Some(s) = shape.style.stroke {
            note(s.color);
        }
        for k in shape.fill_keys.iter().chain(&shape.stroke_keys) {
            note(k.value);
        }
    };
    doc.walk(|layer| match &layer.kind {
        LayerKind::Vector { shapes: list } => list.iter().for_each(&mut shapes),
        LayerKind::Frames { cels } => {
            for cel in cels {
                for part in Part::ALL {
                    cel.part(part).iter().for_each(&mut shapes);
                }
            }
        }
        _ => {}
    });
    let mut out: Vec<PaletteEntry> = found.into_iter().map(|(_, e)| e).collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.count));
    out
}

/// Whether two colours show the same 8-bit colour.
pub fn same_colour(a: Color, b: Color) -> bool {
    a.to_hex() == b.to_hex()
}

/// Change every paint that shows `from` to `to`, exactly (a replacement
/// carries `to`'s own alpha: what is picked is what is drawn). Returns how
/// many paints changed.
pub fn replace_colour(doc: &mut Document, from: Color, to: Color) -> usize {
    if same_colour(from, to) {
        return 0;
    }
    let mut changed = 0;
    let mut swap = |c: &mut Color| {
        if same_colour(*c, from) {
            *c = to;
            changed += 1;
        }
    };
    if doc.background.a > 0.0 {
        swap(&mut doc.background);
    }
    let mut shapes = |shape: &mut Shape| {
        if let Some(c) = &mut shape.style.fill {
            swap(c);
        }
        if let Some(s) = &mut shape.style.stroke {
            swap(&mut s.color);
        }
        for k in shape.fill_keys.iter_mut().chain(shape.stroke_keys.iter_mut()) {
            swap(&mut k.value);
        }
    };
    doc.walk_mut(|layer| match &mut layer.kind {
        LayerKind::Vector { shapes: list } => list.iter_mut().for_each(&mut shapes),
        LayerKind::Frames { cels } => {
            for cel in cels {
                for part in Part::ALL {
                    cel.part_mut(part).iter_mut().for_each(&mut shapes);
                }
            }
        }
        _ => {}
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Interpolation, Key};
    use crate::frames::Cel;
    use crate::path::{Path, Style};
    use glam::Vec2;

    fn shape(style: Style) -> Shape {
        Shape::new("s", Path::rect(Vec2::ZERO, Vec2::new(10.0, 10.0), 0.0), style)
    }

    fn red() -> Color {
        Color::rgb(1.0, 0.0, 0.0)
    }

    fn doc() -> Document {
        let mut doc = Document::new("p", 100.0, 100.0, 24.0);
        doc.background = Color::WHITE;
        let mut keyed = shape(Style::fill(Color::BLACK));
        keyed.fill_keys = vec![Key {
            frame: 1,
            value: red(),
            interp: Interpolation::Step,
        }];
        let a = doc.new_layer(
            "a",
            LayerKind::Vector {
                shapes: vec![
                    shape(Style::fill(red()).with_stroke(Color::BLACK, 2.0)),
                    shape(Style::fill(red())),
                    keyed,
                ],
            },
        );
        let mut cel = Cel::new(1);
        cel.line.push(shape(Style::default().with_stroke(red(), 1.0)));
        cel.colour.push(shape(Style::fill(Color::rgb(0.0, 0.0, 1.0))));
        let b = doc.new_layer("b", LayerKind::Frames { cels: vec![cel] });
        doc.layers.push(a);
        doc.layers.push(b);
        doc
    }

    #[test]
    fn the_palette_lists_each_colour_once_with_its_uses_most_used_first() {
        let colours = colours_used(&doc());
        let by_hex: Vec<(String, usize)> = colours.iter().map(|e| (e.color.to_hex(), e.count)).collect();
        // red: two fills, a colour key and a frame stroke; black: a stroke and a key base; white: the page; blue: one fill
        assert_eq!(by_hex[0], ("#FF0000".to_owned(), 4), "{by_hex:?}");
        assert!(by_hex.contains(&("#000000".to_owned(), 2)));
        assert!(by_hex.contains(&("#FFFFFF".to_owned(), 1)));
        assert!(by_hex.contains(&("#0000FF".to_owned(), 1)));
        assert_eq!(by_hex.len(), 4);
        // an empty project has no colours but its page
        assert_eq!(colours_used(&Document::new("e", 10.0, 10.0, 24.0)).len(), 1);
    }

    #[test]
    fn replacing_a_colour_changes_every_paint_that_shows_it_and_nothing_else() {
        let mut d = doc();
        let blue = Color::rgb(0.0, 0.0, 1.0);
        let n = replace_colour(&mut d, red(), blue);
        assert_eq!(n, 4);
        let after: Vec<(String, usize)> = colours_used(&d)
            .iter()
            .map(|e| (e.color.to_hex(), e.count))
            .collect();
        assert!(!after.iter().any(|(h, _)| h == "#FF0000"), "{after:?}");
        assert!(after.contains(&("#0000FF".to_owned(), 5)), "{after:?}");
        assert!(
            after.contains(&("#000000".to_owned(), 2)),
            "the black is untouched"
        );
        // the page too, and a colour that is not there changes nothing
        assert_eq!(replace_colour(&mut d, Color::WHITE, Color::rgb(0.9, 0.9, 0.8)), 1);
        assert_eq!(d.background.to_hex(), "#E6E6CC");
        assert_eq!(replace_colour(&mut d, Color::rgb(0.5, 0.5, 0.5), blue), 0);
        // the same colour back to itself is no change at all
        assert_eq!(replace_colour(&mut d, blue, blue), 0);
    }

    #[test]
    fn colours_that_differ_in_the_eighth_bit_are_one_colour_and_a_see_through_one_is_another() {
        let mut d = Document::new("p", 10.0, 10.0, 24.0);
        d.background = Color::rgba(0.0, 0.0, 0.0, 0.0);
        let layer = d.new_layer(
            "l",
            LayerKind::Vector {
                shapes: vec![
                    shape(Style::fill(Color::rgb(0.2, 0.4, 0.6))),
                    shape(Style::fill(Color::rgb(0.2001, 0.4001, 0.6001))),
                    shape(Style::fill(Color::rgba(0.2, 0.4, 0.6, 0.5))),
                ],
            },
        );
        d.layers.push(layer);
        let colours = colours_used(&d);
        assert_eq!(colours.len(), 2, "a transparent page is not a colour");
        assert_eq!(colours[0].count, 2);
        // a replacement carries the new colour's own alpha
        let n = replace_colour(
            &mut d,
            Color::rgb(0.2, 0.4, 0.6),
            Color::rgba(1.0, 1.0, 1.0, 0.25),
        );
        assert_eq!(n, 2);
    }

    #[test]
    fn hostile_colours_never_panic() {
        let mut d = doc();
        let nan = Color::rgba(f32::NAN, f32::INFINITY, -5.0, 9.0);
        let _ = colours_used(&d);
        let _ = replace_colour(&mut d, red(), nan);
        let _ = colours_used(&d);
        let _ = replace_colour(&mut d, nan, red());
    }
}
