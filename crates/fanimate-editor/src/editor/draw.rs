//! Drawing on the canvas: brush strokes, shapes, the Paint Bucket and
//! Eyedropper, and the style new artwork gets.

use fanimate_core::brush::{self, BrushOptions};
use fanimate_core::{
    BrushSample, Color, LayerId, LayerKind, Path, Shape, Stabiliser, Stroke, Style, Vec2, evaluate,
};
use serde::{Deserialize, Serialize};

use super::{Editor, StatusKind};
use crate::action::Tool;

/// Colours and brush settings for new artwork. The app keeps them between
/// sessions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawStyle {
    /// Colour of brush strokes and shape outlines.
    pub line: Color,
    /// Brush size at full pressure, and outline width, in document units.
    pub width: f32,
    /// Fill of new shapes; `None` draws outlines only.
    pub fill: Option<Color>,
    /// Whether new shapes get an outline. Shapes with no fill always do.
    pub outline: bool,
    /// How much the brush smooths shaky lines, from 0 (off) to 1.
    pub smoothing: f32,
    /// Length of each tapered end of a brush stroke, in document units.
    pub taper: f32,
    /// Eraser diameter, in document units.
    pub eraser: f32,
    /// Weight brush diameter, in document units.
    pub weight_brush: f32,
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self {
            line: Color::BLACK,
            width: 6.0,
            fill: Some(Color::WHITE),
            outline: true,
            smoothing: 0.5,
            taper: 20.0,
            eraser: 20.0,
            weight_brush: 40.0,
        }
    }
}

/// Distance between resampled brush points, in screen pixels. Smoothing is
/// counted in these points, so it feels the same at every zoom.
const BRUSH_SPACING_PX: f32 = 2.0;

/// How far simplifying a finished stroke may move it, in screen pixels:
/// below what the eye can see at the zoom it was drawn at.
const BRUSH_TOLERANCE_PX: f32 = 0.35;

pub(super) struct EraseInProgress {
    layer: LayerId,
    /// Where the eraser was last, in document space.
    last: Vec2,
    /// Screen pixels per document unit.
    zoom: f32,
    /// Whether this gesture has recorded its undo step yet. It does so only
    /// once something is erased, so a drag over nothing adds no step.
    recorded: bool,
}

pub(super) struct StrokeInProgress {
    /// The layer the stroke goes on; `None` makes a new vector layer.
    target: Option<LayerId>,
    stabiliser: Stabiliser,
    /// Screen pixels per document unit when the stroke started.
    zoom: f32,
}

impl Editor {
    /// Where new artwork goes: the selected vector layer (`Some`), or a new
    /// layer (`None`) when anything else or nothing is selected. Fails, with
    /// a status message, when the selection is hidden.
    pub(super) fn drawing_target(&mut self) -> Result<Option<LayerId>, ()> {
        match self.primary().and_then(|id| self.doc.find(id)) {
            Some(layer) if !self.shown(layer.id) => {
                let msg = format!("\"{}\" is hidden. Show it to draw on it.", layer.name);
                self.set_status(StatusKind::Warning, msg);
                Err(())
            }
            Some(layer) if matches!(layer.kind, LayerKind::Vector { .. } | LayerKind::Frames { .. }) => {
                // a layer scaled to nothing has no space to draw in
                if self.world(layer.id).inverse().is_finite() {
                    Ok(Some(layer.id))
                } else {
                    let msg = format!("\"{}\" is scaled to zero, so it can't be drawn on.", layer.name);
                    self.set_status(StatusKind::Warning, msg);
                    Err(())
                }
            }
            _ => Ok(None),
        }
    }

    /// Start a brush stroke at `at`, in document space. `zoom` is screen
    /// pixels per document unit. Returns false, with a status message, when
    /// the selection cannot be drawn on.
    pub fn begin_stroke(&mut self, at: Vec2, pressure: f32, zoom: f32) -> bool {
        let Ok(target) = self.drawing_target() else {
            return false;
        };
        let zoom = sane_zoom(zoom);
        let mut stabiliser = Stabiliser::new(self.draw.smoothing, BRUSH_SPACING_PX / zoom);
        stabiliser.push(BrushSample::new(at, pressure));
        self.stroke = Some(StrokeInProgress {
            target,
            stabiliser,
            zoom,
        });
        true
    }

