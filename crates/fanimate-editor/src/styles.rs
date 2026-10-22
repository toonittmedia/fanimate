//! Saved styles for brushes and new shapes: choosing one, saving the look in
//! the Style panel as a new one, updating, renaming and deleting, and which
//! one is the default.
//!
//! The panel is a single line over [`StyleBook`]; this module holds the
//! rules so they are tested without a window. The book is the app's list. A
//! project also keeps the styles (`Document::styles`) so another computer has
//! them: opening a project [`merge`](StyleBook::merge)s its styles in.

use fanimate_core::{MAX_STYLES, StylePreset};

use crate::DrawStyle;

/// The built-in style: Fanimate's own starting look. It is always first, and
/// can be neither changed, renamed nor deleted, so there is always a style to
/// go back to.
pub const BUILT_IN: &str = "Default";

/// Why a styles command did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleError {
    /// The name has nothing in it.
    Empty,
    /// Another style has the name (the built-in one counts).
    Taken,
    /// The built-in style cannot be changed.
    BuiltIn,
    /// There is no room for another style.
    Full,
}

impl StyleError {
    /// What to tell the user.
    pub fn message(self) -> &'static str {
        match self {
            Self::Empty => "Give the style a name",
            Self::Taken => "A style with that name already exists",
            Self::BuiltIn => "The Default style cannot be changed. Save a new style instead",
            Self::Full => "Too many styles. Delete one first",
        }
    }
}

impl DrawStyle {
    /// This look as a named style (the eraser and weight brush sizes are
    /// tools' settings, not part of a style).
    pub fn to_preset(&self, name: impl Into<String>) -> StylePreset {
        StylePreset {
            name: name.into(),
            line: self.line,
            width: self.width,
            fill: self.fill,
            outline: self.outline,
            smoothing: self.smoothing,
            taper: self.taper,
            pressure: self.pressure,
        }
    }

    /// Whether this look is the style's, whatever the style is called.
    pub fn is_look_of(&self, preset: &StylePreset) -> bool {
        let same = self.to_preset(preset.name.clone());
        same == *preset
    }

    /// Take on a style's look, held within what the panel allows. The sizes
    /// of the eraser and weight brush stay as they are.
    pub fn apply(&mut self, preset: &StylePreset) {
        self.line = preset.line;
        self.width = preset.width.clamp(0.1, 500.0);
        self.fill = preset.fill;
        self.outline = preset.outline;
        self.smoothing = preset.smoothing.clamp(0.0, 1.0);
        self.taper = preset.taper.clamp(0.0, 500.0);
        self.pressure = preset.pressure;
    }
}

/// The saved styles, which one is chosen, and which is the default.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleBook {
    built_in: StylePreset,
    saved: Vec<StylePreset>,
    /// The name of the style the panel shows as chosen.
    current: String,
    /// The name of the style new sessions start with and Reset goes back to.
    default: String,
}

impl Default for StyleBook {
    fn default() -> Self {
        Self::new(Vec::new(), BUILT_IN)
    }
}

impl StyleBook {
    /// A book from the saved styles and the default's name. Styles that are
    /// unusable, repeated, or named like the built-in one are left out; a
    /// default that is not there falls back to the built-in style.
    pub fn new(saved: Vec<StylePreset>, default: &str) -> Self {
        let mut book = Self {
            built_in: DrawStyle::default().to_preset(BUILT_IN),
            saved: Vec::new(),
            current: BUILT_IN.to_owned(),
            default: BUILT_IN.to_owned(),
        };
        book.merge(&saved);
        if book.get(default).is_some() {
            default.clone_into(&mut book.default);
        }
        book.current = book.default.clone();
        book
    }

    /// Every style, the built-in one first.
    pub fn all(&self) -> impl Iterator<Item = &StylePreset> {
        std::iter::once(&self.built_in).chain(self.saved.iter())
    }

    /// The styles to keep (not the built-in one), for the settings file and
    /// the project.
    pub fn saved(&self) -> &[StylePreset] {
        &self.saved
    }

