//! The undo history, as the crash recovery journal keeps it.
//!
//! The editor records each state of the document as a list of top-level
//! layers behind shared pointers, so a step that changed one layer shares
//! every other layer with the step before. The journal keeps that sharing
//! on disk: each distinct layer state is one file, written once, and an
//! index lists which layer files make up each state:
//!
//! ```text
//! history/
//! ├── index.json           labels, cursor, and each state's layer files
//! └── layers/
//!     ├── 1.json           one layer as it was at some point
//!     └── 2.json
//! ```
//!
//! So recording a step writes about one small file, and a crash leaves the
//! undo history behind with the work.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fanimate_core::{Document, Layer, LayerId};
use serde::{Deserialize, Serialize};

use crate::io::{io_err, write_json_atomic};
use crate::{FnmError, Result};

/// The document at one point in its history.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryState {
    /// Everything but the layers (its `layers` is empty).
    pub meta: Arc<Document>,
    /// Top-level layers, bottom first. States share unchanged layers.
    pub layers: Vec<Arc<Layer>>,
    /// The selected layers.
    pub selection: Vec<LayerId>,
}

impl HistoryState {
    /// Capture `doc`, sharing every part that is unchanged from `like`.
    pub fn capture(doc: &Document, selection: &[LayerId], like: Option<&HistoryState>) -> Self {
        let meta = Document {
            layers: Vec::new(),
            ..doc_meta(doc)
        };
        let meta = match like {
            Some(l) if *l.meta == meta => Arc::clone(&l.meta),
            _ => Arc::new(meta),
        };
        let layers = doc
            .layers
            .iter()
            .enumerate()
            .map(|(i, layer)| {
                let old = like.and_then(|l| {
                    // usually at the same place; otherwise search by id
                    l.layers
                        .get(i)
                        .filter(|o| o.id == layer.id)
                        .or_else(|| l.layers.iter().find(|o| o.id == layer.id))
                });
                match old {
                    Some(o) if **o == *layer => Arc::clone(o),
                    _ => Arc::new(layer.clone()),
                }
            })
            .collect();
        Self {
            meta,
            layers,
            selection: selection.to_vec(),
        }
    }

    /// The document this state holds.
    pub fn document(&self) -> Document {
        Document {
            layers: self.layers.iter().map(|l| (**l).clone()).collect(),
            ..doc_meta(&self.meta)
        }
    }

    /// The layer `id`, at any depth.
    pub fn find(&self, id: LayerId) -> Option<&Layer> {
        fn go(layer: &Layer, id: LayerId) -> Option<&Layer> {
            if layer.id == id {
                return Some(layer);
            }
            layer.kind.children()?.iter().find_map(|c| go(c, id))
        }
        self.layers.iter().find_map(|l| go(l, id))
    }
}

fn doc_meta(doc: &Document) -> Document {
    Document {
        name: doc.name.clone(),
        width: doc.width,
        height: doc.height,
        fps: doc.fps,
        start: doc.start,
        end: doc.end,
        background: doc.background,
        layers: Vec::new(),
        next_layer_id: doc.next_layer_id,
        guides: doc.guides.clone(),
        camera: doc.camera.clone(),
        action_edit: doc.action_edit.clone(),
        media: doc.media.clone(),
    }
}

/// A whole undo history: `states[0]` is the oldest state, `labels[i]` names
/// the step from `states[i]` to `states[i + 1]`, and `states[cursor]` is
/// the document as it is now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoryRecord {
    pub labels: Vec<String>,
    pub states: Vec<HistoryState>,
    pub cursor: usize,
}

impl HistoryRecord {
    /// Whether the parts agree: one more state than labels, and the cursor
    /// on a state.
    pub fn is_consistent(&self) -> bool {
        self.states.len() == self.labels.len() + 1 && self.cursor < self.states.len()
    }
}

const INDEX: &str = "index.json";
const LAYERS: &str = "layers";

#[derive(Serialize, Deserialize)]
struct IndexFile {
    cursor: usize,
    labels: Vec<String>,
    states: Vec<StateFile>,
}

#[derive(Serialize, Deserialize)]
struct StateFile {
    document: Document,
    /// Layer file numbers, bottom first.
    layers: Vec<u64>,
    selection: Vec<LayerId>,
}

/// Writes a history folder, remembering which layer states are already
/// on disk so each is written once.
pub(crate) struct HistoryWriter {
    dir: PathBuf,
    /// Layer states on disk, by address, with the file number. Holding the
    /// `Arc` keeps the address from being reused by another layer.
    written: HashMap<usize, (Arc<Layer>, u64)>,
    next: u64,
}

/// A layer state's identity: its address, only ever compared.
fn key(layer: &Arc<Layer>) -> usize {
    Arc::as_ptr(layer) as usize
}

