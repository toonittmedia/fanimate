//! Text layers: words that are drawn, and animated, as shapes.
//!
//! A text layer holds a string and how to set it (font, size, weight,
//! tracking, wrapping, alignment, colour), and a list of **animators**. The
//! engine (`fanimate-text`) shapes and lays the string out once and turns
//! each glyph into an ordinary [`Shape`], so the tessellator, the exporters
//! and the browser player need no text code. Evaluating a frame only moves,
//! turns, scales, fades and tints those shapes; it never lays text out again.
//!
//! An **animator** acts on the text a character, a word or a line at a time.
//! Its **selector** says how much each one is affected, from 0 to 1, by where
//! it sits in a range of the text (a start, an end and an offset, in percent,
//! all of them animatable) and the shape of the selection (a hard edge, a
//! ramp, a bell). Its properties say what an affected unit does at full
//! effect: move, turn, grow or shrink, fade, change colour. Everything is an
//! ordinary [`Channel`], so keys, easing, loops and the graph editor work as
//! they do for any other value, and the keying rule applies.
//!
//! The first selected character of a title fading in, the whole line sliding
//! up one word after another, a wave of colour sweeping across: all are a
//! selector range that moves, over a property that is set once.

use std::sync::{Arc, OnceLock};

use glam::{Affine2, Vec2};
use serde::{Deserialize, Serialize};

use crate::Color;
use crate::anim::Animatable;
use crate::anim::{Channel, Frame};
use crate::eval::DrawItem;
use crate::path::{Path, PathCmd, Shape, Stroke, Style};
use crate::scene::LayerId;

use fanimate_text as engine;

/// The font used when a text layer names one Fanimate does not have.
pub const DEFAULT_FONT: &str = engine::fonts::DEFAULT_FAMILY;

/// The font families Fanimate has, in the order a font menu lists them.
pub fn families() -> Vec<&'static str> {
    engine::fonts::families().collect()
}
/// The most animators one text layer holds.
pub const MAX_ANIMATORS: usize = 64;
/// The most OpenType features one text layer switches.
pub const MAX_FEATURES: usize = 32;
/// The longest language tag.
pub const MAX_LANGUAGE: usize = 35;
/// The most characters in one text layer. More are ignored.
pub const MAX_CONTENT: usize = engine::MAX_CHARS;

/// Where lines sit in the box; "start" and "end" follow the text's direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

/// The direction paragraphs start in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextDirection {
    /// From the first strongly directional character.
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "ltr")]
    LeftToRight,
    #[serde(rename = "rtl")]
    RightToLeft,
}

/// An OpenType feature switched on or off for the whole text, such as `liga`
/// (ligatures) or `smcp` (small capitals).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFeature {
    /// Four ASCII letters or digits.
    pub tag: String,
    pub on: bool,
}

impl TextFeature {
    /// Whether the tag is four ASCII letters or digits.
    pub fn is_sound(&self) -> bool {
        self.tag.len() == 4 && self.tag.bytes().all(|b| b.is_ascii_alphanumeric())
    }
}

/// How the text is set. Changing any of it means laying the text out again,
/// so it is only reached through [`Text::look_mut`], which forgets the layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub content: String,
    /// One of the bundled families; another is set in the default font.
    pub family: String,
    /// The em size, in document units.
    pub size: f32,
    /// 100 to 900, for fonts with a weight axis.
    pub weight: f32,
    /// Extra space after every character, in thousandths of an em.
    pub tracking: f32,
    /// Distance between baselines, as a multiple of `size`.
    pub line_height: f32,
    pub align: TextAlign,
    pub direction: TextDirection,
    /// A language tag such as `en` or `yo`; empty when unknown.
    pub language: String,
    pub features: Vec<TextFeature>,
    /// Wrap lines at this width (area text); `None` is point text.
    pub width: Option<f32>,
    pub fill: Color,
    pub stroke: Option<Stroke>,
    /// Set the text along this path instead of a straight line, in the text
    /// layer's own space: the first subpath only, closed or open. The glyphs
    /// stand on it and turn with it. See [`Text::path_offset`].
    pub path: Option<Path>,
    /// Run along the path from its end to its start.
    pub reverse_path: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            content: String::new(),
            family: engine::fonts::DEFAULT_FAMILY.into(),
            size: 72.0,
            weight: 400.0,
            tracking: 0.0,
            line_height: 1.2,
            align: TextAlign::Start,
            direction: TextDirection::Auto,
            language: String::new(),
            features: Vec::new(),
            width: None,
            fill: Color::BLACK,
            stroke: None,
            path: None,
            reverse_path: false,
        }
    }
}

impl Look {
    /// Whether every number is usable and every list within its limit.
    pub fn is_sound(&self) -> bool {
        let (lo, hi) = engine::SIZE_RANGE;
        self.content.chars().count() <= MAX_CONTENT
            && self.size.is_finite()
            && (lo..=hi).contains(&self.size)
            && self.weight.is_finite()
            && (1.0..=1000.0).contains(&self.weight)
            && self.tracking.is_finite()
            && (-1000.0..=1000.0).contains(&self.tracking)
            && self.line_height.is_finite()
            && (0.2..=5.0).contains(&self.line_height)
            && self.width.is_none_or(|w| w.is_finite() && w > 0.0)
            && self.language.chars().count() <= MAX_LANGUAGE
            && self.features.len() <= MAX_FEATURES
            && self.features.iter().all(TextFeature::is_sound)
            && self.fill.is_finite()
            && self
                .stroke
                .is_none_or(|s| s.color.is_finite() && s.width.is_finite() && s.width >= 0.0)
            && self.path.as_ref().is_none_or(path_is_sound)
    }

    /// Hold everything within what [`is_sound`](Self::is_sound) accepts.
    pub fn repair(&mut self) {
        let (lo, hi) = engine::SIZE_RANGE;
        if let Some((end, _)) = self.content.char_indices().nth(MAX_CONTENT) {
            self.content.truncate(end);
        }
        let held =
            |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        self.size = held(self.size, lo, hi, 72.0);
        self.weight = held(self.weight, 1.0, 1000.0, 400.0);
        self.tracking = held(self.tracking, -1000.0, 1000.0, 0.0);
        self.line_height = held(self.line_height, 0.2, 5.0, 1.2);
        if self.width.is_some_and(|w| !(w.is_finite() && w > 0.0)) {
            self.width = None;
        }
        if self.language.chars().count() > MAX_LANGUAGE {
            self.language = self.language.chars().take(MAX_LANGUAGE).collect();
        }
        self.features.retain(TextFeature::is_sound);
        self.features.truncate(MAX_FEATURES);
        if !self.fill.is_finite() {
            self.fill = Color::BLACK;
        }
        if self
            .stroke
            .is_some_and(|s| !(s.color.is_finite() && s.width.is_finite() && s.width >= 0.0))
        {
            self.stroke = None;
        }
        if self.path.as_ref().is_some_and(|p| !path_is_sound(p)) {
            self.path = None;
        }
    }

    /// Whether the font has a weight axis, so `weight` changes how it looks.
    pub fn font_has_weight_axis(&self) -> bool {
        engine::fonts::find(&self.family).is_some_and(|f| f.variable)
    }

    /// Whether the family is one Fanimate has. If not the text is set in the
    /// default font, and the project can say so.
    pub fn font_is_available(&self) -> bool {
        engine::fonts::find(&self.family).is_some()
    }

    fn layout_params<'a>(&'a self, features: &'a [engine::Feature]) -> engine::Params<'a> {
        engine::Params {
            family: &self.family,
            size: self.size,
            weight: self.weight,
            tracking: self.tracking,
            line_height: self.line_height,
            width: self.width,
            align: match self.align {
                TextAlign::Start => engine::Align::Start,
                TextAlign::Center => engine::Align::Center,
                TextAlign::End => engine::Align::End,
            },
            direction: match self.direction {
                TextDirection::Auto => engine::Direction::Auto,
                TextDirection::LeftToRight => engine::Direction::LeftToRight,
                TextDirection::RightToLeft => engine::Direction::RightToLeft,
            },
            language: &self.language,
            features,
        }
    }
}

/// The most commands in the path text is set along.
pub const MAX_PATH_COMMANDS: usize = 10_000;

/// The widest outline an animator can give a glyph, in document units.
const MAX_STROKE: f32 = 1000.0;

/// Whether a path can be set text along: not too long, and every point a place
/// on the page. Anything else is dropped rather than followed.
fn path_is_sound(path: &Path) -> bool {
    path.cmds.len() <= MAX_PATH_COMMANDS
        && path.cmds.iter().all(|c| match c {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => ok(*p),
            PathCmd::CubicTo(a, b, c) => ok(*a) && ok(*b) && ok(*c),
            PathCmd::Close => true,
        })
}

fn ok(p: Vec2) -> bool {
    p.is_finite() && p.abs().max_element() <= 1.0e7
}

/// What an animator acts on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextUnit {
    #[default]
    Character,
    Word,
    Line,
}

impl TextUnit {
    pub const ALL: [TextUnit; 3] = [Self::Character, Self::Word, Self::Line];

    pub fn label(self) -> &'static str {
        match self {
            Self::Character => "Characters",
            Self::Word => "Words",
            Self::Line => "Lines",
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::Character => 0,
            Self::Word => 1,
            Self::Line => 2,
        }
    }
}

/// How much a unit is affected as it sits further through the selected range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorShape {
    /// Everything in the range, fully; nothing outside it.
    #[default]
    Square,
    /// Nothing at the start of the range, fully at its end.
    RampUp,
    /// Fully at the start of the range, nothing at its end.
    RampDown,
    /// Nothing at both ends, fully in the middle, in straight lines.
    Triangle,
    /// Like a triangle but round: a half circle.
    Round,
    /// Like a triangle but smooth: a bell.
    Smooth,
}

impl SelectorShape {
    pub const ALL: [SelectorShape; 6] = [
        Self::Square,
        Self::RampUp,
        Self::RampDown,
        Self::Triangle,
        Self::Round,
        Self::Smooth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::RampUp => "Ramp up",
            Self::RampDown => "Ramp down",
            Self::Triangle => "Triangle",
            Self::Round => "Round",
            Self::Smooth => "Smooth",
        }
    }

    /// The amount, 0 to 1, at `t` (0 to 1) of the way through the range.
    pub fn amount(self, t: f32) -> f32 {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        match self {
            Self::Square => 1.0,
            Self::RampUp => t,
            Self::RampDown => 1.0 - t,
            Self::Triangle => 1.0 - (2.0 * t - 1.0).abs(),
            Self::Round => (1.0 - (2.0 * t - 1.0) * (2.0 * t - 1.0)).max(0.0).sqrt(),
            Self::Smooth => 0.5 - 0.5 * (std::f32::consts::TAU * t).cos(),
        }
    }
}