    pub fn get(&self, name: &str) -> Option<&StylePreset> {
        self.all().find(|p| p.name == name)
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    pub fn default_name(&self) -> &str {
        &self.default
    }

    /// Mark `name` as the chosen style without changing the panel's look,
    /// as when a session starts with the look it ended with. Ignored if
    /// there is no such style.
    pub fn restore_current(&mut self, name: &str) {
        if self.get(name).is_some() {
            name.clone_into(&mut self.current);
        }
    }

    /// Whether the chosen style is the built-in one.
    pub fn current_is_built_in(&self) -> bool {
        self.current == BUILT_IN
    }

    /// Whether the panel's look differs from the chosen style's.
    pub fn is_edited(&self, draw: &DrawStyle) -> bool {
        self.get(&self.current).is_none_or(|p| !draw.is_look_of(p))
    }

    /// Choose a style and take its look. Returns false if there is none by
    /// that name.
    pub fn select(&mut self, name: &str, draw: &mut DrawStyle) -> bool {
        let Some(preset) = self.get(name).cloned() else {
            return false;
        };
        draw.apply(&preset);
        self.current = preset.name;
        true
    }

    /// Save the panel's look as a new style and choose it. Returns its name.
    pub fn save_as(&mut self, name: &str, draw: &DrawStyle) -> Result<String, StyleError> {
        let name = StylePreset::tidy_name(name);
        if name.is_empty() {
            return Err(StyleError::Empty);
        }
        if self.get(&name).is_some() {
            return Err(StyleError::Taken);
        }
        if self.saved.len() >= MAX_STYLES {
            return Err(StyleError::Full);
        }
        self.saved.push(draw.to_preset(name.clone()));
        self.current = name.clone();
        Ok(name)
    }

    /// Replace the chosen style's look with the panel's.
    pub fn update(&mut self, draw: &DrawStyle) -> Result<(), StyleError> {
        let name = self.current.clone();
        let Some(slot) = self.saved.iter_mut().find(|p| p.name == name) else {
            return Err(StyleError::BuiltIn);
        };
        *slot = draw.to_preset(name);
        Ok(())
    }

    /// Rename the chosen style.
    pub fn rename(&mut self, new_name: &str) -> Result<(), StyleError> {
        let new_name = StylePreset::tidy_name(new_name);
        if new_name.is_empty() {
            return Err(StyleError::Empty);
        }
        if new_name == self.current {
            return Ok(());
        }
        if self.current_is_built_in() {
            return Err(StyleError::BuiltIn);
        }
        if self.get(&new_name).is_some() {
            return Err(StyleError::Taken);
        }
        let old = self.current.clone();
        if let Some(slot) = self.saved.iter_mut().find(|p| p.name == old) {
            slot.name = new_name.clone();
        }
        if self.default == old {
            self.default = new_name.clone();
        }
        self.current = new_name;
        Ok(())
    }

    /// Delete the chosen style and go to the default (or the built-in one if
    /// the default was the one deleted).
    pub fn delete(&mut self, draw: &mut DrawStyle) -> Result<(), StyleError> {
        if self.current_is_built_in() {
            return Err(StyleError::BuiltIn);
        }
        let old = self.current.clone();
        self.saved.retain(|p| p.name != old);
        if self.default == old {
            self.default = BUILT_IN.to_owned();
        }
        let default = self.default.clone();
        self.select(&default, draw);
        Ok(())
    }

    /// Make the chosen style the default.
    pub fn set_default(&mut self) {
        self.default = self.current.clone();
    }

    /// Go back to the default style.
    pub fn reset(&mut self, draw: &mut DrawStyle) {
        let default = self.default.clone();
        self.select(&default, draw);
    }

    /// Add styles from elsewhere (a project that was opened) that this book
    /// does not have by name. Unusable ones are skipped. Returns how many
    /// were added.
    pub fn merge(&mut self, incoming: &[StylePreset]) -> usize {
        let mut added = 0;
        for style in incoming {
            let name = StylePreset::tidy_name(&style.name);
            if !style.is_sound()
                || name.is_empty()
                || self.saved.len() >= MAX_STYLES
                || self.get(&name).is_some()
            {
                continue;
            }
            self.saved.push(StylePreset {
                name,
                ..style.clone()
            });
            added += 1;
        }
        added
    }
}

#[cfg(test)]
mod tests {
    use fanimate_core::Color;

