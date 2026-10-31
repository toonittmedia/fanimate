//! Laying a string out: shaping, bidirectional order, line breaking and
//! alignment.
//!
//! The steps, per paragraph (text between line breaks):
//!
//! 1. **Bidirectional analysis** splits it into runs of one direction, as the
//!    Unicode algorithm says, so Hebrew or Arabic inside English (or the other
//!    way round) reads in the right order.
//! 2. **Shaping** each run with HarfBuzz's algorithm (the `rustybuzz` port)
//!    turns characters into glyphs: ligatures, kerning, joining forms and
//!    reordering come from the font's own tables.
//! 3. **Line breaking** is greedy at the places the Unicode line breaking
//!    algorithm allows, and anywhere when a single word is wider than the box.
//! 4. **Visual order** is worked out per line, then **alignment** places it.
//!
//! Every glyph keeps the character and word it belongs to, so animators can
//! move text by character, word or line. Everything is deterministic: the
//! same text, font and settings give the same positions on every platform.
//! The input is never trusted: sizes, widths and lengths are held to sane
//! ranges, so nothing a damaged project says can hang or panic the layout.

use std::ops::Range;
use std::str::FromStr;

use glam::Vec2;
use rustybuzz::ttf_parser::Tag;
use rustybuzz::{Face, UnicodeBuffer};
use unicode_bidi::{BidiInfo, Level};
use unicode_linebreak::linebreaks;
use unicode_script::Script;
use unicode_segmentation::UnicodeSegmentation;

use crate::fonts::{self, Font};

/// The most characters laid out; the rest of a longer string is ignored.
pub const MAX_CHARS: usize = 100_000;
/// The range of sizes, in document units, a layout accepts.
pub const SIZE_RANGE: (f32, f32) = (1.0, 4096.0);

/// Where lines sit in the box. "Start" and "end" follow the paragraph's
/// direction, so they are the left and right of English text and the right and
/// left of Arabic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// The direction paragraphs start in. `Auto` takes it from the first strongly
/// directional character, as most editors do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Auto,
    LeftToRight,
    RightToLeft,
}

/// An OpenType feature to switch on or off for the whole text, such as
/// `liga` (ligatures) or `smcp` (small capitals): `value` 1 is on, 0 is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feature {
    pub tag: [u8; 4],
    pub value: u32,
}

/// What a layout needs besides the text.
#[derive(Clone, Copy, Debug)]
pub struct Params<'a> {
    /// A bundled family; an unknown one gets the default (see [`fonts`]).
    pub family: &'a str,
    /// The em size in document units.
    pub size: f32,
    /// Weight (100 to 900) for fonts that have a weight axis.
    pub weight: f32,
    /// Extra space after every glyph, in thousandths of an em.
    pub tracking: f32,
    /// Distance between baselines, as a multiple of `size`.
    pub line_height: f32,
    /// Wrap at this width; `None` is point text: only line breaks end a line.
    pub width: Option<f32>,
    pub align: Align,
    pub direction: Direction,
    /// A language tag such as `en` or `yo`; empty when unknown. Some fonts
    /// shape differently for different languages.
    pub language: &'a str,
    pub features: &'a [Feature],
}

impl Default for Params<'_> {
    fn default() -> Self {
        Self {
            family: fonts::DEFAULT_FAMILY,
            size: 48.0,
            weight: 400.0,
            tracking: 0.0,
            line_height: 1.2,
            width: None,
            align: Align::Start,
            direction: Direction::Auto,
            language: "",
            features: &[],
        }
    }
}

/// One glyph, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub font: &'static Font,
    /// The glyph's number in the font.
    pub id: u16,
    /// Where its origin (the left end of the baseline) sits, in text space:
    /// x to the right and y **down** from the top left of the box.
    pub origin: Vec2,
    /// How far the pen moved for it, tracking included.
    pub advance: f32,
    /// The bytes of the string it stands for (a ligature stands for several
    /// characters but is placed once, at the first).
    pub bytes: Range<usize>,
    /// Counting from 0: the character (grapheme cluster) it belongs to, the
    /// word, and the line.
    pub character: usize,
    pub word: usize,
    pub line: usize,
    /// Whether it was set right to left.
    pub rtl: bool,
}

/// A laid-out string.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Laid {
    pub glyphs: Vec<Glyph>,
    pub characters: usize,
    pub words: usize,
    pub lines: usize,
    /// The size of the box: its width (the wrap width, or the widest line for
    /// point text) and the height of all the lines.
    pub size: Vec2,
}

/// One glyph while it is being placed, in logical order.
struct Item {
    /// The font the glyph is in: not always the one asked for, when it lacks a character.
    font: &'static Font,
    id: u16,
    /// Byte offset of its cluster in the whole string.
    start: usize,
    end: usize,
    advance: f32,
    offset: Vec2,
    level: u8,
}

/// A line, in visual order, before it is positioned.
struct Line {
    items: Vec<Item>,
    /// The width of the line without the spaces at its end.
    ink: f32,
    rtl: bool,
}

