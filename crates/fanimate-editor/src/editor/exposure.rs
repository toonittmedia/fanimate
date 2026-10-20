//! Stroke Exposure: drawing only part of a stroke, and animating how much.
//!
//! Dragging along a stroke with the tool sets how far it is drawn: the end by
//! default, the start with Alt. Properties has the same two numbers. They
//! follow the keying rule like any property: on the first frame the stroke
//! is set for the whole animation, on a later frame the change is keyed.

use fanimate_core::{Affine2, LayerId, Vec2};

use super::{Editor, StatusKind};

/// A drag of one end of a stroke's exposure.
pub(super) struct ExposureDrag {
    layer: LayerId,
    index: usize,
    /// The start (Alt) rather than the end.
    start: bool,
    /// Document space to the layer's space.
    to_local: Affine2,
}

impl Editor {
    /// Press on a stroke with the Stroke Exposure tool: begin changing how
    /// much of it is drawn. `start` changes where it begins, not where it
    /// ends. Returns false (saying why) when there is no stroke to change.
    pub fn begin_exposure(&mut self, at: Vec2, reach: f32, start: bool) -> bool {
        let Some((layer, index)) = self.editable_shape_at(at, reach) else {
            self.set_status(
                StatusKind::Info,
                "Press on a stroke, then drag along it to show more or less of it",
            );
            return false;
        };
        let Some(shape) = self.point_shape((layer, index)) else {
            return false;
        };
        if shape.style.stroke.is_none() {
            self.set_status(StatusKind::Info, "This shape has no stroke to trim");
            return false;
        }
        // a shape on a bone is drawn somewhere other than its points
        if shape.bone.is_some() || shape.skin.is_some() {
            self.set_status(
                StatusKind::Info,
                "A shape that follows a bone cannot be trimmed by dragging yet: use the fields in Properties",
            );
            return false;
        }
        self.select(layer, super::SelectMode::Replace);
        let to_local = self.world(layer).inverse();
        if !to_local.is_finite() {
            return false;
        }
        self.checkpoint("Stroke exposure");
        self.exposure = Some(ExposureDrag {
            layer,
            index,
            start,
            to_local,
        });
        self.drag_exposure(at);
        true
    }

    /// Move the end being dragged to the point of the stroke nearest `to`
    /// (document space).
    pub fn drag_exposure(&mut self, to: Vec2) {
        let Some(drag) = &self.exposure else { return };
        let (layer, index, start, to_local) = (drag.layer, drag.index, drag.start, drag.to_local);
        let (frame, keying) = (self.current_frame(), self.keying());
        let local = to_local.transform_point2(to);
        let Some(shape) = self.point_shape_mut((layer, index)) else {
            return;
        };
        let Some((along, _)) = shape.path_at(frame as f32).exposure_at(local) else {
            return;
        };
        let now = shape.trim_at(frame as f32);
        // the start stays before the end
        let next = if start {
            Vec2::new(along.min(now.y), now.y)
        } else {
            Vec2::new(now.x, along.max(now.x))
        };
        if next != now {
            shape.set_trim_at(frame, next, keying);
            self.dirty = true;
        }
    }

    pub fn end_exposure(&mut self) {
        self.exposure = None;
    }

    /// Set the exposure of one shape from Properties: `trim` is the start
    /// and end, each 0 to 1. Call [`checkpoint`](Self::checkpoint) once
    /// per edit first.
    pub fn set_stroke_exposure(&mut self, layer: LayerId, index: usize, trim: Vec2) {
        let (frame, keying) = (self.current_frame(), self.keying());
        if let Some(shape) = self.point_shape_mut((layer, index)) {
            shape.set_trim_at(frame, trim, keying);
            self.dirty = true;
        }
    }

