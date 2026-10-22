//! Cut, copy and paste by focus.
//!
//! Plain Ctrl+C, Ctrl+X and Ctrl+V act on whatever the user last worked in:
//! points and shapes on the canvas, keys in the timeline. Layers have their
//! own commands (`CopyLayers`, `CutLayers`, `PasteLayers`, Ctrl+Shift+C, X
//! and V), so with the Layers panel in focus the plain ones only hint at them.
//!
//! Shapes travel as JSON tagged `"shapes"` (layers are tagged `"layers"`), on
//! the system clipboard so they can be pasted into another tab, with an
//! internal copy for when something else has replaced the text there. Their
//! points are stored in document space, so a paste lands where the copy was
//! even when the two layers are moved, turned or scaled differently.

use std::collections::{BTreeMap, BTreeSet};

use fanimate_core::{Affine2, LayerId, Path, PathCmd, Shape, Vec2};
use serde::{Deserialize, Serialize};

use super::{Editor, StatusKind, plural};

/// Which part of the window Cut, Copy and Paste act on: the last place the
/// user clicked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    /// Points and shapes.
    #[default]
    Canvas,
    /// Keyframes.
    Timeline,
    /// Layers, which have their own commands.
    Layers,
}

/// Shapes on the clipboard, as JSON.
#[derive(Serialize, Deserialize)]
struct ShapesData {
    fanimate: String,
    version: u32,
    /// Every shape with its points in document space.
    shapes: Vec<Shape>,
}

const SHAPES_TAG: &str = "shapes";

/// Just the tag of a payload, to tell what kind of thing a text holds.
#[derive(Deserialize)]
struct Tagged {
    fanimate: String,
}

fn parse_shapes(text: &str) -> Option<ShapesData> {
    serde_json::from_str::<ShapesData>(text)
        .ok()
        .filter(|d| d.fanimate == SHAPES_TAG && d.version <= fanimate_fnm::FORMAT_VERSION)
}

fn holds_layers(text: &str) -> bool {
    serde_json::from_str::<Tagged>(text).is_ok_and(|t| t.fanimate == "layers")
}

/// The point a command ends on.
fn end_of(cmd: &PathCmd) -> Vec2 {
    match cmd {
        PathCmd::MoveTo(p) | PathCmd::LineTo(p) | PathCmd::CubicTo(_, _, p) => *p,
        PathCmd::Close => Vec2::ZERO,
    }
}

/// The chosen points of `path` as an open path. Neighbours that are chosen
/// together keep the segment between them (a curve stays a curve); other
/// points start a new piece. A lone point draws nothing, so pieces of one
/// point are left out. Also returns the numbers of the points kept.
fn open_path_of(path: &Path, chosen: &BTreeSet<usize>) -> (Path, Vec<usize>) {
    let ends: Vec<PathCmd> = path
        .cmds
        .iter()
        .copied()
        .filter(|c| !matches!(c, PathCmd::Close))
        .collect();
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for &k in chosen {
        let Some(cmd) = ends.get(k) else { continue };
        // a move starts a subpath: it does not join the point before it
        let joins = !matches!(cmd, PathCmd::MoveTo(_));
        match runs.last_mut() {
            Some(run) if joins && run.last().copied() == k.checked_sub(1) => run.push(k),
            _ => runs.push(vec![k]),
        }
    }
    let mut cmds = Vec::new();
    let mut kept = Vec::new();
    for run in runs.into_iter().filter(|r| r.len() >= 2) {
        for (n, &k) in run.iter().enumerate() {
            let cmd = ends[k];
            cmds.push(if n == 0 {
                PathCmd::MoveTo(end_of(&cmd))
            } else {
                cmd
            });
            kept.push(k);
        }
    }
    (Path { cmds }, kept)
}

