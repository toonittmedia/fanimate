//! The fonts that ship with Fanimate.
//!
//! Inter and JetBrains Mono are bundled (SIL Open Font Licence, see
//! `brand/fonts`). A text layer names a family; one that is not here falls
//! back to the default and the caller can tell (see [`find`]), so a project
//! made with another font still opens and says so. Fonts for more scripts
//! (Arabic, Hebrew, Devanagari, Thai) are added here, each with its licence
//! recorded in `docs/legal/licences.md`. A character the chosen font does not
//! have is set in the font that does (see [`covering`]), so mixed text works
//! without the user picking a font for each script.

use unicode_script::Script;

/// One bundled font file.
pub struct Font {
    pub family: &'static str,
    pub data: &'static [u8],
    /// Whether it has a weight axis (`wght`), so weights other than its own
    /// can be asked for.
    pub variable: bool,
    /// The scripts the font is for, or empty for a general font that any script
    /// may use when it has the glyph. A script font is only a fallback for the
    /// scripts named here (and a user may still choose it by name).
    pub scripts: &'static [Script],
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Font({})", self.family)
    }
}

/// Fonts are the same when they are the same family: the bundled set has one file per family.
impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.family == other.family
    }
}

/// The family used when none is named, or the one named is not here.
pub const DEFAULT_FAMILY: &str = "Inter";

static FONTS: [Font; 6] = [
    Font {
        family: "Inter",
        data: include_bytes!("../../../brand/fonts/Inter-Variable.ttf"),
        variable: true,
        scripts: &[],
    },
    Font {
        family: "JetBrains Mono",
        data: include_bytes!("../../../brand/fonts/JetBrainsMono-Regular.ttf"),
        variable: false,
        scripts: &[],
    },
    Font {
        family: "Noto Sans Arabic",
        data: include_bytes!("../../../brand/fonts/NotoSansArabic-Regular.ttf"),
        variable: false,
        scripts: &[Script::Arabic],
    },
    Font {
        family: "Noto Sans Hebrew",
        data: include_bytes!("../../../brand/fonts/NotoSansHebrew-Regular.ttf"),
        variable: false,
        scripts: &[Script::Hebrew],
    },
    Font {
        family: "Noto Sans Devanagari",
        data: include_bytes!("../../../brand/fonts/NotoSansDevanagari-Regular.ttf"),
        variable: false,
        scripts: &[Script::Devanagari],
    },
    Font {
        family: "Noto Sans Thai",
        data: include_bytes!("../../../brand/fonts/NotoSansThai-Regular.ttf"),
        variable: false,
        scripts: &[Script::Thai],
    },
];

/// Every bundled font, general fonts first.
pub fn all() -> &'static [Font] {
    &FONTS
}

/// Whether `font` may be used for text in `script`: a general font for any,
/// a script font for its own scripts.
pub fn suits(font: &Font, script: Script) -> bool {
    font.scripts.is_empty() || font.scripts.contains(&script)
}

/// The first bundled font that suits `c`'s script and has a glyph for it, if any.
pub fn covering(c: char) -> Option<&'static Font> {
    use unicode_script::UnicodeScript;
    let script = c.script();
    FONTS.iter().find(|f| {
        suits(f, script)
            && rustybuzz::ttf_parser::Face::parse(f.data, 0)
                .ok()
                .is_some_and(|face| face.glyph_index(c).is_some())
    })
}

/// The bundled families, in the order a font menu lists them.
pub fn families() -> impl Iterator<Item = &'static str> {
    FONTS.iter().map(|f| f.family)
}

/// The font for `family`, ignoring case and surrounding spaces; `None` if
/// Fanimate does not have it.
pub fn find(family: &str) -> Option<&'static Font> {
    let family = family.trim();
    FONTS.iter().find(|f| f.family.eq_ignore_ascii_case(family))
}

/// The font for `family`, or the default font if there is no such family.
pub fn find_or_default(family: &str) -> &'static Font {
    find(family).unwrap_or(&FONTS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_are_found_whatever_their_case() {
        assert_eq!(find("Inter").unwrap().family, "Inter");
        assert_eq!(find("  inter ").unwrap().family, "Inter");
        assert_eq!(find("JETBRAINS MONO").unwrap().family, "JetBrains Mono");
        assert!(find("Comic Sans").is_none());
        assert_eq!(find_or_default("Comic Sans").family, DEFAULT_FAMILY);
        assert_eq!(
            families().collect::<Vec<_>>(),
            [
                "Inter",
                "JetBrains Mono",
                "Noto Sans Arabic",
                "Noto Sans Hebrew",
                "Noto Sans Devanagari",
                "Noto Sans Thai"
            ]
        );
    }

    #[test]
    fn every_bundled_font_parses() {
        for f in &FONTS {
            let face = rustybuzz::Face::from_slice(f.data, 0).expect(f.family);
            assert!(
                face.units_per_em() > 0 && face.number_of_glyphs() > 100,
                "{}",
                f.family
            );
            assert_eq!(
                face.variation_axes()
                    .into_iter()
                    .any(|a| a.tag == ttf_tag(b"wght")),
                f.variable,
                "{}",
                f.family
            );
        }
    }

    #[test]
    fn each_script_has_a_font_that_covers_it_and_latin_stays_in_the_default() {
        for (c, family) in [
            ('a', "Inter"),
            ('\u{0434}', "Inter"),
            ('\u{0645}', "Noto Sans Arabic"),
            ('\u{05d0}', "Noto Sans Hebrew"),
            ('\u{0915}', "Noto Sans Devanagari"),
            ('\u{0e01}', "Noto Sans Thai"),
        ] {
            assert_eq!(covering(c).map(|f| f.family), Some(family), "{c:?}");
        }
    }

    fn ttf_tag(b: &[u8; 4]) -> rustybuzz::ttf_parser::Tag {
        rustybuzz::ttf_parser::Tag::from_bytes(b)
    }
}