    /// The shape whose exposure Properties edits: the one shape selected,
    /// when it has a stroke.
    pub fn exposure_target(&self) -> Option<(LayerId, usize, Vec2)> {
        let (layer, index) = self.single_shape_target()?;
        let shape = self.point_shape((layer, index))?;
        shape.style.stroke?;
        Some((layer, index, shape.trim_at(self.current_frame() as f32)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;

    /// A layer with one horizontal stroke from (0, 50) to (100, 50).
    fn editor() -> (Editor, LayerId) {
        use fanimate_core::{Color, LayerKind, Path, Shape, Style};
        let mut ed = Editor::untitled();
        let id = ed.primary().unwrap();
        let shape = Shape::new(
            "stroke",
            Path::line(Vec2::new(0.0, 50.0), Vec2::new(100.0, 50.0)),
            Style::default().with_stroke(Color::BLACK, 6.0),
        );
        match &mut ed.doc.find_mut(id).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.push(shape),
            _ => panic!("not a vector layer"),
        }
        (ed, id)
    }

    #[test]
    fn dragging_along_a_stroke_sets_where_it_ends() {
        let (mut ed, _) = editor();
        assert!(ed.begin_exposure(Vec2::new(10.0, 50.0), 6.0, false));
        ed.drag_exposure(Vec2::new(60.0, 50.0));
        ed.end_exposure();
        let trim = ed.exposure_target().map(|(_, _, t)| t).expect("a stroke");
        assert!(
            (trim.x - 0.0).abs() < 1e-4 && (trim.y - 0.6).abs() < 0.02,
            "{trim}"
        );
        // it is one undo step, back to the whole stroke
        ed.run(Action::Undo);
        let trim = ed.exposure_target().map(|(_, _, t)| t).expect("a stroke");
        assert_eq!(trim, Vec2::new(0.0, 1.0));
    }

    #[test]
    fn alt_moves_the_start_and_the_start_stays_before_the_end() {
        let (mut ed, _) = editor();
        assert!(ed.begin_exposure(Vec2::new(10.0, 50.0), 6.0, true));
        ed.drag_exposure(Vec2::new(30.0, 50.0));
        ed.end_exposure();
        let trim = ed.exposure_target().map(|(_, _, t)| t).unwrap();
        assert!((trim.x - 0.3).abs() < 0.02 && trim.y == 1.0, "{trim}");
        // past the end it stops at the end
        assert!(ed.begin_exposure(Vec2::new(60.0, 50.0), 6.0, false));
        ed.drag_exposure(Vec2::new(10.0, 50.0));
        ed.end_exposure();
        let trim = ed.exposure_target().map(|(_, _, t)| t).unwrap();
        assert!(trim.y >= trim.x, "{trim}");
    }

    #[test]
    fn on_a_later_frame_the_exposure_is_keyed() {
        let (mut ed, id) = editor();
        ed.go_to(12);
        assert!(ed.begin_exposure(Vec2::new(10.0, 50.0), 6.0, false));
        ed.drag_exposure(Vec2::new(40.0, 50.0));
        ed.end_exposure();
        let keys = ed
            .doc
            .find(id)
            .and_then(|l| l.channel_keys(fanimate_core::ChannelId::Trim(0)))
            .expect("exposure keys");
        let mut frames = Vec::new();
        keys.frames_into(&mut frames);
        assert_eq!(frames, vec![0, 12], "a key to start from and the new one");
        // frame 0 still shows the whole stroke
        ed.go_to(0);
        assert_eq!(ed.exposure_target().map(|(_, _, t)| t), Some(Vec2::new(0.0, 1.0)));
    }

    #[test]
    fn a_press_that_misses_or_a_shape_without_a_stroke_does_nothing() {
        let (mut ed, _) = editor();
        let before = ed.revision();
        assert!(!ed.begin_exposure(Vec2::new(50.0, 10.0), 3.0, false));
        assert_eq!(ed.revision(), before, "nothing to undo");
        // Properties sets it directly
        let (layer, index, _) = ed.exposure_target().unwrap();
        ed.checkpoint("Stroke exposure");
        ed.set_stroke_exposure(layer, index, Vec2::new(0.2, 3.0));
        assert_eq!(ed.exposure_target().map(|(_, _, t)| t), Some(Vec2::new(0.2, 1.0)));
    }
}
