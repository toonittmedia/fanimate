//! Where a layer's content came from, when a tool rather than the artist's
//! own hand made it.
//!
//! Fanimate's AI tools never replace the artist's control and never hide
//! what they did: whatever they make becomes ordinary layers, and each such
//! layer carries a record of the tool, so a studio can tell what was
//! generated (format version 7). A layer the artist drew has none.

use serde::{Deserialize, Serialize};

/// The longest text a record holds, in characters. Longer is damage.
pub const MAX_PROVENANCE_TEXT: usize = 200;

/// What made a layer's content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// The tool, such as `line-clean-up` or `import-psd`. Never empty.
    pub tool: String,
    /// The model, when one made the content; empty when the tool is not a model.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    /// The tool's or model's version.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// The day it was made, as `YYYY-MM-DD`; empty when unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date: String,
}

impl Provenance {
    pub fn new(tool: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            model: String::new(),
            version: String::new(),
            date: String::new(),
        }
    }

    /// Whether the record holds only what a sound one does: a tool, text no
    /// longer than [`MAX_PROVENANCE_TEXT`], and a date that looks like one.
    pub fn is_sound(&self) -> bool {
        !self.tool.trim().is_empty()
            && [&self.tool, &self.model, &self.version, &self.date]
                .iter()
                .all(|s| s.chars().count() <= MAX_PROVENANCE_TEXT)
            && (self.date.is_empty() || is_date(&self.date))
    }

    /// Fix what [`is_sound`](Self::is_sound) objects to: cut over-long text,
    /// forget a date that is not a date, and name a missing tool "unknown"
    /// (the record still says that something other than the artist made the
    /// content, which is the point of keeping it).
    pub fn repair(&mut self) {
        for s in [&mut self.tool, &mut self.model, &mut self.version, &mut self.date] {
            if s.chars().count() > MAX_PROVENANCE_TEXT {
                *s = s.chars().take(MAX_PROVENANCE_TEXT).collect();
            }
        }
        if self.tool.trim().is_empty() {
            self.tool = "unknown".into();
        }
        if !self.date.is_empty() && !is_date(&self.date) {
            self.date.clear();
        }
    }
}

/// `YYYY-MM-DD` with a month 1 to 12 and a day 1 to 31. Calendar-exact
/// checking is not worth a date library for a label.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && matches!(s[5..7].parse::<u32>(), Ok(1..=12))
        && matches!(s[8..10].parse::<u32>(), Ok(1..=31))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_record_is_sound_and_leaves_out_what_is_empty() {
        let p = Provenance::new("line-clean-up");
        assert!(p.is_sound());
        assert_eq!(serde_json::to_string(&p).unwrap(), r#"{"tool":"line-clean-up"}"#);
    }

    #[test]
    fn dates_must_look_like_dates() {
        let mut p = Provenance::new("t");
        for good in ["2026-10-30", "1999-01-31"] {
            p.date = good.into();
            assert!(p.is_sound(), "{good}");
        }
        for bad in [
            "30-10-2026",
            "2026-13-01",
            "2026-00-10",
            "2026-10-32",
            "2026/10/30",
            "yesterday",
            "2026-1-30",
        ] {
            p.date = bad.into();
            assert!(!p.is_sound(), "{bad}");
        }
    }

    #[test]
    fn repair_cuts_text_forgets_bad_dates_and_names_a_missing_tool() {
        let mut p = Provenance {
            tool: "  ".into(),
            model: "m".repeat(MAX_PROVENANCE_TEXT + 50),
            version: "1".into(),
            date: "soon".into(),
        };
        assert!(!p.is_sound());
        p.repair();
        assert!(p.is_sound());
        assert_eq!(p.tool, "unknown");
        assert_eq!(p.model.chars().count(), MAX_PROVENANCE_TEXT);
        assert_eq!(p.date, "");
        assert_eq!(p.version, "1");
    }

    #[test]
    fn repair_counts_characters_not_bytes() {
        let mut p = Provenance::new("t");
        p.model = "é".repeat(MAX_PROVENANCE_TEXT + 1);
        p.repair();
        assert_eq!(p.model.chars().count(), MAX_PROVENANCE_TEXT);
    }
}