/// Lay out `text`. Never fails: text that cannot be shown gives an empty
/// layout, and anything unusable in `params` is held to a usable value.
pub fn layout(text: &str, params: &Params) -> Laid {
    if text.is_empty() {
        return Laid::default();
    }
    let size = if params.size.is_finite() {
        params.size
    } else {
        48.0
    }
    .clamp(SIZE_RANGE.0, SIZE_RANGE.1);
    let weight = if params.weight.is_finite() {
        params.weight
    } else {
        400.0
    }
    .clamp(1.0, 1000.0);
    let tracking = if params.tracking.is_finite() {
        params.tracking
    } else {
        0.0
    }
    .clamp(-1000.0, 1000.0)
        / 1000.0
        * size;
    let line_height = if params.line_height.is_finite() {
        params.line_height
    } else {
        1.2
    }
    .clamp(0.2, 5.0);
    let wrap = params
        .width
        .filter(|w| w.is_finite() && *w > 0.0)
        .map(|w| w.max(1.0));

    let font = fonts::find_or_default(params.family);
    let Some(mut face) = Face::from_slice(font.data, 0) else {
        return Laid::default();
    };
    if font.variable {
        let _ = face.set_variation(Tag::from_bytes(b"wght"), weight);
    }
    let scale = size / face.units_per_em().max(1) as f32;
    let ascent = f32::from(face.ascender()) * scale;
    let descent = -f32::from(face.descender()) * scale;
    let pitch = size * line_height;
    // a line's text is centred in its line box, as most editors do
    let baseline = (pitch - (ascent + descent)) / 2.0 + ascent;

    // only the first MAX_CHARS characters, cut on a character boundary
    let text = match text.char_indices().nth(MAX_CHARS) {
        Some((end, _)) => &text[..end],
        None => text,
    };
    let (character_of, word_of, characters, words) = index_text(text);
    let features: Vec<rustybuzz::Feature> = params
        .features
        .iter()
        .map(|f| rustybuzz::Feature::new(Tag::from_bytes(&f.tag), f.value, ..))
        .collect();
    let language = (!params.language.trim().is_empty())
        .then(|| rustybuzz::Language::from_str(params.language.trim()).ok())
        .flatten();
    let base = match params.direction {
        Direction::Auto => None,
        Direction::LeftToRight => Some(Level::ltr()),
        Direction::RightToLeft => Some(Level::rtl()),
    };

    let mut faces = Faces::new(font, weight);

    // 1 to 3: every paragraph shaped and broken into lines
    let mut lines: Vec<Line> = Vec::new();
    let mut offset = 0;
    for paragraph in text.split('\n') {
        let end = offset + paragraph.len();
        let body = paragraph.strip_suffix('\r').unwrap_or(paragraph);
        if body.is_empty() {
            lines.push(Line {
                items: Vec::new(),
                ink: 0.0,
                rtl: base.is_some_and(|l| l.is_rtl()),
            });
        } else {
            let info = BidiInfo::new(body, base);
            let rtl = info.paragraphs.first().is_some_and(|p| p.level.is_rtl());
            let items = shape_paragraph(
                &mut faces,
                &features,
                language.as_ref(),
                &info,
                offset,
                size,
                tracking,
            );
            for range in break_lines(body, offset, &items, wrap) {
                lines.push(finish_line(&items[range], rtl, text));
            }
        }
        offset = end + 1;
    }

    // 4: the box, then every line placed in it
    let ink_widest = lines.iter().map(|l| l.ink).fold(0.0f32, f32::max);
    let box_width = wrap.unwrap_or(ink_widest);
    let mut glyphs = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let free = (box_width - line.ink).max(0.0);
        let mut x = match (params.align, line.rtl) {
            (Align::Center, _) => free / 2.0,
            (Align::Start, false) | (Align::End, true) => 0.0,
            (Align::Start, true) | (Align::End, false) => free,
        };
        let y = n as f32 * pitch + baseline;
        for item in &line.items {
            let rtl = item.level % 2 == 1;
            let at = item.start.min(text.len());
            glyphs.push(Glyph {
                font: item.font,
                id: item.id,
                origin: Vec2::new(x + item.offset.x, y - item.offset.y),
                advance: item.advance,
                bytes: item.start..item.end,
                character: character_of.get(at).copied().unwrap_or(0) as usize,
                word: word_of
                    .get(character_of.get(at).copied().unwrap_or(0) as usize)
                    .copied()
                    .unwrap_or(0) as usize,
                line: n,
                rtl,
            });
            x += item.advance;
        }
    }
    Laid {
        glyphs,
        characters,
        words,
        lines: lines.len(),
        size: Vec2::new(box_width, lines.len() as f32 * pitch),
    }
}

