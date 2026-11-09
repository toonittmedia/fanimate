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
    /// How hard the edge of a stroke on a raster layer is, from 0 (all soft) to 1. Styles saved
    /// before this existed have none and leave the setting as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardness: Option<f32>,
    /// How strongly each dab of a raster brush lays paint, 0 to 1. Optional like `hardness`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<f32>,
    /// The distance between a raster brush's dabs as a fraction of its size, 0.05 to 1. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
    /// How much pen pressure also lowers a raster brush's flow, 0 to 1. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_pressure: Option<f32>,
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
            && self
                .hardness
                .is_none_or(|h| h.is_finite() && (0.0..=1.0).contains(&h))
            && [self.flow, self.flow_pressure]
                .iter()
                .all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v)))
            && self
                .spacing
                .is_none_or(|v| v.is_finite() && (0.05..=1.0).contains(&v))
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
            hardness: None,
            flow: None,
            spacing: None,
            flow_pressure: None,
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
    fn hardness_is_optional_in_the_file_and_older_styles_read_without_it() {
        // a style as an older Fanimate wrote it: the same file with no hardness at all
        let mut value = serde_json::to_value(StylePreset {
            hardness: Some(0.4),
            ..preset()
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("hardness");
        let old = value.to_string();
        let p: StylePreset = serde_json::from_str(&old).unwrap();
        assert_eq!(p.hardness, None);
        assert!(p.is_sound());
        // a style without one writes none, a style with one keeps it
        assert!(!serde_json::to_string(&p).unwrap().contains("hardness"));
        let soft = StylePreset {
            hardness: Some(0.25),
            ..p
        };
        let again: StylePreset = serde_json::from_str(&serde_json::to_string(&soft).unwrap()).unwrap();
        assert_eq!(again.hardness, Some(0.25));
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
            StylePreset {
                hardness: Some(1.5),
                ..preset()
            },
            StylePreset {
                hardness: Some(f32::NAN),
                ..preset()
            },
            StylePreset {
                flow: Some(-0.1),
                ..preset()
            },
            StylePreset {
                spacing: Some(0.0),
                ..preset()
            },
            StylePreset {
                flow_pressure: Some(f32::INFINITY),
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