    pub fn continue_stroke(&mut self, at: Vec2, pressure: f32) {
        if let Some(stroke) = &mut self.stroke {
            stroke.stabiliser.push(BrushSample::new(at, pressure));
        }
    }

    pub fn is_drawing(&self) -> bool {
        self.stroke.is_some()
    }

    /// The stroke being drawn, in document space, as it would be if the
    /// pen lifted now.
    pub fn stroke_preview(&self) -> Option<Shape> {
        let stroke = self.stroke.as_ref()?;
        brush::build_stroke(&stroke.stabiliser.samples(), &self.brush_options(stroke.zoom))
    }

    pub fn cancel_stroke(&mut self) {
        self.stroke = None;
    }

    /// Finish the stroke and add it to its layer, as one undo step.
    pub fn finish_stroke(&mut self) {
        let Some(stroke) = self.stroke.take() else {
            return;
        };
        if let Some(shape) =
            brush::build_stroke(&stroke.stabiliser.samples(), &self.brush_options(stroke.zoom))
        {
            // the layer may have gone (undo while drawing): draw on a new one
            let target = stroke.target.filter(|id| self.doc.find(*id).is_some());
            self.add_shape(target, shape);
        }
    }

    /// Start erasing at `at` (document space) on the selected vector layer.
    /// Returns false, with a status message, when there is no such layer.
    pub fn begin_erase(&mut self, at: Vec2, zoom: f32) -> bool {
        let layer = match self.drawing_target() {
            Ok(Some(layer)) => layer,
            Ok(None) => {
                self.set_status(StatusKind::Info, "Select a vector layer to erase on");
                return false;
            }
            Err(()) => return false,
        };
        self.erase = Some(EraseInProgress {
            layer,
            last: at,
            zoom: sane_zoom(zoom),
            recorded: false,
        });
        self.erase_to(at);
        true
    }

    /// Move the eraser to `to`, erasing along the way. The whole gesture,
    /// from [`begin_erase`](Self::begin_erase) on, is one undo step.
    pub fn erase_to(&mut self, to: Vec2) {
        let Some(erase) = &self.erase else {
            return;
        };
        let (id, from, zoom) = (erase.layer, erase.last, erase.zoom);
        let inv = self.world(id).inverse();
        let Some(shapes) = self.layer_shapes(id) else {
            return;
        };
        // erase in the layer's own space, where its shapes are
        let eraser = [inv.transform_point2(from), inv.transform_point2(to)];
        let scale = inv.matrix2.determinant().abs().sqrt();
        let radius = self.draw.eraser / 2.0 * scale;
        let tolerance = BRUSH_TOLERANCE_PX / zoom * scale;
        // shapes bound to bones are drawn posed, not where they are stored,
        // and cutting a shape with point keys would lose its animation
        let changes: Vec<(usize, Vec<Shape>)> = shapes
            .iter()
            .enumerate()
            .filter(|(_, s)| s.bone.is_none() && !s.is_animated())
            .filter_map(|(i, s)| brush::erase(s, &eraser, radius, tolerance).map(|left| (i, left)))
            .collect();
        if let Some(erase) = &mut self.erase {
            erase.last = to;
        }
        if changes.is_empty() {
            return;
        }
        if !self.erase.as_ref().is_some_and(|e| e.recorded) {
            self.checkpoint("Erase");
            if let Some(erase) = &mut self.erase {
                erase.recorded = true;
            }
        }
        if let Some(shapes) = self.layer_shapes_mut(id) {
            // back to front, so earlier indices stay valid
            for (i, left) in changes.into_iter().rev() {
                shapes.splice(i..=i, left);
            }
        }
    }

    pub fn end_erase(&mut self) {
        self.erase = None;
    }

    pub fn is_erasing(&self) -> bool {
        self.erase.is_some()
    }

    fn brush_options(&self, zoom: f32) -> BrushOptions {
        BrushOptions {
            color: self.draw.line,
            size: self.draw.width,
            taper: self.draw.taper,
            tolerance: BRUSH_TOLERANCE_PX / zoom,
        }
    }

