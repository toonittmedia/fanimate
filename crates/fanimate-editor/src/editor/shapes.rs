//! Which shapes the drawing and point tools edit.
//!
//! On a vector layer, its shapes. On a frame-by-frame layer, the active
//! sub-layer (rough, colour, line or overlay) of the drawing shown on the
//! current frame. Drawing on a frame before the layer's first drawing
//! starts a new drawing there; elsewhere the drawing being held is edited,
//! as in Animate and Harmony.

use fanimate_core::frames::{Cel, cel_at};
use fanimate_core::{DrawItem, LayerId, LayerKind, Shape};

use super::Editor;

impl Editor {
    /// Whether the drawing tools can work on layer `id`.
    pub fn is_drawable(&self, id: LayerId) -> bool {
        matches!(
            self.doc.find(id).map(|l| &l.kind),
            Some(LayerKind::Vector { .. } | LayerKind::Frames { .. })
        )
    }

    /// The shapes the tools edit on layer `id` now.
    pub fn layer_shapes(&self, id: LayerId) -> Option<&Vec<Shape>> {
        match &self.doc.find(id)?.kind {
            LayerKind::Vector { shapes } => Some(shapes),
            LayerKind::Frames { cels } => {
                let i = cel_at(cels, self.current_frame() as f32)?;
                Some(cels[i].part(self.part))
            }
            _ => None,
        }
    }

    pub(super) fn layer_shapes_mut(&mut self, id: LayerId) -> Option<&mut Vec<Shape>> {
        let frame = self.current_frame() as f32;
        let part = self.part;
        match &mut self.doc.find_mut(id)?.kind {
            LayerKind::Vector { shapes } => Some(shapes),
            LayerKind::Frames { cels } => {
                let i = cel_at(cels, frame)?;
                Some(cels[i].part_mut(part))
            }
            _ => None,
        }
    }

    /// Like [`layer_shapes_mut`](Self::layer_shapes_mut), starting a new
    /// drawing on the current frame when the layer shows none.
    pub(super) fn drawing_shapes_mut(&mut self, id: LayerId) -> Option<&mut Vec<Shape>> {
        let frame = self.current_frame();
        if let Some(LayerKind::Frames { cels }) = self.doc.find_mut(id).map(|l| &mut l.kind)
            && cel_at(cels, frame as f32).is_none()
        {
            cels.insert(0, Cel::new(frame));
        }
        self.layer_shapes_mut(id)
    }

    /// Set the style of shape `index` of the shapes the tools edit on layer
    /// `id`. Does not checkpoint; callers checkpoint once per gesture.
    ///
    /// Colours follow the keying rule (see [`Keying`]): on the first frame
    /// they change outright; on a later frame the change is a colour
    /// keyframe, and the first frame keeps the colour it had. The stroke's
    /// width is not animated, and turning a fill or stroke on or off ends
    /// that colour's animation.
    pub fn set_shape_style(&mut self, id: LayerId, index: usize, style: fanimate_core::Style) {
        let frame = self.current_frame();
        let keying = self.keying();
        let Some(s) = self.layer_shapes_mut(id).and_then(|s| s.get_mut(index)) else {
            return;
        };
        // what the person changing it was looking at: the colours as they are now
        let now = s.style_at(frame as f32);
        match (now.fill, style.fill) {
            (Some(a), Some(b)) => {
                if a != b {
                    s.set_fill_at(frame, b, keying);
                }
            }
            (_, fill) => {
                s.style.fill = fill;
                s.fill_keys.clear();
            }
        }
        match (now.stroke, style.stroke) {
            (Some(a), Some(b)) => {
                if a.color != b.color {
                    s.set_stroke_color_at(frame, b.color, keying);
                }
                if a.width != b.width
                    && let Some(stroke) = &mut s.style.stroke
                {
                    stroke.width = b.width;
                }
            }
            (_, stroke) => {
                s.style.stroke = stroke;
                s.stroke_keys.clear();
            }
        }
        self.dirty = true;
    }

    /// Move shape `from` to position `to` (both indices in the current
    /// shapes list). One undo step. Does nothing when `from == to` or
    /// either index is out of range.
    pub fn reorder_shape(&mut self, id: LayerId, from: usize, to: usize) {
        if from == to {
            return;
        }
        let count = self.layer_shapes(id).map_or(0, Vec::len);
        if from >= count || to >= count {
            return;
        }
        self.checkpoint("Reorder shapes");
        if let Some(shapes) = self.layer_shapes_mut(id) {
            let shape = shapes.remove(from);
            shapes.insert(to, shape);
        }
        // fix up any point selections that referred to the moved shapes
        self.points.clear();
        self.dirty = true;
    }