impl HistoryWriter {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            written: HashMap::new(),
            next: 1,
        }
    }

    pub(crate) fn write(&mut self, record: &HistoryRecord) -> Result<()> {
        let layers_dir = self.dir.join(LAYERS);
        if self.written.is_empty() {
            fs::create_dir_all(&layers_dir).map_err(|e| io_err(&layers_dir, e))?;
        }
        let mut states = Vec::with_capacity(record.states.len());
        for state in &record.states {
            let mut ids = Vec::with_capacity(state.layers.len());
            for layer in &state.layers {
                let key = key(layer);
                let id = match self.written.get(&key) {
                    Some((_, id)) => *id,
                    None => {
                        let id = self.next;
                        self.next += 1;
                        write_json_atomic(&layers_dir.join(format!("{id}.json")), &**layer)?;
                        self.written.insert(key, (Arc::clone(layer), id));
                        id
                    }
                };
                ids.push(id);
            }
            states.push(StateFile {
                document: (*state.meta).clone(),
                layers: ids,
                selection: state.selection.clone(),
            });
        }
        let index = IndexFile {
            cursor: record.cursor,
            labels: record.labels.clone(),
            states,
        };
        write_json_atomic(&self.dir.join(INDEX), &index)?;

        // forget, and delete, layer states no step uses any more
        let used: std::collections::HashSet<usize> = record
            .states
            .iter()
            .flat_map(|s| s.layers.iter().map(key))
            .collect();
        let gone: Vec<usize> = self
            .written
            .keys()
            .filter(|k| !used.contains(k))
            .copied()
            .collect();
        for key in gone {
            if let Some((_, id)) = self.written.remove(&key) {
                let _ = fs::remove_file(layers_dir.join(format!("{id}.json")));
            }
        }
        Ok(())
    }
}

/// Read a history folder written by [`HistoryWriter`]. States share layers
/// as they did when written.
pub(crate) fn read(dir: &Path) -> Result<HistoryRecord> {
    let text = fs::read_to_string(dir.join(INDEX)).map_err(|e| io_err(&dir.join(INDEX), e))?;
    let index: IndexFile = serde_json::from_str(&text).map_err(|source| FnmError::Json {
        path: dir.join(INDEX),
        source,
    })?;
    let mut loaded: HashMap<u64, Arc<Layer>> = HashMap::new();
    let mut states = Vec::with_capacity(index.states.len());
    for state in index.states {
        let mut layers = Vec::with_capacity(state.layers.len());
        for id in state.layers {
            let layer = match loaded.get(&id) {
                Some(l) => Arc::clone(l),
                None => {
                    let path = dir.join(LAYERS).join(format!("{id}.json"));
                    let text = fs::read_to_string(&path).map_err(|e| io_err(&path, e))?;
                    let layer: Layer =
                        serde_json::from_str(&text).map_err(|source| FnmError::Json { path, source })?;
                    let layer = Arc::new(layer);
                    loaded.insert(id, Arc::clone(&layer));
                    layer
                }
            };
            layers.push(layer);
        }
        states.push(HistoryState {
            meta: Arc::new(Document {
                layers: Vec::new(),
                ..state.document
            }),
            layers,
            selection: state.selection,
        });
    }
    Ok(HistoryRecord {
        labels: index.labels,
        states,
        cursor: index.cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{LayerKind, samples};

    #[test]
    fn capture_shares_unchanged_layers() {
        let mut doc = samples::hello();
        let layer = doc.new_layer("extra", LayerKind::vector(vec![]));
        doc.layers.push(layer);
        let a = HistoryState::capture(&doc, &[], None);
        doc.layers[1].name = "renamed".into();
        let b = HistoryState::capture(&doc, &[], Some(&a));
        assert!(Arc::ptr_eq(&a.layers[0], &b.layers[0]));
        assert!(!Arc::ptr_eq(&a.layers[1], &b.layers[1]));
        assert!(Arc::ptr_eq(&a.meta, &b.meta));
        assert_eq!(b.document(), doc);
        assert!(b.find(doc.layers[1].id).is_some());
    }

    #[test]
    fn history_round_trips_and_writes_each_layer_state_once() {
        let dir = std::env::temp_dir().join(format!("fanimate-history-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut doc = samples::hello();
        let s0 = HistoryState::capture(&doc, &[], None);
        doc.layers[0].name = "one".into();
        let s1 = HistoryState::capture(&doc, &[doc.layers[0].id], Some(&s0));
        let record = HistoryRecord {
            labels: vec!["Rename".into()],
            states: vec![s0, s1],
            cursor: 1,
        };
        assert!(record.is_consistent());
        let mut writer = HistoryWriter::new(dir.clone());
        writer.write(&record).unwrap();
        let files = fs::read_dir(dir.join(LAYERS)).unwrap().count();
        assert_eq!(files, 2 * doc.layers.len() - (doc.layers.len() - 1));
        let back = read(&dir).unwrap();
        assert_eq!(back, record);
        // sharing survives the round trip
        if doc.layers.len() > 1 {
            assert!(Arc::ptr_eq(&back.states[0].layers[1], &back.states[1].layers[1]));
        }
        // dropping the first state deletes the layer only it used
        let short = HistoryRecord {
            labels: vec![],
            states: vec![record.states[1].clone()],
            cursor: 0,
        };
        writer.write(&short).unwrap();
        assert_eq!(fs::read_dir(dir.join(LAYERS)).unwrap().count(), doc.layers.len());
        assert_eq!(read(&dir).unwrap(), short);
        fs::remove_dir_all(dir).unwrap();
    }
}