    /// The rectangle or ellipse a drag from `from` to `to` (document space)
    /// makes, in the current style. `even` makes a square or circle;
    /// `centred` grows the shape out from `from`. `None` for other tools,
    /// or a drag smaller than a screen pixel (`zoom` is screen pixels per
    /// document unit).
    pub fn drag_shape(
        &self,
        tool: Tool,
        from: Vec2,
        to: Vec2,
        even: bool,
        centred: bool,
        zoom: f32,
    ) -> Option<Shape> {
        let mut half = (to - from) / if centred { 1.0 } else { 2.0 };
        if even {
            let side = half.abs().max_element();
            half = Vec2::new(side.copysign(half.x), side.copysign(half.y));
        }
        let center = if centred { from } else { from + half };
        let radii = half.abs();
        if !radii.is_finite() || radii.min_element() * sane_zoom(zoom) < 0.5 {
            return None;
        }
        let d = &self.draw;
        let outline = (d.outline || d.fill.is_none()).then_some(Stroke {
            color: d.line,
            width: d.width,
        });
        let style = Style {
            fill: d.fill,
            stroke: outline,
        };
        Some(match tool {
            // a rectangle remembers it is one, so its corners can be rounded
            Tool::Rectangle => Shape::rectangle("Rectangle", center - radii, radii * 2.0, 0.0, style),
            Tool::Ellipse => Shape::new("Ellipse", Path::ellipse(center, radii), style),
            _ => return None,
        })
    }

    /// Add the shape from [`drag_shape`](Self::drag_shape), as one undo step.
    /// Returns whether a shape was added.
    pub fn add_drag_shape(
        &mut self,
        tool: Tool,
        from: Vec2,
        to: Vec2,
        even: bool,
        centred: bool,
        zoom: f32,
    ) -> bool {
        let Some(shape) = self.drag_shape(tool, from, to, even, centred, zoom) else {
            return false;
        };
        let Ok(target) = self.drawing_target() else {
            return false;
        };
        self.add_shape(target, shape);
        true
    }

    /// Add a shape given in document space to `target`, or to a new vector
    /// layer: inside the selected group or bone layer, else above the
    /// selection. The shape's name is a word ("Stroke"), numbered here.
    /// One undo step. Returns the shape's index in its layer.
    pub(super) fn add_shape(&mut self, target: Option<LayerId>, mut shape: Shape) -> Option<usize> {
        self.checkpoint("Draw");
        let id = match target {
            Some(id) => id,
            None => self.new_drawing_layer(),
        };
        shape.transform(self.world(id).inverse());
        if let Some(shapes) = self.drawing_shapes_mut(id) {
            let prefix = format!("{} ", shape.name);
            let n = shapes.iter().filter(|s| s.name.starts_with(&prefix)).count() + 1;
            shape.name = format!("{prefix}{n}");
            shapes.push(shape);
            return Some(shapes.len() - 1);
        }
        None
    }

    /// A new, empty vector layer for drawing on, selected.
    fn new_drawing_layer(&mut self) -> LayerId {
        let name = self.unused_name("Layer");
        let layer = super::labelled(self.doc.new_layer(name, LayerKind::vector(vec![])));
        let id = layer.id;
        let (parent, index) = match self.primary().and_then(|p| self.doc.find(p)) {
            Some(p) if p.kind.children().is_some() => (Some(p.id), p.kind.children().map_or(0, Vec::len)),
            _ => self.insertion_point(),
        };
        self.doc.insert(parent, index, layer);
        self.selection = vec![id];
        self.anchor = Some(id);
        id
    }

    /// The topmost visible shape under `p` (document space), within
    /// `tolerance` document units: its layer and index in that layer.
    pub fn shape_at(&self, p: Vec2, tolerance: f32) -> Option<(LayerId, usize)> {
        self.hit(p, tolerance, false, false)
    }

    /// Like [`shape_at`](Self::shape_at), among the shapes the tools edit
    /// (see [`Editor::layer_shapes`]).
    pub(super) fn editable_shape_at(&self, p: Vec2, tolerance: f32) -> Option<(LayerId, usize)> {
        self.hit(p, tolerance, true, false)
    }