    /// Whether a drawn item is one the tools edit: any shape of a vector
    /// layer, or a shape in the active sub-layer of a drawing.
    pub(super) fn editable(&self, item: &DrawItem<'_>) -> bool {
        item.image.is_none() && item.part.is_none_or(|p| p == self.part) && !self.is_locked(item.layer)
    }

    /// Start a new, empty drawing on the current frame of the selected
    /// frame-by-frame layer (Animate's blank keyframe).
    pub(super) fn new_drawing(&mut self, copy: bool) -> bool {
        let Some(id) = self.primary() else { return false };
        let frame = self.current_frame();
        let Some(LayerKind::Frames { cels }) = self.doc.find(id).map(|l| &l.kind) else {
            return false;
        };
        if cels.iter().any(|c| c.frame == frame) {
            self.set_status(
                super::StatusKind::Info,
                "There is already a drawing on this frame",
            );
            return false;
        }
        let held = cel_at(cels, frame as f32).map(|i| cels[i].clone());
        self.checkpoint(if copy { "Duplicate drawing" } else { "New drawing" });
        if let Some(LayerKind::Frames { cels }) = self.doc.find_mut(id).map(|l| &mut l.kind) {
            let mut cel = match (copy, held) {
                (true, Some(c)) => c,
                _ => Cel::new(frame),
            };
            cel.frame = frame;
            let at = cels.partition_point(|c| c.frame < frame);
            cels.insert(at, cel);
        }
        self.points.clear();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Tool};
    use fanimate_core::{Document, Part, Vec2};

    fn editor() -> (Editor, LayerId) {
        let mut doc = Document::new("t", 200.0, 200.0, 24.0);
        let layer = doc.new_layer("anim", LayerKind::Frames { cels: vec![] });
        let id = layer.id;
        doc.layers.push(layer);
        let mut ed = Editor::new(doc);
        ed.selection = vec![id];
        (ed, id)
    }

    fn cels(ed: &Editor, id: LayerId) -> Vec<Cel> {
        match &ed.doc.find(id).unwrap().kind {
            LayerKind::Frames { cels } => cels.clone(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn drawing_on_any_frame_starts_or_holds_a_drawing() {
        let (mut ed, id) = editor();
        ed.go_to(3);
        assert!(ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(20.0), false, false, 1.0));
        assert_eq!(cels(&ed, id).len(), 1);
        assert_eq!(cels(&ed, id)[0].frame, 3);
        assert_eq!(cels(&ed, id)[0].line.len(), 1);
        // a later frame holds that drawing: drawing there adds to it
        ed.go_to(7);
        ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(20.0), false, false, 1.0);
        assert_eq!(cels(&ed, id)[0].line.len(), 2);
        // before it, a new drawing starts
        ed.go_to(0);
        ed.add_drag_shape(Tool::Ellipse, Vec2::ZERO, Vec2::splat(20.0), false, false, 1.0);
        assert_eq!(cels(&ed, id).iter().map(|c| c.frame).collect::<Vec<_>>(), [0, 3]);
    }

    #[test]
    fn sub_layers_keep_their_shapes_apart() {
        let (mut ed, id) = editor();
        ed.part = Part::Rough;
        ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(20.0), false, false, 1.0);
        ed.part = Part::Colour;
        ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(20.0), false, false, 1.0);
        let c = &cels(&ed, id)[0];
        assert_eq!((c.rough.len(), c.colour.len(), c.line.len()), (1, 1, 0));
        // the point tools see only the active sub-layer
        ed.tool = Tool::SelectPoints;
        ed.select_all_points();
        assert_eq!(ed.points.len(), 4);
        assert!(ed.points.iter().all(|p| p.shape == 0));
        assert!(ed.shape_at(Vec2::splat(10.0), 0.0).is_some());
    }

    #[test]
    fn new_and_duplicated_drawings_and_their_timing() {
        let (mut ed, id) = editor();
        ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(20.0), false, false, 1.0);
        ed.go_to(4);
        ed.run(Action::NewDrawing);
        ed.go_to(8);
        ed.run(Action::AddKey);
        let c = cels(&ed, id);
        assert_eq!(c.iter().map(|c| c.frame).collect::<Vec<_>>(), [0, 4, 8]);
        assert!(c[1].is_empty());
        // the copy holds what was showing: the blank drawing
        assert!(c[2].is_empty());
        assert_eq!(ed.key_frames(id), [0, 4, 8]);
        // retime by dragging, like keys
        assert!(ed.begin_key_drag(4));
        ed.drag_keys_to(6);
        ed.end_key_drag();
        assert_eq!(ed.key_frames(id), [0, 6, 8]);
        ed.go_to(8);
        ed.run(Action::DeleteKey);
        assert_eq!(ed.key_frames(id), [0, 6]);
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
    }
}