    use super::*;

    fn inked() -> DrawStyle {
        DrawStyle {
            line: Color::rgb(0.2, 0.1, 0.4),
            width: 9.0,
            fill: None,
            outline: false,
            smoothing: 0.25,
            taper: 4.0,
            pressure: false,
            ..DrawStyle::default()
        }
    }

    #[test]
    fn a_new_book_has_only_the_built_in_style_and_it_is_the_default() {
        let book = StyleBook::default();
        assert_eq!(book.all().count(), 1);
        assert_eq!(book.current(), BUILT_IN);
        assert_eq!(book.default_name(), BUILT_IN);
        assert!(book.current_is_built_in());
        // its look is Fanimate's starting look, so a fresh panel is not "edited"
        assert!(!book.is_edited(&DrawStyle::default()));
        assert!(book.is_edited(&inked()));
    }

    #[test]
    fn saving_choosing_and_editing_a_style() {
        let mut book = StyleBook::default();
        let mut draw = inked();
        assert_eq!(book.save_as("  Ink  ", &draw), Ok("Ink".to_owned()));
        assert_eq!(book.current(), "Ink");
        assert!(!book.is_edited(&draw));
        // changing the panel marks it edited; choosing another style takes its look
        draw.width = 20.0;
        assert!(book.is_edited(&draw));
        assert!(book.select(BUILT_IN, &mut draw));
        assert_eq!(draw.width, DrawStyle::default().width);
        assert!(!book.is_edited(&draw));
        // the eraser and weight brush sizes are not part of a style
        draw.eraser = 77.0;
        assert!(book.select("Ink", &mut draw));
        assert_eq!(draw.eraser, 77.0);
        assert_eq!(draw.width, 9.0);
        assert!(!draw.pressure);
        assert!(!book.select("Nothing", &mut draw));
        assert_eq!(book.current(), "Ink");
    }

    #[test]
    fn names_must_be_new_and_not_empty() {
        let mut book = StyleBook::default();
        let draw = inked();
        assert_eq!(book.save_as("   ", &draw), Err(StyleError::Empty));
        assert_eq!(book.save_as(BUILT_IN, &draw), Err(StyleError::Taken));
        assert!(book.save_as("Ink", &draw).is_ok());
        assert_eq!(book.save_as("Ink", &draw), Err(StyleError::Taken));
        for i in 0..MAX_STYLES - 1 {
            book.save_as(&format!("s{i}"), &draw).unwrap();
        }
        assert_eq!(book.save_as("one more", &draw), Err(StyleError::Full));
    }

    #[test]
    fn update_rename_and_delete_work_on_saved_styles_only() {
        let mut book = StyleBook::default();
        let mut draw = inked();
        assert_eq!(book.update(&draw), Err(StyleError::BuiltIn));
        assert_eq!(book.rename("Mine"), Err(StyleError::BuiltIn));
        assert_eq!(book.delete(&mut draw), Err(StyleError::BuiltIn));

        book.save_as("Ink", &draw).unwrap();
        draw.width = 3.0;
        assert!(book.is_edited(&draw));
        book.update(&draw).unwrap();
        assert!(!book.is_edited(&draw));
        assert_eq!(book.get("Ink").unwrap().width, 3.0);

        book.save_as("Paint", &draw).unwrap();
        book.select("Ink", &mut draw);
        assert_eq!(book.rename("Paint"), Err(StyleError::Taken));
        assert_eq!(book.rename(" "), Err(StyleError::Empty));
        book.rename("Line art").unwrap();
        assert_eq!(book.current(), "Line art");
        assert!(book.get("Ink").is_none());

        book.delete(&mut draw).unwrap();
        assert!(book.get("Line art").is_none());
        assert_eq!(book.current(), BUILT_IN);
        assert_eq!(book.saved().len(), 1);
    }

