//! Undo and redo, kept per layer.
//!
//! The history is a timeline of document states with a cursor on the
//! current one. Each state holds the top-level layers behind shared
//! pointers, and a layer that a step did not change is shared with the
//! state before, so a step costs about the layers it changed. That also
//! makes the history cheap to journal (each layer state is written once),
//! so it survives a crash, and lets a single layer be taken back to before
//! its own last change while the rest of the document stays as it is.

use fanimate_core::{Document, Layer, LayerId};
use fanimate_fnm::{HistoryRecord, HistoryState};

pub struct History {
    /// `labels[i]` names the step from state `i` to state `i + 1`.
    labels: Vec<String>,
    /// Every state but the current one, which is the live document
    /// (`None` at `cursor`).
    states: Vec<Option<HistoryState>>,
    cursor: usize,
    limit: usize,
    /// The state captured or restored most recently, to share unchanged
    /// layers with.
    latest: Option<HistoryState>,
}

impl Default for History {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// One step, for the History panel.
#[derive(Clone, Debug, PartialEq)]
pub struct StepInfo {
    pub label: String,
    /// Names of the top-level layers the step changed, added or removed.
    pub layers: Vec<String>,
    /// Whether the step is undone (a redo away).
    pub undone: bool,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            labels: Vec::new(),
            states: vec![None],
            cursor: 0,
            limit: limit.max(1),
            latest: None,
        }
    }

    /// Adopt a history read back from the crash recovery journal. Its state
    /// at the cursor is replaced by the live document, as always.
    pub fn from_record(record: HistoryRecord) -> Option<Self> {
        if !record.is_consistent() {
            return None;
        }
        let latest = record.states.get(record.cursor).cloned();
        let mut states: Vec<Option<HistoryState>> = record.states.into_iter().map(Some).collect();
        states[record.cursor] = None;
        Some(Self {
            labels: record.labels,
            states,
            cursor: record.cursor,
            limit: Self::default().limit,
            latest,
        })
    }

    /// The whole history, with the live document as the current state, for
    /// the journal.
    pub fn record(&mut self, doc: &Document, selection: &[LayerId]) -> HistoryRecord {
        let now = self.capture(doc, selection);
        let states = self
            .states
            .iter()
            .map(|s| s.clone().unwrap_or_else(|| now.clone()))
            .collect();
        HistoryRecord {
            labels: self.labels.clone(),
            states,
            cursor: self.cursor,
        }
    }

    fn capture(&mut self, doc: &Document, selection: &[LayerId]) -> HistoryState {
        let state = HistoryState::capture(doc, selection, self.latest.as_ref());
        self.latest = Some(state.clone());
        state
    }

    /// Record the state *before* an edit called `label`.
    pub fn push(&mut self, label: &str, doc: &Document, selection: &[LayerId]) {
        let before = self.capture(doc, selection);
        self.labels.truncate(self.cursor);
        self.states.truncate(self.cursor + 1);
        self.states[self.cursor] = Some(before);
        self.labels.push(label.to_owned());
        self.states.push(None);
        self.cursor += 1;
        if self.labels.len() > self.limit {
            self.labels.remove(0);
            self.states.remove(0);
            self.cursor -= 1;
        }
    }

    /// Make the steps taken since state `from` (a [`cursor`](Self::cursor)
    /// noted earlier) one step called `label`, so one undo takes back all of
    /// them: a plugin run that made many calls is then one Ctrl+Z. Does
    /// nothing, and says so, when fewer than two steps were taken or when
    /// something has been undone since (a redo is waiting).
    pub fn merge_since(&mut self, from: usize, label: &str) -> bool {
        if from + 1 >= self.cursor || self.cursor != self.labels.len() || self.cursor >= self.states.len() {
            return false;
        }
        self.states.drain(from + 1..self.cursor);
        self.labels.drain(from..self.cursor);
        self.labels.insert(from, label.to_owned());
        self.cursor = from + 1;
        true
    }

    /// Step back. `doc` and `selection` become the earlier state.
    pub fn undo(&mut self, doc: &mut Document, selection: &mut Vec<LayerId>) -> Option<String> {
        if self.cursor == 0 {
            return None;
        }
        self.move_to(self.cursor - 1, doc, selection);
        Some(self.labels[self.cursor].clone())
    }

    pub fn redo(&mut self, doc: &mut Document, selection: &mut Vec<LayerId>) -> Option<String> {
        if self.cursor >= self.labels.len() {
            return None;
        }
        self.move_to(self.cursor + 1, doc, selection);
        Some(self.labels[self.cursor - 1].clone())
    }

    /// Go straight to state `target` (0 is the oldest). Returns false if
    /// there is no such state or it is the current one.
    pub fn go_to(&mut self, target: usize, doc: &mut Document, selection: &mut Vec<LayerId>) -> bool {
        if target >= self.states.len() || target == self.cursor {
            return false;
        }
        self.move_to(target, doc, selection);
        true
    }

    fn move_to(&mut self, target: usize, doc: &mut Document, selection: &mut Vec<LayerId>) {
        let now = self.capture(doc, selection);
        self.states[self.cursor] = Some(now);
        let Some(state) = self.states[target].take() else {
            return;
        };
        *doc = state.document();
        selection.clone_from(&state.selection);
        self.latest = Some(state);
        self.cursor = target;
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.cursor.checked_sub(1).map(|i| self.labels[i].as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.labels.get(self.cursor).map(String::as_str)
    }

    /// Which state is current: 0 is the oldest kept.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Every step, oldest first, with the layers it touched. `doc` is the
    /// live document.
    pub fn steps(&self, doc: &Document) -> Vec<StepInfo> {
        let names = |a: Lookup, b: Lookup| -> Vec<String> {
            // the later name, so a renamed layer shows its new name
            let mut seen = Vec::new();
            let mut out = Vec::new();
            for layer in b.top().into_iter().chain(a.top()) {
                if seen.contains(&layer.id) {
                    continue;
                }
                seen.push(layer.id);
                if a.get(layer.id) != b.get(layer.id) {
                    out.push(layer.name.clone());
                }
            }
            out
        };
        (0..self.labels.len())
            .map(|i| StepInfo {
                label: self.labels[i].clone(),
                layers: names(self.lookup(i, doc), self.lookup(i + 1, doc)),
                undone: i >= self.cursor,
            })
            .collect()
    }

    fn lookup<'a>(&'a self, i: usize, doc: &'a Document) -> Lookup<'a> {
        match &self.states[i] {
            Some(s) => Lookup::State(s),
            None => Lookup::Doc(doc),
        }
    }

    /// The state of layer `id` (at any depth) before the most recent step
    /// that changed it, with that step's label. `None` when no kept step
    /// changed it, or the step that did created it.
    pub fn before_last_change(&self, id: LayerId, doc: &Document) -> Option<(Layer, String)> {
        for i in (0..self.cursor).rev() {
            let (before, after) = (self.lookup(i, doc), self.lookup(i + 1, doc));
            if before.shares_top(after, id) {
                continue;
            }
            let (old, new) = (before.find(id), after.find(id));
            if old != new {
                return old.map(|l| (l.clone(), self.labels[i].clone()));
            }
        }
        None
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.limit);
    }
}

