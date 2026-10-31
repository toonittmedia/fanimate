//! Colours.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// An sRGB colour with straight (non-premultiplied) alpha, each channel 0..=1.
///
/// Stored in project files as a hex string (`"#8450F5"` or `"#8450F5CC"`)
/// so that files stay readable and diff cleanly. A colour that 8-bit hex
/// cannot hold exactly (for example a blend) is stored as `[r, g, b, a]`
/// instead, so saving never changes a colour.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "ColorRepr", try_from = "ColorRepr")]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Parse `#RGB`, `#RRGGBB` or `#RRGGBBAA` (the `#` is optional).
    pub fn from_hex(hex: &str) -> Result<Self, ParseColorError> {
        let h = hex.trim().trim_start_matches('#');
        let digit = |i: usize| {
            u8::from_str_radix(h.get(i..i + 1).ok_or(ParseColorError)?, 16).map_err(|_| ParseColorError)
        };
        let byte = |i: usize| {
            u8::from_str_radix(h.get(i..i + 2).ok_or(ParseColorError)?, 16).map_err(|_| ParseColorError)
        };
        let (r, g, b, a) = match h.len() {
            3 => (digit(0)? * 17, digit(1)? * 17, digit(2)? * 17, 255),
            6 => (byte(0)?, byte(2)?, byte(4)?, 255),
            8 => (byte(0)?, byte(2)?, byte(4)?, byte(6)?),
            _ => return Err(ParseColorError),
        };
        Ok(Self::rgba(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ))
    }

    /// Format as `#RRGGBB`, or `#RRGGBBAA` when not fully opaque.
    pub fn to_hex(self) -> String {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if q(self.a) == 255 {
            format!("#{:02X}{:02X}{:02X}", q(self.r), q(self.g), q(self.b))
        } else {
            format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                q(self.r),
                q(self.g),
                q(self.b),
                q(self.a)
            )
        }
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Convert to linear light with premultiplied alpha, as the GPU blends.
    pub fn to_linear_premultiplied(self) -> [f32; 4] {
        fn lin(c: f32) -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        let a = self.a.clamp(0.0, 1.0);
        [lin(self.r) * a, lin(self.g) * a, lin(self.b) * a, a]
    }

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let m = |x: f32, y: f32| x + (y - x) * t;
        let mixed = Self::rgba(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a));
        // an easing that overshoots takes t past the ends; a colour cannot follow
        if (0.0..=1.0).contains(&t) {
            mixed
        } else {
            Self::rgba(
                mixed.r.clamp(0.0, 1.0),
                mixed.g.clamp(0.0, 1.0),
                mixed.b.clamp(0.0, 1.0),
                mixed.a.clamp(0.0, 1.0),
            )
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseColorError;

impl fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a colour like #RRGGBB or #RRGGBBAA")
    }
}

impl std::error::Error for ParseColorError {}

impl FromStr for Color {
    type Err = ParseColorError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum ColorRepr {
    Hex(String),
    Floats([f32; 4]),
}

impl From<Color> for ColorRepr {
    fn from(c: Color) -> Self {
        let hex = c.to_hex();
        match Color::from_hex(&hex) {
            Ok(back) if back == c => ColorRepr::Hex(hex),
            _ => ColorRepr::Floats([c.r, c.g, c.b, c.a]),
        }
    }
}

impl TryFrom<ColorRepr> for Color {
    type Error = ParseColorError;
    fn try_from(r: ColorRepr) -> Result<Self, Self::Error> {
        match r {
            ColorRepr::Hex(s) => Self::from_hex(&s),
            ColorRepr::Floats([r, g, b, a]) => Ok(Self::rgba(r, g, b, a)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c = Color::from_hex("#8450F5").unwrap();
        assert_eq!(c.to_hex(), "#8450F5");
        assert_eq!(Color::from_hex("8450f5cc").unwrap().to_hex(), "#8450F5CC");
        assert_eq!(Color::from_hex("#fff").unwrap(), Color::WHITE);
        assert!(Color::from_hex("#12345").is_err());
        assert!(Color::from_hex("#zzzzzz").is_err());
    }

    #[test]
    fn serialises_as_hex_string() {
        let json = serde_json::to_string(&Color::from_hex("#1C0B3D").unwrap()).unwrap();
        assert_eq!(json, "\"#1C0B3D\"");
    }

    #[test]
    fn non_8_bit_colours_round_trip_exactly() {
        let c = Color::lerp(Color::BLACK, Color::WHITE, 0.3);
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.starts_with('['), "{json}");
        assert_eq!(serde_json::from_str::<Color>(&json).unwrap(), c);
    }
}