/// Which units an animator reaches, and how much each.
///
/// The range runs from `start` to `end`, in percent of the text, and `offset`
/// slides the whole range, so animating the offset sweeps it along the text.
/// A unit is at the percent where its middle sits: five characters are at 10,
/// 30, 50, 70 and 90.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Selector {
    #[serde(default = "start_default", skip_serializing_if = "is_start_default")]
    pub start: Channel<f32>,
    #[serde(default = "end_default", skip_serializing_if = "is_end_default")]
    pub end: Channel<f32>,
    #[serde(default = "zero_f32", skip_serializing_if = "is_zero_f32")]
    pub offset: Channel<f32>,
    #[serde(default)]
    pub shape: SelectorShape,
    /// Take the units in a shuffled order instead of reading order.
    #[serde(default, skip_serializing_if = "is_false")]
    pub random: bool,
    /// Which shuffle; the same seed gives the same order on every machine.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub seed: u32,
    /// For the square shape: how wide, in percent of the text, the soft edge at
    /// each end of the range is. 0 is a hard edge; a unit in the soft part is
    /// affected by part of the amount. It is what lets a range that moves along
    /// the text fade characters in one after another instead of switching them.
    #[serde(default, skip_serializing_if = "is_zero_f32_plain")]
    pub smoothness: f32,
}

impl Default for Selector {
    fn default() -> Self {
        Self {
            start: start_default(),
            end: end_default(),
            offset: zero_f32(),
            shape: SelectorShape::Square,
            random: false,
            seed: 0,
            smoothness: 0.0,
        }
    }
}

impl Selector {
    /// How much a unit sitting at `percent` (0 to 100) is affected, given the
    /// range's values on this frame.
    pub fn amount_at(start: f32, end: f32, offset: f32, shape: SelectorShape, percent: f32) -> f32 {
        Self::amount_with(start, end, offset, shape, 0.0, percent)
    }

    /// [`amount_at`](Self::amount_at) with a soft edge: for the square shape, the
    /// amount rises over `smoothness` percent around the start of the range and
    /// falls over as much around its end.
    pub fn amount_with(
        start: f32,
        end: f32,
        offset: f32,
        shape: SelectorShape,
        smoothness: f32,
        percent: f32,
    ) -> f32 {
        let (a, b) = (start + offset, end + offset);
        let (lo, hi) = (a.min(b), a.max(b));
        if shape == SelectorShape::Square && smoothness.is_finite() && smoothness > 0.0 {
            let soft = smoothness.min(1000.0);
            let rise = ((percent - (lo - soft / 2.0)) / soft).clamp(0.0, 1.0);
            let fall = (((hi + soft / 2.0) - percent) / soft).clamp(0.0, 1.0);
            return if percent.is_nan() { 0.0 } else { rise.min(fall) };
        }
        if !(percent >= lo && percent <= hi) {
            return 0.0;
        }
        // a range of no width selects what sits exactly on it, fully
        let t = if (b - a).abs() < 1e-6 {
            1.0
        } else {
            (percent - a) / (b - a)
        };
        shape.amount(t)
    }
}

/// What an animator does to the units it selects, at full effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animator {
    pub name: String,
    #[serde(default)]
    pub unit: TextUnit,
    #[serde(default)]
    pub selector: Selector,
    /// Move by this much.
    #[serde(default = "zero_vec2", skip_serializing_if = "is_zero_vec2")]
    pub position: Channel<Vec2>,
    /// Turn by this much, in radians, about the middle of the unit.
    #[serde(default = "zero_f32", skip_serializing_if = "is_zero_f32")]
    pub rotation: Channel<f32>,
    /// Multiply the size by this, about the middle of the unit (1 is no change).
    #[serde(default = "one_vec2", skip_serializing_if = "is_one_vec2")]
    pub scale: Channel<Vec2>,
    /// Multiply the opacity by this (1 is no change).
    #[serde(default = "one_f32", skip_serializing_if = "is_one_f32")]
    pub opacity: Channel<f32>,
    /// Change the fill towards this colour. `None` leaves the colour alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Channel<Color>>,
    /// Move the point that rotation and scale turn about, from the middle of the unit, by this much.
    #[serde(default = "zero_vec2", skip_serializing_if = "is_zero_vec2")]
    pub anchor: Channel<Vec2>,
    /// Extra space after each character the animator reaches, in thousandths of the size (the
    /// characters after it move along, so the line grows or tightens).
    #[serde(default = "zero_f32", skip_serializing_if = "is_zero_f32")]
    pub tracking: Channel<f32>,
    /// Add this to the outline's width (a glyph with no outline gets one).
    #[serde(default = "zero_f32", skip_serializing_if = "is_zero_f32")]
    pub stroke_width: Channel<f32>,
    /// Change the outline's colour towards this one. `None` leaves it alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<Channel<Color>>,
}

impl Animator {
    pub fn new(name: impl Into<String>, unit: TextUnit) -> Self {
        Self {
            name: name.into(),
            unit,
            selector: Selector::default(),
            position: zero_vec2(),
            rotation: zero_f32(),
            scale: one_vec2(),
            opacity: one_f32(),
            fill: None,
            anchor: zero_vec2(),
            tracking: zero_f32(),
            stroke_width: zero_f32(),
            stroke_color: None,
        }
    }

    /// Every list of keys the animator holds.
    pub fn key_lists(&self, mut f: impl FnMut(&dyn crate::anim::KeyList)) {
        f(&self.selector.start.keys);
        f(&self.selector.end.keys);
        f(&self.selector.offset.keys);
        f(&self.position.keys);
        f(&self.rotation.keys);
        f(&self.scale.keys);
        f(&self.opacity.keys);
        if let Some(fill) = &self.fill {
            f(&fill.keys);
        }
        f(&self.anchor.keys);
        f(&self.tracking.keys);
        f(&self.stroke_width.keys);
        if let Some(c) = &self.stroke_color {
            f(&c.keys);
        }
    }

    /// The same with the lists editable.
    pub fn key_lists_mut(&mut self, mut f: impl FnMut(&mut dyn crate::anim::KeyList)) {
        f(&mut self.selector.start.keys);
        f(&mut self.selector.end.keys);
        f(&mut self.selector.offset.keys);
        f(&mut self.position.keys);
        f(&mut self.rotation.keys);
        f(&mut self.scale.keys);
        f(&mut self.opacity.keys);
        if let Some(fill) = &mut self.fill {
            f(&mut fill.keys);
        }
        f(&mut self.anchor.keys);
        f(&mut self.tracking.keys);
        f(&mut self.stroke_width.keys);
        if let Some(c) = &mut self.stroke_color {
            f(&mut c.keys);
        }
    }
}

fn zero_f32() -> Channel<f32> {
    Channel::new(0.0)
}
fn one_f32() -> Channel<f32> {
    Channel::new(1.0)
}
fn start_default() -> Channel<f32> {
    Channel::new(0.0)
}
fn end_default() -> Channel<f32> {
    Channel::new(100.0)
}
fn zero_vec2() -> Channel<Vec2> {
    Channel::new(Vec2::ZERO)
}
fn one_vec2() -> Channel<Vec2> {
    Channel::new(Vec2::ONE)
}
fn plain<T: crate::anim::Animatable>(c: &Channel<T>, value: T) -> bool {
    c.keys.is_empty() && c.base == value && c.loop_mode.is_off()
}
fn is_zero_f32(c: &Channel<f32>) -> bool {
    plain(c, 0.0)
}
fn is_one_f32(c: &Channel<f32>) -> bool {
    plain(c, 1.0)
}
fn is_start_default(c: &Channel<f32>) -> bool {
    plain(c, 0.0)
}
fn is_end_default(c: &Channel<f32>) -> bool {
    plain(c, 100.0)
}
fn is_zero_vec2(c: &Channel<Vec2>) -> bool {
    plain(c, Vec2::ZERO)
}
fn is_one_vec2(c: &Channel<Vec2>) -> bool {
    plain(c, Vec2::ONE)
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

/// Ready-made animators for the effects titles reach for most. Each is an
/// ordinary [`Animator`] built from the same parts the user has, so once added
/// it can be changed, keyed again or taken apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextPreset {
    /// Characters fade in one after another, each over a soft edge.
    FadeIn,
    /// Characters appear one by one, with nothing between.
    Typewriter,
    /// Words rise into place while they fade in.
    RiseIn,
    /// Characters grow from nothing, one after another.
    PopIn,
    /// A bump runs along the text, lifting the characters it passes, and repeats.
    Wave,
}

impl TextPreset {
    pub const ALL: [TextPreset; 5] = [
        Self::FadeIn,
        Self::Typewriter,
        Self::RiseIn,
        Self::PopIn,
        Self::Wave,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::FadeIn => "Fade in by character",
            Self::Typewriter => "Typewriter",
            Self::RiseIn => "Rise in by word",
            Self::PopIn => "Pop in by character",
            Self::Wave => "Wave",
        }
    }

    /// The animator, with its keys starting on `first` and running for
    /// `frames` frames (at least one).
    pub fn animator(self, first: Frame, frames: Frame) -> Animator {
        use crate::Interpolation::Linear;
        let last = first.saturating_add(frames.max(1));
        let mut a = Animator::new(self.label(), TextUnit::Character);
        // the range of the reveal presets starts before the first unit and ends
        // past the last, and its start sweeps across the text; the soft edge is
        // what turns that into a fade
        let sweep = |a: &mut Animator, from: f32, to: f32| {
            a.selector.end.base = 300.0;
            a.selector.start.set_key(first, from, Linear);
            a.selector.start.set_key(last, to, Linear);
        };
        match self {
            Self::FadeIn => {
                a.opacity.base = 0.0;
                a.selector.smoothness = 40.0;
                sweep(&mut a, -20.0, 125.0);
            }
            Self::Typewriter => {
                a.opacity.base = 0.0;
                sweep(&mut a, 0.0, 100.0);
            }
            Self::RiseIn => {
                a.unit = TextUnit::Word;
                a.position.base = Vec2::new(0.0, 40.0);
                a.opacity.base = 0.0;
                a.selector.smoothness = 50.0;
                sweep(&mut a, -25.0, 125.0);
            }
            Self::PopIn => {
                a.scale.base = Vec2::ZERO;
                a.selector.smoothness = 40.0;
                sweep(&mut a, -20.0, 125.0);
            }
            Self::Wave => {
                a.position.base = Vec2::new(0.0, -24.0);
                a.selector.shape = SelectorShape::Triangle;
                a.selector.start.base = 0.0;
                a.selector.end.base = 40.0;
                a.selector.offset.set_key(first, -40.0, Linear);
                a.selector.offset.set_key(last, 100.0, Linear);
                a.selector.offset.loop_mode = crate::LoopMode::Absolute;
            }
        }
        a
    }
}

