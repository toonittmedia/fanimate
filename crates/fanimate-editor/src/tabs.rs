//! The open documents, as tabs.
//!
//! Only one document is live at a time: the app keeps it where it always
//! did (the editor, the canvas view, the timeline zoom, the journal) and
//! parks the others in their tabs. `Tabs<S>` holds the parked state `S` of
//! every other tab, the order of the tabs, and which one is live. It is
//! generic over `S` so the tab logic needs no window to test.
//!
//! The Home tab is not a document. It sits in front of the document tabs
//! and, when it is the front one, hides the live document without closing
//! it.

use std::path::{Path, PathBuf};

/// What the tab strip shows about a document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabInfo {
    pub title: String,
    pub path: Option<PathBuf>,
    /// Unsaved changes.
    pub dirty: bool,
}

struct Slot<S> {
    /// The parked document; `None` for the live one.
    state: Option<S>,
    info: TabInfo,
}

pub struct Tabs<S> {
    slots: Vec<Slot<S>>,
    /// The live document's slot; `None` when no document is open.
    active: Option<usize>,
    home: bool,
}

impl<S> Default for Tabs<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Tabs<S> {
    /// No documents, and the Home tab in front.
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            active: None,
            home: true,
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The live document's tab.
    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// Whether the Home tab is in front (or there is nothing else to show).
    pub fn home_front(&self) -> bool {
        self.home || self.active.is_none()
    }

    pub fn show_home(&mut self) {
        self.home = true;
    }

    pub fn info(&self, tab: usize) -> Option<&TabInfo> {
        self.slots.get(tab).map(|s| &s.info)
    }

    pub fn infos(&self) -> impl Iterator<Item = &TabInfo> {
        self.slots.iter().map(|s| &s.info)
    }

    /// Update what the tab strip shows for the live document.
    pub fn set_active_info(&mut self, info: TabInfo) {
        if let Some(slot) = self.active.and_then(|a| self.slots.get_mut(a)) {
            slot.info = info;
        }
    }

    /// The tab whose document is saved at `path`.
    pub fn find_path(&self, path: &Path) -> Option<usize> {
        let canonical = std::fs::canonicalize(path).ok();
        self.slots.iter().position(|s| {
            s.info.path.as_deref().is_some_and(|p| {
                p == path || canonical.is_some() && std::fs::canonicalize(p).ok() == canonical
            })
        })
    }

    /// Whether any document, live or parked, has unsaved changes.
    pub fn any_dirty(&self) -> bool {
        self.slots.iter().any(|s| s.info.dirty)
    }

    /// Open a new document and make it the live one. `live` is the state of
    /// the document that was live until now, to be parked; `None` if no
    /// document was open (what was showing was only a placeholder).
    pub fn open(&mut self, live: Option<S>, info: TabInfo) -> usize {
        if let (Some(a), Some(state)) = (self.active, live) {
            self.slots[a].state = Some(state);
        }
        self.slots.push(Slot { state: None, info });
        self.active = Some(self.slots.len() - 1);
        self.home = false;
        self.slots.len() - 1
    }

    /// Bring tab `to` to the front. If it was not the live document,
    /// `live` (the state of the one that was) is parked and the state to go
    /// live is returned.
    pub fn switch(&mut self, to: usize, live: impl FnOnce() -> S) -> Option<S> {
        if to >= self.slots.len() {
            return None;
        }
        self.home = false;
        if self.active == Some(to) {
            return None;
        }
        if let Some(a) = self.active {
            self.slots[a].state = Some(live());
        }
        self.active = Some(to);
        self.slots[to].state.take()
    }

    /// Close the live document. Returns the state of the tab that takes its
    /// place, or `None` when no document is left (Home shows then).
    pub fn close_active(&mut self) -> Option<S> {
        let a = self.active?;
        self.slots.remove(a);
        if self.slots.is_empty() {
            self.active = None;
            self.home = true;
            return None;
        }
        // the neighbour on the right takes over, else the one on the left
        let next = a.min(self.slots.len() - 1);
        self.active = Some(next);
        self.slots[next].state.take()
    }

    /// Close a tab that is not the live document, giving back its state so
    /// the caller can end its journal.
    pub fn close_parked(&mut self, tab: usize) -> Option<S> {
        if tab >= self.slots.len() || self.active == Some(tab) {
            return None;
        }
        let slot = self.slots.remove(tab);
        if let Some(a) = self.active
            && a > tab
        {
            self.active = Some(a - 1);
        }
        slot.state
    }

    /// Every parked state, leaving the tabs in place, for closing them all
    /// when the app quits.
    pub fn take_parked(&mut self) -> Vec<S> {
        self.slots.iter_mut().filter_map(|s| s.state.take()).collect()
    }

