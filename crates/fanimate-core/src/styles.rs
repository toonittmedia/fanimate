//! Saved styles: named looks for new brush strokes and shapes.
//!
//! A style is the colour, size and behaviour of the brush and of new shapes
//! (the Style panel's "Brushes and new shapes"). Styles are kept by the app
//! and also saved in the project (format version 6), so a project opened on
//! another computer still has the styles it was made with.

use serde::{Deserialize, Serialize};

use crate::Color;

/// Most styles a project holds. More are dropped on load.
pub const MAX_STYLES: usize = 64;

/// Longest style name, in characters.
pub const MAX_STYLE_NAME: usize = 60;

/// A named brush look.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StylePreset {
    pub name: String,
    /// Colour of brush strokes and shape outlines.
    pub line: Color,
    /// Brush size at full pressure, and outline width.
    pub width: f32,
    /// Fill of new shapes; none draws outlines only.
    #[serde(default)]
    pub fill: Option<Color>,
    /// Whether new shapes get an outline.
    pub outline: bool,
    /// How much the brush smooths shaky lines, from 0 (off) to 1.
    pub smoothing: f32,
    /// Length of each tapered end of a brush stroke.
    pub taper: f32,
    /// Whether a pen's pressure sets the width.
    pub pressure: bool,
}

impl StylePreset {
    /// Whether every number is usable, so the style can be written to a file
    /// and applied.
    pub fn is_sound(&self) -> bool {
        let colour = |c: &Color| [c.r, c.g, c.b, c.a].iter().all(|v| v.is_finite());
        colour(&self.line)
            && self.fill.as_ref().is_none_or(colour)
            && self.width.is_finite()
            && self.width > 0.0
            && self.smoothing.is_finite()
            && (0.0..=1.0).contains(&self.smoothing)
            && self.taper.is_finite()
            && self.taper >= 0.0
    }

    /// The name tidied to fit: no leading or trailing spaces, at most
    /// [`MAX_STYLE_NAME`] characters. Empty if nothing is left.
    pub fn tidy_name(name: &str) -> String {
        name.trim()
            .chars()
            .take(MAX_STYLE_NAME)
            .collect::<String>()
            .trim()
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset() -> StylePreset {
        StylePreset {
            name: "Ink".into(),
            line: Color::BLACK,
            width: 6.0,
            fill: None,
            outline: true,
            smoothing: 0.5,
            taper: 20.0,
            pressure: true,
        }
    }

    #[test]
    fn a_preset_round_trips_through_json_with_and_without_a_fill() {
        let mut p = preset();
        for fill in [None, Some(Color::WHITE)] {
            p.fill = fill;
            let text = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<StylePreset>(&text).unwrap(), p);
        }
    }

    #[test]
    fn numbers_that_are_not_usable_are_caught() {
        assert!(preset().is_sound());
        for bad in [
            StylePreset {
                width: f32::NAN,
                ..preset()
            },
            StylePreset {
                width: 0.0,
                ..preset()
            },
            StylePreset {
                smoothing: 1.5,
                ..preset()
            },
            StylePreset {
                taper: -1.0,
                ..preset()
            },
            StylePreset {
                fill: Some(Color::rgba(f32::INFINITY, 0.0, 0.0, 1.0)),
                ..preset()
            },
        ] {
            assert!(!bad.is_sound(), "{bad:?}");
        }
    }

    #[test]
    fn names_are_trimmed_and_cut_to_length() {
        assert_eq!(StylePreset::tidy_name("  Ink  "), "Ink");
        assert_eq!(StylePreset::tidy_name("   "), "");
        let long = "x".repeat(200);
        assert_eq!(StylePreset::tidy_name(&long).chars().count(), MAX_STYLE_NAME);
    }
}