    #[test]
    fn the_default_style_is_what_reset_and_delete_go_back_to() {
        let mut book = StyleBook::default();
        let mut draw = inked();
        book.save_as("Ink", &draw).unwrap();
        book.set_default();
        assert_eq!(book.default_name(), "Ink");
        // another style is chosen and the panel is changed, then Reset
        book.select(BUILT_IN, &mut draw);
        draw.width = 1.5;
        book.reset(&mut draw);
        assert_eq!(book.current(), "Ink");
        assert_eq!(draw.width, 9.0);
        // renaming the default keeps it the default
        book.rename("Pen").unwrap();
        assert_eq!(book.default_name(), "Pen");
        // deleting the default makes the built-in style the default again
        book.delete(&mut draw).unwrap();
        assert_eq!(book.default_name(), BUILT_IN);
        assert_eq!(book.current(), BUILT_IN);
    }

    #[test]
    fn a_book_made_from_saved_styles_drops_what_cannot_be_kept() {
        let draw = inked();
        let mut bad = draw.to_preset("Bad");
        bad.width = f32::NAN;
        let saved = vec![
            draw.to_preset("Ink"),
            draw.to_preset("Ink"),
            draw.to_preset(BUILT_IN),
            draw.to_preset("  "),
            bad,
            draw.to_preset("Paint"),
        ];
        let book = StyleBook::new(saved, "Paint");
        let names: Vec<_> = book.all().map(|p| p.name.as_str()).collect();
        assert_eq!(names, [BUILT_IN, "Ink", "Paint"]);
        assert_eq!(book.default_name(), "Paint");
        assert_eq!(book.current(), "Paint");
        // a default that is not there falls back to the built-in style
        assert_eq!(StyleBook::new(vec![], "Gone").default_name(), BUILT_IN);
    }

    #[test]
    fn a_projects_styles_are_merged_by_name() {
        let draw = inked();
        let mut book = StyleBook::default();
        book.save_as("Ink", &draw).unwrap();
        let mut other = draw;
        other.width = 30.0;
        let added = book.merge(&[
            other.to_preset("Ink"),
            other.to_preset("Brush"),
            other.to_preset(BUILT_IN),
        ]);
        assert_eq!(added, 1);
        // the app's own Ink is not replaced by the project's
        assert_eq!(book.get("Ink").unwrap().width, 9.0);
        assert_eq!(book.get("Brush").unwrap().width, 30.0);
    }

    #[test]
    fn applying_holds_numbers_within_what_the_panel_allows() {
        let mut draw = DrawStyle::default();
        let mut wild = draw.to_preset("Wild");
        wild.width = 9999.0;
        wild.smoothing = 7.0;
        wild.taper = -3.0;
        draw.apply(&wild);
        assert_eq!((draw.width, draw.smoothing, draw.taper), (500.0, 1.0, 0.0));
    }

    #[test]
    fn pressure_off_draws_an_even_width() {
        use crate::Editor;
        use fanimate_core::Vec2;
        let mut ed = Editor::untitled();
        let layer = ed
            .doc
            .new_layer("L", fanimate_core::LayerKind::vector(Vec::new()));
        let id = layer.id;
        ed.doc.layers.push(layer);
        ed.select(id, crate::SelectMode::Replace);
        let width_profile = |ed: &mut Editor, pressure: bool| {
            ed.draw.pressure = pressure;
            ed.draw.taper = 0.0;
            ed.draw.smoothing = 0.0;
            assert!(ed.begin_stroke(Vec2::new(0.0, 0.0), 0.2, 1.0));
            for i in 1..40 {
                ed.continue_stroke(Vec2::new(i as f32 * 5.0, 0.0), 0.2);
            }
            ed.finish_stroke();
            let shapes = ed.layer_shapes(id).unwrap();
            shapes.last().unwrap().widths.clone().unwrap_or_default()
        };
        let light = width_profile(&mut ed, true);
        let even = width_profile(&mut ed, false);
        assert!(!light.is_empty() && !even.is_empty());
        // a light touch is thinner than full width; with pressure off it is full
        let mean = |w: &[f32]| w.iter().sum::<f32>() / w.len() as f32;
        assert!(
            mean(&light) < mean(&even) - 0.1,
            "{} vs {}",
            mean(&light),
            mean(&even)
        );
    }
}