/// A path flattened to a polyline, with the distance along it at every
/// point, so glyphs can be placed by how far along the path they are.
#[derive(Debug)]
struct Track {
    points: Vec<Vec2>,
    /// The distance from the start to each point; `along[0]` is 0.
    along: Vec<f32>,
    closed: bool,
}

impl Track {
    /// The first subpath of `path`, flattened (16 steps for each curve),
    /// run backwards when `reverse`. `None` when it is too short to follow.
    fn new(path: &Path, reverse: bool) -> Option<Self> {
        let mut points: Vec<Vec2> = Vec::new();
        let mut closed = false;
        for cmd in &path.cmds {
            match *cmd {
                PathCmd::MoveTo(p) => {
                    if !points.is_empty() {
                        break;
                    }
                    points.push(p);
                }
                PathCmd::LineTo(p) => points.push(p),
                PathCmd::CubicTo(a, b, c) => {
                    let from = *points.last()?;
                    for step in 1..=16 {
                        let t = step as f32 / 16.0;
                        let u = 1.0 - t;
                        points.push(
                            from * (u * u * u)
                                + a * (3.0 * u * u * t)
                                + b * (3.0 * u * t * t)
                                + c * (t * t * t),
                        );
                    }
                }
                PathCmd::Close => {
                    closed = true;
                    if let (Some(first), Some(last)) = (points.first().copied(), points.last().copied())
                        && first != last
                    {
                        points.push(first);
                    }
                    break;
                }
            }
        }
        if reverse {
            points.reverse();
        }
        if points.len() < 2 {
            return None;
        }
        let mut along = Vec::with_capacity(points.len());
        let mut total = 0.0f32;
        along.push(0.0);
        for pair in points.windows(2) {
            total += pair[0].distance(pair[1]);
            along.push(total);
        }
        (total > 1e-3 && total.is_finite()).then_some(Self {
            points,
            along,
            closed,
        })
    }

    fn length(&self) -> f32 {
        self.along.last().copied().unwrap_or(0.0)
    }

    /// The point `s` along the path and the direction it runs there, in
    /// radians. A closed path goes round; past either end of an open one there
    /// is nothing (`None`).
    fn at(&self, s: f32) -> Option<(Vec2, f32)> {
        let len = self.length();
        let s = if self.closed { s.rem_euclid(len) } else { s };
        if !(s >= 0.0 && s <= len) {
            return None;
        }
        let i = self
            .along
            .partition_point(|&d| d <= s)
            .clamp(1, self.points.len() - 1);
        let (a, b) = (self.points[i - 1], self.points[i]);
        let span = self.along[i] - self.along[i - 1];
        let t = if span > 1e-9 {
            (s - self.along[i - 1]) / span
        } else {
            0.0
        };
        let direction = b - a;
        let angle = if direction.length_squared() > 0.0 {
            direction.y.atan2(direction.x)
        } else {
            0.0
        };
        Some((a.lerp(b, t), angle))
    }
}

/// The text laid out and turned into shapes: made once for a look, then only
/// read.
#[derive(Debug)]
pub struct Built {
    /// One shape for each glyph that has ink, in text space (x right, y
    /// down, the top left of the box at the origin).
    shapes: Vec<Shape>,
    /// Alongside `shapes`: where each glyph sits in its character, word and line.
    info: Vec<GlyphInfo>,
    /// How many characters, words and lines there are, by [`TextUnit`] slot.
    counts: [usize; 3],
    /// The size of the box.
    pub size: Vec2,
    /// The path the text is set along, flattened, if it has one that can be followed.
    track: Option<Track>,
    /// The y of the first line's baseline: what sits on the path.
    baseline: f32,
    /// For every character, in order: the index of its character, word and line, and whether it
    /// was set right to left. Tracking is added up along a line with it.
    chars: Vec<CharInfo>,
    /// The em size the text was set at.
    size_em: f32,
}

#[derive(Debug, Clone, Copy)]
struct CharInfo {
    index: [u32; 3],
    rtl: bool,
}

#[derive(Debug)]
struct GlyphInfo {
    /// The index of the glyph's character, word and line.
    index: [u32; 3],
    /// The middle of its character, word and line, to turn and scale about.
    anchor: [Vec2; 3],
    /// The x of the middle of the glyph's advance: where it meets the path.
    centre_x: f32,
}

impl Built {
    /// How many units of each kind the text has.
    pub fn count(&self, unit: TextUnit) -> usize {
        self.counts[unit.slot()]
    }

    /// How many glyphs have ink (spaces have none).
    pub fn glyph_count(&self) -> usize {
        self.shapes.len()
    }

    /// The outline shape of the `n`th glyph with ink, in text space.
    pub fn shape(&self, n: usize) -> Option<&Shape> {
        self.shapes.get(n)
    }
}

fn build(look: &Look) -> Built {
    let features: Vec<engine::Feature> = look
        .features
        .iter()
        .filter(|f| f.is_sound())
        .take(MAX_FEATURES)
        .map(|f| {
            let b = f.tag.as_bytes();
            engine::Feature {
                tag: [b[0], b[1], b[2], b[3]],
                value: u32::from(f.on),
            }
        })
        .collect();
    let laid = engine::layout(&look.content, &look.layout_params(&features));
    let style = Style {
        fill: Some(look.fill),
        stroke: look.stroke,
    };

    // the middle of each character, word and line: the box of the glyphs in it,
    // about halfway up the letters
    let mid = look.size * 0.3;
    let n_words = laid.words.max(1);
    let n_lines = laid.lines.max(1);
    let mut words: Vec<(Vec2, Vec2)> = vec![(Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)); n_words];
    let mut lines: Vec<(Vec2, Vec2)> = vec![(Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)); n_lines];
    let span = |g: &engine::Glyph| {
        (
            Vec2::new(g.origin.x, g.origin.y - mid),
            Vec2::new(g.origin.x + g.advance, g.origin.y - mid),
        )
    };
    for g in &laid.glyphs {
        let (lo, hi) = span(g);
        if let Some(w) = words.get_mut(g.word) {
            w.0 = w.0.min(lo);
            w.1 = w.1.max(hi);
        }
        if let Some(l) = lines.get_mut(g.line) {
            l.0 = l.0.min(lo);
            l.1 = l.1.max(hi);
        }
    }
    let middle = |b: &(Vec2, Vec2)| {
        if b.0.x <= b.1.x {
            (b.0 + b.1) / 2.0
        } else {
            Vec2::ZERO
        }
    };

    let mut shapes = Vec::new();
    let mut info = Vec::new();
    // a glyph can be in a font other than the one asked for (a character it lacks is set in
    // one that has it), so each font used gets its own outliner, made when first needed
    let mut outliners: Vec<(&'static engine::fonts::Font, Option<engine::Outliner>)> = Vec::new();
    {
        for g in &laid.glyphs {
            let at = match outliners.iter().position(|(f, _)| std::ptr::eq(*f, g.font)) {
                Some(at) => at,
                None => {
                    outliners.push((g.font, engine::Outliner::new(g.font, look.weight, look.size)));
                    outliners.len() - 1
                }
            };
            let Some(outliner) = &outliners[at].1 else {
                continue;
            };
            let segs = outliner.glyph(g.id);
            if segs.is_empty() {
                continue;
            }
            let cmds = segs
                .iter()
                .map(|s| match *s {
                    engine::Seg::Move(p) => PathCmd::MoveTo(p + g.origin),
                    engine::Seg::Line(p) => PathCmd::LineTo(p + g.origin),
                    engine::Seg::Cubic(a, b, c) => PathCmd::CubicTo(a + g.origin, b + g.origin, c + g.origin),
                    engine::Seg::Close => PathCmd::Close,
                })
                .collect();
            let name = look.content.get(g.bytes.clone()).unwrap_or_default();
            shapes.push(Shape::new(name, Path { cmds }, style));
            info.push(GlyphInfo {
                index: [g.character as u32, g.word as u32, g.line as u32],
                anchor: [
                    Vec2::new(g.origin.x + g.advance / 2.0, g.origin.y - mid),
                    words.get(g.word).map_or(Vec2::ZERO, middle),
                    lines.get(g.line).map_or(Vec2::ZERO, middle),
                ],
                centre_x: g.origin.x + g.advance / 2.0,
            });
        }
    }
    let mut chars = vec![
        CharInfo {
            index: [0; 3],
            rtl: false
        };
        laid.characters
    ];
    let mut seen = vec![false; laid.characters];
    for g in &laid.glyphs {
        if let (Some(c), Some(done)) = (chars.get_mut(g.character), seen.get_mut(g.character))
            && !*done
        {
            *done = true;
            *c = CharInfo {
                index: [g.character as u32, g.word as u32, g.line as u32],
                rtl: g.rtl,
            };
        }
    }
    Built {
        shapes,
        info,
        chars,
        size_em: look.size,
        counts: [laid.characters, laid.words, laid.lines],
        size: laid.size,
        track: look.path.as_ref().and_then(|p| Track::new(p, look.reverse_path)),
        baseline: laid.glyphs.first().map_or(0.0, |g| g.origin.y),
    }
}

