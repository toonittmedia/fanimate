//! Fonts the host installs on the computer's behalf. The provider is global, so
//! this lives in its own test binary: no other test sees it.

use std::sync::atomic::{AtomicUsize, Ordering};

use fanimate_text::fonts::{self, SystemFont, SystemFonts};
use fanimate_text::{Params, layout};

static LOADS: AtomicUsize = AtomicUsize::new(0);

/// Stands in for the computer's fonts: "Test Serif" has a regular and a bold
/// file (both Inter's bytes, which is enough to tell which was asked for by
/// the loads), "Broken" is not a font, and Inter is also offered to prove a
/// bundled family is never replaced.
struct Computer;

const FILE: &[u8] = include_bytes!("../../../brand/fonts/JetBrainsMono-Regular.ttf");

impl SystemFonts for Computer {
    fn families(&self) -> Vec<String> {
        ["Test Serif", "Broken", "Inter", "  Zebra Hand  ", ""]
            .map(String::from)
            .to_vec()
    }

    fn load(&self, family: &str, _bold: bool) -> Option<SystemFont> {
        LOADS.fetch_add(1, Ordering::SeqCst);
        match family {
            "Test Serif" | "Zebra Hand" | "Inter" => Some(SystemFont {
                data: FILE.to_vec(),
                index: 0,
            }),
            "Broken" => Some(SystemFont {
                data: b"not a font".to_vec(),
                index: 0,
            }),
            _ => None,
        }
    }
}

#[test]
fn system_fonts_are_listed_found_loaded_once_and_never_replace_bundled_ones() {
    // before a provider is installed there are none
    assert!(fonts::system_families().is_empty());
    assert!(fonts::find("Test Serif").is_none());

    assert!(fonts::install_system_fonts(Box::new(Computer)));
    // only the first provider counts
    assert!(!fonts::install_system_fonts(Box::new(Computer)));

    // listed sorted and trimmed, without the bundled name and without blanks
    assert_eq!(fonts::system_families(), ["Broken", "Test Serif", "Zebra Hand"]);

    // found whatever the case, and loaded once
    let font = fonts::find("test serif").expect("installed");
    assert_eq!(font.family, "Test Serif");
    assert!(std::ptr::eq(font, fonts::find(" TEST SERIF ").unwrap()));
    assert_eq!(LOADS.load(Ordering::SeqCst), 1);

    // a family nobody listed is never asked for
    assert!(fonts::find("Comic Nobody").is_none());
    assert_eq!(LOADS.load(Ordering::SeqCst), 1);

    // a file that is not a font is refused, and the refusal is remembered
    assert!(fonts::find("Broken").is_none());
    assert!(fonts::find("Broken").is_none());
    assert_eq!(LOADS.load(Ordering::SeqCst), 2);

    // a bundled family keeps its own file
    let inter = fonts::find("Inter").unwrap();
    assert!(inter.variable && inter.data.len() != FILE.len());
    assert_eq!(LOADS.load(Ordering::SeqCst), 2);

    // a heavy weight asks for the bold file, a light one does not
    let before = LOADS.load(Ordering::SeqCst);
    assert!(fonts::find_weighted("Test Serif", 700.0).is_some());
    assert_eq!(LOADS.load(Ordering::SeqCst), before + 1);
    assert!(fonts::find_weighted("Test Serif", 300.0).is_some());
    assert_eq!(LOADS.load(Ordering::SeqCst), before + 1);

    // a missing family falls back to the default font
    assert_eq!(fonts::find_or_default("Comic Nobody").family, "Inter");

    text_set_in_a_system_font_uses_its_glyphs();
}

/// Run at the end of the one test, because the provider is global and tests run in parallel.
fn text_set_in_a_system_font_uses_its_glyphs() {
    let params = Params {
        family: "Test Serif",
        ..Params::default()
    };
    let laid = layout("Hello", &params);
    assert_eq!(laid.glyphs.len(), 5);
    assert!(laid.glyphs.iter().all(|g| g.font.family == "Test Serif"));
    // the stand-in is a monospaced file, so equal letters advance equally
    let wide = layout("lll", &params);
    assert!((wide.glyphs[0].advance - wide.glyphs[2].advance).abs() < 1e-3);
    // the glyphs can be outlined
    let outliner = fanimate_text::Outliner::new(laid.glyphs[0].font, 400.0, 48.0).expect("outlines");
    assert!(!outliner.glyph(laid.glyphs[0].id).is_empty());
}