    /// Move a tab, keeping the live document's tab the live one.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.slots.len() || to >= self.slots.len() || from == to {
            return;
        }
        let slot = self.slots.remove(from);
        self.slots.insert(to, slot);
        self.active = self.active.map(|a| {
            if a == from {
                to
            } else if from < a && a <= to {
                a - 1
            } else if to <= a && a < from {
                a + 1
            } else {
                a
            }
        });
    }

    /// The tab after (or before) the front one, wrapping round.
    pub fn neighbour(&self, forward: bool) -> Option<usize> {
        let n = self.slots.len();
        let a = self.active?;
        (n > 1).then(|| if forward { (a + 1) % n } else { (a + n - 1) % n })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(title: &str) -> TabInfo {
        TabInfo {
            title: title.into(),
            ..Default::default()
        }
    }

    fn titles(t: &Tabs<&'static str>) -> Vec<String> {
        t.infos().map(|i| i.title.clone()).collect()
    }

    /// Three documents a, b, c with c live.
    fn three() -> Tabs<&'static str> {
        let mut t = Tabs::new();
        t.open(None, info("a"));
        t.open(Some("a-state"), info("b"));
        t.open(Some("b-state"), info("c"));
        t
    }

    #[test]
    fn starts_empty_with_home_in_front() {
        let t: Tabs<&str> = Tabs::new();
        assert!(t.is_empty() && t.home_front() && t.active().is_none());
        assert_eq!(t.neighbour(true), None);
    }

    #[test]
    fn opening_makes_the_new_document_live_and_parks_the_old() {
        let t = three();
        assert_eq!(titles(&t), ["a", "b", "c"]);
        assert_eq!((t.active(), t.home_front()), (Some(2), false));
    }

    #[test]
    fn switching_swaps_the_live_state() {
        let mut t = three();
        assert_eq!(t.switch(0, || "c-state"), Some("a-state"));
        assert_eq!(t.active(), Some(0));
        assert_eq!(t.switch(2, || "a-state"), Some("c-state"));
        // switching to the live tab parks nothing
        assert_eq!(t.switch(2, || unreachable!()), None);
        assert_eq!(t.switch(9, || unreachable!()), None);
    }

    #[test]
    fn home_hides_the_live_document_without_closing_it() {
        let mut t = three();
        t.show_home();
        assert!(t.home_front());
        assert_eq!(t.active(), Some(2));
        // choosing the live document's own tab shows it again
        assert_eq!(t.switch(2, || unreachable!()), None);
        assert!(!t.home_front());
    }

    #[test]
    fn closing_the_live_tab_brings_its_neighbour_forward() {
        let mut t = three();
        t.switch(1, || "c-state");
        // b closes: c, the one on its right, takes over
        assert_eq!(t.close_active(), Some("c-state"));
        assert_eq!((titles(&t), t.active()), (vec!["a".into(), "c".into()], Some(1)));
        // c closes: it was last, so a takes over
        assert_eq!(t.close_active(), Some("a-state"));
        assert_eq!((titles(&t), t.active()), (vec!["a".into()], Some(0)));
        // the last one closes: nothing is left and Home shows
        assert_eq!(t.close_active(), None);
        assert!(t.is_empty() && t.home_front());
        assert_eq!(t.close_active(), None);
    }

    #[test]
    fn closing_a_parked_tab_keeps_the_live_one_live() {
        let mut t = three();
        assert_eq!(t.close_parked(0), Some("a-state"));
        assert_eq!((titles(&t), t.active()), (vec!["b".into(), "c".into()], Some(1)));
        // the live tab cannot be closed this way
        assert_eq!(t.close_parked(1), None);
        assert_eq!(t.close_parked(7), None);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn moving_tabs_keeps_track_of_the_live_one() {
        for (from, to) in [(0, 2), (2, 0), (0, 1), (1, 0), (1, 2), (2, 1)] {
            for live in 0..3 {
                let mut t = three();
                t.switch(live, || "x");
                let before = titles(&t)[live].clone();
                t.move_tab(from, to);
                let a = t.active().unwrap();
                assert_eq!(titles(&t)[a], before, "moved {from} to {to}, live {live}");
            }
        }
        let mut t = three();
        t.move_tab(0, 9);
        t.move_tab(1, 1);
        assert_eq!(titles(&t), ["a", "b", "c"]);
    }

    #[test]
    fn neighbours_wrap_round() {
        let mut t = three();
        assert_eq!(t.neighbour(true), Some(0));
        assert_eq!(t.neighbour(false), Some(1));
        t.switch(0, || "c-state");
        assert_eq!(t.neighbour(false), Some(2));
        let mut one: Tabs<&str> = Tabs::new();
        one.open(None, info("only"));
        assert_eq!(one.neighbour(true), None);
    }

    #[test]
    fn tab_info_follows_the_live_document_and_finds_paths() {
        let mut t = three();
        t.set_active_info(TabInfo {
            title: "c*".into(),
            path: Some(PathBuf::from("c.fnm")),
            dirty: true,
        });
        assert!(t.any_dirty());
        assert_eq!(t.find_path(Path::new("c.fnm")), Some(2));
        assert_eq!(t.find_path(Path::new("other.fnm")), None);
        assert_eq!(t.info(2).unwrap().title, "c*");
        t.set_active_info(info("c"));
        assert!(!t.any_dirty());
    }

    #[test]
    fn quitting_takes_every_parked_state() {
        let mut t = three();
        let mut parked = t.take_parked();
        parked.sort();
        assert_eq!(parked, ["a-state", "b-state"]);
        assert_eq!(t.len(), 3);
    }
}