/// A text layer's content: how it is set, and its animators.
#[derive(Clone, Debug)]
pub struct Text {
    look: Look,
    pub animators: Vec<Animator>,
    /// How far along the path the text starts, in percent of its length (keyable). Only
    /// used when the look has a path; animating it scrolls the text along.
    pub path_offset: Channel<f32>,
    /// The layout, made when first needed. A clone shares it, and changing
    /// the look gives that text a fresh, empty one, so a stale layout is never
    /// drawn.
    built: Arc<OnceLock<Built>>,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self::with_look(Look {
            content: content.into(),
            ..Look::default()
        })
    }

    pub fn with_look(look: Look) -> Self {
        Self {
            look,
            animators: Vec::new(),
            path_offset: Channel::new(0.0),
            built: Arc::new(OnceLock::new()),
        }
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    /// Edit how the text is set. The layout is forgotten, and made again the
    /// next time it is drawn.
    pub fn look_mut(&mut self) -> &mut Look {
        self.built = Arc::new(OnceLock::new());
        &mut self.look
    }

    /// The text laid out, shaped into glyph outlines.
    /// The size of the box the text fills, in its own space (the top left is the layer's origin).
    pub fn box_size(&self) -> Vec2 {
        self.built().size
    }

    /// The y of the first line's baseline in the text's own space: how far down from the
    /// top of the box the letters stand.
    pub fn first_baseline(&self) -> f32 {
        self.built().baseline
    }

    pub fn built(&self) -> &Built {
        self.built.get_or_init(|| build(&self.look))
    }

    /// Every list of keys the text's animators hold.
    pub fn key_lists(&self, mut f: impl FnMut(&dyn crate::anim::KeyList)) {
        f(&self.path_offset.keys);
        for a in &self.animators {
            a.key_lists(&mut f);
        }
    }

    /// The same with the lists editable. Animators are not part of the layout,
    /// so this keeps it.
    pub fn key_lists_mut(&mut self, mut f: impl FnMut(&mut dyn crate::anim::KeyList)) {
        f(&mut self.path_offset.keys);
        for a in &mut self.animators {
            a.key_lists_mut(&mut f);
        }
    }

    /// Draw the text on `frame`: one item for each glyph, moved, turned,
    /// scaled, faded and tinted by the animators. A glyph the animators leave
    /// alone, or only move, turn, scale and fade, borrows its shape from the
    /// layout, so a frame costs no allocation for those; one that is tinted
    /// needs its own copy.
    pub(crate) fn items<'a>(
        &'a self,
        layer: LayerId,
        frame: f32,
        world: Affine2,
        opacity: f32,
        scratch: &mut Scratch,
        out: &mut Vec<DrawItem<'a>>,
    ) {
        let built = self.built();
        if built.shapes.is_empty() {
            return;
        }
        scratch.prepare(self, built, frame);
        // where along the path the text starts, as a distance
        let start = built.track.as_ref().map(|t| {
            let percent = self.path_offset.sample(frame);
            if percent.is_finite() {
                percent / 100.0 * t.length()
            } else {
                0.0
            }
        });
        for (n, (shape, glyph)) in built.shapes.iter().zip(&built.info).enumerate() {
            let mut matrix = Affine2::IDENTITY;
            let mut alpha = opacity;
            let mut fill = None;
            let mut stroke_add = 0.0f32;
            let mut stroke_to = None;
            for (animator, s) in self.animators.iter().zip(&scratch.sampled) {
                let slot = animator.unit.slot();
                if let Some(at) = s.shifts {
                    let shift = scratch
                        .shifts
                        .get(at + glyph.index[0] as usize)
                        .copied()
                        .unwrap_or(0.0);
                    matrix = Affine2::from_translation(Vec2::new(shift, 0.0)) * matrix;
                }
                let amount = s.amount(scratch, glyph.index[slot] as usize, built.counts[slot]);
                if amount <= 0.0 {
                    continue;
                }
                let anchor = glyph.anchor[slot] + s.anchor;
                let position = s.position * amount;
                let scale = Vec2::ONE + (s.scale - Vec2::ONE) * amount;
                matrix = Affine2::from_translation(position + anchor)
                    * Affine2::from_angle(s.rotation * amount)
                    * Affine2::from_scale(scale)
                    * Affine2::from_translation(-anchor)
                    * matrix;
                alpha *= 1.0 + (s.opacity - 1.0) * amount;
                if let Some(target) = s.fill {
                    let from = fill.unwrap_or(self.look.fill);
                    fill = Some(Color::lerp(from, target, amount));
                }
                stroke_add += s.stroke_width * amount;
                if let Some(target) = s.stroke_color {
                    let from = stroke_to
                        .or(self.look.stroke.map(|st| st.color))
                        .unwrap_or(self.look.fill);
                    stroke_to = Some(Color::lerp(from, target, amount));
                }
            }
            // on a path the glyph stands on it and turns with it; past the end of an open path
            // there is nothing to stand on
            let mut placed = Affine2::IDENTITY;
            if let (Some(track), Some(start)) = (&built.track, start) {
                match track.at(start + glyph.centre_x) {
                    Some((point, angle)) => {
                        placed = Affine2::from_translation(point)
                            * Affine2::from_angle(angle)
                            * Affine2::from_translation(-Vec2::new(glyph.centre_x, built.baseline));
                    }
                    None => continue,
                }
            }
            // a glyph faded to nothing, or thrown to a place that is not a number, is
            // not drawn (layers that fade out are skipped the same way)
            if alpha <= 0.0
                || !alpha.is_finite()
                || !(matrix.matrix2.is_finite() && matrix.translation.is_finite())
            {
                continue;
            }
            let restyled = fill.is_some() || stroke_to.is_some() || stroke_add != 0.0;
            let shape = if restyled {
                let mut owned = shape.clone();
                if let Some(color) = fill {
                    owned.style.fill = Some(color);
                }
                if stroke_to.is_some() || stroke_add != 0.0 {
                    let base = owned.style.stroke;
                    let width = (base.map_or(0.0, |st| st.width) + stroke_add).clamp(0.0, MAX_STROKE);
                    let color = stroke_to.or(base.map(|st| st.color)).unwrap_or(self.look.fill);
                    owned.style.stroke =
                        (width > 0.0 && width.is_finite()).then_some(Stroke { color, width });
                }
                std::borrow::Cow::Owned(owned)
            } else {
                std::borrow::Cow::Borrowed(shape)
            };
            out.push(DrawItem {
                layer,
                index: n,
                shape,
                image: None,
                part: None,
                transform: world * placed * matrix,
                opacity: alpha.clamp(0.0, 1.0),
            });
        }
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.look == other.look && self.animators == other.animators && self.path_offset == other.path_offset
    }
}

/// One animator's values on one frame.
struct Sampled {
    start: f32,
    end: f32,
    offset: f32,
    shape: SelectorShape,
    smoothness: f32,
    position: Vec2,
    rotation: f32,
    scale: Vec2,
    opacity: f32,
    fill: Option<Color>,
    anchor: Vec2,
    stroke_width: f32,
    stroke_color: Option<Color>,
    /// Where this animator's tracking shifts start in `Scratch::shifts` (one for each
    /// character). `None` when it adds no space.
    shifts: Option<usize>,
    /// Where this animator's shuffled order starts in `Scratch::ranks`; the
    /// order is `count` long. `None` when it reads in order.
    ranks: Option<usize>,
}

impl Sampled {
    /// How much the unit numbered `index` out of `count` is affected.
    fn amount(&self, scratch: &Scratch, index: usize, count: usize) -> f32 {
        if count == 0 {
            return 0.0;
        }
        let place = match self.ranks {
            Some(at) => scratch.ranks.get(at + index).copied().unwrap_or(index as u32) as usize,
            None => index,
        };
        let percent = (place as f32 + 0.5) / count as f32 * 100.0;
        Selector::amount_with(
            self.start,
            self.end,
            self.offset,
            self.shape,
            self.smoothness,
            percent,
        )
    }
}

/// Buffers a frame of text uses, kept between frames so playing does not
/// allocate.
#[derive(Default)]
pub(crate) struct Scratch {
    sampled: Vec<Sampled>,
    ranks: Vec<u32>,
    /// How far each character is pushed along its line by the tracking of every animator before
    /// it, in text units: `count` for each animator that has any.
    shifts: Vec<f32>,
}

impl Scratch {
    /// Sample every animator on `frame` and make the shuffled orders.
    fn prepare(&mut self, text: &Text, built: &Built, frame: f32) {
        self.sampled.clear();
        self.ranks.clear();
        self.shifts.clear();
        for a in text.animators.iter().take(MAX_ANIMATORS) {
            let count = built.counts[a.unit.slot()];
            let ranks = (a.selector.random && count > 1).then(|| {
                let at = self.ranks.len();
                shuffled(count, a.selector.seed, &mut self.ranks);
                at
            });
            let mut sampled = Sampled {
                start: a.selector.start.sample(frame),
                end: a.selector.end.sample(frame),
                offset: a.selector.offset.sample(frame),
                shape: a.selector.shape,
                smoothness: a.selector.smoothness,
                position: a.position.sample(frame),
                rotation: a.rotation.sample(frame),
                scale: a.scale.sample(frame),
                opacity: a.opacity.sample(frame),
                fill: a.fill.as_ref().map(|c| c.sample(frame)),
                anchor: a.anchor.sample(frame),
                stroke_width: a.stroke_width.sample(frame),
                stroke_color: a.stroke_color.as_ref().map(|c| c.sample(frame)),
                shifts: None,
                ranks,
            };
            let tracking = a.tracking.sample(frame);
            if tracking != 0.0 && tracking.is_finite() && !built.chars.is_empty() {
                let at = self.shifts.len();
                sampled.shifts = Some(at);
                // push the characters along their line: each one moves by what the ones before it
                // (in reading order) were given, so a line that is tracked out grows from its start
                let unit = a.unit.slot();
                let per = tracking / 1000.0 * built.size_em;
                self.shifts.resize(at + built.chars.len(), 0.0);
                let (mut line, mut run) = (u32::MAX, 0.0f32);
                for (c, info) in built.chars.iter().enumerate() {
                    if info.index[2] != line {
                        line = info.index[2];
                        run = 0.0;
                    }
                    let sign = if info.rtl { -1.0 } else { 1.0 };
                    self.shifts[at + c] = run * sign;
                    run += sampled.amount(self, info.index[unit] as usize, built.counts[unit]) * per;
                }
            }
            self.sampled.push(sampled);
        }
    }
}

/// Append a shuffle of `0..count` to `out`: Fisher and Yates with a small
/// generator seeded from `seed`, so the same seed gives the same order on every
/// machine. `out[at + unit]` is the place `unit` takes in reading order.
fn shuffled(count: usize, seed: u32, out: &mut Vec<u32>) {
    let at = out.len();
    out.extend(0..count as u32);
    // xorshift64*, seeded so that seed 0 is not a fixed point
    let mut state = (u64::from(seed) << 32 | u64::from(!seed)) ^ 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    for i in (1..count).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        out.swap(at + i, at + j);
    }
}

// The file form: every setting flat in one object, the way a layer file
// reads, instead of nested.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct TextFile {
    content: String,
    family: String,
    size: f32,
    weight: f32,
    #[serde(skip_serializing_if = "is_zero_f32_plain")]
    tracking: f32,
    line_height: f32,
    #[serde(skip_serializing_if = "is_default_value")]
    align: TextAlign,
    #[serde(skip_serializing_if = "is_default_value")]
    direction: TextDirection,
    #[serde(skip_serializing_if = "String::is_empty")]
    language: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    features: Vec<TextFeature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<f32>,
    fill: Color,
    #[serde(skip_serializing_if = "Option::is_none")]
    stroke: Option<Stroke>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    animators: Vec<Animator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Path>,
    #[serde(skip_serializing_if = "is_false")]
    reverse_path: bool,
    #[serde(skip_serializing_if = "is_zero_f32")]
    path_offset: Channel<f32>,
}

fn is_zero_f32_plain(v: &f32) -> bool {
    *v == 0.0
}

fn is_default_value<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

