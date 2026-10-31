//! The fonts installed on the computer, offered to text layers.
//!
//! `fanimate-text` reads no files, so it asks a [`SystemFonts`] provider for
//! them; this is the one the programs install (`install`). The computer's font
//! folders are scanned once, the first time a name is looked up or the list is
//! shown (`warm` does it in the background at start so the first font menu is
//! not slow), and only the file of a font a text actually uses is read.
//! The browser build has no fonts to read and installs nothing.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::OnceLock;

    use fanimate_text::fonts::{SystemFont, SystemFonts};
    use fontdb::{Database, FaceInfo, Stretch, Style, Weight};

    /// A font database that is scanned on first use.
    pub struct Computer {
        db: OnceLock<Database>,
        scan: fn() -> Database,
    }

    impl Computer {
        pub fn system() -> Self {
            Self {
                db: OnceLock::new(),
                scan: || {
                    let mut db = Database::new();
                    db.load_system_fonts();
                    db
                },
            }
        }

        /// Reads one folder instead of the computer's (the tests use it).
        #[cfg(test)]
        pub fn folder(dir: std::path::PathBuf) -> Self {
            // a fn pointer cannot capture, so the folder goes through a static
            static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
            let _ = DIR.set(dir);
            Self {
                db: OnceLock::new(),
                scan: || {
                    let mut db = Database::new();
                    db.load_fonts_dir(DIR.get().expect("folder set"));
                    db
                },
            }
        }

        fn db(&self) -> &Database {
            self.db.get_or_init(self.scan)
        }
    }

    /// The name a menu shows for a face: its first family (the English one when
    /// the font has several), unless it is a hidden variant (Windows lists
    /// vertical versions as "@Name").
    fn primary(face: &FaceInfo) -> Option<&str> {
        let name = face.families.first()?.0.trim();
        (!name.is_empty() && !name.starts_with(['@', '.'])).then_some(name)
    }

    /// How far a face is from what was asked for; the upright, normal-width
    /// faces of the family come first, then the nearest weight.
    fn distance(face: &FaceInfo, weight: u16) -> (bool, bool, u16) {
        (
            face.style != Style::Normal,
            face.stretch != Stretch::Normal,
            face.weight.0.abs_diff(weight),
        )
    }

    impl SystemFonts for Computer {
        fn families(&self) -> Vec<String> {
            let mut names: Vec<String> = self.db().faces().filter_map(primary).map(str::to_owned).collect();
            names.sort_by_key(|n| n.to_lowercase());
            names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
            names
        }

        fn load(&self, family: &str, bold: bool) -> Option<SystemFont> {
            let db = self.db();
            let want = if bold { Weight::BOLD.0 } else { Weight::NORMAL.0 };
            let face = db
                .faces()
                .filter(|f| primary(f).is_some_and(|n| n.eq_ignore_ascii_case(family)))
                .min_by_key(|f| distance(f, want))?;
            let index = face.index;
            db.with_face_data(face.id, |data, _| SystemFont {
                data: data.to_vec(),
                index,
            })
        }
    }
}

/// Lets text layers use the fonts installed on the computer. Safe to call more
/// than once; cheap (nothing is scanned until a font is needed).
pub fn install() {
    #[cfg(not(target_arch = "wasm32"))]
    fanimate_text::fonts::install_system_fonts(Box::new(native::Computer::system()));
}

/// Scans the computer's fonts on another thread, so the first font menu and the
/// first project that names a font do not wait for the scan.
pub fn warm() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // an unspawnable thread only means the scan happens on first use
        let _ = std::thread::Builder::new().name("system-fonts".into()).spawn(|| {
            fanimate_text::fonts::system_families();
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use fanimate_text::fonts::SystemFonts;

    use super::native::Computer;

    fn fonts_folder() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../brand/fonts")
    }

    #[test]
    fn a_folder_of_fonts_lists_its_families_and_loads_one_file() {
        let computer = Computer::folder(fonts_folder());
        let families = computer.families();
        assert!(families.iter().any(|f| f == "JetBrains Mono"), "{families:?}");
        assert!(families.iter().any(|f| f == "Noto Sans Thai"), "{families:?}");
        assert!(families.iter().all(|f| !f.starts_with('@')));
        // sorted without regard to case, each once
        let mut sorted = families.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(families, sorted);

        let file = computer.load("jetbrains mono", false).expect("regular");
        assert!(file.data.len() > 10_000);
        assert!(
            fanimate_text::Outliner::new(
                Box::leak(Box::new(fanimate_text::fonts::Font {
                    family: "x",
                    data: Box::leak(file.data.into_boxed_slice()),
                    variable: false,
                    scripts: &[],
                    index: file.index
                })),
                400.0,
                48.0
            )
            .is_some()
        );
        assert!(computer.load("No Such Font", false).is_none());
        // no bold file: bold asks for the nearest, which is the regular one
        assert!(computer.load("JetBrains Mono", true).is_some());
    }
}