/// For every byte of `text`, the character (grapheme cluster) it is in; for
/// every character, the word it is in (spaces belong to the word before);
/// and the number of characters and words.
fn index_text(text: &str) -> (Vec<u32>, Vec<u32>, usize, usize) {
    let mut character_of = vec![0u32; text.len() + 1];
    let mut word_of = Vec::new();
    let mut word = 0u32;
    let mut in_word = false;
    for (n, (start, cluster)) in text.grapheme_indices(true).enumerate() {
        character_of[start..start + cluster.len()].fill(n as u32);
        let space = cluster.chars().all(char::is_whitespace);
        if !space && !in_word && n > 0 {
            word += 1;
        }
        in_word = !space;
        word_of.push(word);
    }
    let characters = word_of.len();
    let words = if characters == 0 { 0 } else { word as usize + 1 };
    let last = characters.saturating_sub(1) as u32;
    if let Some(slot) = character_of.last_mut() {
        *slot = last;
    }
    (character_of, word_of, characters, words)
}

/// The faces a layout uses, made when first needed: the font asked for is
/// always first (index 0), the fonts that stand in for characters it lacks
/// follow.
struct Faces {
    list: Vec<Entry>,
    weight: f32,
}

struct Entry {
    font: &'static Font,
    face: Face<'static>,
}

impl Faces {
    fn new(primary: &'static Font, weight: f32) -> Self {
        let mut faces = Self {
            list: Vec::new(),
            weight,
        };
        // the bundled fonts always parse; if one did not, text would be empty
        faces.index_of(primary);
        faces
    }

    /// The place of `font` in the list, adding it if it is not there. `None` if it
    /// cannot be read.
    fn index_of(&mut self, font: &'static Font) -> Option<usize> {
        if let Some(i) = self.list.iter().position(|e| std::ptr::eq(e.font, font)) {
            return Some(i);
        }
        let mut face = Face::from_slice(font.data, 0)?;
        if font.variable {
            let _ = face.set_variation(Tag::from_bytes(b"wght"), self.weight);
        }
        self.list.push(Entry { font, face });
        Some(self.list.len() - 1)
    }

    fn has(&self, index: usize, c: char) -> bool {
        self.list
            .get(index)
            .is_some_and(|e| e.face.glyph_index(c).is_some())
    }

    /// The font for the character `c`, as an index: the asked-for font when it
    /// has the glyph, else the first font that suits the character's script and
    /// has it. Spaces, digits, marks and joiners (no script of their own) stay
    /// with the font of the character before, so a word is not cut in pieces.
    fn choose(&mut self, c: char, current: usize) -> usize {
        use unicode_script::UnicodeScript;
        let script = c.script();
        let neutral = matches!(script, Script::Common | Script::Inherited | Script::Unknown);
        if neutral && self.has(current, c) {
            return current;
        }
        if self.has(0, c) {
            return 0;
        }
        if let Some(font) = fonts::all()
            .iter()
            .find(|f| fonts::suits(f, script) && covers(f, c))
            && let Some(i) = self.index_of(font)
        {
            return i;
        }
        // no font has it: show the missing-glyph box in the font in use
        if neutral { current } else { 0 }
    }
}

/// Whether `font` has a glyph for `c`.
fn covers(font: &Font, c: char) -> bool {
    rustybuzz::ttf_parser::Face::parse(font.data, 0)
        .ok()
        .is_some_and(|face| face.glyph_index(c).is_some())
}

/// Shape one paragraph, run by run (a run is text of one bidirectional level,
/// cut again where the font changes), into glyphs in logical order. `offset`
/// is where the paragraph starts in the whole string.
fn shape_paragraph(
    faces: &mut Faces,
    features: &[rustybuzz::Feature],
    language: Option<&rustybuzz::Language>,
    info: &BidiInfo,
    offset: usize,
    size: f32,
    tracking: f32,
) -> Vec<Item> {
    let text = info.text;
    let mut items = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let level = info.levels[start];
        let mut end = start;
        for (i, c) in text[start..].char_indices() {
            if info.levels[start + i] != level {
                break;
            }
            end = start + i + c.len_utf8();
        }
        // the pieces of the run, each in one font
        let mut pieces: Vec<(usize, usize, usize)> = Vec::new();
        let mut current = 0;
        for (i, c) in text[start..end].char_indices() {
            let at = start + i;
            current = faces.choose(c, current);
            match pieces.last_mut() {
                Some(piece) if piece.2 == current => piece.1 = at + c.len_utf8(),
                _ => pieces.push((at, at + c.len_utf8(), current)),
            }
        }
        for (piece_start, piece_end, font) in pieces {
            let Some(entry) = faces.list.get(font) else {
                continue;
            };
            let scale = size / entry.face.units_per_em().max(1) as f32;
            let mut buffer = UnicodeBuffer::new();
            buffer.push_str(&text[piece_start..piece_end]);
            buffer.set_direction(if level.is_rtl() {
                rustybuzz::Direction::RightToLeft
            } else {
                rustybuzz::Direction::LeftToRight
            });
            if let Some(language) = language {
                buffer.set_language(language.clone());
            }
            buffer.guess_segment_properties();
            let shaped = rustybuzz::shape(&entry.face, features, buffer);
            let mut run: Vec<Item> = shaped
                .glyph_infos()
                .iter()
                .zip(shaped.glyph_positions())
                .map(|(g, p)| Item {
                    font: entry.font,
                    id: u16::try_from(g.glyph_id).unwrap_or(0),
                    start: offset + piece_start + g.cluster as usize,
                    end: 0,
                    advance: p.x_advance as f32 * scale + tracking,
                    offset: Vec2::new(p.x_offset as f32 * scale, p.y_offset as f32 * scale),
                    level: level.number(),
                })
                .collect();
            // shaping returns a right-to-left run in visual order; the rest of the
            // layout wants logical order, in which clusters only go up
            if level.is_rtl() {
                run.reverse();
            }
            // a glyph's cluster ends where the next different one starts
            let run_end = offset + piece_end;
            let mut next = run_end;
            for item in run.iter_mut().rev() {
                if item.start < next {
                    item.end = next;
                    next = item.start;
                } else {
                    item.end = next.max(item.start);
                }
            }
            // glyphs of one cluster all report the whole cluster's bytes
            let mut i = 0;
            while i < run.len() {
                let mut j = i + 1;
                while j < run.len() && run[j].start == run[i].start {
                    j += 1;
                }
                let end = run[j - 1].end;
                for item in &mut run[i..j] {
                    item.end = end;
                }
                i = j;
            }
            items.extend(run);
        }
        start = end;
    }
    items
}