impl Editor {
    /// Where Cut, Copy and Paste act now.
    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// Set by the app from the last click: the canvas, the timeline or the
    /// Layers panel.
    pub fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
    }

    /// Plain Copy: by focus. Returns text for the system clipboard when
    /// there is some.
    pub(super) fn copy_focused(&mut self) -> Option<String> {
        match self.focus {
            Focus::Canvas => self.copy_points(),
            Focus::Timeline => {
                self.copy_keys();
                None
            }
            Focus::Layers => {
                self.hint_layers("copy");
                None
            }
        }
    }

    /// Plain Cut: by focus.
    pub(super) fn cut_focused(&mut self) -> Option<String> {
        match self.focus {
            Focus::Canvas => self.cut_points(),
            Focus::Timeline => {
                self.cut_keys();
                None
            }
            Focus::Layers => {
                self.hint_layers("cut");
                None
            }
        }
    }

    /// Plain Paste: by focus. `text` is what the system clipboard held, when
    /// the app has it. Returns false only when there was nothing to paste and
    /// nothing was said about it.
    pub fn paste_focused(&mut self, text: Option<&str>) -> bool {
        match self.focus {
            Focus::Timeline => self.paste_keys(),
            Focus::Canvas => {
                if self.paste_points(text) {
                    return true;
                }
                // layers are on the clipboard: say where they go
                if text.is_some_and(holds_layers) || self.clipboard.is_some() {
                    self.hint_layers("paste");
                    return true;
                }
                false
            }
            Focus::Layers => {
                self.hint_layers("paste");
                true
            }
        }
    }

    fn hint_layers(&mut self, verb: &str) {
        let keys = match verb {
            "copy" => "Ctrl+Shift+C",
            "cut" => "Ctrl+Shift+X",
            _ => "Ctrl+Shift+V",
        };
        self.set_status(StatusKind::Info, format!("To {verb} layers use {keys}"));
    }

    /// Whether Paste has something to paste now, for the Edit menu.
    pub(super) fn can_paste(&self) -> bool {
        self.clipboard.is_some()
            || self.shape_clipboard.is_some()
            || (self.focus == Focus::Timeline && self.has_key_clipboard())
    }

    /// Whether Copy and Cut have something to act on, for the Edit menu.
    pub(super) fn can_copy_focused(&self) -> bool {
        match self.focus {
            Focus::Canvas => !self.points.is_empty(),
            Focus::Timeline => self.has_selected_keys(),
            Focus::Layers => false,
        }
    }

    /// The chosen points as the clipboard holds them, and what to call them
    /// in the status ("2 shapes", "3 points"). A shape with every point
    /// chosen is copied whole: fill, stroke, widths, keys, rectangle and
    /// exposure. Part of a shape is copied as an open path of just those
    /// points, in the shape's style.
    fn shapes_to_copy(&mut self) -> Option<(String, String)> {
        self.prune_points();
        if self.points.is_empty() {
            self.set_status(
                StatusKind::Info,
                "Choose some points to copy. To copy layers use Ctrl+Shift+C",
            );
            return None;
        }
        let whole: BTreeSet<(LayerId, usize)> = self.selected_shapes().into_iter().collect();
        let transforms: BTreeMap<(LayerId, usize), Affine2> = self.shape_transforms().into_iter().collect();
        let frame = self.current_frame() as f32;
        let mut chosen: BTreeMap<(LayerId, usize), BTreeSet<usize>> = BTreeMap::new();
        for r in &self.points {
            chosen.entry((r.layer, r.shape)).or_default().insert(r.point);
        }
        let mut shapes = Vec::new();
        let mut partial_points = 0;
        let mut any_partial = false;
        for (key, points) in chosen {
            let (Some(shape), Some(to_doc)) = (self.point_shape(key), transforms.get(&key)) else {
                continue;
            };
            let mut copy = if whole.contains(&key) {
                shape.clone()
            } else {
                let path = shape.path_at(frame).into_owned();
                let (open, kept) = open_path_of(&path, &points);
                if kept.is_empty() {
                    continue;
                }
                let count = path.on_curve_count();
                let mut c = shape.clone();
                c.widths = shape
                    .widths
                    .as_ref()
                    .filter(|w| w.len() == count)
                    .map(|w| kept.iter().map(|&k| w[k]).collect());
                c.path = open;
                c.path_keys.clear();
                c.rect = None;
                partial_points += kept.len();
                any_partial = true;
                c
            };
            // what a bone does to a shape is not carried along
            copy.bone = None;
            copy.skin = None;
            if !to_doc.is_finite() {
                continue;
            }
            copy.transform(*to_doc);
            shapes.push(copy);
        }
        if shapes.is_empty() {
            self.set_status(
                StatusKind::Info,
                "Choose at least two neighbouring points to copy",
            );
            return None;
        }
        let summary = if any_partial && shapes.len() == 1 {
            format!("{partial_points} point{}", plural(partial_points))
        } else {
            format!("{} shape{}", shapes.len(), plural(shapes.len()))
        };
        let data = ShapesData {
            fanimate: SHAPES_TAG.into(),
            version: fanimate_fnm::FORMAT_VERSION,
            shapes,
        };
        let text = serde_json::to_string(&data).ok()?;
        self.shape_clipboard = Some(text.clone());
        Some((text, summary))
    }

    /// Copy the chosen points or shapes. Returns text for the system
    /// clipboard, `None` when nothing was chosen.
    pub fn copy_points(&mut self) -> Option<String> {
        let (text, summary) = self.shapes_to_copy()?;
        self.set_status(StatusKind::Info, format!("Copied {summary}"));
        Some(text)
    }

    /// Copy the chosen points or shapes and take them out of the drawing.
    /// One undo step.
    pub fn cut_points(&mut self) -> Option<String> {
        let (text, summary) = self.shapes_to_copy()?;
        self.remove_chosen_points("Cut");
        self.set_status(StatusKind::Info, format!("Cut {summary}"));
        Some(text)
    }

    /// Paste shapes from `text` (the system clipboard) if it holds Fanimate
    /// shapes, else from the internal clipboard, into the selected vector
    /// layer, where they were copied from. The pasted points are chosen.
    /// One undo step. Returns false when there was nothing of that kind to
    /// paste; true when it handled the request, even to say it cannot.
    pub fn paste_points(&mut self, text: Option<&str>) -> bool {
        let data = text
            .and_then(parse_shapes)
            .or_else(|| self.shape_clipboard.as_deref().and_then(parse_shapes));
        let Some(data) = data else {
            return false;
        };
        if data.shapes.is_empty() {
            return false;
        }
        let Some(layer) = self.point_layer() else {
            self.set_status(StatusKind::Info, "Select a vector layer to paste into");
            return true;
        };
        if self.is_locked(layer) {
            self.set_status(StatusKind::Info, "Unlock the layer to paste into it");
            return true;
        }
        // document space to the layer's own space
        let into_layer = self.world(layer).inverse();
        if !into_layer.is_finite() {
            self.set_status(
                StatusKind::Error,
                "Cannot paste into a layer that is squashed flat",
            );
            return true;
        }
        let mut shapes = data.shapes;
        for shape in &mut shapes {
            shape.transform(into_layer);
        }
        shapes.retain(|s| !s.path.cmds.is_empty() && s.path.is_finite());
        if shapes.is_empty() {
            return false;
        }
        self.checkpoint("Paste shapes");
        let Some(target) = self.drawing_shapes_mut(layer) else {
            return true;
        };
        let start = target.len();
        let n = shapes.len();
        target.extend(shapes);
        let end = start + n;
        self.points = self
            .point_positions()
            .into_iter()
            .map(|(r, _)| r)
            .filter(|r| r.layer == layer && (start..end).contains(&r.shape))
            .collect();
        self.set_status(StatusKind::Info, format!("Pasted {n} shape{}", plural(n)));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Outcome, PointRef};
    use crate::{Action, Keymap, KeymapKind, Tool};
    use fanimate_core::{AnimTransform, LayerKind};

    /// An editor whose first layer holds a 100x100 rectangle, the points
    /// tool active and the canvas in focus.
    fn editor() -> (Editor, LayerId) {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::new(10.0, 0.0),
            Vec2::new(110.0, 100.0),
            false,
            false,
            1.0,
        );
        ed.tool = Tool::SelectPoints;
        ed.set_focus(Focus::Canvas);
        (ed, id)
    }

    fn shapes_of(ed: &Editor, id: LayerId) -> Vec<Shape> {
        match &ed.doc.find(id).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.clone(),
            _ => Vec::new(),
        }
    }

    fn point(layer: LayerId, shape: usize, point: usize) -> PointRef {
        PointRef { layer, shape, point }
    }

    fn status(ed: &Editor) -> String {
        ed.status.as_ref().map(|s| s.1.clone()).unwrap_or_default()
    }

    #[test]
    fn a_whole_shape_pastes_into_another_layer_where_it_was() {
        let (mut ed, id) = editor();
        ed.select_all_points();
        let text = ed.copy_points().unwrap();
        assert!(text.contains("\"shapes\""));
        assert_eq!(status(&ed), "Copied 1 shape");
        ed.run(Action::NewVectorLayer);
        let other = ed.primary().unwrap();
        assert_ne!(other, id);
        assert!(ed.paste_points(Some(&text)));
        let pasted = shapes_of(&ed, other);
        assert_eq!(pasted.len(), 1);
        let pts = pasted[0].path.on_curve_points();
        assert!(pts[0].abs_diff_eq(Vec2::new(10.0, 0.0), 1e-3));
        assert!(pts[2].abs_diff_eq(Vec2::new(110.0, 100.0), 1e-3));
        // it is still a rectangle, with its fill and stroke
        assert!(pasted[0].parametric_rect().is_some());
        assert_eq!(pasted[0].style, shapes_of(&ed, id)[0].style);
        // the original is untouched, and the pasted points are the chosen ones
        assert_eq!(shapes_of(&ed, id).len(), 1);
        assert_eq!(ed.points.len(), 4);
        assert!(ed.points.iter().all(|p| p.layer == other && p.shape == 0));
    }

    #[test]
    fn paste_converts_between_layers_that_are_placed_differently() {
        let (mut ed, _) = editor();
        ed.select_all_points();
        let text = ed.copy_points().unwrap();
        ed.run(Action::NewVectorLayer);
        let other = ed.primary().unwrap();
        ed.doc.find_mut(other).unwrap().transform = AnimTransform::at(Vec2::new(50.0, 0.0));
        assert!(ed.paste_points(Some(&text)));
        // in the layer's own space, which is moved by 50, so it lands in the same place
        let pts = shapes_of(&ed, other)[0].path.on_curve_points();
        assert!(pts[0].abs_diff_eq(Vec2::new(-40.0, 0.0), 1e-3));
        assert!(
            ed.world(other)
                .transform_point2(pts[0])
                .abs_diff_eq(Vec2::new(10.0, 0.0), 1e-3)
        );
    }

    #[test]
    fn some_points_copy_as_an_open_path_in_the_shapes_style() {
        let (mut ed, id) = editor();
        ed.points = BTreeSet::from([point(id, 0, 0), point(id, 0, 1)]);
        let text = ed.copy_points().unwrap();
        assert_eq!(status(&ed), "Copied 2 points");
        assert!(ed.paste_points(Some(&text)));
        let shapes = shapes_of(&ed, id);
        assert_eq!(shapes.len(), 2);
        let open = &shapes[1];
        assert_eq!(open.path.on_curve_count(), 2);
        assert!(matches!(open.path.cmds[0], PathCmd::MoveTo(_)));
        assert!(matches!(open.path.cmds[1], PathCmd::LineTo(_)));
        assert!(!open.path.cmds.contains(&PathCmd::Close));
        assert!(open.rect.is_none());
        assert_eq!(open.style, shapes[0].style);
        // the first shape is as it was, and the new points are the chosen ones
        assert_eq!(shapes[0].path.on_curve_count(), 4);
        assert_eq!(ed.points, BTreeSet::from([point(id, 1, 0), point(id, 1, 1)]));
    }

    #[test]
    fn single_points_alone_are_not_copied() {
        let (mut ed, id) = editor();
        ed.points = BTreeSet::from([point(id, 0, 0), point(id, 0, 2)]);
        assert!(ed.copy_points().is_none());
        assert!(ed.shape_clipboard.is_none());
        assert!(status(&ed).contains("two neighbouring points"));
    }

    #[test]
    fn pasting_needs_a_vector_layer_and_leaves_the_drawing_alone() {
        let (mut ed, id) = editor();
        ed.select_all_points();
        let text = ed.copy_points().unwrap();
        ed.run(Action::NewGroupLayer);
        let revision = ed.revision();
        assert!(ed.paste_points(Some(&text)));
        assert_eq!(status(&ed), "Select a vector layer to paste into");
        assert_eq!(ed.revision(), revision);
        assert_eq!(shapes_of(&ed, id).len(), 1);
    }

    #[test]
    fn nothing_to_paste_is_not_handled() {
        let (mut ed, _) = editor();
        assert!(!ed.paste_points(Some("just some text")));
        assert!(!ed.paste_points(None));
        assert!(!ed.paste_focused(Some("just some text")));
    }

    #[test]
    fn cut_removes_the_points_and_undo_brings_them_back() {
        let (mut ed, id) = editor();
        ed.select_all_points();
        let Outcome::CopyText(text) = ed.run(Action::Cut) else {
            panic!("cutting points copies text");
        };
        assert!(text.contains("\"shapes\""));
        assert!(shapes_of(&ed, id).is_empty());
        assert_eq!(status(&ed), "Cut 1 shape");
        ed.run(Action::Undo);
        assert_eq!(shapes_of(&ed, id).len(), 1);
        // and the cut can be pasted
        ed.run(Action::Redo);
        assert!(ed.paste_points(Some(&text)));
        assert_eq!(shapes_of(&ed, id).len(), 1);
    }

    #[test]
    fn paste_is_one_undo_step() {
        let (mut ed, id) = editor();
        ed.select_all_points();
        let text = ed.copy_points().unwrap();
        assert!(ed.paste_points(Some(&text)));
        assert!(ed.paste_points(Some(&text)));
        assert_eq!(shapes_of(&ed, id).len(), 3);
        ed.run(Action::Undo);
        assert_eq!(shapes_of(&ed, id).len(), 2);
    }

    #[test]
    fn shapes_paste_into_another_document() {
        let (mut a, _) = editor();
        a.select_all_points();
        let text = a.copy_points().unwrap();
        let mut b = Editor::untitled();
        let target = b.primary().unwrap();
        assert!(b.paste_points(Some(&text)));
        assert_eq!(shapes_of(&b, target).len(), 1);
        // pasting does not make a copy of its own to fall back on
        assert!(!b.paste_points(Some("other text")));
    }

    #[test]
    fn paste_can_run_with_either_kind_of_clipboard() {
        let (mut ed, _) = editor();
        assert!(!ed.can_run(Action::Paste));
        assert!(!ed.can_run(Action::PasteLayers));
        ed.select_all_points();
        ed.copy_points();
        assert!(ed.can_run(Action::Paste));
        assert!(!ed.can_run(Action::PasteLayers));
        let (mut ed, _) = editor();
        ed.run(Action::CopyLayers);
        assert!(ed.can_run(Action::Paste));
        assert!(ed.can_run(Action::PasteLayers));
    }

    #[test]
    fn plain_copy_follows_the_focus() {
        let (mut ed, _) = editor();
        ed.select_all_points();

        // the Layers panel only hints
        ed.set_focus(Focus::Layers);
        assert_eq!(ed.run(Action::Copy), Outcome::Done);
        assert_eq!(status(&ed), "To copy layers use Ctrl+Shift+C");
        assert!(ed.shape_clipboard.is_none() && ed.clipboard.is_none());
        assert_eq!(ed.run(Action::Cut), Outcome::Done);
        assert_eq!(status(&ed), "To cut layers use Ctrl+Shift+X");
        assert!(ed.paste_focused(None));
        assert_eq!(status(&ed), "To paste layers use Ctrl+Shift+V");

        // the timeline never copies points
        ed.set_focus(Focus::Timeline);
        assert!(!matches!(ed.run(Action::Copy), Outcome::CopyText(_)));
        assert!(ed.shape_clipboard.is_none());

        // the canvas copies points, never layers
        ed.set_focus(Focus::Canvas);
        let Outcome::CopyText(text) = ed.run(Action::Copy) else {
            panic!("copying points copies text");
        };
        assert!(text.contains("\"shapes\""));
        assert!(ed.clipboard.is_none());
    }

    #[test]
    fn plain_copy_on_the_canvas_with_no_points_says_what_to_do() {
        let (mut ed, _) = editor();
        assert_eq!(ed.run(Action::Copy), Outcome::Done);
        assert!(status(&ed).contains("Ctrl+Shift+C"));
        // a layers payload pasted on the canvas points at the layer command
        ed.run(Action::CopyLayers);
        assert!(ed.paste_focused(None));
        assert_eq!(status(&ed), "To paste layers use Ctrl+Shift+V");
    }

    #[test]
    fn layer_commands_ignore_the_focus_and_keep_the_old_behaviour() {
        for focus in [Focus::Canvas, Focus::Timeline, Focus::Layers] {
            let (mut ed, id) = editor();
            ed.set_focus(focus);
            ed.select(id, crate::SelectMode::Replace);
            let Outcome::CopyText(text) = ed.run(Action::CopyLayers) else {
                panic!("copying layers copies text");
            };
            assert!(text.contains("\"layers\""), "{focus:?}");
            ed.run(Action::PasteLayers);
            assert_eq!(ed.doc.layers.len(), 2, "{focus:?}");
            assert!(matches!(ed.run(Action::CutLayers), Outcome::CopyText(_)));
            assert_eq!(ed.doc.layers.len(), 1, "{focus:?}");
        }
    }

    #[test]
    fn the_two_payloads_do_not_read_as_each_other() {
        let (mut ed, id) = editor();
        ed.select_all_points();
        let shapes = ed.copy_points().unwrap();
        ed.select(id, crate::SelectMode::Replace);
        let Outcome::CopyText(layers) = ed.run(Action::CopyLayers) else {
            panic!("copying layers copies text");
        };
        assert!(parse_shapes(&layers).is_none());
        assert!(holds_layers(&layers) && !holds_layers(&shapes));
        // a shapes text is not layers, even to a paste that has no layers of its own
        let layer_count = ed.doc.layers.len();
        ed.clipboard = None;
        assert!(!ed.paste(Some(&shapes)));
        assert_eq!(ed.doc.layers.len(), layer_count);
    }

    #[test]
    fn layer_shortcuts_are_shown_in_both_keymaps() {
        if cfg!(target_os = "macos") {
            return;
        }
        for kind in KeymapKind::ALL {
            let map = Keymap::new(kind);
            assert_eq!(map.shortcut_text(Action::CopyLayers), "Ctrl+Shift+C", "{kind:?}");
            assert_eq!(map.shortcut_text(Action::CutLayers), "Ctrl+Shift+X", "{kind:?}");
            assert_eq!(map.shortcut_text(Action::PasteLayers), "Ctrl+Shift+V", "{kind:?}");
            assert_eq!(map.shortcut_text(Action::Copy), "Ctrl+C", "{kind:?}");
        }
    }
}