impl Default for TextFile {
    fn default() -> Self {
        Text::with_look(Look::default()).into()
    }
}

impl From<Text> for TextFile {
    fn from(t: Text) -> Self {
        let Look {
            content,
            family,
            size,
            weight,
            tracking,
            line_height,
            align,
            direction,
            language,
            features,
            width,
            fill,
            path,
            reverse_path,
            stroke,
        } = t.look;
        Self {
            content,
            family,
            size,
            weight,
            tracking,
            line_height,
            align,
            direction,
            language,
            features,
            width,
            fill,
            path,
            reverse_path,
            path_offset: t.path_offset,
            stroke,
            animators: t.animators,
        }
    }
}

impl From<TextFile> for Text {
    fn from(f: TextFile) -> Self {
        let mut text = Text::with_look(Look {
            content: f.content,
            family: f.family,
            size: f.size,
            weight: f.weight,
            tracking: f.tracking,
            line_height: f.line_height,
            align: f.align,
            direction: f.direction,
            language: f.language,
            features: f.features,
            width: f.width,
            fill: f.fill,
            stroke: f.stroke,
            path: f.path,
            reverse_path: f.reverse_path,
        });
        text.animators = f.animators;
        text.path_offset = f.path_offset;
        text
    }
}

impl Serialize for Text {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        TextFile::from(self.clone()).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        TextFile::deserialize(d).map(Text::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_amounts_follow_their_shapes() {
        let at = |shape: SelectorShape, t: f32| shape.amount(t);
        assert_eq!(at(SelectorShape::Square, 0.0), 1.0);
        assert_eq!(at(SelectorShape::Square, 1.0), 1.0);
        assert_eq!(at(SelectorShape::RampUp, 0.25), 0.25);
        assert_eq!(at(SelectorShape::RampDown, 0.25), 0.75);
        assert_eq!(at(SelectorShape::Triangle, 0.0), 0.0);
        assert_eq!(at(SelectorShape::Triangle, 0.5), 1.0);
        assert_eq!(at(SelectorShape::Triangle, 0.25), 0.5);
        assert!((at(SelectorShape::Round, 0.5) - 1.0).abs() < 1e-6);
        assert!(at(SelectorShape::Round, 0.0).abs() < 1e-6);
        assert!((at(SelectorShape::Round, 0.25) - 0.866_025_4).abs() < 1e-5);
        assert!(at(SelectorShape::Smooth, 0.0).abs() < 1e-6);
        assert!((at(SelectorShape::Smooth, 0.5) - 1.0).abs() < 1e-6);
        assert!((at(SelectorShape::Smooth, 0.25) - 0.5).abs() < 1e-6);
        for shape in SelectorShape::ALL {
            assert!((0.0..=1.0).contains(&shape.amount(f32::NAN)));
            assert!((0.0..=1.0).contains(&shape.amount(-9.0)));
            assert!((0.0..=1.0).contains(&shape.amount(9.0)));
        }
    }

    #[test]
    fn a_range_selects_the_units_whose_middle_is_inside_it() {
        let amounts = |start, end, offset| -> Vec<f32> {
            // five characters, their middles at 10, 30, 50, 70 and 90 percent
            (0..5)
                .map(|u| {
                    Selector::amount_at(
                        start,
                        end,
                        offset,
                        SelectorShape::Square,
                        (u as f32 + 0.5) / 5.0 * 100.0,
                    )
                })
                .collect()
        };
        assert_eq!(amounts(0.0, 100.0, 0.0), [1.0; 5]);
        assert_eq!(amounts(0.0, 50.0, 0.0), [1.0, 1.0, 1.0, 0.0, 0.0]);
        assert_eq!(amounts(40.0, 80.0, 0.0), [0.0, 0.0, 1.0, 1.0, 0.0]);
        // the offset slides the whole range along the text
        assert_eq!(amounts(0.0, 40.0, 40.0), [0.0, 0.0, 1.0, 1.0, 0.0]);
        assert_eq!(amounts(0.0, 40.0, -20.0), [1.0, 0.0, 0.0, 0.0, 0.0]);
        // a range given backwards is the same range
        assert_eq!(amounts(50.0, 0.0, 0.0), [1.0, 1.0, 1.0, 0.0, 0.0]);
        // a range of no width selects only what sits on it
        assert_eq!(amounts(50.0, 50.0, 0.0), [0.0, 0.0, 1.0, 0.0, 0.0]);
        // a ramp is progress through the range
        let ramp: Vec<f32> = (0..5)
            .map(|u| {
                Selector::amount_at(
                    0.0,
                    100.0,
                    0.0,
                    SelectorShape::RampUp,
                    (u as f32 + 0.5) / 5.0 * 100.0,
                )
            })
            .collect();
        assert!(ramp.windows(2).all(|w| w[1] > w[0]) && (ramp[0] - 0.1).abs() < 1e-5);
        // numbers that are not numbers select nothing and do not panic
        assert_eq!(
            Selector::amount_at(f32::NAN, 100.0, 0.0, SelectorShape::Square, 50.0),
            0.0
        );
        assert_eq!(
            Selector::amount_at(0.0, f32::INFINITY, 0.0, SelectorShape::Square, f32::NAN),
            0.0
        );
    }

    #[test]
    fn a_shuffle_is_a_permutation_that_the_seed_decides() {
        let make = |count, seed| {
            let mut v = Vec::new();
            shuffled(count, seed, &mut v);
            v
        };
        for count in [0usize, 1, 2, 5, 37, 500] {
            let mut sorted = make(count, 7);
            sorted.sort_unstable();
            assert_eq!(sorted, (0..count as u32).collect::<Vec<_>>());
        }
        assert_eq!(make(20, 3), make(20, 3), "the same seed, the same order");
        assert_ne!(make(20, 3), make(20, 4));
        assert_ne!(make(20, 0), (0..20).collect::<Vec<_>>(), "seed 0 still shuffles");
        // appended after what is there
        let mut v = vec![99];
        shuffled(3, 1, &mut v);
        assert_eq!(v[0], 99);
        assert_eq!(v.len(), 4);
    }

    #[test]
    fn a_text_lays_out_once_and_only_a_look_edit_lays_it_out_again() {
        let mut text = Text::new("Hello world");
        let first = text.built() as *const Built;
        assert_eq!(text.built() as *const Built, first, "the layout is kept");
        assert_eq!(text.built().count(TextUnit::Character), 11);
        assert_eq!(text.built().count(TextUnit::Word), 2);
        assert_eq!(text.built().count(TextUnit::Line), 1);
        assert_eq!(text.built().glyph_count(), 10, "the space has no ink");
        // animators do not change the layout
        text.animators.push(Animator::new("a", TextUnit::Word));
        assert_eq!(text.built() as *const Built, first);
        // a look edit does
        text.look_mut().content = "Hi".into();
        assert_eq!(text.built().count(TextUnit::Character), 2);
        // a clone keeps what it had when the original changes
        let copy = text.clone();
        text.look_mut().content = "Hello there again".into();
        assert_eq!(copy.built().count(TextUnit::Character), 2);
        assert_eq!(text.built().count(TextUnit::Word), 3);
    }

    #[test]
    fn glyph_shapes_are_filled_outlines_in_the_look_colours() {
        let mut look = Look {
            content: "I".into(),
            fill: Color::rgba(1.0, 0.0, 0.0, 1.0),
            stroke: Some(Stroke {
                color: Color::WHITE,
                width: 2.0,
            }),
            ..Look::default()
        };
        look.size = 100.0;
        let text = Text::with_look(look);
        let shape = text.built().shape(0).unwrap();
        assert_eq!(shape.style.fill, Some(Color::rgba(1.0, 0.0, 0.0, 1.0)));
        assert_eq!(shape.style.stroke.unwrap().width, 2.0);
        assert!(
            matches!(shape.path.cmds[0], PathCmd::MoveTo(_))
                && shape.path.cmds.last() == Some(&PathCmd::Close)
        );
        assert_eq!(shape.name, "I");
    }

    #[test]
    fn the_file_form_is_flat_short_and_round_trips() {
        let mut text = Text::new("Hi");
        text.look_mut().size = 120.0;
        text.look_mut().weight = 700.0;
        let json = serde_json::to_string(&text).unwrap();
        assert!(
            json.contains(r#""content":"Hi""#) && json.contains(r#""size":120.0"#),
            "{json}"
        );
        for left_out in [
            "tracking",
            "align",
            "direction",
            "language",
            "features",
            "width",
            "stroke",
            "animators",
        ] {
            assert!(!json.contains(left_out), "{left_out} is a default: {json}");
        }
        assert_eq!(serde_json::from_str::<Text>(&json).unwrap(), text);

        let mut a = Animator::new("Rise", TextUnit::Word);
        a.position.base = Vec2::new(0.0, 40.0);
        a.opacity.base = 0.0;
        a.selector.shape = SelectorShape::RampUp;
        a.selector.random = true;
        a.selector.seed = 9;
        a.selector.end.set_key(1, 0.0, crate::Interpolation::Linear);
        a.selector.end.set_key(30, 100.0, crate::Interpolation::Smooth);
        a.fill = Some(Channel::new(Color::WHITE));
        text.animators.push(a);
        let json = serde_json::to_string(&text).unwrap();
        assert_eq!(serde_json::from_str::<Text>(&json).unwrap(), text, "{json}");
        // an animator with nothing set is as short as can be
        let bare = serde_json::to_string(&Animator::new("x", TextUnit::Line)).unwrap();
        assert_eq!(
            bare,
            r#"{"name":"x","unit":"line","selector":{"shape":"square"}}"#
        );
        // a file may leave out everything but the words
        let sparse: Text = serde_json::from_str(r#"{"content":"x"}"#).unwrap();
        assert_eq!(sparse.look().size, 72.0);
        assert_eq!(sparse.look().family, "Inter");
    }

    #[test]
    fn a_look_is_sound_or_is_repaired_to_one() {
        assert!(Look::default().is_sound());
        let mut wild = Look {
            content: "x".repeat(MAX_CONTENT + 5),
            size: f32::NAN,
            weight: -3.0,
            tracking: 1e9,
            line_height: 0.0,
            width: Some(-4.0),
            language: "x".repeat(100),
            features: vec![
                TextFeature {
                    tag: "liga".into(),
                    on: false,
                },
                TextFeature {
                    tag: "toolong".into(),
                    on: true,
                },
                TextFeature {
                    tag: "a b ".into(),
                    on: true,
                },
            ],
            fill: Color::rgba(f32::NAN, 0.0, 0.0, 1.0),
            stroke: Some(Stroke {
                color: Color::WHITE,
                width: -1.0,
            }),
            ..Look::default()
        };
        assert!(!wild.is_sound());
        wild.repair();
        assert!(wild.is_sound(), "{wild:?}");
        assert_eq!(wild.content.chars().count(), MAX_CONTENT);
        assert_eq!(
            wild.features,
            [TextFeature {
                tag: "liga".into(),
                on: false
            }]
        );
        assert_eq!(wild.width, None);
        assert_eq!(wild.size, 72.0);
        // a family Fanimate does not have is allowed but noticed
        let odd = Look {
            family: "Papyrus".into(),
            ..Look::default()
        };
        assert!(odd.is_sound() && !odd.font_is_available());
    }

    // ----- evaluation -----

    use crate::scene::{Document, LayerKind};
    use std::borrow::Cow;

    fn doc_with(text: Text) -> Document {
        let mut doc = Document::new("t", 800.0, 400.0, 24.0);
        let layer = doc.new_layer("title", LayerKind::Text(text));
        doc.layers.push(layer);
        doc
    }

    fn items_at(doc: &Document, frame: f32) -> Vec<(usize, Affine2, f32, bool)> {
        crate::evaluate(doc, frame)
            .into_iter()
            .map(|i| (i.index, i.transform, i.opacity, matches!(i.shape, Cow::Owned(_))))
            .collect()
    }

    fn fade_in_by_character() -> Text {
        let mut text = Text::new("ABCDE");
        text.look_mut().size = 60.0;
        let mut a = Animator::new("Fade", TextUnit::Character);
        a.opacity.base = 0.0;
        a.selector.end.set_key(1, 0.0, crate::Interpolation::Linear);
        a.selector.end.set_key(21, 100.0, crate::Interpolation::Linear);
        text.animators.push(a);
        text
    }

    #[test]
    fn a_text_layer_draws_one_item_per_glyph_with_the_layers_transform_and_opacity() {
        let mut text = Text::new("Hello world");
        text.look_mut().fill = Color::rgba(1.0, 0.0, 0.0, 1.0);
        let mut doc = doc_with(text);
        let items = crate::evaluate(&doc, 1.0);
        assert_eq!(items.len(), 10, "the space has no ink");
        assert!(
            items.iter().all(|i| matches!(i.shape, Cow::Borrowed(_))),
            "no copies for plain text"
        );
        assert!(
            items
                .iter()
                .all(|i| i.transform == Affine2::IDENTITY && i.opacity == 1.0)
        );
        // the layer's own transform and opacity apply to every glyph
        doc.layers[0].transform.position.base = Vec2::new(100.0, 50.0);
        doc.layers[0].opacity.base = 0.5;
        let items = crate::evaluate(&doc, 1.0);
        assert!(
            items
                .iter()
                .all(|i| i.transform.translation == Vec2::new(100.0, 50.0) && i.opacity == 0.5)
        );
        doc.layers[0].visible = false;
        assert!(crate::evaluate(&doc, 1.0).is_empty());
    }

    #[test]
    fn a_selector_range_that_grows_fades_the_characters_in_one_by_one() {
        let doc = doc_with(fade_in_by_character());
        let shown = |frame: f32| items_at(&doc, frame).len();
        // each character's middle is at 10, 30, 50, 70, 90 percent; the opacity
        // animator takes the selected characters to nothing, so the ones the
        // range has not reached yet are the ones still drawn
        assert_eq!(shown(1.0), 5, "range 0 to 0: nothing selected, everything drawn");
        assert_eq!(
            shown(21.0),
            0,
            "range 0 to 100: every character selected, faded out"
        );
        let mut last = 6;
        for frame in 1..=21 {
            let n = shown(frame as f32);
            assert!(n <= last, "frame {frame}: {n} after {last}");
            last = n;
        }
        // halfway the range is 0 to 50: three of five characters are selected
        assert_eq!(shown(11.0), 2);
    }

    #[test]
    fn a_word_animator_moves_the_words_it_reaches_and_turns_them_about_their_middle() {
        let mut text = Text::new("one two three");
        text.look_mut().size = 50.0;
        let mut a = Animator::new("Hop", TextUnit::Word);
        a.position.base = Vec2::new(0.0, -30.0);
        a.rotation.base = std::f32::consts::FRAC_PI_2;
        a.selector.start.base = 40.0; // the middle word only: its middle is at 50 percent
        a.selector.end.base = 60.0;
        text.animators.push(a);
        let doc = doc_with(text.clone());
        let built = text.built();
        let items = crate::evaluate(&doc, 1.0);
        assert_eq!(items.len(), 11);
        for (i, item) in items.iter().enumerate() {
            let word = built.info[i].index[TextUnit::Word.slot()];
            if word == 1 {
                // turned a quarter about the word's middle, then lifted
                let anchor = built.info[i].anchor[TextUnit::Word.slot()];
                let at = item.transform.transform_point2(anchor);
                assert!(
                    (at - (anchor + Vec2::new(0.0, -30.0))).length() < 1e-3,
                    "{at} from {anchor}"
                );
                assert!(item.transform.matrix2.x_axis.y.abs() > 0.99, "turned a quarter");
            } else {
                assert_eq!(item.transform, Affine2::IDENTITY, "word {word} is not touched");
            }
        }
        // a line animator reaches whole lines: one line here, so all of it moves together
        let mut text = Text::new("one two three");
        let mut l = Animator::new("Slide", TextUnit::Line);
        l.position.base = Vec2::new(10.0, 0.0);
        text.animators.push(l);
        let line_doc = doc_with(text);
        let moved = crate::evaluate(&line_doc, 1.0);
        assert!(
            moved
                .iter()
                .all(|i| i.transform.translation == Vec2::new(10.0, 0.0))
        );
    }

    #[test]
    fn scale_opacity_and_colour_blend_towards_their_values_by_the_amount_selected() {
        let mut text = Text::new("AB");
        text.look_mut().fill = Color::BLACK;
        let mut a = Animator::new("Pop", TextUnit::Character);
        a.scale.base = Vec2::splat(3.0);
        a.opacity.base = 0.5;
        a.fill = Some(Channel::new(Color::WHITE));
        a.selector.shape = SelectorShape::RampUp; // two characters sit at 25 and 75 percent: amounts 0.25 and 0.75
        text.animators.push(a);
        let doc = doc_with(text);
        let items = crate::evaluate(&doc, 1.0);
        let scale = |i: usize| items[i].transform.matrix2.x_axis.x;
        assert!(
            (scale(0) - 1.5).abs() < 1e-3 && (scale(1) - 2.5).abs() < 1e-3,
            "{} {}",
            scale(0),
            scale(1)
        );
        assert!((items[0].opacity - 0.875).abs() < 1e-3 && (items[1].opacity - 0.625).abs() < 1e-3);
        let red = |i: usize| items[i].shape.style.fill.unwrap().r;
        assert!((red(0) - 0.25).abs() < 1e-3 && (red(1) - 0.75).abs() < 1e-3);
        assert!(
            items.iter().all(|i| matches!(i.shape, Cow::Owned(_))),
            "tinted glyphs have their own colour"
        );
    }

    #[test]
    fn only_tinted_glyphs_are_copied() {
        let mut text = Text::new("ABCD");
        let mut a = Animator::new("Tint", TextUnit::Character);
        a.fill = Some(Channel::new(Color::WHITE));
        a.selector.end.base = 50.0; // the first two
        text.animators.push(a);
        let owned = items_at(&doc_with(text), 1.0)
            .iter()
            .map(|i| i.3)
            .collect::<Vec<_>>();
        assert_eq!(owned, [true, true, false, false]);
    }

    #[test]
    fn a_random_order_selects_the_same_number_in_a_different_order_and_is_repeatable() {
        let make = |seed: u32| {
            let mut text = Text::new("a b c d e f g h i j");
            let mut a = Animator::new("Pop", TextUnit::Word);
            a.opacity.base = 0.0;
            a.selector.end.base = 50.0;
            a.selector.random = true;
            a.selector.seed = seed;
            text.animators.push(a);
            doc_with(text)
        };
        let shown = |doc: &Document| -> Vec<usize> { items_at(doc, 1.0).iter().map(|i| i.0).collect() };
        for seed in [0, 1, 2, 3] {
            assert_eq!(
                shown(&make(seed)).len(),
                5,
                "half of ten words, whatever the seed"
            );
        }
        assert_eq!(shown(&make(7)), shown(&make(7)));
        let sets: std::collections::BTreeSet<Vec<usize>> = (0..8).map(|s| shown(&make(s))).collect();
        assert!(sets.len() > 3, "seeds give different orders: {sets:?}");
        // without random the first five words are the ones faded out
        let mut text = Text::new("a b c d e f g h i j");
        let mut a = Animator::new("x", TextUnit::Word);
        a.opacity.base = 0.0;
        a.selector.end.base = 50.0;
        text.animators.push(a);
        assert_eq!(shown(&doc_with(text)), [5, 6, 7, 8, 9]);
    }

    #[test]
    fn a_frame_is_the_same_whatever_was_drawn_before_it() {
        let doc = doc_with(fade_in_by_character());
        let straight: Vec<_> = (1..=21).map(|f| items_at(&doc, f as f32)).collect();
        let mut evaluator = crate::eval::Evaluator::default();
        let mut out = Vec::new();
        for frame in [17, 3, 21, 1, 9, 9, 14, 5, 20, 2] {
            evaluator.evaluate_into(&doc, frame as f32, &mut out);
            let got: Vec<_> = out
                .iter()
                .map(|i| (i.index, i.transform, i.opacity, matches!(i.shape, Cow::Owned(_))))
                .collect();
            assert_eq!(got, straight[frame as usize - 1], "frame {frame}");
        }
    }

    #[test]
    fn animators_with_values_that_are_not_numbers_draw_what_they_can_and_do_not_panic() {
        let mut text = Text::new("ABC");
        let mut a = Animator::new("Bad", TextUnit::Character);
        a.position.base = Vec2::new(f32::NAN, 0.0);
        a.selector.end.base = 20.0; // the first character only (the three sit at 17, 50 and 83 percent)
        let mut b = Animator::new("Worse", TextUnit::Word);
        b.selector.start.base = f32::NAN;
        b.opacity.base = f32::INFINITY;
        text.animators.push(a);
        text.animators.push(b);
        for _ in 0..100 {
            text.animators.push(Animator::new("many", TextUnit::Line));
        }
        let doc = doc_with(text);
        let items = crate::evaluate(&doc, 1.0);
        assert!(
            items.len() >= 2,
            "the glyph with the broken position is skipped, not the rest"
        );
        assert!(
            items
                .iter()
                .all(|i| i.transform.translation.is_finite() && i.opacity.is_finite())
        );
        // empty text and a family that is missing draw without trouble
        assert!(crate::evaluate(&doc_with(Text::new("")), 1.0).is_empty());
        let mut odd = Text::new("Hi");
        odd.look_mut().family = "No Such Font".into();
        assert_eq!(crate::evaluate(&doc_with(odd), 1.0).len(), 2);
    }

    #[test]
    fn animator_keys_count_as_the_layers_keys_and_move_with_the_timeline() {
        let mut doc = doc_with(fade_in_by_character());
        assert_eq!(doc.layers[0].key_frames(), [1, 21]);
        assert!(doc.layers[0].is_animated());
        doc.shift_frames(10);
        assert_eq!(doc.layers[0].key_frames(), [11, 31]);
        let mut frames = Vec::new();
        doc.layers[0].key_lists_mut(|k| {
            k.move_key(11, 12);
        });
        doc.layers[0].key_lists(|k| k.frames_into(&mut frames));
        frames.sort_unstable();
        assert_eq!(frames, [12, 31]);
    }

    fn preset_doc(preset: TextPreset) -> (Document, usize) {
        let mut text = Text::new("Fanimate titles");
        text.look_mut().size = 80.0;
        text.animators.push(preset.animator(1, 24));
        let all = text.built().glyph_count();
        (doc_with(text), all)
    }

    #[test]
    fn a_soft_edge_fades_units_across_it_and_a_hard_edge_does_not() {
        let at = |smooth: f32, percent: f32| {
            Selector::amount_with(40.0, 100.0, 0.0, SelectorShape::Square, smooth, percent)
        };
        assert_eq!(at(0.0, 39.0), 0.0);
        assert_eq!(at(0.0, 41.0), 1.0);
        // with a 20 percent soft edge the amount rises from 30 to 50
        assert_eq!(at(20.0, 30.0), 0.0);
        assert!((at(20.0, 40.0) - 0.5).abs() < 1e-5);
        assert_eq!(at(20.0, 50.0), 1.0);
        // and falls over as much around the end
        assert!((at(20.0, 100.0) - 0.5).abs() < 1e-5);
        assert_eq!(at(20.0, 120.0), 0.0);
        // other shapes ignore it, and numbers that are not numbers select nothing
        assert_eq!(
            Selector::amount_with(0.0, 100.0, 0.0, SelectorShape::RampUp, 50.0, 25.0),
            0.25
        );
        assert_eq!(at(f32::NAN, 70.0), 1.0);
        assert_eq!(at(20.0, f32::NAN), 0.0);
    }

    #[test]
    fn fade_in_starts_with_nothing_shown_ends_with_everything_and_fades_in_between() {
        let (doc, all) = preset_doc(TextPreset::FadeIn);
        let items = |f: f32| crate::evaluate(&doc, f);
        assert_eq!(items(1.0).len(), 0, "nothing at the first frame");
        assert_eq!(items(25.0).len(), all, "everything at the last");
        assert!(items(25.0).iter().all(|i| i.opacity == 1.0));
        let mid = items(13.0);
        assert!(!mid.is_empty() && mid.len() < all, "{} of {all}", mid.len());
        assert!(
            mid.iter().any(|i| i.opacity > 0.0 && i.opacity < 1.0),
            "some are part way"
        );
        // the glyphs to the left are ahead of those to the right
        let alpha: Vec<f32> = mid.iter().map(|i| i.opacity).collect();
        assert!(alpha.windows(2).all(|w| w[0] >= w[1] - 1e-5), "{alpha:?}");
        // never goes backwards over the course of the effect
        let mut shown = 0.0;
        for f in 1..=25 {
            let sum: f32 = items(f as f32).iter().map(|i| i.opacity).sum();
            assert!(sum >= shown - 1e-4, "frame {f}");
            shown = sum;
        }
    }

    #[test]
    fn the_typewriter_shows_one_more_character_at_a_time_with_no_fading() {
        let (doc, all) = preset_doc(TextPreset::Typewriter);
        let counts: Vec<usize> = (1..=25).map(|f| crate::evaluate(&doc, f as f32).len()).collect();
        assert_eq!(counts[0], 0);
        assert_eq!(counts[24], all);
        assert!(counts.windows(2).all(|w| w[1] >= w[0]), "{counts:?}");
        assert!(
            crate::evaluate(&doc, 13.0).iter().all(|i| i.opacity == 1.0),
            "characters are not half there"
        );
        // no more than a few characters at a time arrive
        assert!(counts.windows(2).all(|w| w[1] - w[0] <= 3), "{counts:?}");
    }

    #[test]
    fn rise_in_lifts_words_into_place_and_pop_in_grows_characters() {
        let (rise, all) = preset_doc(TextPreset::RiseIn);
        let at_end = crate::evaluate(&rise, 25.0);
        assert_eq!(at_end.len(), all);
        assert!(
            at_end.iter().all(|i| i.transform.translation.y.abs() < 1e-3),
            "in place at the end"
        );
        let mid = crate::evaluate(&rise, 9.0); // the first word is half way through its rise
        assert!(
            mid.iter().any(|i| i.transform.translation.y > 1.0),
            "some are still low, y grows down"
        );

        let (pop, all) = preset_doc(TextPreset::PopIn);
        let end = crate::evaluate(&pop, 25.0);
        assert_eq!(end.len(), all);
        assert!(
            end.iter()
                .all(|i| (i.transform.matrix2.x_axis.x - 1.0).abs() < 1e-3),
            "full size at the end"
        );
        let start = crate::evaluate(&pop, 1.0);
        assert!(
            start.iter().all(|i| i.transform.matrix2.x_axis.x.abs() < 1e-3),
            "no size at the start"
        );
    }

    #[test]
    fn the_wave_lifts_a_moving_group_of_characters_and_repeats() {
        let (doc, _) = preset_doc(TextPreset::Wave);
        let lifted = |f: f32| -> Vec<usize> {
            crate::evaluate(&doc, f)
                .iter()
                .filter(|i| i.transform.translation.y < -1.0)
                .map(|i| i.index)
                .collect()
        };
        let early = lifted(4.0);
        let later = lifted(16.0);
        assert!(!early.is_empty() && !later.is_empty());
        assert!(
            early.iter().sum::<usize>() / early.len() < later.iter().sum::<usize>() / later.len(),
            "it moves right"
        );
        assert_eq!(
            lifted(4.0),
            lifted(4.0 + 24.0),
            "and goes round again one cycle later"
        );
        assert!(lifted(4.0).len() < 8, "a bump, not the whole line");
    }

    #[test]
    fn presets_are_ordinary_animators_that_save_and_load() {
        for preset in TextPreset::ALL {
            let a = preset.animator(5, 30);
            assert_eq!(a.name, preset.label());
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Animator>(&json).unwrap(), a, "{preset:?}");
            // a length of nothing, or a start near the end of time, still makes a usable animator
            let _ = preset.animator(0, 0);
            let _ = preset.animator(i32::MAX, i32::MAX);
        }
        let labels: std::collections::BTreeSet<_> = TextPreset::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(labels.len(), TextPreset::ALL.len());
    }

    // ----- text on a path -----

    fn on_path(content: &str, path: Path) -> Text {
        let mut text = Text::new(content);
        let look = text.look_mut();
        look.size = 40.0;
        look.path = Some(path);
        text
    }

    /// Where the middle of every glyph's baseline lands, and the direction its
    /// baseline runs.
    fn landings(text: &Text) -> Vec<(Vec2, Vec2)> {
        let doc = doc_with(text.clone());
        let built = text.built();
        crate::evaluate(&doc, 1.0)
            .into_iter()
            .map(|i| {
                let info = &built.info[i.index];
                let at = i
                    .transform
                    .transform_point2(Vec2::new(info.centre_x, built.baseline));
                (at, i.transform.transform_vector2(Vec2::X))
            })
            .collect()
    }

    #[test]
    fn text_on_a_straight_path_stands_on_it() {
        let line = Path::new()
            .move_to(Vec2::new(0.0, 100.0))
            .line_to(Vec2::new(1000.0, 100.0));
        let text = on_path("Rail", line);
        let built = text.built();
        let got = landings(&text);
        assert_eq!(got.len(), 4);
        for ((at, along), info) in got.iter().zip(&built.info) {
            assert!((at.x - info.centre_x).abs() < 1e-2, "{at}");
            assert!((at.y - 100.0).abs() < 1e-3, "{at}");
            assert!((*along - Vec2::X).length() < 1e-4);
        }
    }

    #[test]
    fn text_on_a_downward_path_turns_a_quarter() {
        let drop = Path::new()
            .move_to(Vec2::new(50.0, 0.0))
            .line_to(Vec2::new(50.0, 1000.0));
        let text = on_path("Fall", drop);
        let built = text.built();
        for ((at, along), info) in landings(&text).iter().zip(&built.info) {
            assert!((at.x - 50.0).abs() < 1e-3, "{at}");
            assert!((at.y - info.centre_x).abs() < 1e-2, "{at}");
            assert!(
                (*along - Vec2::Y).length() < 1e-4,
                "the baseline runs down: {along}"
            );
        }
    }

    #[test]
    fn running_the_path_backwards_reads_from_its_other_end() {
        let line = Path::new()
            .move_to(Vec2::new(0.0, 0.0))
            .line_to(Vec2::new(500.0, 0.0));
        let mut text = on_path("ab", line);
        text.look_mut().reverse_path = true;
        for (at, along) in landings(&text) {
            assert!((along + Vec2::X).length() < 1e-4, "{along}");
            assert!(at.x < 500.0 && at.x > 400.0, "starts at the far end: {at}");
        }
    }

    #[test]
    fn text_longer_than_an_open_path_is_cut_off_where_the_path_ends() {
        let short = Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(60.0, 0.0));
        let text = on_path("Overflowing", short);
        let drawn = landings(&text).len();
        assert!(drawn > 0 && drawn < text.built().glyph_count(), "{drawn}");
    }

    #[test]
    fn a_closed_path_carries_the_text_round() {
        let circle = Path::new()
            .move_to(Vec2::new(100.0, 0.0))
            .line_to(Vec2::new(100.0, 100.0))
            .line_to(Vec2::new(0.0, 100.0))
            .line_to(Vec2::new(0.0, 0.0))
            .close();
        let mut text = on_path("Round and round and round", circle);
        text.look_mut().size = 30.0;
        // 400 long, the text is longer: nothing is dropped, it goes round
        assert_eq!(landings(&text).len(), text.built().glyph_count());
    }

    #[test]
    fn the_offset_scrolls_the_text_along_and_is_keyed_like_any_channel() {
        let line = Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(1000.0, 0.0));
        let mut text = on_path("go", line);
        text.path_offset.set_key(1, 0.0, crate::Interpolation::Linear);
        text.path_offset.set_key(11, 50.0, crate::Interpolation::Linear);
        let doc = doc_with(text.clone());
        let first = |frame: f32| crate::evaluate(&doc, frame)[0].transform.translation.x;
        assert!((first(11.0) - first(1.0) - 500.0).abs() < 1e-2);
        assert!((first(6.0) - first(1.0) - 250.0).abs() < 1e-2);
        let mut frames = Vec::new();
        text.key_lists(|k| k.frames_into(&mut frames));
        assert_eq!(frames.len(), 2, "the offset's keys count as the layer's");
    }

