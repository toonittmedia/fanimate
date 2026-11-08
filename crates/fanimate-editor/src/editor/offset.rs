//! Offset Path: a new shape a set distance outside a shape, or inside it.
//!
//! As in Illustrator, the original stays: the offset outline is added as a
//! new shape just above it in the same layer, with its style, and is the
//! one selected afterwards. Growing rounds the corners; shrinking keeps them
//! sharp and drops the parts that shrink to nothing. An open line has no
//! inside, so a positive distance makes the closed band around it and a
//! negative one makes nothing. The distance is `Editor::offset_distance`,
//! set in the Properties panel.

use std::collections::BTreeSet;

use fanimate_core::boolean::{self, MAX_OFFSET};
use fanimate_core::{LayerId, Shape};

use super::{Editor, SelectMode, StatusKind, plural};

/// How far an offset outline may stray from the exact one, in document
/// units: well below a pixel at 100%, as for the boolean commands.
const TOLERANCE: f32 = 0.25;

impl Editor {
    /// The shapes Offset Path acts on: those with selected points on the
    /// point layer, or else the one shape of the only selected layer.
    pub fn offset_targets(&self) -> Option<(LayerId, Vec<usize>)> {
        let layer = self.point_layer()?;
        let picked: BTreeSet<usize> = self
            .points
            .iter()
            .filter(|r| r.layer == layer)
            .map(|r| r.shape)
            .collect();
        let count = self.layer_shapes(layer)?.len();
        let picked: Vec<usize> = if picked.is_empty() {
            let (l, index) = self.single_shape_target()?;
            (l == layer).then_some(vec![index])?
        } else {
            picked.into_iter().filter(|&i| i < count).collect()
        };
        (!picked.is_empty()).then_some((layer, picked))
    }

    /// The outline of shape `index` of `layer`, `distance` outside it
    /// (inside when negative), as drawn on the current frame, with the
    /// shape's style, ready to add beside it. The new shape has no point
    /// keys, width profile or hidden edges (its points are new ones), and
    /// follows the same bone when the original follows one rigidly; smooth
    /// binding weights belong to the original's points and are not copied.
    fn offset_of(&self, layer: LayerId, index: usize, distance: f32) -> Result<Shape, String> {
        let shapes = self
            .layer_shapes(layer)
            .ok_or_else(|| "That layer has no shapes".to_string())?;
        let source = shapes
            .get(index)
            .ok_or_else(|| format!("There is no shape {index} on that layer"))?;
        let path = boolean::offset(&source.path_at(self.frame), distance, TOLERANCE);
        if path.cmds.is_empty() {
            return Err(if distance < 0.0 {
                "Nothing is left: the shape is thinner than that, or is an open line".into()
            } else {
                "Nothing to offset: the shape has no outline".into()
            });
        }
        let mut shape = Shape::new(format!("{} offset", source.name), path, source.style);
        shape.bone = source.bone.filter(|_| source.skin.is_none());
        Ok(shape)
    }

    fn check_offset_distance(distance: f32) -> Result<(), String> {
        if !distance.is_finite() || distance == 0.0 || distance.abs() > MAX_OFFSET {
            return Err(format!(
                "The offset is a distance from -{MAX_OFFSET} to {MAX_OFFSET} px, and not 0"
            ));
        }
        Ok(())
    }

    /// Offset Path on one shape: add its outline `distance` outside it
    /// (inside when negative) as a new shape just above it, and select the
    /// new shape. One undo step. Returns the new shape's index; on an error
    /// nothing changes.
    pub fn offset_shape(&mut self, layer: LayerId, index: usize, distance: f32) -> Result<usize, String> {
        Self::check_offset_distance(distance)?;
        if !self.is_drawable(layer) {
            return Err("Offset Path works on the shapes of a vector or frame-by-frame layer".into());
        }
        if self.is_locked(layer) {
            return Err("The layer is locked".into());
        }
        let shape = self.offset_of(layer, index, distance)?;
        self.checkpoint("Offset path");
        let shapes = self
            .layer_shapes_mut(layer)
            .ok_or_else(|| "That layer has no shapes".to_string())?;
        shapes.insert(index + 1, shape);
        self.dirty = true;
        self.points.clear();
        self.select_shape(layer, index + 1, SelectMode::Replace);
        Ok(index + 1)
    }

