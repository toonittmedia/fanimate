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
//!
//! The fonts installed on the computer are reached through a [`SystemFonts`]
//! provider the host installs (this crate does no IO): the family names it
//! lists can be chosen like a bundled font, and are loaded, once, the first time
//! a text uses them. Bundled fonts win a name clash, so the look of the
//! bundled set never depends on the computer.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use unicode_script::Script;

/// One font file: a bundled one, or one the computer has (loaded on first use).
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
    /// Which face of the file to use (a collection holds several).
    pub index: u32,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Font({})", self.family)
    }
}

/// Fonts are the same when they are the same family: the set has one file per family and weight.
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
        index: 0,
    },
    Font {
        family: "JetBrains Mono",
        data: include_bytes!("../../../brand/fonts/JetBrainsMono-Regular.ttf"),
        variable: false,
        scripts: &[],
        index: 0,
    },
    Font {
        family: "Noto Sans Arabic",
        data: include_bytes!("../../../brand/fonts/NotoSansArabic-Regular.ttf"),
        variable: false,
        scripts: &[Script::Arabic],
        index: 0,
    },
    Font {
        family: "Noto Sans Hebrew",
        data: include_bytes!("../../../brand/fonts/NotoSansHebrew-Regular.ttf"),
        variable: false,
        scripts: &[Script::Hebrew],
        index: 0,
    },
    Font {
        family: "Noto Sans Devanagari",
        data: include_bytes!("../../../brand/fonts/NotoSansDevanagari-Regular.ttf"),
        variable: false,
        scripts: &[Script::Devanagari],
        index: 0,
    },
    Font {
        family: "Noto Sans Thai",
        data: include_bytes!("../../../brand/fonts/NotoSansThai-Regular.ttf"),
        variable: false,
        scripts: &[Script::Thai],
        index: 0,
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
            && rustybuzz::ttf_parser::Face::parse(f.data, f.index)
                .ok()
                .is_some_and(|face| face.glyph_index(c).is_some())
    })
}

/// The bundled families, in the order a font menu lists them.
pub fn families() -> impl Iterator<Item = &'static str> {
    FONTS.iter().map(|f| f.family)
}

/// A font file the host found on the computer.
pub struct SystemFont {
    pub data: Vec<u8>,
    /// Which face of the file (a collection holds several).
    pub index: u32,
}

/// The fonts installed on the computer, supplied by the host (this crate reads
/// no files). Both calls may be slow the first time; the answers are kept.
pub trait SystemFonts: Send + Sync {
    /// The family names, each once.
    fn families(&self) -> Vec<String>;
    /// The regular face of `family`, or its bold one when `bold` is set and
    /// the family has a separate bold file. `None` if it cannot be read.
    fn load(&self, family: &str, bold: bool) -> Option<SystemFont>;
}

/// The system fonts, what the provider listed, and the ones loaded so far.
struct System {
    provider: Box<dyn SystemFonts>,
    /// Lower-case name to the name as the provider spells it.
    names: OnceLock<HashMap<String, String>>,
    loaded: Mutex<HashMap<(String, bool), Option<&'static Font>>>,
}

static SYSTEM: OnceLock<System> = OnceLock::new();

/// Lets text use the fonts installed on the computer. Only the first call
/// counts (the host installs one provider at start); returns whether this one
/// was taken.
pub fn install_system_fonts(provider: Box<dyn SystemFonts>) -> bool {
    SYSTEM
        .set(System {
            provider,
            names: OnceLock::new(),
            loaded: Mutex::new(HashMap::new()),
        })
        .is_ok()
}

impl System {
    fn names(&self) -> &HashMap<String, String> {
        self.names.get_or_init(|| {
            self.provider
                .families()
                .into_iter()
                .filter(|f| !f.trim().is_empty() && f.len() <= 200)
                .map(|f| (f.trim().to_lowercase(), f.trim().to_owned()))
                .collect()
        })
    }

    /// The font for a family the provider listed (a name it did not list is
    /// never loaded, so a hostile project cannot fill the cache). A bold
    /// request for a family with a weight axis is the regular file.
    fn font(&self, family: &str, bold: bool) -> Option<&'static Font> {
        let name = self.names().get(&family.trim().to_lowercase())?;
        let key = (name.to_lowercase(), bold);
        if let Some(found) = self.loaded.lock().ok()?.get(&key) {
            return *found;
        }
        let font = if bold {
            // a variable font has every weight in its regular file
            match self.font(name, false) {
                Some(regular) if regular.variable => Some(regular),
                _ => self.read(name, true),
            }
        } else {
            self.read(name, false)
        };
        self.loaded.lock().ok()?.insert(key, font);
        font
    }

    fn read(&self, name: &str, bold: bool) -> Option<&'static Font> {
        let file = self.provider.load(name, bold)?;
        let data: &'static [u8] = Box::leak(file.data.into_boxed_slice());
        // a file that is not a font is not used
        let face = rustybuzz::Face::from_slice(data, file.index)?;
        if face.units_per_em() == 0 || face.number_of_glyphs() == 0 {
            return None;
        }
        let variable = face
            .variation_axes()
            .into_iter()
            .any(|a| a.tag == rustybuzz::ttf_parser::Tag::from_bytes(b"wght"));
        Some(Box::leak(Box::new(Font {
            family: Box::leak(name.to_owned().into_boxed_str()),
            data,
            variable,
            scripts: &[],
            index: file.index,
        })))
    }
}

/// The families installed on the computer that a font menu can add to the
/// bundled ones, sorted, without the bundled names. Empty until a provider is
/// installed.
pub fn system_families() -> Vec<String> {
    let Some(system) = SYSTEM.get() else {
        return Vec::new();
    };
    let mut names: Vec<String> = system
        .names()
        .values()
        .filter(|n| find_bundled(n).is_none())
        .cloned()
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

fn find_bundled(family: &str) -> Option<&'static Font> {
    let family = family.trim();
    FONTS.iter().find(|f| f.family.eq_ignore_ascii_case(family))
}

/// The font for `family`, ignoring case and surrounding spaces: a bundled one,
/// else one the computer has; `None` if neither does.
pub fn find(family: &str) -> Option<&'static Font> {
    find_weighted(family, 400.0)
}

/// Like [`find`], for text of the given weight: a family that is a separate
/// file per weight gives its bold file from 600 up.
pub fn find_weighted(family: &str, weight: f32) -> Option<&'static Font> {
    find_bundled(family).or_else(|| SYSTEM.get()?.font(family, weight >= 600.0))
}

/// The font for `family`, or the default font if there is no such family.
pub fn find_or_default(family: &str) -> &'static Font {
    find_weighted(family, 400.0).unwrap_or(&FONTS[0])
}

/// The font for `family` at `weight`, or the default font if there is no such family.
pub fn find_weighted_or_default(family: &str, weight: f32) -> &'static Font {
    find_weighted(family, weight).unwrap_or(&FONTS[0])
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