    #[test]
    fn a_path_that_cannot_be_followed_leaves_the_text_straight() {
        let straight = Text::new("hi");
        let a_point = on_path("hi", Path::new().move_to(Vec2::new(5.0, 5.0)));
        let nothing = on_path("hi", Path::new());
        let same = |t: &Text| {
            let doc = doc_with(t.clone());
            crate::evaluate(&doc, 1.0)
                .iter()
                .map(|i| i.transform)
                .collect::<Vec<_>>()
        };
        assert_eq!(same(&a_point), same(&straight));
        assert_eq!(same(&nothing), same(&straight));
        assert!(a_point.built().track.is_none());
    }

    #[test]
    fn hostile_paths_are_unsound_and_repaired_away() {
        let nan = Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(f32::NAN, 0.0));
        let huge = Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(1.0e30, 0.0));
        let mut many = Path::new().move_to(Vec2::ZERO);
        for n in 0..=MAX_PATH_COMMANDS {
            many = many.line_to(Vec2::splat(n as f32));
        }
        for path in [nan, huge, many] {
            let mut look = Look {
                path: Some(path),
                ..Look::default()
            };
            assert!(!look.is_sound());
            look.repair();
            assert!(look.is_sound() && look.path.is_none());
        }
        // and a path whose points all coincide has no length to follow
        let stuck = on_path("hi", Path::new().move_to(Vec2::ONE).line_to(Vec2::ONE));
        assert!(stuck.built().track.is_none());
        // an offset that is not a number does not move the text off the page
        let mut text = on_path(
            "hi",
            Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(500.0, 0.0)),
        );
        text.path_offset.base = f32::NAN;
        assert!(!landings(&text).is_empty());
    }

    #[test]
    fn a_path_and_its_offset_survive_saving() {
        let curve = Path::new().move_to(Vec2::ZERO).cubic_to(
            Vec2::new(50.0, -80.0),
            Vec2::new(150.0, -80.0),
            Vec2::new(200.0, 0.0),
        );
        let mut text = on_path("Arc", curve);
        text.look_mut().reverse_path = true;
        text.path_offset.set_key(1, 0.0, crate::Interpolation::Linear);
        text.path_offset.set_key(5, 20.0, crate::Interpolation::Linear);
        let json = serde_json::to_string(&text).unwrap();
        let back: Text = serde_json::from_str(&json).unwrap();
        assert_eq!(back, text);
        // text without a path writes nothing for it
        let plain = serde_json::to_string(&Text::new("x")).unwrap();
        assert!(!plain.contains("path"), "{plain}");
    }

    fn xs(doc: &Document, frame: f32) -> Vec<f32> {
        crate::evaluate(doc, frame)
            .into_iter()
            .map(|i| i.transform.translation.x)
            .collect()
    }

    fn tracked(text: &str, tracking: f32) -> Text {
        let mut t = Text::new(text);
        let mut a = Animator::new("Track", TextUnit::Character);
        a.tracking.base = tracking;
        t.animators.push(a);
        t
    }

    #[test]
    fn tracking_pushes_each_character_by_what_the_ones_before_it_were_given() {
        let plain = xs(&doc_with(Text::new("HHH")), 1.0);
        // 500 thousandths of an em after each character: half the 72 point size
        let spread = xs(&doc_with(tracked("HHH", 500.0)), 1.0);
        assert_eq!(plain.len(), 3);
        assert!(
            (spread[0] - plain[0]).abs() < 1e-3,
            "the first has nothing before it"
        );
        assert!((spread[1] - plain[1] - 36.0).abs() < 1e-2, "{spread:?}");
        assert!((spread[2] - plain[2] - 72.0).abs() < 1e-2, "{spread:?}");
        // negative tracking pulls them together
        let tight = xs(&doc_with(tracked("HHH", -100.0)), 1.0);
        assert!((tight[2] - plain[2] + 14.4).abs() < 1e-2, "{tight:?}");
    }

    #[test]
    fn tracking_only_counts_the_characters_the_animator_reaches_and_starts_again_on_each_line() {
        let mut text = tracked("HHH", 500.0);
        text.animators[0].selector.end.base = 40.0; // the first of three characters (at 17 percent)
        let plain = xs(&doc_with(Text::new("HHH")), 1.0);
        let some = xs(&doc_with(text), 1.0);
        assert!((some[1] - plain[1] - 36.0).abs() < 1e-2);
        assert!(
            (some[2] - plain[2] - 36.0).abs() < 1e-2,
            "the others were not given any more: {some:?}"
        );
        // a new line begins at its own start
        let two = xs(&doc_with(tracked("HH\nHH", 500.0)), 1.0);
        let plain_two = xs(&doc_with(Text::new("HH\nHH")), 1.0);
        assert!(
            (two[2] - plain_two[2]).abs() < 1e-3,
            "the second line's first character stays: {two:?}"
        );
        assert!((two[3] - plain_two[3] - 36.0).abs() < 1e-2);
    }

    #[test]
    fn tracking_that_is_not_a_number_is_ignored_and_keys_move_it_over_time() {
        let nan = xs(&doc_with(tracked("HHH", f32::NAN)), 1.0);
        let plain = xs(&doc_with(Text::new("HHH")), 1.0);
        assert_eq!(nan, plain);
        let mut text = Text::new("HHH");
        let mut a = Animator::new("Track", TextUnit::Character);
        a.tracking.set_key(1, 0.0, crate::Interpolation::Linear);
        a.tracking.set_key(11, 1000.0, crate::Interpolation::Linear);
        text.animators.push(a);
        let doc = doc_with(text);
        let start = xs(&doc, 1.0);
        let end = xs(&doc, 11.0);
        assert!((start[2] - plain[2]).abs() < 1e-3);
        assert!(
            (end[2] - plain[2] - 144.0).abs() < 1e-2,
            "two characters of a full em each: {end:?}"
        );
    }

    #[test]
    fn an_animator_can_thicken_and_recolour_the_outline_of_glyphs_that_had_none() {
        let mut text = Text::new("Hi");
        let mut a = Animator::new("Outline", TextUnit::Character);
        a.stroke_width.base = 10.0;
        a.stroke_color = Some(Channel::new(Color::rgb(1.0, 0.0, 0.0)));
        text.animators.push(a);
        let doc = doc_with(text);
        let items = crate::evaluate(&doc, 1.0);
        assert_eq!(items.len(), 2);
        for item in &items {
            let stroke = item.shape.style.stroke.expect("an outline was given");
            assert_eq!(stroke.width, 10.0);
            assert_eq!(stroke.color, Color::rgb(1.0, 0.0, 0.0));
        }
        // text without the animator has none, and one already outlined keeps its colour
        assert!(
            crate::evaluate(&doc_with(Text::new("Hi")), 1.0)[0]
                .shape
                .style
                .stroke
                .is_none()
        );
        let mut outlined = Text::new("Hi");
        outlined.look_mut().stroke = Some(Stroke {
            color: Color::BLACK,
            width: 4.0,
        });
        let mut grow = Animator::new("Grow", TextUnit::Character);
        grow.stroke_width.base = 6.0;
        outlined.animators.push(grow);
        let s = crate::evaluate(&doc_with(outlined), 1.0)[0]
            .shape
            .style
            .stroke
            .unwrap();
        assert_eq!((s.width, s.color), (10.0, Color::BLACK));
    }

    #[test]
    fn an_outline_taken_down_to_nothing_goes_and_hostile_widths_are_held() {
        let mut text = Text::new("Hi");
        text.look_mut().stroke = Some(Stroke {
            color: Color::BLACK,
            width: 4.0,
        });
        let mut remove = Animator::new("Remove", TextUnit::Character);
        remove.stroke_width.base = -50.0;
        text.animators.push(remove);
        assert!(
            crate::evaluate(&doc_with(text), 1.0)[0]
                .shape
                .style
                .stroke
                .is_none()
        );
        let mut text = Text::new("Hi");
        let mut huge = Animator::new("Huge", TextUnit::Character);
        huge.stroke_width.base = 1.0e30;
        let mut bad = Animator::new("Bad", TextUnit::Character);
        bad.stroke_width.base = f32::NAN;
        text.animators.push(huge);
        text.animators.push(bad);
        for item in crate::evaluate(&doc_with(text), 1.0) {
            assert!(
                item.shape
                    .style
                    .stroke
                    .is_none_or(|s| s.width.is_finite() && s.width <= 1000.0)
            );
        }
    }

    #[test]
    fn moving_the_anchor_changes_what_a_turn_or_scale_is_about() {
        let build = |anchor: Vec2| {
            let mut text = Text::new("H");
            let mut a = Animator::new("Grow", TextUnit::Character);
            a.scale.base = Vec2::splat(2.0);
            a.anchor.base = anchor;
            text.animators.push(a);
            crate::evaluate(&doc_with(text), 1.0)[0].transform.translation
        };
        let middle = build(Vec2::ZERO);
        let moved = build(Vec2::new(100.0, 0.0));
        assert!(middle.is_finite() && moved.is_finite());
        // scaling by 2 about a point 100 to the right pulls the glyph 100 to the left of where it grew
        assert!((moved.x - middle.x + 100.0).abs() < 1e-2, "{middle} {moved}");
    }

    #[test]
    fn the_new_animator_properties_survive_saving_and_old_animators_write_nothing_for_them() {
        let mut a = Animator::new("All", TextUnit::Word);
        a.anchor.base = Vec2::new(3.0, -4.0);
        a.tracking.set_key(1, 0.0, crate::Interpolation::Linear);
        a.tracking.set_key(9, 250.0, crate::Interpolation::Smooth);
        a.stroke_width.base = 2.5;
        a.stroke_color = Some(Channel::new(Color::rgb(0.0, 0.5, 1.0)));
        let back: Animator = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back, a);
        let plain = serde_json::to_string(&Animator::new("Plain", TextUnit::Character)).unwrap();
        for key in ["anchor", "tracking", "stroke"] {
            assert!(!plain.contains(key), "{plain}");
        }
        // an animator written before these existed reads with them off
        let old: Animator = serde_json::from_str(r#"{"name":"Old","unit":"word"}"#).unwrap();
        assert_eq!(old.tracking.base, 0.0);
        assert!(old.stroke_color.is_none());
    }
}