    /// The Offset Path command: every target shape gets its offset outline
    /// beside it, by `offset_distance`, in one undo step.
    pub(super) fn offset_path(&mut self) {
        let distance = self.offset_distance;
        if let Err(e) = Self::check_offset_distance(distance) {
            self.set_status(StatusKind::Info, e);
            return;
        }
        let Some((layer, targets)) = self.offset_targets() else {
            self.set_status(StatusKind::Info, "Select a shape to offset");
            return;
        };
        if self.is_locked(layer) {
            self.set_status(StatusKind::Info, "The layer is locked");
            return;
        }
        let mut made: Vec<(usize, Shape)> = Vec::new();
        let mut last_error = None;
        for &index in &targets {
            match self.offset_of(layer, index, distance) {
                Ok(shape) => made.push((index, shape)),
                Err(e) => last_error = Some(e),
            }
        }
        if made.is_empty() {
            self.set_status(
                StatusKind::Info,
                last_error.unwrap_or_else(|| "Select a shape to offset".into()),
            );
            return;
        }
        self.checkpoint("Offset path");
        let n = made.len();
        let Some(shapes) = self.layer_shapes_mut(layer) else {
            return;
        };
        // back to front, so earlier indices stay valid
        for (index, shape) in made.iter().rev() {
            shapes.insert(index + 1, shape.clone());
        }
        self.dirty = true;
        self.points.clear();
        // each new shape sits above its original, after the ones added below it
        for (k, (index, _)) in made.iter().enumerate() {
            self.select_shape(layer, index + 1 + k, SelectMode::Range);
        }
        let skipped = targets.len() - n;
        let message = if skipped == 0 {
            format!("Offset {n} shape{} by {distance} px", plural(n))
        } else {
            format!(
                "Offset {n} shape{} by {distance} px; {skipped} left nothing",
                plural(n)
            )
        };
        self.set_status(StatusKind::Info, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Tool};
    use fanimate_core::{LayerKind, Vec2};

    fn shapes(ed: &Editor, id: LayerId) -> Vec<Shape> {
        match &ed.doc.find(id).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.clone(),
            _ => panic!(),
        }
    }

