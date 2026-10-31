//! What an exporter could not carry. Every route that is not a straight
//! picture of the frames (SVG, Lottie, sprite sheets) returns one of these
//! beside its file, so the user is told what was simplified or left out
//! instead of finding out in the other program.

/// Counts of what an exporter simplified or left out, by kind, in the order
/// the kinds first came up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    notes: Vec<(&'static str, usize)>,
}

impl Report {
    /// Note one more of `what`, a plural phrase that says what happened:
    /// "images left out", "width profiles drawn as an even stroke".
    pub fn add(&mut self, what: &'static str) {
        self.add_many(what, 1);
    }

    pub fn add_many(&mut self, what: &'static str, count: usize) {
        if count == 0 {
            return;
        }
        match self.notes.iter_mut().find(|(w, _)| *w == what) {
            Some((_, n)) => *n += count,
            None => self.notes.push((what, count)),
        }
    }

    /// Nothing was simplified or left out.
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// How many were noted for `what` (0 when none).
    pub fn count(&self, what: &str) -> usize {
        self.notes.iter().find(|(w, _)| *w == what).map_or(0, |(_, n)| *n)
    }

    /// One line per kind: "2 images left out".
    pub fn lines(&self) -> Vec<String> {
        self.notes.iter().map(|(w, n)| format!("{n} {w}")).collect()
    }

    /// All the lines in one sentence for the status bar, or "" when empty.
    pub fn summary(&self) -> String {
        self.lines().join(", ")
    }

    /// Fold another report into this one.
    pub fn merge(&mut self, other: &Report) {
        for (what, n) in &other.notes {
            self.add_many(what, *n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_counted_by_kind_in_order() {
        let mut r = Report::default();
        assert!(r.is_empty());
        assert_eq!(r.summary(), "");
        r.add("images left out");
        r.add("masks left out");
        r.add("images left out");
        r.add_many("masks left out", 0);
        assert_eq!(r.lines(), ["2 images left out", "1 masks left out"]);
        assert_eq!(r.count("images left out"), 2);
        assert_eq!(r.count("nothing"), 0);
        let mut other = Report::default();
        other.add("images left out");
        r.merge(&other);
        assert_eq!(r.count("images left out"), 3);
    }
}