    /// The shape under `p` that the topmost hit is, where a closed outline
    /// with no fill yet counts as hit inside, as the Paint Bucket needs.
    /// `interior`: treat closed outlines as filled.
    fn hit(&self, p: Vec2, tolerance: f32, editable: bool, interior: bool) -> Option<(LayerId, usize)> {
        let items = evaluate(&self.doc, self.frame);
        items
            .iter()
            .rev()
            .filter(|i| !editable || self.editable(i))
            .find_map(|item| {
                let inv = item.transform.inverse();
                if !inv.is_finite() {
                    return None;
                }
                let local_tolerance = tolerance * inv.matrix2.determinant().abs().sqrt();
                let local = inv.transform_point2(p);
                let closed = item
                    .shape
                    .path
                    .cmds
                    .iter()
                    .any(|c| matches!(c, fanimate_core::PathCmd::Close));
                let hit = if interior && closed && item.shape.style.fill.is_none() {
                    // only at click time, so the copy costs nothing per frame
                    let mut filled = item.shape.as_ref().clone();
                    filled.style.fill = Some(fanimate_core::Color::BLACK);
                    filled.contains(local, local_tolerance)
                } else {
                    item.shape.contains(local, local_tolerance)
                };
                hit.then_some((item.layer, item.index))
            })
    }

    /// Paint Bucket: fill the shape under `p` with the fill colour, or, for
    /// a brush stroke, recolour it with the line colour. One undo step.
    /// Returns whether anything changed.
    pub fn paint_at(&mut self, p: Vec2, tolerance: f32) -> bool {
        let Some((id, index)) = self.hit(p, tolerance, true, true) else {
            return false;
        };
        let Some(shape) = self.shape(id, index) else {
            return false;
        };
        let brush_stroke = shape.widths.is_some() && shape.style.fill.is_none();
        let mut style = shape.style;
        if brush_stroke {
            if let Some(stroke) = &mut style.stroke {
                stroke.color = self.draw.line;
            }
        } else {
            let Some(fill) = self.draw.fill else {
                self.set_status(StatusKind::Info, "Turn on Fill in the Style panel to fill shapes");
                return false;
            };
            style.fill = Some(fill);
        }
        if style == shape.style {
            return false;
        }
        self.checkpoint(if brush_stroke {
            "Recolour stroke"
        } else {
            "Fill shape"
        });
        if let Some(shape) = self.shape_mut(id, index) {
            shape.style = style;
        }
        true
    }

    /// Eyedropper: take the colours of the shape under `p` for new artwork.
    /// Not an undo step: it changes no artwork.
    pub fn pick_style_at(&mut self, p: Vec2, tolerance: f32) -> bool {
        let Some((id, index)) = self.editable_shape_at(p, tolerance) else {
            return false;
        };
        let Some(shape) = self.shape(id, index) else {
            return false;
        };
        let (style, name) = (shape.style, shape.name.clone());
        if let Some(stroke) = style.stroke {
            self.draw.line = stroke.color;
        }
        if style.fill.is_some() {
            self.draw.fill = style.fill;
        }
        self.set_status(StatusKind::Info, format!("Picked the colours of \"{name}\""));
        true
    }

    fn shape(&self, id: LayerId, index: usize) -> Option<&Shape> {
        self.layer_shapes(id)?.get(index)
    }

    fn shape_mut(&mut self, id: LayerId, index: usize) -> Option<&mut Shape> {
        self.layer_shapes_mut(id)?.get_mut(index)
    }

    /// Whether a layer and all its ancestors are visible.
    pub fn shown(&self, id: LayerId) -> bool {
        let mut current = Some(id);
        while let Some(c) = current {
            if self.doc.find(c).is_none_or(|l| !l.visible) {
                return false;
            }
            current = self.doc.locate(c).and_then(|(p, _)| p);
        }
        true
    }
}