    /// A 100 by 60 rectangle at the origin and an ellipse to its right.
    fn editor() -> (Editor, LayerId) {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::ZERO,
            Vec2::new(100.0, 60.0),
            false,
            false,
            1.0,
        );
        ed.add_drag_shape(
            Tool::Ellipse,
            Vec2::new(250.0, 0.0),
            Vec2::new(350.0, 60.0),
            false,
            false,
            1.0,
        );
        ed.tool = Tool::SelectPoints;
        (ed, id)
    }

    #[test]
    fn offset_path_adds_a_selected_shape_above_the_original_in_one_step() {
        let (mut ed, id) = editor();
        ed.select_shape(id, 0, SelectMode::Replace);
        ed.offset_distance = 10.0;
        assert!(ed.can_run(Action::OffsetPath));
        ed.run(Action::OffsetPath);
        let s = shapes(&ed, id);
        assert_eq!(s.len(), 3);
        assert_eq!(s[1].name, "Rectangle 1 offset");
        assert_eq!(s[1].style, s[0].style);
        assert!(s[1].contains(Vec2::new(-8.0, 30.0), 0.0) && !s[0].contains(Vec2::new(-8.0, 30.0), 0.0));
        // a plain path: no rectangle, width profile, keys or hidden edges to keep in step
        assert!(s[1].parametric_rect().is_none() && s[1].widths.is_none());
        assert!(s[1].path_keys.is_empty() && s[1].hidden_edge_flags.is_empty());
        // the original is untouched and the new shape is the one selected
        assert!(s[0].parametric_rect().is_some());
        assert!(ed.shape_selected(id, 1) && !ed.shape_selected(id, 0));
        ed.run(Action::Undo);
        assert_eq!(shapes(&ed, id).len(), 2);
        ed.run(Action::Redo);
        assert_eq!(shapes(&ed, id).len(), 3);
    }

    #[test]
    fn offset_path_works_on_every_shape_with_chosen_points() {
        let (mut ed, id) = editor();
        ed.select_all_points();
        ed.offset_distance = -5.0;
        ed.run(Action::OffsetPath);
        let s = shapes(&ed, id);
        let names: Vec<&str> = s.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Rectangle 1",
                "Rectangle 1 offset",
                "Ellipse 1",
                "Ellipse 1 offset"
            ]
        );
        // shrunk: inside the original, short of its edge
        assert!(s[1].contains(Vec2::new(50.0, 30.0), 0.0) && !s[1].contains(Vec2::new(1.0, 30.0), 0.0));
        assert!(ed.shape_selected(id, 1) && ed.shape_selected(id, 3));
        assert!(!ed.shape_selected(id, 0) && !ed.shape_selected(id, 2));
        ed.run(Action::Undo);
        assert_eq!(shapes(&ed, id).len(), 2, "all of it is one step");
    }

    #[test]
    fn a_whole_layer_with_one_shape_offsets_without_points_chosen() {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.add_drag_shape(Tool::Ellipse, Vec2::ZERO, Vec2::splat(80.0), false, false, 1.0);
        ed.points.clear();
        assert_eq!(ed.offset_targets(), Some((id, vec![0])));
        ed.run(Action::OffsetPath);
        assert_eq!(shapes(&ed, id).len(), 2);
    }

    #[test]
    fn an_open_line_grows_into_a_band_and_cannot_shrink() {
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        ed.add_shape(
            Some(id),
            Shape::new(
                "Line",
                fanimate_core::Path::line(Vec2::ZERO, Vec2::new(100.0, 0.0)),
                fanimate_core::Style {
                    fill: None,
                    stroke: Some(fanimate_core::Stroke {
                        color: fanimate_core::Color::BLACK,
                        width: 2.0,
                    }),
                },
            ),
        );
        assert_eq!(ed.offset_shape(id, 0, 6.0), Ok(1));
        let s = shapes(&ed, id);
        assert!(
            s[1].path
                .cmds
                .iter()
                .any(|c| matches!(c, fanimate_core::PathCmd::Close))
        );
        ed.run(Action::Undo);
        let steps = ed.history.undo_label().map(str::to_owned);
        assert!(ed.offset_shape(id, 0, -6.0).is_err());
        assert_eq!(shapes(&ed, id).len(), 1);
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps, "no empty step");
    }

    #[test]
    fn offset_path_refuses_nonsense_and_changes_nothing() {
        let (mut ed, id) = editor();
        let steps = ed.history.undo_label().map(str::to_owned);
        for d in [0.0, f32::NAN, f32::INFINITY, MAX_OFFSET * 2.0] {
            assert!(ed.offset_shape(id, 0, d).is_err(), "{d}");
        }
        assert!(ed.offset_shape(id, 9, 5.0).is_err());
        assert!(ed.offset_shape(LayerId(9999), 0, 5.0).is_err());
        // shrinking a shape away entirely says so and adds nothing
        ed.select_shape(id, 0, SelectMode::Replace);
        ed.offset_distance = -40.0;
        ed.run(Action::OffsetPath);
        assert!(matches!(ed.status, Some((StatusKind::Info, _))));
        // nothing selected: nothing to offset
        ed.points.clear();
        ed.offset_distance = 5.0;
        ed.run(Action::OffsetPath);
        assert_eq!(shapes(&ed, id).len(), 2);
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps);
        // a locked layer is left alone
        ed.select_shape(id, 0, SelectMode::Replace);
        ed.run(Action::ToggleLock);
        assert!(ed.offset_shape(id, 0, 5.0).is_err());
    }

    #[test]
    fn an_animated_shape_is_offset_as_it_is_drawn_on_this_frame() {
        let (mut ed, id) = editor();
        ed.select_shape(id, 0, SelectMode::Replace);
        ed.frame = 10.0;
        assert!(ed.begin_point_drag());
        ed.drag_points(Vec2::new(200.0, 0.0));
        assert!(shapes(&ed, id)[0].is_animated());
        let at = ed.offset_shape(id, 0, 5.0).unwrap();
        let made = &shapes(&ed, id)[at];
        assert!(!made.is_animated());
        assert!(
            made.contains(Vec2::new(250.0, 30.0), 0.0),
            "where it is on frame 10"
        );
    }
}