/// A state of the timeline: a kept one, or the live document.
#[derive(Clone, Copy)]
enum Lookup<'a> {
    State(&'a HistoryState),
    Doc(&'a Document),
}

impl<'a> Lookup<'a> {
    fn top(self) -> Vec<&'a Layer> {
        match self {
            Self::State(s) => s.layers.iter().map(|l| &**l).collect(),
            Self::Doc(d) => d.layers.iter().collect(),
        }
    }

    fn get(self, id: LayerId) -> Option<&'a Layer> {
        self.top().into_iter().find(|l| l.id == id)
    }

    fn find(self, id: LayerId) -> Option<&'a Layer> {
        fn go(layer: &Layer, id: LayerId) -> Option<&Layer> {
            if layer.id == id {
                return Some(layer);
            }
            layer.kind.children()?.iter().find_map(|c| go(c, id))
        }
        self.top().into_iter().find_map(|l| go(l, id))
    }

    /// Whether both states hold the very same top-level layer around `id`,
    /// so nothing in it can differ. A quick test before comparing.
    fn shares_top(self, other: Lookup<'a>, id: LayerId) -> bool {
        let (Self::State(a), Lookup::State(b)) = (self, other) else {
            return false;
        };
        let holder = |s: &'a HistoryState| {
            s.layers.iter().position(|l| {
                fn within(layer: &Layer, id: LayerId) -> bool {
                    layer.id == id
                        || layer
                            .kind
                            .children()
                            .is_some_and(|c| c.iter().any(|l| within(l, id)))
                }
                within(l, id)
            })
        };
        match (holder(a), holder(b)) {
            (Some(i), Some(j)) => std::sync::Arc::ptr_eq(&a.layers[i], &b.layers[j]),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{LayerKind, samples};

    fn doc() -> Document {
        let mut doc = samples::hello();
        for name in ["a", "b"] {
            let l = doc.new_layer(name, LayerKind::vector(vec![]));
            doc.layers.push(l);
        }
        doc
    }

    fn rename(h: &mut History, doc: &mut Document, i: usize, name: &str) {
        h.push("Rename", doc, &[]);
        doc.layers[i].name = name.into();
    }

    #[test]
    fn many_steps_merge_into_one_that_one_undo_takes_back() {
        let mut d = doc();
        let original = d.clone();
        let mut sel = Vec::new();
        let mut h = History::default();
        let n = d.layers.len();
        rename(&mut h, &mut d, n - 1, "earlier");
        let mark = h.cursor();
        let after_earlier = d.clone();
        for (i, name) in ["x1", "x2", "x3"].into_iter().enumerate() {
            rename(&mut h, &mut d, n - 1 - (i % 2), name);
        }
        let finished = d.clone();
        // fewer than two steps, or a waiting redo, merge nothing
        assert!(!h.merge_since(h.cursor(), "none"));
        assert!(h.merge_since(mark, "Plugin"));
        assert_eq!(h.steps(&d).len(), 2);
        assert_eq!(h.undo_label(), Some("Plugin"));
        // one undo puts back everything the three did, and the earlier step is still there
        h.undo(&mut d, &mut sel);
        assert_eq!(d, after_earlier);
        h.redo(&mut d, &mut sel);
        assert_eq!(d, finished);
        h.undo(&mut d, &mut sel);
        h.undo(&mut d, &mut sel);
        assert_eq!(d, original);
        // with a redo waiting there is nothing to merge
        assert!(!h.merge_since(0, "again"));
    }

    #[test]
    fn undo_redo_and_jumping() {
        let mut d = doc();
        let original = d.clone();
        let mut sel = Vec::new();
        let mut h = History::default();
        let n = d.layers.len();
        rename(&mut h, &mut d, n - 1, "b1");
        rename(&mut h, &mut d, n - 2, "a1");
        let edited = d.clone();
        assert_eq!(h.undo(&mut d, &mut sel).as_deref(), Some("Rename"));
        assert_eq!(d.layers[n - 2].name, "a");
        assert!(h.go_to(0, &mut d, &mut sel));
        assert_eq!(d, original);
        assert!(h.go_to(2, &mut d, &mut sel));
        assert_eq!(d, edited);
        assert!(h.redo(&mut d, &mut sel).is_none());
        // a new edit drops what was undone
        h.undo(&mut d, &mut sel);
        rename(&mut h, &mut d, 0, "z");
        assert_eq!(h.steps(&d).len(), 2);
        assert!(h.redo_label().is_none());
    }

    #[test]
    fn steps_name_the_layers_they_touched() {
        let mut d = doc();
        let mut sel = Vec::new();
        let mut h = History::default();
        let n = d.layers.len();
        rename(&mut h, &mut d, n - 1, "b1");
        h.push("New layer", &d, &[]);
        let l = d.new_layer("c", LayerKind::vector(vec![]));
        d.layers.push(l);
        h.undo(&mut d, &mut sel);
        let steps = h.steps(&d);
        assert_eq!(steps[0].layers, ["b1"]);
        assert!(!steps[0].undone);
        assert_eq!(steps[1].layers, ["c"]);
        assert!(steps[1].undone);
    }

    #[test]
    fn unchanged_layers_are_shared_between_states() {
        let mut d = doc();
        let mut h = History::default();
        let n = d.layers.len();
        rename(&mut h, &mut d, n - 1, "b1");
        rename(&mut h, &mut d, n - 1, "b2");
        let r = h.record(&d, &[]);
        assert!(std::sync::Arc::ptr_eq(
            &r.states[0].layers[0],
            &r.states[2].layers[0]
        ));
        assert!(!std::sync::Arc::ptr_eq(
            &r.states[1].layers[n - 1],
            &r.states[2].layers[n - 1]
        ));
    }

    #[test]
    fn finds_a_layer_before_its_own_last_change() {
        let mut d = doc();
        let n = d.layers.len();
        let (a, b) = (d.layers[n - 2].id, d.layers[n - 1].id);
        let mut h = History::default();
        rename(&mut h, &mut d, n - 2, "a1");
        rename(&mut h, &mut d, n - 1, "b1");
        rename(&mut h, &mut d, n - 1, "b2");
        let (layer, label) = h.before_last_change(a, &d).unwrap();
        assert_eq!((layer.name.as_str(), label.as_str()), ("a", "Rename"));
        assert_eq!(h.before_last_change(b, &d).unwrap().0.name, "b1");
        // a layer created by its last step has nothing to go back to
        h.push("New layer", &d, &[]);
        let l = d.new_layer("c", LayerKind::vector(vec![]));
        let c = l.id;
        d.layers.push(l);
        assert!(h.before_last_change(c, &d).is_none());
    }

    #[test]
    fn the_limit_drops_the_oldest_steps() {
        let mut d = doc();
        let mut sel = Vec::new();
        let mut h = History::new(2);
        for name in ["1", "2", "3"] {
            rename(&mut h, &mut d, 0, name);
        }
        assert_eq!(h.steps(&d).len(), 2);
        h.undo(&mut d, &mut sel);
        h.undo(&mut d, &mut sel);
        assert!(h.undo(&mut d, &mut sel).is_none());
        assert_eq!(d.layers[0].name, "1");
    }

    #[test]
    fn a_journalled_history_comes_back() {
        let mut d = doc();
        let mut sel = Vec::new();
        let mut h = History::default();
        rename(&mut h, &mut d, 0, "x");
        rename(&mut h, &mut d, 0, "y");
        h.undo(&mut d, &mut sel);
        let record = h.record(&d, &sel);
        let mut back = History::from_record(record).unwrap();
        assert_eq!(back.cursor(), 1);
        assert_eq!(back.redo(&mut d, &mut sel).as_deref(), Some("Rename"));
        assert_eq!(d.layers[0].name, "y");
        back.undo(&mut d, &mut sel);
        back.undo(&mut d, &mut sel);
        assert_eq!(d, doc());
        let broken = HistoryRecord {
            labels: vec!["x".into()],
            states: vec![],
            cursor: 0,
        };
        assert!(History::from_record(broken).is_none());
    }
}