fn sane_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{AnimTransform, Document, Layer, PathCmd};

    fn editor() -> (Editor, LayerId) {
        let ed = Editor::untitled();
        let id = ed.primary().unwrap();
        (ed, id)
    }

    fn shapes(ed: &Editor, id: LayerId) -> Vec<Shape> {
        match &ed.doc.find(id).unwrap().kind {
            LayerKind::Vector { shapes } => shapes.clone(),
            _ => panic!("not a vector layer"),
        }
    }

    fn draw_line(ed: &mut Editor, from: Vec2, to: Vec2) {
        assert!(ed.begin_stroke(from, 1.0, 1.0));
        for i in 1..=50 {
            ed.continue_stroke(from.lerp(to, i as f32 / 50.0), 1.0);
        }
        assert!(ed.stroke_preview().is_some());
        ed.finish_stroke();
        assert!(!ed.is_drawing());
    }

    #[test]
    fn stroke_goes_on_the_selected_layer_as_one_undo_step() {
        let (mut ed, id) = editor();
        draw_line(&mut ed, Vec2::new(10.0, 10.0), Vec2::new(200.0, 10.0));
        let s = shapes(&ed, id);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Stroke 1");
        assert!(s[0].widths.is_some());
        assert_eq!(s[0].style.stroke.unwrap().color, ed.draw.line);
        assert!(ed.dirty);
        ed.run(crate::Action::Undo);
        assert!(shapes(&ed, id).is_empty());
    }

    #[test]
    fn stroke_is_stored_in_the_layers_own_space() {
        let (mut ed, id) = editor();
        ed.doc.find_mut(id).unwrap().transform = AnimTransform {
            scale: fanimate_core::Channel::new(Vec2::splat(2.0)),
            ..AnimTransform::at(Vec2::new(100.0, 0.0))
        };
        draw_line(&mut ed, Vec2::new(100.0, 0.0), Vec2::new(300.0, 0.0));
        let s = &shapes(&ed, id)[0];
        let pts: Vec<Vec2> = s.path.points().collect();
        assert!(pts[0].abs_diff_eq(Vec2::ZERO, 1e-3), "{:?}", pts[0]);
        assert!(pts.last().unwrap().abs_diff_eq(Vec2::new(100.0, 0.0), 1e-3));
        // half the width, since the layer doubles it again when drawn
        assert_eq!(s.style.stroke.unwrap().width, ed.draw.width / 2.0);
    }

    #[test]
    fn drawing_with_a_group_selected_makes_a_layer_inside_it() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let group = doc.new_layer("G", LayerKind::Group { children: vec![] });
        let gid = group.id;
        doc.layers.push(group);
        let mut ed = Editor::new(doc);
        ed.selection = vec![gid];
        draw_line(&mut ed, Vec2::ZERO, Vec2::new(50.0, 50.0));
        let new = ed.primary().unwrap();
        assert_ne!(new, gid);
        assert_eq!(ed.doc.locate(new), Some((Some(gid), 0)));
        assert_eq!(ed.doc.find(new).unwrap().name, "Layer 1");
        assert_eq!(shapes(&ed, new).len(), 1);
        // one undo removes both the stroke and the layer it made
        ed.run(crate::Action::Undo);
        assert!(ed.doc.find(new).is_none());
    }

    #[test]
    fn drawing_with_nothing_selected_makes_a_top_layer() {
        let (mut ed, id) = editor();
        ed.selection.clear();
        draw_line(&mut ed, Vec2::ZERO, Vec2::new(50.0, 0.0));
        let new = ed.primary().unwrap();
        assert_ne!(new, id);
        assert_eq!(ed.doc.locate(new), Some((None, 1)));
    }

    #[test]
    fn hidden_layers_cannot_be_drawn_on() {
        let (mut ed, id) = editor();
        ed.doc.find_mut(id).unwrap().visible = false;
        assert!(!ed.begin_stroke(Vec2::ZERO, 1.0, 1.0));
        assert!(!ed.is_drawing());
        assert!(matches!(ed.status, Some((StatusKind::Warning, _))));
        // nothing was recorded
        assert!(ed.history.undo_label().is_none());
    }

    #[test]
    fn a_tap_adds_a_dot() {
        let (mut ed, id) = editor();
        assert!(ed.begin_stroke(Vec2::new(5.0, 5.0), 0.5, 1.0));
        ed.finish_stroke();
        let s = shapes(&ed, id);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Dot 1");
        assert_eq!(s[0].style.fill, Some(ed.draw.line));
        assert!(matches!(s[0].path.cmds[0], PathCmd::MoveTo(_)));
    }

    #[test]
    fn cancelled_stroke_leaves_no_trace() {
        let (mut ed, id) = editor();
        assert!(ed.begin_stroke(Vec2::ZERO, 1.0, 1.0));
        ed.continue_stroke(Vec2::new(40.0, 0.0), 1.0);
        ed.cancel_stroke();
        ed.finish_stroke();
        assert!(shapes(&ed, id).is_empty());
        assert!(ed.history.undo_label().is_none());
    }

    #[test]
    fn rectangle_and_ellipse_follow_the_drag_and_style() {
        let (mut ed, id) = editor();
        let (a, b) = (Vec2::new(10.0, 10.0), Vec2::new(50.0, 30.0));
        assert!(ed.add_drag_shape(Tool::Rectangle, a, b, false, false, 1.0));
        assert!(ed.add_drag_shape(Tool::Ellipse, a, b, true, true, 1.0));
        let s = shapes(&ed, id);
        assert_eq!(
            [s[0].name.as_str(), s[1].name.as_str()],
            ["Rectangle 1", "Ellipse 1"]
        );
        assert_eq!(s[0].style.fill, ed.draw.fill);
        assert_eq!(s[0].style.stroke.unwrap().width, ed.draw.width);
        let bounds = |sh: &Shape| {
            sh.path
                .polylines(0.01)
                .iter()
                .flat_map(|(p, _)| p.clone())
                .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)))
        };
        assert_eq!(bounds(&s[0]), (a, b));
        // even and centred: a circle of radius 40 around `a`
        let (lo, hi) = bounds(&s[1]);
        assert!(lo.abs_diff_eq(a - Vec2::splat(40.0), 0.01) && hi.abs_diff_eq(a + Vec2::splat(40.0), 0.01));
        // a drag too small to see adds nothing
        assert!(!ed.add_drag_shape(Tool::Rectangle, a, a + Vec2::splat(0.1), false, false, 1.0));
        assert!(ed.drag_shape(Tool::Freehand, a, b, false, false, 1.0).is_none());
    }

    #[test]
    fn shapes_without_fill_always_get_an_outline() {
        let (mut ed, _) = editor();
        ed.draw.fill = None;
        ed.draw.outline = false;
        let s = ed
            .drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(10.0), false, false, 1.0)
            .unwrap();
        assert!(s.style.stroke.is_some());
    }

    #[test]
    fn shape_at_finds_the_topmost_shape_on_screen() {
        let (mut ed, id) = editor();
        ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(100.0), false, false, 1.0);
        ed.add_drag_shape(
            Tool::Rectangle,
            Vec2::splat(50.0),
            Vec2::splat(150.0),
            false,
            false,
            1.0,
        );
        assert_eq!(ed.shape_at(Vec2::splat(75.0), 0.0), Some((id, 1)));
        assert_eq!(ed.shape_at(Vec2::splat(25.0), 0.0), Some((id, 0)));
        assert_eq!(ed.shape_at(Vec2::splat(500.0), 0.0), None);
        ed.doc.find_mut(id).unwrap().visible = false;
        assert_eq!(ed.shape_at(Vec2::splat(75.0), 0.0), None);
    }

    #[test]
    fn paint_bucket_fills_shapes_and_recolours_strokes() {
        let (mut ed, id) = editor();
        ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(100.0), false, false, 1.0);
        draw_line(&mut ed, Vec2::new(0.0, 200.0), Vec2::new(200.0, 200.0));
        let red = Color::rgb(1.0, 0.0, 0.0);
        ed.draw.fill = Some(red);
        ed.draw.line = red;
        assert!(ed.paint_at(Vec2::splat(50.0), 1.0));
        assert!(ed.paint_at(Vec2::new(100.0, 200.0), 1.0));
        let s = shapes(&ed, id);
        assert_eq!(s[0].style.fill, Some(red));
        assert_eq!(s[1].style.stroke.unwrap().color, red);
        assert!(s[1].style.fill.is_none());
        // already that colour: no change, no undo step
        let steps = ed.history.undo_label().map(str::to_owned);
        assert!(!ed.paint_at(Vec2::splat(50.0), 1.0));
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps);
        // undo takes back one fill
        ed.run(crate::Action::Undo);
        assert_eq!(shapes(&ed, id)[1].style.stroke.unwrap().color, Color::BLACK);
        assert!(!ed.paint_at(Vec2::splat(500.0), 1.0));
    }

    #[test]
    fn paint_bucket_fills_inside_an_outline_with_no_fill() {
        let (mut ed, id) = editor();
        // a rectangle drawn with Fill off is only an outline
        ed.draw.fill = None;
        ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(100.0), false, false, 1.0);
        assert!(shapes(&ed, id)[0].style.fill.is_none());
        let red = Color::rgb(1.0, 0.0, 0.0);
        ed.draw.fill = Some(red);
        // clicking in the middle, far from the outline, fills it
        assert!(ed.paint_at(Vec2::splat(50.0), 1.0));
        assert_eq!(shapes(&ed, id)[0].style.fill, Some(red));
        // an open line is still only hit near the line
        draw_line(&mut ed, Vec2::new(0.0, 200.0), Vec2::new(200.0, 300.0));
        assert!(!ed.paint_at(Vec2::new(150.0, 220.0), 1.0));
    }

    #[test]
    fn eyedropper_takes_colours_without_an_undo_step() {
        let (mut ed, _) = editor();
        let blue = Color::rgb(0.0, 0.0, 1.0);
        ed.draw.fill = Some(blue);
        ed.add_drag_shape(Tool::Rectangle, Vec2::ZERO, Vec2::splat(100.0), false, false, 1.0);
        ed.draw.fill = Some(Color::WHITE);
        let steps = ed.history.undo_label().map(str::to_owned);
        assert!(ed.pick_style_at(Vec2::splat(50.0), 0.0));
        assert_eq!(ed.draw.fill, Some(blue));
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps);
    }

    #[test]
    fn one_erase_gesture_is_one_undo_step() {
        let (mut ed, id) = editor();
        draw_line(&mut ed, Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0));
        draw_line(&mut ed, Vec2::new(0.0, 100.0), Vec2::new(200.0, 100.0));
        let before = shapes(&ed, id);
        let steps_before = ed.history.undo_label().map(str::to_owned);
        // a vertical drag across both lines, in several moves
        assert!(ed.begin_erase(Vec2::new(100.0, -20.0), 1.0));
        for y in [20.0, 60.0, 120.0] {
            ed.erase_to(Vec2::new(100.0, y));
        }
        ed.end_erase();
        assert!(!ed.is_erasing());
        assert_eq!(shapes(&ed, id).len(), 4);
        ed.run(crate::Action::Undo);
        assert_eq!(shapes(&ed, id), before);
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps_before);
    }

    #[test]
    fn erasing_nothing_records_nothing() {
        let (mut ed, _) = editor();
        draw_line(&mut ed, Vec2::ZERO, Vec2::new(200.0, 0.0));
        let steps = ed.history.undo_label().map(str::to_owned);
        let dirty_before = ed.history.redo_label().map(str::to_owned);
        assert!(ed.begin_erase(Vec2::new(0.0, 300.0), 1.0));
        ed.erase_to(Vec2::new(200.0, 300.0));
        ed.end_erase();
        assert_eq!(ed.history.undo_label().map(str::to_owned), steps);
        assert_eq!(ed.history.redo_label().map(str::to_owned), dirty_before);
    }

    #[test]
    fn eraser_works_in_the_layers_space() {
        let (mut ed, id) = editor();
        ed.doc.find_mut(id).unwrap().transform = AnimTransform::at(Vec2::new(1000.0, 0.0));
        draw_line(&mut ed, Vec2::new(1000.0, 0.0), Vec2::new(1200.0, 0.0));
        assert!(ed.begin_erase(Vec2::new(1100.0, 0.0), 1.0));
        ed.end_erase();
        assert_eq!(shapes(&ed, id).len(), 2);
    }

    #[test]
    fn erasing_needs_a_vector_layer() {
        let (mut ed, _) = editor();
        ed.selection.clear();
        assert!(!ed.begin_erase(Vec2::ZERO, 1.0));
        assert!(matches!(ed.status, Some((StatusKind::Info, _))));
    }

    #[test]
    fn layer_shown_only_when_its_ancestors_are() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let child = doc.new_layer("c", LayerKind::vector(vec![]));
        let cid = child.id;
        let mut group: Layer = doc.new_layer(
            "g",
            LayerKind::Group {
                children: vec![child],
            },
        );
        group.visible = false;
        doc.layers.push(group);
        let ed = Editor::new(doc);
        assert!(!ed.shown(cid));
    }
}