/// Where the lines of a paragraph start and end, as ranges of `items`: at the
/// break opportunities when `wrap` is set, and once a word alone is too wide,
/// between any two glyphs.
fn break_lines(body: &str, offset: usize, items: &[Item], wrap: Option<f32>) -> Vec<Range<usize>> {
    let Some(limit) = wrap else {
        return std::iter::once(0..items.len()).collect();
    };
    let mut allowed = vec![false; body.len() + 1];
    for (at, _) in linebreaks(body) {
        if at < allowed.len() {
            allowed[at] = true;
        }
    }
    let mut lines = Vec::new();
    let mut line_start = 0;
    // the last place the line could have ended, and the width up to it
    let mut candidate: Option<usize> = None;
    let mut width = 0.0f32;
    let mut i = 0;
    while i < items.len() {
        let item = &items[i];
        let local = item.start.saturating_sub(offset);
        if i > line_start && allowed.get(local).copied().unwrap_or(false) {
            candidate = Some(i);
        }
        let visible = if is_space(body, item) { 0.0 } else { item.advance };
        let fits = width + visible <= limit + 1e-3;
        if !fits && i > line_start {
            // end the line at the last opportunity, or here when there is none
            let at = candidate.filter(|&c| c > line_start).unwrap_or(i);
            lines.push(line_start..at);
            line_start = at;
            candidate = None;
            width = 0.0;
            i = at;
            continue;
        }
        width += item.advance;
        i += 1;
    }
    lines.push(line_start..items.len());
    lines
}

/// Whether a glyph stands for white space only.
fn is_space(body: &str, item: &Item) -> bool {
    let local = item.start.min(body.len());
    let end = item.end.min(body.len()).max(local);
    body.get(local..end)
        .is_some_and(|s| !s.is_empty() && s.chars().all(char::is_whitespace))
}

/// A line's glyphs in visual order, and how wide its text is.
fn finish_line(items: &[Item], rtl: bool, whole: &str) -> Line {
    let mut ordered: Vec<Item> = items
        .iter()
        .map(|i| Item {
            font: i.font,
            id: i.id,
            start: i.start,
            end: i.end,
            advance: i.advance,
            offset: i.offset,
            level: i.level,
        })
        .collect();
    reorder(&mut ordered);
    // spaces at the end of the logical line do not count toward its width
    let mut ink: f32 = ordered.iter().map(|i| i.advance).sum();
    for item in items.iter().rev() {
        let space = whole
            .get(item.start.min(whole.len())..item.end.min(whole.len()).max(item.start.min(whole.len())))
            .is_some_and(|s| !s.is_empty() && s.chars().all(char::is_whitespace));
        if space {
            ink -= item.advance;
        } else {
            break;
        }
    }
    Line {
        items: ordered,
        ink: ink.max(0.0),
        rtl,
    }
}

/// Rule L2 of the Unicode bidirectional algorithm: from the highest level
/// down to the lowest odd one, reverse every run of glyphs at that level or
/// above.
fn reorder(items: &mut [Item]) {
    let Some(highest) = items.iter().map(|i| i.level).max() else {
        return;
    };
    let Some(lowest_odd) = items.iter().map(|i| i.level).filter(|l| l % 2 == 1).min() else {
        return;
    };
    for level in (lowest_odd..=highest).rev() {
        let mut i = 0;
        while i < items.len() {
            if items[i].level >= level {
                let mut j = i;
                while j < items.len() && items[j].level >= level {
                    j += 1;
                }
                items[i..j].reverse();
                i = j;
            } else {
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(text: &str, p: &Params) -> f32 {
        layout(text, p).size.x
    }

    #[test]
    fn a_word_is_laid_out_left_to_right_with_the_right_indexes() {
        let laid = layout("Hello", &Params::default());
        assert_eq!(laid.glyphs.len(), 5);
        assert_eq!((laid.characters, laid.words, laid.lines), (5, 1, 1));
        let xs: Vec<f32> = laid.glyphs.iter().map(|g| g.origin.x).collect();
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "{xs:?}");
        assert_eq!(
            laid.glyphs.iter().map(|g| g.character).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        assert!(laid.glyphs.iter().all(|g| g.word == 0 && g.line == 0 && !g.rtl));
        // the box is as wide as the pen travelled
        let travelled: f32 = laid.glyphs.iter().map(|g| g.advance).sum();
        assert!((laid.size.x - travelled).abs() < 1e-3);
        assert!(laid.size.y > 40.0 && laid.size.y < 70.0, "{}", laid.size.y);
        // the first baseline is below the top of the box and above its bottom
        assert!(laid.glyphs[0].origin.y > 0.0 && laid.glyphs[0].origin.y < laid.size.y);
    }

    #[test]
    fn words_and_characters_are_counted_with_spaces_in_the_word_before() {
        let laid = layout("ab cd  ef", &Params::default());
        assert_eq!((laid.characters, laid.words), (9, 3));
        let words: Vec<usize> = laid.glyphs.iter().map(|g| g.word).collect();
        assert_eq!(words, [0, 0, 0, 1, 1, 1, 1, 2, 2]);
        let chars: Vec<usize> = laid.glyphs.iter().map(|g| g.character).collect();
        assert_eq!(chars, (0..9).collect::<Vec<_>>());
    }

    #[test]
    fn the_font_kerns_pairs_that_want_it() {
        let p = Params::default();
        let pair = width("AV", &p);
        let apart = width("A", &p) + width("V", &p);
        assert!(pair < apart - 0.5, "AV {pair} against A + V {apart}");
    }

    #[test]
    fn tracking_adds_exactly_its_share_to_every_glyph() {
        let plain = width("Hello", &Params::default());
        let loose = width(
            "Hello",
            &Params {
                tracking: 100.0,
                ..Params::default()
            },
        );
        // 100 thousandths of a 48-unit em, after each of five glyphs
        assert!((loose - plain - 5.0 * 4.8).abs() < 0.01, "{plain} {loose}");
    }

    #[test]
    fn size_scales_everything_and_a_mono_font_has_equal_advances() {
        let small = width("Hello", &Params::default());
        let big = width(
            "Hello",
            &Params {
                size: 96.0,
                ..Params::default()
            },
        );
        assert!((big / small - 2.0).abs() < 0.001);
        let mono = layout(
            "iiWW",
            &Params {
                family: "JetBrains Mono",
                ..Params::default()
            },
        );
        let a: Vec<f32> = mono.glyphs.iter().map(|g| g.advance).collect();
        assert!(a.iter().all(|x| (x - a[0]).abs() < 1e-3), "{a:?}");
    }

    #[test]
    fn a_weight_axis_changes_a_variable_font_and_not_a_fixed_one() {
        let bold = |family| {
            width(
                "Hello",
                &Params {
                    family,
                    weight: 900.0,
                    ..Params::default()
                },
            ) - width(
                "Hello",
                &Params {
                    family,
                    weight: 100.0,
                    ..Params::default()
                },
            )
        };
        assert!(bold("Inter") > 5.0, "{}", bold("Inter"));
        assert!(bold("JetBrains Mono").abs() < 1e-3);
    }

    #[test]
    fn lines_wrap_at_spaces_and_fit_the_box() {
        let p = Params {
            width: Some(200.0),
            ..Params::default()
        };
        let text = "one two three four five six seven";
        let laid = layout(text, &p);
        assert!(laid.lines >= 3, "{}", laid.lines);
        assert_eq!(laid.size.x, 200.0);
        for line in 0..laid.lines {
            let on: Vec<&Glyph> = laid.glyphs.iter().filter(|g| g.line == line).collect();
            let last = on.last().unwrap();
            let visible_right = on
                .iter()
                .filter(|g| !text[g.bytes.clone()].trim().is_empty())
                .map(|g| g.origin.x + g.advance)
                .fold(0.0, f32::max);
            assert!(
                visible_right <= 200.5,
                "line {line} ends at {visible_right} ({:?})",
                last.bytes
            );
        }
        // words are not split: every word's glyphs are on one line
        for w in 0..laid.words {
            let lines: std::collections::BTreeSet<usize> = laid
                .glyphs
                .iter()
                .filter(|g| g.word == w)
                .map(|g| g.line)
                .collect();
            assert!(lines.len() <= 1, "word {w} is on lines {lines:?}");
        }
        // each line sits one pitch below the one before
        let y0 = laid.glyphs.iter().find(|g| g.line == 0).unwrap().origin.y;
        let y1 = laid.glyphs.iter().find(|g| g.line == 1).unwrap().origin.y;
        assert!((y1 - y0 - 48.0 * 1.2).abs() < 1e-3);
        assert!((laid.size.y - laid.lines as f32 * 48.0 * 1.2).abs() < 1e-3);
    }

    #[test]
    fn a_word_wider_than_the_box_is_broken_instead_of_overflowing_or_looping() {
        let p = Params {
            width: Some(60.0),
            ..Params::default()
        };
        let laid = layout("Extraordinarily", &p);
        assert!(laid.lines > 1);
        assert_eq!(laid.glyphs.len(), 15, "no glyph is lost");
        // a box narrower than one glyph still puts one glyph on every line
        let tiny = layout(
            "Wide",
            &Params {
                width: Some(1.0),
                ..Params::default()
            },
        );
        assert_eq!(tiny.lines, 4);
    }

    #[test]
    fn hard_line_breaks_make_lines_and_empty_lines_count() {
        let laid = layout("a\n\nb", &Params::default());
        assert_eq!(laid.lines, 3);
        assert_eq!(laid.glyphs.iter().map(|g| g.line).collect::<Vec<_>>(), [0, 2]);
        // windows line endings are the same as plain ones
        assert_eq!(layout("a\r\nb", &Params::default()).lines, 2);
        // and the index of a character counts across the break
        assert_eq!(laid.glyphs[1].character, 3);
    }

    #[test]
    fn alignment_moves_lines_inside_the_box() {
        let at = |align| {
            let laid = layout(
                "Hi",
                &Params {
                    width: Some(300.0),
                    align,
                    ..Params::default()
                },
            );
            let ink: f32 = laid.glyphs.iter().map(|g| g.advance).sum();
            (laid.glyphs[0].origin.x, ink)
        };
        let (start, ink) = at(Align::Start);
        let (center, _) = at(Align::Center);
        let (end, _) = at(Align::End);
        assert!(start.abs() < 1e-3);
        assert!((center - (300.0 - ink) / 2.0).abs() < 1e-3);
        assert!((end - (300.0 - ink)).abs() < 1e-3);
    }

    #[test]
    fn spaces_at_the_end_of_a_line_do_not_push_it_off_centre() {
        let a = layout(
            "Hi",
            &Params {
                width: Some(300.0),
                align: Align::Center,
                ..Params::default()
            },
        );
        let b = layout(
            "Hi    ",
            &Params {
                width: Some(300.0),
                align: Align::Center,
                ..Params::default()
            },
        );
        assert!((a.glyphs[0].origin.x - b.glyphs[0].origin.x).abs() < 1e-3);
    }

    #[test]
    fn a_right_to_left_paragraph_aligns_to_the_right_and_still_reads_english_left_to_right() {
        // an English sentence in a right-to-left paragraph reads left to right
        // as a whole (the space between two Latin words takes the Latin
        // direction); what the paragraph direction changes is where it starts
        let p = Params {
            direction: Direction::RightToLeft,
            width: Some(400.0),
            ..Params::default()
        };
        let laid = layout("abc def", &p);
        let x_of = |laid: &Laid, byte: usize| {
            laid.glyphs
                .iter()
                .find(|g| g.bytes.start == byte)
                .unwrap()
                .origin
                .x
        };
        assert!(x_of(&laid, 0) < x_of(&laid, 4), "the words read left to right");
        let right_edge = laid
            .glyphs
            .iter()
            .map(|g| g.origin.x + g.advance)
            .fold(0.0, f32::max);
        assert!(
            (right_edge - 400.0).abs() < 1.0,
            "start alignment is the right edge: {right_edge}"
        );
        assert!(
            laid.glyphs.iter().all(|g| !g.rtl),
            "Latin letters are set left to right"
        );
        // the same text in a left-to-right paragraph starts at the left
        let ltr = layout(
            "abc def",
            &Params {
                direction: Direction::LeftToRight,
                ..p
            },
        );
        assert!(ltr.glyphs[0].origin.x.abs() < 1e-3);
    }

    #[test]
    fn right_to_left_characters_inside_english_are_reversed_in_place() {
        // Hebrew letters (the bundled fonts have no glyphs for them, which does
        // not matter here: what is tested is where the layout puts them) form a
        // run of their own that reads right to left inside a left-to-right line
        let text = "ab \u{5d0}\u{5d1}\u{5d2} cd";
        let laid = layout(text, &Params::default());
        let hebrew: Vec<&Glyph> = laid.glyphs.iter().filter(|g| g.rtl).collect();
        assert_eq!(hebrew.len(), 3);
        let x_of = |byte: usize| {
            laid.glyphs
                .iter()
                .find(|g| g.bytes.start == byte)
                .unwrap()
                .origin
                .x
        };
        let (aleph, bet, gimel) = (3, 5, 7);
        assert!(
            x_of(gimel) < x_of(bet) && x_of(bet) < x_of(aleph),
            "the run reads right to left"
        );
        // the English around it keeps its order, with the run between
        let (a, b, c, d) = (0, 1, 10, 11);
        assert!(x_of(a) < x_of(b) && x_of(b) < x_of(gimel) && x_of(aleph) < x_of(c) && x_of(c) < x_of(d));
        // each Hebrew letter is still its own character, counted in reading order
        let chars: Vec<usize> = laid.glyphs.iter().map(|g| g.character).collect();
        let mut sorted = chars.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..chars.len()).collect::<Vec<_>>(), "{chars:?}");
        // and in a right-to-left paragraph the Hebrew leads
        let rtl = layout(
            "\u{5d0}\u{5d1} ab",
            &Params {
                direction: Direction::Auto,
                ..Params::default()
            },
        );
        // the paragraph takes its direction from the Hebrew, so the space after
        // it is right to left too, and the English sits to the left of the Hebrew
        assert_eq!(rtl.glyphs.iter().filter(|g| g.rtl).count(), 3);
        let latin = rtl.glyphs.iter().find(|g| g.bytes.start == 5).unwrap().origin.x;
        let first = rtl.glyphs.iter().find(|g| g.bytes.start == 0).unwrap().origin.x;
        let second = rtl.glyphs.iter().find(|g| g.bytes.start == 2).unwrap().origin.x;
        assert!(
            first > second,
            "the first letter of a Hebrew word is on the right"
        );
        assert!(latin < second, "the English follows the Hebrew, to its left");
    }

    #[test]
    fn the_visual_order_rule_reverses_nested_levels() {
        let mk = |levels: &[u8]| -> Vec<Item> {
            levels
                .iter()
                .enumerate()
                .map(|(i, &level)| Item {
                    font: fonts::find_or_default("Inter"),
                    id: i as u16,
                    start: i,
                    end: i + 1,
                    advance: 1.0,
                    offset: Vec2::ZERO,
                    level,
                })
                .collect()
        };
        let order = |levels: &[u8]| -> Vec<u16> {
            let mut items = mk(levels);
            reorder(&mut items);
            items.iter().map(|i| i.id).collect()
        };
        assert_eq!(order(&[0, 0, 0]), [0, 1, 2]);
        assert_eq!(order(&[1, 1, 1]), [2, 1, 0]);
        // a left-to-right number inside right-to-left text keeps its own order
        assert_eq!(order(&[1, 1, 2, 2, 1]), [4, 2, 3, 1, 0]);
        assert_eq!(order(&[0, 1, 1, 0]), [0, 2, 1, 3]);
        assert_eq!(order(&[]), Vec::<u16>::new());
    }

    #[test]
    fn nothing_a_project_can_say_hangs_or_panics() {
        // sizes and numbers that are not usable are held to usable ones
        for p in [
            Params {
                size: f32::NAN,
                ..Params::default()
            },
            Params {
                size: -50.0,
                ..Params::default()
            },
            Params {
                size: 1e30,
                ..Params::default()
            },
            Params {
                weight: f32::INFINITY,
                tracking: f32::NAN,
                line_height: f32::NEG_INFINITY,
                width: Some(f32::NAN),
                ..Params::default()
            },
            Params {
                width: Some(-5.0),
                ..Params::default()
            },
            Params {
                family: "No Such Font",
                language: "not a language tag at all",
                ..Params::default()
            },
        ] {
            let laid = layout("Some text, with words\nand lines", &p);
            assert!(laid.size.is_finite());
            assert!(
                laid.glyphs
                    .iter()
                    .all(|g| g.origin.is_finite() && g.advance.is_finite())
            );
        }
        // awkward strings
        for text in [
            "",
            "\n",
            "\n\n\n",
            " ",
            "\u{200d}\u{200d}",
            "e\u{301}\u{301}\u{301}",
            "👨‍👩‍👧‍👦 family",
            "\u{0}\u{1}\u{7f}",
            "\u{202e}reversed\u{202c}",
            "مرحبا hello שלום",
            "a\u{feff}b",
        ] {
            for width in [None, Some(30.0)] {
                for direction in [Direction::Auto, Direction::RightToLeft] {
                    let laid = layout(
                        text,
                        &Params {
                            width,
                            direction,
                            ..Params::default()
                        },
                    );
                    assert!(laid.size.is_finite(), "{text:?}");
                }
            }
        }
        assert!(layout("", &Params::default()).glyphs.is_empty());
        assert_eq!(layout("", &Params::default()).lines, 0);
    }

    #[test]
    fn an_enormous_string_is_cut_and_still_lays_out() {
        let text = "word ".repeat(40_000); // 200,000 characters
        let laid = layout(
            &text,
            &Params {
                width: Some(500.0),
                ..Params::default()
            },
        );
        assert_eq!(laid.characters, MAX_CHARS);
        assert!(laid.glyphs.len() <= MAX_CHARS);
    }

    #[test]
    fn layout_is_deterministic() {
        let p = Params {
            width: Some(220.0),
            ..Params::default()
        };
        let a = layout("The quick brown fox jumps over the lazy dog", &p);
        let b = layout("The quick brown fox jumps over the lazy dog", &p);
        assert_eq!(a, b);
    }

    #[test]
    fn a_feature_can_switch_ligatures_off() {
        // fi, fl and ffi ligatures: turning `liga` off can only add glyphs
        let on = layout("office fix", &Params::default());
        let off = layout(
            "office fix",
            &Params {
                features: &[Feature {
                    tag: *b"liga",
                    value: 0,
                }],
                ..Params::default()
            },
        );
        assert!(off.glyphs.len() >= on.glyphs.len());
        assert_eq!(off.characters, on.characters);
    }

    fn families(laid: &Laid) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for g in &laid.glyphs {
            if out.last() != Some(&g.font.family) {
                out.push(g.font.family);
            }
        }
        out
    }

    /// No glyph is the missing-glyph box (glyph 0 in every bundled font).
    fn nothing_missing(laid: &Laid) -> bool {
        !laid.glyphs.is_empty() && laid.glyphs.iter().all(|g| g.id != 0)
    }

    #[test]
    fn arabic_set_in_inter_falls_back_to_the_arabic_font_and_joins_right_to_left() {
        let laid = layout("مرحبا بالعالم", &Params::default());
        assert!(
            nothing_missing(&laid),
            "{:?}",
            laid.glyphs.iter().map(|g| g.id).collect::<Vec<_>>()
        );
        assert_eq!(
            families(&laid),
            ["Noto Sans Arabic"],
            "the space stays with the words around it"
        );
        assert!(laid.glyphs.iter().all(|g| g.rtl));
        // glyphs come out in visual order, left to right, so the last letter of the word is first
        assert!(laid.glyphs.first().unwrap().character > laid.glyphs.last().unwrap().character);
        assert_eq!(laid.characters, 13);
    }

    #[test]
    fn hebrew_and_thai_find_their_fonts_and_no_glyph_is_missing() {
        let hebrew = layout("שלום עולם", &Params::default());
        assert_eq!(families(&hebrew), ["Noto Sans Hebrew"]);
        assert!(nothing_missing(&hebrew));
        let thai = layout("สวัสดีครับ", &Params::default());
        assert_eq!(families(&thai), ["Noto Sans Thai"]);
        assert!(nothing_missing(&thai));
        // Thai vowels and tone marks sit on the letters (zero advance), they are not set beside them
        assert!(thai.glyphs.iter().any(|g| g.advance.abs() < 1e-3), "marks stack");
    }

    #[test]
    fn devanagari_conjuncts_are_formed_from_the_fonts_own_rules() {
        // क + virama + ष is one conjunct letter: three characters' code points, fewer glyphs than a
        // string with no joining would give
        let joined = layout("क्ष", &Params::default());
        assert_eq!(families(&joined), ["Noto Sans Devanagari"]);
        assert!(nothing_missing(&joined));
        let apart = layout("कष", &Params::default());
        assert_eq!(apart.characters, 2);
        assert_eq!(joined.characters, 1, "the conjunct is one character to a reader");
        let word = layout("नमस्ते दुनिया", &Params::default());
        assert!(nothing_missing(&word) && families(&word) == ["Noto Sans Devanagari"]);
    }

    #[test]
    fn mixed_scripts_switch_fonts_where_the_script_does_and_keep_word_and_line_indexes() {
        let laid = layout("Hello مرحبا World", &Params::default());
        assert_eq!(families(&laid), ["Inter", "Noto Sans Arabic", "Inter"]);
        assert!(nothing_missing(&laid));
        let words: Vec<usize> = laid.glyphs.iter().map(|g| g.word).collect();
        assert_eq!(*words.iter().max().unwrap(), 2);
        assert_eq!((laid.characters, laid.words, laid.lines), (17, 3, 1));
        // the Latin letters keep their left-to-right order around the Arabic
        let first = laid.glyphs.iter().find(|g| g.font.family == "Inter").unwrap();
        let last = laid
            .glyphs
            .iter()
            .rev()
            .find(|g| g.font.family == "Inter")
            .unwrap();
        assert!(first.origin.x < last.origin.x);
        // digits and marks stay with the script they follow
        let numbers = layout("مرحبا 123", &Params::default());
        assert!(nothing_missing(&numbers));
    }

    #[test]
    fn a_character_no_bundled_font_has_is_the_missing_box_not_a_crash_and_text_around_it_is_kept() {
        let laid = layout("A中B", &Params::default());
        assert_eq!(laid.characters, 3);
        assert_eq!(laid.glyphs.len(), 3);
        assert_eq!(laid.glyphs[1].id, 0, "the box");
        assert!(laid.glyphs[0].id != 0 && laid.glyphs[2].id != 0);
    }

    #[test]
    fn a_script_font_asked_for_by_name_shapes_its_own_script_and_sets_latin_in_the_default_when_it_lacks_it()
    {
        let params = Params {
            family: "Noto Sans Arabic",
            ..Params::default()
        };
        let laid = layout("مرحبا", &params);
        assert_eq!(families(&laid), ["Noto Sans Arabic"]);
        // lines, ranges and hard breaks still work with several fonts in a line
        let wrapped = layout(
            "Hello مرحبا World Hello مرحبا World",
            &Params {
                width: Some(150.0),
                ..Params::default()
            },
        );
        assert!(wrapped.lines >= 2 && nothing_missing(&wrapped));
    }

    #[test]
    fn fallback_layout_is_deterministic() {
        let a = layout("Hello مرحبا क्ष สวัสดี שלום", &Params::default());
        let b = layout("Hello مرحبا क्ष สวัสดี שלום", &Params::default());
        assert_eq!(a, b);
        assert_eq!(
            families(&a),
            [
                "Inter",
                "Noto Sans Arabic",
                // the space between a right-to-left and a left-to-right run is a run of its own
                "Inter",
                "Noto Sans Devanagari",
                "Noto Sans Thai",
                "Noto Sans Hebrew"
            ]
        );
    }
}
