//! Text layers: adding them, setting them, and animating them.
//!
//! The engine and the data are in `fanimate-core` (`text`); this is how the
//! user, a script or the AI tools change them. Every command is one undo step,
//! and animator values follow the keying rule like every other animated value:
//! on the rest pose they change the value itself, on a later frame they set a
//! key (and seed one on the rest pose so nothing jumps).

use fanimate_core::{Animator, Channel, Color, Look, Text, TextPreset, Vec2};
use fanimate_core::{LayerId, LayerKind};

use super::{Editor, StatusKind, labelled};

/// Which of an animator's values a change is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimatorProp {
    RangeStart,
    RangeEnd,
    RangeOffset,
    Position,
    Rotation,
    Scale,
    Opacity,
    Fill,
    Anchor,
    Tracking,
    StrokeWidth,
    StrokeColour,
}

impl AnimatorProp {
    pub const ALL: [AnimatorProp; 12] = [
        Self::RangeStart,
        Self::RangeEnd,
        Self::RangeOffset,
        Self::Position,
        Self::Rotation,
        Self::Scale,
        Self::Opacity,
        Self::Fill,
        Self::Anchor,
        Self::Tracking,
        Self::StrokeWidth,
        Self::StrokeColour,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RangeStart => "Range start",
            Self::RangeEnd => "Range end",
            Self::RangeOffset => "Range offset",
            Self::Position => "Position",
            Self::Rotation => "Rotation",
            Self::Scale => "Scale",
            Self::Opacity => "Opacity",
            Self::Fill => "Fill colour",
            Self::Anchor => "Anchor",
            Self::Tracking => "Letter spacing",
            Self::StrokeWidth => "Outline width",
            Self::StrokeColour => "Outline colour",
        }
    }
}

/// A value for an [`AnimatorProp`]: a number for the range, rotation and
/// opacity, a pair for position and scale, a colour for the fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropValue {
    Number(f32),
    Pair(Vec2),
    Colour(Color),
}

impl PropValue {
    fn is_finite(self) -> bool {
        match self {
            Self::Number(n) => n.is_finite(),
            Self::Pair(p) => p.is_finite(),
            Self::Colour(c) => [c.r, c.g, c.b, c.a].iter().all(|v| v.is_finite()),
        }
    }
}

/// Where a new text layer goes on the canvas when no point was chosen, as
/// fractions of the canvas: a little in from the left, about a third down.
const DEFAULT_SPOT: Vec2 = Vec2::new(0.1, 0.33);

impl Editor {
    /// The text layer with this id, if it is one.
    pub fn text(&self, id: LayerId) -> Option<&Text> {
        match &self.doc.find(id)?.kind {
            LayerKind::Text(t) => Some(t),
            _ => None,
        }
    }

    fn text_mut(&mut self, id: LayerId) -> Option<&mut Text> {
        match &mut self.doc.find_mut(id)?.kind {
            LayerKind::Text(t) => Some(t),
            _ => None,
        }
    }

    /// The topmost shown text layer whose box holds the document point `p`, with a
    /// margin of `slop` document units (and at least one line of height, so that
    /// empty or one-word text can still be hit).
    pub fn text_at(&self, p: Vec2, slop: f32) -> Option<LayerId> {
        let mut found = None;
        self.doc.walk(|layer| {
            let LayerKind::Text(text) = &layer.kind else {
                return;
            };
            if !self.shown(layer.id) {
                return;
            }
            let inverse = self.world(layer.id).inverse();
            if !inverse.is_finite() {
                return;
            }
            let local = inverse.transform_point2(p);
            let size = text.box_size().max(Vec2::splat(text.look().size * 0.5));
            let slop = slop.max(0.0);
            if local.x >= -slop && local.y >= -slop && local.x <= size.x + slop && local.y <= size.y + slop {
                // layers are drawn first to last, so the last found is on top
                found = Some(layer.id);
            }
        });
        found
    }

    /// The primary selected layer, when it is a text layer.
    pub fn selected_text(&self) -> Option<LayerId> {
        self.primary().filter(|id| self.text(*id).is_some())
    }

    /// New Text Layer in the menu: a text layer with some words in it, in the
    /// current line colour (black unless changed), above the selection. Its settings are edited in
    /// the Properties panel.
    pub(super) fn new_text_layer(&mut self) {
        let at = Vec2::new(self.doc.width, self.doc.height) * DEFAULT_SPOT;
        self.add_text_layer(at, "Text");
    }

    /// Add a text layer with `content`, its box's top left at `at` in the
    /// document, above the selection (or in the selected group), filled with
    /// the current line colour (the fill is white by default, which would not show) and selected. One undo step. `None` while the
    /// animation plays.
    pub fn add_text_layer(&mut self, at: Vec2, content: &str) -> Option<LayerId> {
        if self.editing_blocked() || !at.is_finite() {
            return None;
        }
        self.checkpoint("New text layer");
        let mut text = Text::new(content);
        text.look_mut().fill = self.draw.line;
        let name = self.unused_name("Text");
        let mut layer = labelled(self.doc.new_layer(name, LayerKind::Text(text)));
        layer.transform.position.base = at;
        let id = layer.id;
        let (parent, index) = self.insertion_point();
        self.doc.insert(parent, index, layer);
        self.selection = vec![id];
        self.anchor = Some(id);
        Some(id)
    }

    /// Change how a text layer is set: `change` is given a copy of its look,
    /// and what it leaves is held within what is allowed and becomes the
    /// look. One undo step, and none if nothing changed. Returns whether
    /// something did.
    pub fn edit_text(&mut self, id: LayerId, label: &str, change: impl FnOnce(&mut Look)) -> bool {
        self.edit_text_with(id, Some(label), change)
    }

    /// [`edit_text`](Self::edit_text) for a widget that has already begun its
    /// edit (`properties::begin_edit` made the one undo step when the drag
    /// started), so every frame of the drag changes the look without a step of
    /// its own.
    pub fn change_text(&mut self, id: LayerId, change: impl FnOnce(&mut Look)) -> bool {
        self.edit_text_with(id, None, change)
    }

    fn edit_text_with(&mut self, id: LayerId, label: Option<&str>, change: impl FnOnce(&mut Look)) -> bool {
        if self.editing_blocked() {
            return false;
        }
        let Some(text) = self.text(id) else {
            return false;
        };
        let mut look = text.look().clone();
        change(&mut look);
        look.repair();
        if look == *text.look() {
            return false;
        }
        if let Some(label) = label {
            self.checkpoint(label);
        }
        if let Some(text) = self.text_mut(id) {
            *text.look_mut() = look;
        }
        true
    }

    /// Put a ready-made animator on a text layer. It starts on the playhead
    /// (or the first frame, at the rest pose) and runs for a second. Returns
    /// its index.
    pub fn add_text_animator(&mut self, id: LayerId, preset: TextPreset) -> Option<usize> {
        if self.editing_blocked() || self.text(id)?.animators.len() >= fanimate_core::text::MAX_ANIMATORS {
            return None;
        }
        let first = self.current_frame().max(self.doc.start);
        let frames = self.doc.fps.round().clamp(1.0, 600.0) as i32;
        self.checkpoint("Add text animator");
        let text = self.text_mut(id)?;
        text.animators.push(preset.animator(first, frames));
        Some(text.animators.len() - 1)
    }

    /// Remove an animator.
    pub fn remove_text_animator(&mut self, id: LayerId, index: usize) -> bool {
        if self.editing_blocked() || self.text(id).is_none_or(|t| index >= t.animators.len()) {
            return false;
        }
        self.checkpoint("Remove text animator");
        if let Some(text) = self.text_mut(id) {
            text.animators.remove(index);
        }
        true
    }

    /// Change what is not animated about an animator: its name, unit, the
    /// shape and randomness of its selector, its soft edge. One undo step, and
    /// none if nothing changed.
    pub fn edit_text_animator(
        &mut self,
        id: LayerId,
        index: usize,
        label: &str,
        change: impl FnOnce(&mut Animator),
    ) -> bool {
        self.edit_text_animator_with(id, index, Some(label), change)
    }

    /// [`edit_text_animator`](Self::edit_text_animator) for a widget that has
    /// already begun its edit: no undo step of its own.
    pub fn change_text_animator(
        &mut self,
        id: LayerId,
        index: usize,
        change: impl FnOnce(&mut Animator),
    ) -> bool {
        self.edit_text_animator_with(id, index, None, change)
    }

    fn edit_text_animator_with(
        &mut self,
        id: LayerId,
        index: usize,
        label: Option<&str>,
        change: impl FnOnce(&mut Animator),
    ) -> bool {
        if self.editing_blocked() {
            return false;
        }
        let Some(current) = self.text(id).and_then(|t| t.animators.get(index)) else {
            return false;
        };
        let mut edited = current.clone();
        change(&mut edited);
        // the soft edge is a percentage; a value that is not usable is none
        if !edited.selector.smoothness.is_finite() {
            edited.selector.smoothness = 0.0;
        }
        edited.selector.smoothness = edited.selector.smoothness.clamp(0.0, 1000.0);
        if edited == *current {
            return false;
        }
        if let Some(label) = label {
            self.checkpoint(label);
        }
        if let Some(a) = self.text_mut(id).and_then(|t| t.animators.get_mut(index)) {
            *a = edited;
        }
        true
    }

    /// Set one of an animator's values on the playhead frame, by the keying
    /// rule. A value of the wrong kind for the property, or one that is not a
    /// number, is refused. Setting the fill when the animator has none gives
    /// it one, starting from the text's own colour.
    pub fn set_animator_value(
        &mut self,
        id: LayerId,
        index: usize,
        prop: AnimatorProp,
        value: PropValue,
    ) -> bool {
        self.animator_value_with(id, index, prop, value, true)
    }

    /// [`set_animator_value`](Self::set_animator_value) for a widget that has
    /// already begun its edit: no undo step of its own.
    pub fn change_animator_value(
        &mut self,
        id: LayerId,
        index: usize,
        prop: AnimatorProp,
        value: PropValue,
    ) -> bool {
        self.animator_value_with(id, index, prop, value, false)
    }

    fn animator_value_with(
        &mut self,
        id: LayerId,
        index: usize,
        prop: AnimatorProp,
        value: PropValue,
        step: bool,
    ) -> bool {
        if self.editing_blocked() || !value.is_finite() {
            return false;
        }
        let keying = self.keying();
        let frame = self.current_frame();
        let Some(text) = self.text(id) else {
            return false;
        };
        let base_fill = text.look().fill;
        let Some(current) = text.animators.get(index) else {
            return false;
        };
        let mut edited = current.clone();
        let ok = write_value(&mut edited, prop, value, frame, keying, base_fill);
        if !ok || edited == *current {
            return false;
        }
        if step {
            self.checkpoint("Set text animator value");
        }
        if let Some(a) = self.text_mut(id).and_then(|t| t.animators.get_mut(index)) {
            *a = edited;
        }
        true
    }

    /// Whether an animator's value is animated (has keys), and whether it has
    /// a key on the playhead frame: what the diamond beside it shows.
    pub fn animator_key_state(&self, id: LayerId, index: usize, prop: AnimatorProp) -> (bool, bool) {
        let Some(a) = self.text(id).and_then(|t| t.animators.get(index)) else {
            return (false, false);
        };
        let mut a = a.clone();
        let frame = self.current_frame();
        match key_list(&mut a, prop) {
            Some(keys) => {
                let mut frames = Vec::new();
                keys.frames_into(&mut frames);
                (!frames.is_empty(), keys.interp_at(frame).is_some())
            }
            None => (false, false),
        }
    }

    /// The diamond beside an animator's value: remove the key on the playhead
    /// frame if there is one, otherwise add one holding the value the property
    /// has there (on the rest pose that only means something once the property
    /// has keys). One undo step. Returns whether anything changed.
    pub fn toggle_animator_key(&mut self, id: LayerId, index: usize, prop: AnimatorProp) -> bool {
        if self.editing_blocked() {
            return false;
        }
        let frame = self.current_frame();
        let Some(current) = self.text(id).and_then(|t| t.animators.get(index)) else {
            return false;
        };
        let mut edited = current.clone();
        let had_key = key_list(&mut edited, prop).is_some_and(|k| k.interp_at(frame).is_some());
        if had_key {
            if let Some(keys) = key_list(&mut edited, prop) {
                keys.delete_key(frame);
            }
        } else {
            let Some(value) = value_at(current, prop, frame as f32) else {
                return false;
            };
            let keying = self.keying();
            // writing the value it already has makes the key without moving anything
            let base_fill = self.text(id).map_or(Color::BLACK, |t| t.look().fill);
            let _ = write_value(&mut edited, prop, value, frame, keying, base_fill);
        }
        if edited == *current {
            return false;
        }
        self.checkpoint(if had_key {
            "Delete text animator key"
        } else {
            "Add text animator key"
        });
        if let Some(a) = self.text_mut(id).and_then(|t| t.animators.get_mut(index)) {
            *a = edited;
        }
        true
    }

    /// Stop an animator changing the colour: forget its fill channel.
    pub fn clear_animator_fill(&mut self, id: LayerId, index: usize) -> bool {
        if self.editing_blocked()
            || self
                .text(id)
                .and_then(|t| t.animators.get(index))
                .is_none_or(|a| a.fill.is_none())
        {
            return false;
        }
        self.checkpoint("Clear text animator colour");
        if let Some(a) = self.text_mut(id).and_then(|t| t.animators.get_mut(index)) {
            a.fill = None;
        }
        true
    }

    /// Set Text Path from Shape in the menu: the primary text layer follows
    /// the first shape of another selected vector layer.
    pub(super) fn set_text_path_from_selection(&mut self) {
        let Some(id) = self.selected_text() else {
            self.set_status(StatusKind::Info, "Select a text layer first.");
            return;
        };
        let source = self.selection.iter().copied().find(|s| {
            *s != id && matches!(self.doc.find(*s).map(|l| &l.kind), Some(LayerKind::Vector { .. }))
        });
        match source {
            Some(source) if self.set_text_path_from(id, source) => {
                self.set_status(StatusKind::Success, "The text follows the shape");
            }
            Some(_) => self.set_status(StatusKind::Info, "That layer has no shape the text can follow."),
            None => self.set_status(
                StatusKind::Info,
                "Select the text layer and, with it, a layer that has the shape to follow.",
            ),
        }
    }

    /// Make a text layer follow the first shape of a vector layer, as that
    /// shape stands on the playhead frame. The path is copied (the text does
    /// not move with the shape afterwards) and kept where it is on the canvas.
    /// One undo step. False when the source has no shape or the path is too
    /// short to follow.
    pub fn set_text_path_from(&mut self, id: LayerId, source: LayerId) -> bool {
        if self.editing_blocked() || self.text(id).is_none() {
            return false;
        }
        let frame = self.current_frame() as f32;
        let Some(LayerKind::Vector { shapes }) = self.doc.find(source).map(|l| &l.kind) else {
            return false;
        };
        let Some(mut path) = shapes.first().map(|s| s.path_at(frame).into_owned()) else {
            return false;
        };
        let into_text = self.world(id).inverse() * self.world(source);
        if !into_text.is_finite() {
            return false;
        }
        path.transform(into_text);
        self.edit_text(id, "Set text path", |look| look.path = Some(path))
            && self.text(id).is_some_and(|t| t.look().path.is_some())
    }

    /// Set the text straight again.
    pub fn clear_text_path(&mut self, id: LayerId) -> bool {
        if self.text(id).is_none_or(|t| t.look().path.is_none()) {
            return false;
        }
        self.edit_text(id, "Remove text path", |look| look.path = None)
    }

    /// Where along its path the text starts, as a percentage of the path's
    /// length, on the playhead frame by the keying rule.
    pub fn set_text_path_offset(&mut self, id: LayerId, percent: f32) -> bool {
        self.path_offset_with(id, percent, true)
    }

    /// [`set_text_path_offset`](Self::set_text_path_offset) for a widget that
    /// has already begun its edit: no undo step of its own.
    pub fn change_text_path_offset(&mut self, id: LayerId, percent: f32) -> bool {
        self.path_offset_with(id, percent, false)
    }

    fn path_offset_with(&mut self, id: LayerId, percent: f32, step: bool) -> bool {
        if self.editing_blocked() || !percent.is_finite() || self.text(id).is_none() {
            return false;
        }
        let keying = self.keying();
        let frame = self.current_frame();
        let Some(text) = self.text(id) else {
            return false;
        };
        let mut offset = text.path_offset.clone();
        offset.write(frame, percent.clamp(-10_000.0, 10_000.0), keying);
        if offset == text.path_offset {
            return false;
        }
        if step {
            self.checkpoint("Set text path offset");
        }
        if let Some(text) = self.text_mut(id) {
            text.path_offset = offset;
        }
        true
    }

    /// Stop an animator changing the outline colour: forget its channel.
    pub fn clear_animator_stroke_colour(&mut self, id: LayerId, index: usize) -> bool {
        if self.editing_blocked()
            || self
                .text(id)
                .and_then(|t| t.animators.get(index))
                .is_none_or(|a| a.stroke_color.is_none())
        {
            return false;
        }
        self.checkpoint("Clear text animator outline colour");
        if let Some(a) = self.text_mut(id).and_then(|t| t.animators.get_mut(index)) {
            a.stroke_color = None;
        }
        true
    }

    /// Turn the selected text layer into a vector layer of the same shapes,
    /// as they look at the rest pose. The words become drawing: they can be
    /// edited point by point, bound to bones and opened on a computer that does
    /// not have the font, and they stop being editable text and lose their
    /// animators.
    pub(super) fn text_to_shapes(&mut self) {
        let Some(id) = self.selected_text() else {
            self.set_status(StatusKind::Info, "Select a text layer first.");
            return;
        };
        let Some(text) = self.text(id) else {
            return;
        };
        let shapes: Vec<_> = (0..text.built().glyph_count())
            .filter_map(|n| text.built().shape(n).cloned())
            .collect();
        if shapes.is_empty() {
            self.set_status(StatusKind::Info, "This text has no letters to turn into shapes.");
            return;
        }
        self.checkpoint("Convert text to shapes");
        if let Some(layer) = self.doc.find_mut(id) {
            layer.kind = LayerKind::vector(shapes);
        }
        self.set_status(StatusKind::Success, "Turned the text into shapes");
    }
}

/// Write a value to a channel the way an edit is recorded, and say the value
/// was usable.
fn write<T: fanimate_core::anim::Animatable>(
    channel: &mut Channel<T>,
    frame: fanimate_core::Frame,
    value: T,
    keying: fanimate_core::Keying,
) -> bool {
    channel.write(frame, value, keying);
    true
}

/// The keys of one of an animator's values, if it has them (the fill only
/// when the animator changes the colour).
fn key_list(a: &mut Animator, prop: AnimatorProp) -> Option<&mut dyn fanimate_core::anim::KeyList> {
    Some(match prop {
        AnimatorProp::RangeStart => &mut a.selector.start.keys,
        AnimatorProp::RangeEnd => &mut a.selector.end.keys,
        AnimatorProp::RangeOffset => &mut a.selector.offset.keys,
        AnimatorProp::Position => &mut a.position.keys,
        AnimatorProp::Rotation => &mut a.rotation.keys,
        AnimatorProp::Scale => &mut a.scale.keys,
        AnimatorProp::Opacity => &mut a.opacity.keys,
        AnimatorProp::Fill => &mut a.fill.as_mut()?.keys,
        AnimatorProp::Anchor => &mut a.anchor.keys,
        AnimatorProp::Tracking => &mut a.tracking.keys,
        AnimatorProp::StrokeWidth => &mut a.stroke_width.keys,
        AnimatorProp::StrokeColour => &mut a.stroke_color.as_mut()?.keys,
    })
}

/// What an animator's value is on `frame`.
fn value_at(a: &Animator, prop: AnimatorProp, frame: f32) -> Option<PropValue> {
    Some(match prop {
        AnimatorProp::RangeStart => PropValue::Number(a.selector.start.sample(frame)),
        AnimatorProp::RangeEnd => PropValue::Number(a.selector.end.sample(frame)),
        AnimatorProp::RangeOffset => PropValue::Number(a.selector.offset.sample(frame)),
        AnimatorProp::Position => PropValue::Pair(a.position.sample(frame)),
        AnimatorProp::Rotation => PropValue::Number(a.rotation.sample(frame)),
        AnimatorProp::Scale => PropValue::Pair(a.scale.sample(frame)),
        AnimatorProp::Opacity => PropValue::Number(a.opacity.sample(frame)),
        AnimatorProp::Fill => PropValue::Colour(a.fill.as_ref()?.sample(frame)),
        AnimatorProp::Anchor => PropValue::Pair(a.anchor.sample(frame)),
        AnimatorProp::Tracking => PropValue::Number(a.tracking.sample(frame)),
        AnimatorProp::StrokeWidth => PropValue::Number(a.stroke_width.sample(frame)),
        AnimatorProp::StrokeColour => PropValue::Colour(a.stroke_color.as_ref()?.sample(frame)),
    })
}

/// Write one value into an animator by the keying rule; false for a value of
/// the wrong kind for the property.
fn write_value(
    a: &mut Animator,
    prop: AnimatorProp,
    value: PropValue,
    frame: fanimate_core::Frame,
    keying: fanimate_core::Keying,
    base_fill: Color,
) -> bool {
    match (prop, value) {
        (AnimatorProp::RangeStart, PropValue::Number(v)) => write(&mut a.selector.start, frame, v, keying),
        (AnimatorProp::RangeEnd, PropValue::Number(v)) => write(&mut a.selector.end, frame, v, keying),
        (AnimatorProp::RangeOffset, PropValue::Number(v)) => write(&mut a.selector.offset, frame, v, keying),
        (AnimatorProp::Rotation, PropValue::Number(v)) => write(&mut a.rotation, frame, v, keying),
        (AnimatorProp::Opacity, PropValue::Number(v)) => write(&mut a.opacity, frame, v, keying),
        (AnimatorProp::Position, PropValue::Pair(v)) => write(&mut a.position, frame, v, keying),
        (AnimatorProp::Scale, PropValue::Pair(v)) => write(&mut a.scale, frame, v, keying),
        (AnimatorProp::Fill, PropValue::Colour(v)) => {
            let channel = a.fill.get_or_insert_with(|| Channel::new(base_fill));
            write(channel, frame, v, keying)
        }
        (AnimatorProp::Anchor, PropValue::Pair(v)) => write(&mut a.anchor, frame, v, keying),
        (AnimatorProp::Tracking, PropValue::Number(v)) => write(&mut a.tracking, frame, v, keying),
        (AnimatorProp::StrokeWidth, PropValue::Number(v)) => write(&mut a.stroke_width, frame, v, keying),
        (AnimatorProp::StrokeColour, PropValue::Colour(v)) => {
            let channel = a.stroke_color.get_or_insert_with(|| Channel::new(base_fill));
            write(channel, frame, v, keying)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;
    use fanimate_core::{Document, Interpolation, TextUnit};

    fn editor() -> Editor {
        Editor::new(Document::new("t", 1000.0, 500.0, 24.0))
    }

    fn with_text() -> (Editor, LayerId) {
        let mut ed = editor();
        let id = ed.add_text_layer(Vec2::new(40.0, 60.0), "Hello text").unwrap();
        (ed, id)
    }

    #[test]
    fn new_text_layer_is_placed_named_selected_filled_and_one_undo_step() {
        let mut ed = editor();
        ed.draw.line = Color::rgba(0.2, 0.4, 0.6, 1.0);
        let id = ed.add_text_layer(Vec2::new(40.0, 60.0), "Hello").unwrap();
        let layer = ed.doc.find(id).unwrap();
        assert_eq!(layer.name, "Text 1");
        assert_eq!(layer.transform.position.base, Vec2::new(40.0, 60.0));
        let text = ed.text(id).unwrap();
        assert_eq!(text.look().content, "Hello");
        assert_eq!(text.look().fill, Color::rgba(0.2, 0.4, 0.6, 1.0));
        assert_eq!(ed.selection, vec![id]);
        assert_eq!(ed.selected_text(), Some(id));
        assert!(ed.dirty);
        ed.run(Action::Undo);
        assert!(ed.doc.layers.is_empty());
        ed.run(Action::Redo);
        assert!(ed.text(id).is_some());
        // a second one is numbered on
        let second = ed.add_text_layer(Vec2::ZERO, "x").unwrap();
        assert_eq!(ed.doc.find(second).unwrap().name, "Text 2");
        // and the menu command makes one at a default place
        ed.run(Action::NewTextLayer);
        let made = ed.selected_text().unwrap();
        assert_eq!(ed.text(made).unwrap().look().content, "Text");
        let at = ed.doc.find(made).unwrap().transform.position.base;
        assert!(at.x > 0.0 && at.x < 500.0 && at.y > 0.0 && at.y < 250.0, "{at}");
    }

    #[test]
    fn new_text_shows_on_the_white_page_with_the_default_colours() {
        // the default fill is white, so text taking it was invisible until it was recoloured
        let mut ed = editor();
        let id = ed.add_text_layer(Vec2::ZERO, "Visible").unwrap();
        assert_ne!(ed.text(id).unwrap().look().fill, Color::WHITE);
        assert_eq!(ed.text(id).unwrap().look().fill, Color::BLACK);
    }

    #[test]
    fn nothing_is_added_or_changed_while_the_animation_plays_or_for_a_place_that_is_not_one() {
        let (mut ed, id) = with_text();
        assert!(ed.add_text_layer(Vec2::new(f32::NAN, 0.0), "x").is_none());
        ed.playing = true;
        assert!(ed.add_text_layer(Vec2::ZERO, "x").is_none());
        assert!(!ed.edit_text(id, "Edit", |l| l.size = 20.0));
        assert!(ed.add_text_animator(id, TextPreset::FadeIn).is_none());
        assert!(!ed.remove_text_animator(id, 0));
        ed.run(Action::NewTextLayer);
        assert_eq!(ed.doc.layers.len(), 1);
        assert_eq!(ed.text(id).unwrap().look().size, 72.0);
    }

    #[test]
    fn editing_the_look_is_one_undo_step_is_held_within_range_and_needs_a_change() {
        let (mut ed, id) = with_text();
        assert!(ed.edit_text(id, "Text size", |l| l.size = 120.0));
        assert_eq!(ed.text(id).unwrap().look().size, 120.0);
        assert_eq!(ed.text(id).unwrap().built().count(TextUnit::Word), 2);
        // out-of-range and unusable values are held within what is allowed
        assert!(ed.edit_text(id, "Text size", |l| l.size = 1.0e9));
        assert_eq!(ed.text(id).unwrap().look().size, 4096.0);
        assert!(ed.edit_text(id, "Text size", |l| l.size = f32::NAN));
        assert_eq!(ed.text(id).unwrap().look().size, 72.0);
        // no change, no step
        let before = ed.revision();
        assert!(!ed.edit_text(id, "Text size", |l| l.size = 72.0));
        assert_eq!(ed.revision(), before);
        // a layer that is not text refuses
        let vector = ed.doc.new_layer("v", LayerKind::vector(vec![]));
        let vid = vector.id;
        ed.doc.layers.push(vector);
        assert!(!ed.edit_text(vid, "x", |l| l.size = 5.0));
        ed.run(Action::Undo);
        ed.run(Action::Undo);
        assert_eq!(ed.text(id).unwrap().look().size, 120.0);
    }

    #[test]
    fn the_layout_follows_an_edit_and_an_undo() {
        let (mut ed, id) = with_text();
        assert_eq!(ed.text(id).unwrap().built().count(TextUnit::Character), 10);
        ed.edit_text(id, "Words", |l| l.content = "One two three four".into());
        assert_eq!(ed.text(id).unwrap().built().count(TextUnit::Word), 4);
        ed.run(Action::Undo);
        assert_eq!(
            ed.text(id).unwrap().built().count(TextUnit::Word),
            2,
            "undo brings the old layout back"
        );
        ed.run(Action::Redo);
        assert_eq!(ed.text(id).unwrap().built().count(TextUnit::Word), 4);
    }

    #[test]
    fn animators_are_added_from_presets_started_at_the_playhead_and_removed() {
        let (mut ed, id) = with_text();
        ed.go_to(10);
        let i = ed.add_text_animator(id, TextPreset::Typewriter).unwrap();
        assert_eq!(i, 0);
        let a = &ed.text(id).unwrap().animators[0];
        assert_eq!(a.selector.start.keys.first().map(|k| k.frame), Some(10));
        assert_eq!(
            a.selector.start.keys.last().map(|k| k.frame),
            Some(34),
            "a second at 24 fps"
        );
        ed.run(Action::AddTextAnimator(TextPreset::Wave));
        assert_eq!(ed.text(id).unwrap().animators.len(), 2);
        assert!(ed.remove_text_animator(id, 0));
        assert_eq!(ed.text(id).unwrap().animators[0].name, TextPreset::Wave.label());
        assert!(!ed.remove_text_animator(id, 5));
        ed.run(Action::Undo);
        assert_eq!(ed.text(id).unwrap().animators.len(), 2);
        // not on a layer that is not text, and not past the limit
        assert!(ed.add_text_animator(LayerId(9999), TextPreset::FadeIn).is_none());
        for _ in 0..80 {
            ed.add_text_animator(id, TextPreset::FadeIn);
        }
        assert_eq!(
            ed.text(id).unwrap().animators.len(),
            fanimate_core::text::MAX_ANIMATORS
        );
    }

    #[test]
    fn animator_values_follow_the_keying_rule() {
        let (mut ed, id) = with_text();
        ed.edit_text(id, "x", |_| {});
        ed.add_text_animator(id, TextPreset::RiseIn);
        // at the rest pose the value itself changes
        ed.go_to(0);
        assert!(ed.set_animator_value(
            id,
            0,
            AnimatorProp::Position,
            PropValue::Pair(Vec2::new(0.0, 80.0))
        ));
        let a = &ed.text(id).unwrap().animators[0];
        assert_eq!(a.position.base, Vec2::new(0.0, 80.0));
        assert!(a.position.keys.is_empty());
        // on a later frame it makes a key, and seeds one on the rest pose
        ed.go_to(12);
        assert!(ed.set_animator_value(
            id,
            0,
            AnimatorProp::Position,
            PropValue::Pair(Vec2::new(0.0, 10.0))
        ));
        let a = &ed.text(id).unwrap().animators[0];
        let frames: Vec<i32> = a.position.keys.iter().map(|k| k.frame).collect();
        assert_eq!(frames, [0, 12]);
        assert_eq!(a.position.keys[0].value, Vec2::new(0.0, 80.0), "nothing jumps");
        assert_eq!(a.position.keys[1].value, Vec2::new(0.0, 10.0));
        // the same for the range, with the interpolation the timeline has chosen
        ed.key_interp = Interpolation::Linear;
        assert!(ed.set_animator_value(id, 0, AnimatorProp::RangeOffset, PropValue::Number(30.0)));
        let a = &ed.text(id).unwrap().animators[0];
        assert_eq!(
            a.selector.offset.keys.last().unwrap().interp,
            Interpolation::Linear
        );
        // a change that does not change anything is not a step
        let before = ed.revision();
        assert!(!ed.set_animator_value(id, 0, AnimatorProp::RangeOffset, PropValue::Number(30.0)));
        assert_eq!(ed.revision(), before);
        ed.run(Action::Undo);
        assert_eq!(ed.text(id).unwrap().animators[0].selector.offset.keys.len(), 0);
    }

    #[test]
    fn animator_values_of_the_wrong_kind_or_not_numbers_are_refused() {
        let (mut ed, id) = with_text();
        ed.add_text_animator(id, TextPreset::FadeIn);
        let before = ed.revision();
        assert!(!ed.set_animator_value(id, 0, AnimatorProp::Position, PropValue::Number(1.0)));
        assert!(!ed.set_animator_value(id, 0, AnimatorProp::Opacity, PropValue::Pair(Vec2::ONE)));
        assert!(!ed.set_animator_value(id, 0, AnimatorProp::Opacity, PropValue::Number(f32::NAN)));
        assert!(!ed.set_animator_value(
            id,
            0,
            AnimatorProp::Scale,
            PropValue::Pair(Vec2::new(f32::INFINITY, 1.0))
        ));
        assert!(!ed.set_animator_value(id, 7, AnimatorProp::Opacity, PropValue::Number(0.5)));
        assert!(!ed.set_animator_value(LayerId(9999), 0, AnimatorProp::Opacity, PropValue::Number(0.5)));
        assert_eq!(ed.revision(), before);
    }

    #[test]
    fn a_colour_change_gives_the_animator_a_fill_that_starts_from_the_texts_colour() {
        let (mut ed, id) = with_text();
        ed.edit_text(id, "Colour", |l| l.fill = Color::rgba(1.0, 0.0, 0.0, 1.0));
        ed.add_text_animator(id, TextPreset::Wave);
        assert!(ed.text(id).unwrap().animators[0].fill.is_none());
        ed.go_to(0);
        assert!(ed.set_animator_value(id, 0, AnimatorProp::Fill, PropValue::Colour(Color::WHITE)));
        let fill = ed.text(id).unwrap().animators[0].fill.clone().unwrap();
        assert_eq!(fill.base, Color::WHITE);
        ed.go_to(8);
        assert!(ed.set_animator_value(id, 0, AnimatorProp::Fill, PropValue::Colour(Color::BLACK)));
        let fill = ed.text(id).unwrap().animators[0].fill.clone().unwrap();
        assert_eq!(fill.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [0, 8]);
        assert!(ed.clear_animator_fill(id, 0));
        assert!(ed.text(id).unwrap().animators[0].fill.is_none());
        assert!(!ed.clear_animator_fill(id, 0), "nothing left to clear");
    }

    #[test]
    fn what_is_not_animated_about_an_animator_is_edited_in_one_step_with_a_usable_soft_edge() {
        let (mut ed, id) = with_text();
        ed.add_text_animator(id, TextPreset::FadeIn);
        assert!(ed.edit_text_animator(id, 0, "Animator unit", |a| a.unit = TextUnit::Word));
        assert_eq!(ed.text(id).unwrap().animators[0].unit, TextUnit::Word);
        assert!(ed.edit_text_animator(id, 0, "Soft edge", |a| a.selector.smoothness = f32::NAN));
        assert_eq!(ed.text(id).unwrap().animators[0].selector.smoothness, 0.0);
        assert!(ed.edit_text_animator(id, 0, "Soft edge", |a| a.selector.smoothness = 1.0e9));
        assert_eq!(ed.text(id).unwrap().animators[0].selector.smoothness, 1000.0);
        assert!(!ed.edit_text_animator(id, 0, "Soft edge", |a| a.selector.smoothness = 1000.0));
        assert!(!ed.edit_text_animator(id, 3, "x", |a| a.name = "y".into()));
    }

    #[test]
    fn converting_text_to_shapes_keeps_the_letters_and_drops_the_editing() {
        let (mut ed, id) = with_text();
        ed.add_text_animator(id, TextPreset::Wave);
        let glyphs = ed.text(id).unwrap().built().glyph_count();
        ed.run(Action::ConvertTextToShapes);
        let layer = ed.doc.find(id).unwrap();
        let LayerKind::Vector { shapes } = &layer.kind else {
            panic!("not a vector layer: {}", layer.kind.type_name());
        };
        assert_eq!(shapes.len(), glyphs);
        assert!(ed.text(id).is_none());
        ed.run(Action::Undo);
        assert_eq!(
            ed.text(id).unwrap().animators.len(),
            1,
            "undo brings the text and its animators back"
        );
        // with nothing selected, or text with no letters, it says so and changes nothing
        ed.selection.clear();
        let before = ed.revision();
        ed.run(Action::ConvertTextToShapes);
        assert_eq!(ed.revision(), before);
        assert!(
            ed.status
                .as_ref()
                .is_some_and(|s| s.1.contains("Select a text layer"))
        );
        ed.edit_text(id, "Empty", |l| l.content = "   ".into());
        ed.selection = vec![id];
        ed.run(Action::ConvertTextToShapes);
        assert!(ed.text(id).is_some(), "spaces have no letters to convert");
    }

    #[test]
    fn the_diamond_toggles_a_key_on_the_playhead_frame() {
        let (mut ed, id) = with_text();
        ed.add_text_animator(id, TextPreset::RiseIn);
        // the preset's range start is keyed on its first and last frames
        ed.go_to(1);
        assert_eq!(
            ed.animator_key_state(id, 0, AnimatorProp::RangeStart),
            (true, true)
        );
        ed.go_to(5);
        assert_eq!(
            ed.animator_key_state(id, 0, AnimatorProp::RangeStart),
            (true, false)
        );
        // opacity has no keys yet
        assert_eq!(
            ed.animator_key_state(id, 0, AnimatorProp::Opacity),
            (false, false)
        );

        // add a key holding the value the range has there
        let held = ed.text(id).unwrap().animators[0].selector.start.sample(5.0);
        assert!(ed.toggle_animator_key(id, 0, AnimatorProp::RangeStart));
        assert_eq!(
            ed.animator_key_state(id, 0, AnimatorProp::RangeStart),
            (true, true)
        );
        let a = &ed.text(id).unwrap().animators[0];
        assert!((a.selector.start.keys.iter().find(|k| k.frame == 5).unwrap().value - held).abs() < 1e-4);
        assert!(
            (a.selector.start.sample(5.0) - held).abs() < 1e-4,
            "adding the key moves nothing"
        );
        // and take it out again
        assert!(ed.toggle_animator_key(id, 0, AnimatorProp::RangeStart));
        assert_eq!(
            ed.animator_key_state(id, 0, AnimatorProp::RangeStart),
            (true, false)
        );
        ed.run(Action::Undo);
        assert_eq!(
            ed.animator_key_state(id, 0, AnimatorProp::RangeStart),
            (true, true)
        );

        // starting to animate a property that has no keys: a key here, one on the rest pose
        ed.go_to(8);
        assert!(ed.toggle_animator_key(id, 0, AnimatorProp::Opacity));
        let frames: Vec<i32> = ed.text(id).unwrap().animators[0]
            .opacity
            .keys
            .iter()
            .map(|k| k.frame)
            .collect();
        assert_eq!(frames, [0, 8]);
        // a fill the animator does not have cannot be keyed
        assert!(!ed.toggle_animator_key(id, 0, AnimatorProp::Fill));
        assert!(!ed.toggle_animator_key(id, 5, AnimatorProp::Opacity));
        assert_eq!(
            ed.animator_key_state(id, 5, AnimatorProp::Opacity),
            (false, false)
        );
    }
    #[test]
    fn a_drag_is_one_undo_step_however_many_frames_it_changes_the_value() {
        let (mut ed, id) = with_text();
        ed.add_text_animator(id, TextPreset::RiseIn);
        ed.go_to(0);
        let (size_before, opacity_before) = (
            ed.text(id).unwrap().look().size,
            ed.text(id).unwrap().animators[0].opacity.base,
        );
        let steps_before = ed.revision();
        // the widget begins the edit once, then changes the value on every frame of the drag
        ed.checkpoint("Drag");
        for n in 1..=30 {
            assert!(ed.change_text(id, |l| l.size = 72.0 + n as f32));
            assert!(ed.change_animator_value(
                id,
                0,
                AnimatorProp::Opacity,
                PropValue::Number(n as f32 / 100.0)
            ));
            assert!(ed.change_text_animator(id, 0, |a| a.selector.smoothness = n as f32));
        }
        assert_eq!(ed.text(id).unwrap().look().size, 102.0);
        assert_eq!(
            ed.revision(),
            steps_before + 1,
            "one checkpoint for the whole drag"
        );
        ed.run(Action::Undo);
        assert_eq!(ed.text(id).unwrap().look().size, size_before);
        assert_eq!(ed.text(id).unwrap().animators[0].opacity.base, opacity_before);
        assert_eq!(
            ed.text(id).unwrap().animators[0].selector.smoothness,
            50.0,
            "the preset's own"
        );
        // and they still refuse what is not allowed
        assert!(!ed.change_text(LayerId(9999), |l| l.size = 5.0));
        assert!(!ed.change_animator_value(id, 0, AnimatorProp::Opacity, PropValue::Number(f32::NAN)));
        ed.playing = true;
        assert!(!ed.change_text(id, |l| l.size = 5.0));
    }

    fn with_curve() -> (Editor, LayerId, LayerId) {
        let (mut ed, text) = with_text();
        let curve = fanimate_core::Path::new()
            .move_to(Vec2::new(100.0, 300.0))
            .line_to(Vec2::new(900.0, 300.0));
        let shape = fanimate_core::Shape::new("curve", curve, fanimate_core::Style::fill(Color::BLACK));
        let mut layer = ed.doc.new_layer("Curve", LayerKind::vector(vec![shape]));
        layer.transform.position.base = Vec2::new(10.0, 20.0);
        let source = layer.id;
        ed.doc.layers.push(layer);
        (ed, text, source)
    }

    #[test]
    fn a_text_follows_the_shape_of_another_layer_wherever_it_stands_on_the_canvas() {
        let (mut ed, text, source) = with_curve();
        assert!(ed.set_text_path_from(text, source));
        let path = ed.text(text).unwrap().look().path.clone().unwrap();
        // the text layer is at (40, 60) and the curve layer at (10, 20): the path is
        // taken into the text layer's own space so it stays where it was drawn
        assert_eq!(
            path.points().next(),
            Some(Vec2::new(100.0 + 10.0 - 40.0, 300.0 + 20.0 - 60.0))
        );
        ed.run(Action::Undo);
        assert!(ed.text(text).unwrap().look().path.is_none());
        ed.run(Action::Redo);
        assert!(ed.text(text).unwrap().look().path.is_some());
        // the menu command needs both layers selected
        ed.selection = vec![text];
        assert!(!ed.can_run(Action::SetTextPath));
        ed.selection = vec![source, text];
        assert!(ed.can_run(Action::SetTextPath));
        ed.run(Action::ClearTextPath);
        assert!(ed.text(text).unwrap().look().path.is_none());
        assert!(!ed.can_run(Action::ClearTextPath));
        ed.run(Action::SetTextPath);
        assert!(ed.text(text).unwrap().look().path.is_some());
    }

    #[test]
    fn a_path_that_cannot_be_taken_is_refused() {
        let (mut ed, text, source) = with_curve();
        let empty = ed.doc.new_layer("Empty", LayerKind::vector(vec![]));
        let empty_id = empty.id;
        ed.doc.layers.push(empty);
        assert!(!ed.set_text_path_from(text, empty_id), "no shape");
        assert!(!ed.set_text_path_from(source, text), "not a text layer");
        assert!(!ed.set_text_path_from(text, text), "text is not a shape");
        assert!(!ed.clear_text_path(text), "nothing to remove");
        ed.playing = true;
        assert!(!ed.set_text_path_from(text, source));
    }

    #[test]
    fn the_start_along_the_path_follows_the_keying_rule() {
        let (mut ed, text, source) = with_curve();
        ed.set_text_path_from(text, source);
        ed.go_to(0);
        assert!(ed.set_text_path_offset(text, 10.0));
        assert_eq!(ed.text(text).unwrap().path_offset.base, 10.0);
        assert!(ed.text(text).unwrap().path_offset.keys.is_empty());
        ed.go_to(12);
        assert!(ed.set_text_path_offset(text, 60.0));
        let offset = &ed.text(text).unwrap().path_offset;
        assert_eq!(offset.keys.len(), 2, "a key here and one seeded on the rest pose");
        assert_eq!(offset.sample(0.0), 10.0);
        assert_eq!(offset.sample(12.0), 60.0);
        // the same value again, and values that are not numbers, change nothing
        assert!(!ed.set_text_path_offset(text, 60.0));
        assert!(!ed.set_text_path_offset(text, f32::NAN));
        assert!(!ed.set_text_path_offset(LayerId(9999), 5.0));
        ed.run(Action::Undo);
        assert_eq!(ed.text(text).unwrap().path_offset.keys.len(), 0);
    }
    #[test]
    fn letter_spacing_outline_and_anchor_values_follow_the_keying_rule_and_clear() {
        let (mut ed, id) = with_text();
        ed.add_text_animator(id, TextPreset::FadeIn);
        ed.go_to(0);
        assert!(ed.set_animator_value(id, 0, AnimatorProp::Tracking, PropValue::Number(120.0)));
        assert_eq!(ed.text(id).unwrap().animators[0].tracking.base, 120.0);
        assert!(ed.set_animator_value(id, 0, AnimatorProp::StrokeWidth, PropValue::Number(3.0)));
        assert!(ed.set_animator_value(id, 0, AnimatorProp::Anchor, PropValue::Pair(Vec2::new(5.0, -5.0))));
        // later frames make keys, and seed the rest pose
        ed.go_to(10);
        assert!(ed.set_animator_value(id, 0, AnimatorProp::Tracking, PropValue::Number(400.0)));
        let a = &ed.text(id).unwrap().animators[0];
        assert_eq!(a.tracking.keys.len(), 2);
        assert_eq!((a.tracking.sample(0.0), a.tracking.sample(10.0)), (120.0, 400.0));
        // the outline colour does not exist until it is set, starts from the given colour, and clears
        assert!(ed.text(id).unwrap().animators[0].stroke_color.is_none());
        assert_eq!(
            ed.animator_key_state(id, 0, AnimatorProp::StrokeColour),
            (false, false)
        );
        ed.go_to(0);
        assert!(ed.set_animator_value(
            id,
            0,
            AnimatorProp::StrokeColour,
            PropValue::Colour(Color::rgb(1.0, 0.0, 0.0))
        ));
        assert_eq!(
            ed.text(id).unwrap().animators[0]
                .stroke_color
                .as_ref()
                .unwrap()
                .base,
            Color::rgb(1.0, 0.0, 0.0)
        );
        assert!(ed.clear_animator_stroke_colour(id, 0));
        assert!(!ed.clear_animator_stroke_colour(id, 0), "nothing left to clear");
        // a value of the wrong kind, or not a number, is refused
        assert!(!ed.set_animator_value(id, 0, AnimatorProp::Tracking, PropValue::Pair(Vec2::ONE)));
        assert!(!ed.set_animator_value(id, 0, AnimatorProp::StrokeWidth, PropValue::Number(f32::NAN)));
        assert!(!ed.set_animator_value(id, 0, AnimatorProp::Anchor, PropValue::Number(1.0)));
        // the diamond adds and removes a key like the others
        ed.go_to(20);
        assert!(ed.toggle_animator_key(id, 0, AnimatorProp::StrokeWidth));
        assert_eq!(
            ed.animator_key_state(id, 0, AnimatorProp::StrokeWidth),
            (true, true)
        );
        assert!(ed.toggle_animator_key(id, 0, AnimatorProp::StrokeWidth));
        assert!(AnimatorProp::ALL.iter().all(|p| !p.label().is_empty()));
    }
    #[test]
    fn the_text_tool_finds_the_topmost_shown_text_under_a_point() {
        let (mut ed, below) = with_text(); // at (40, 60), "Hello text" at 72
        let above = ed.add_text_layer(Vec2::new(40.0, 60.0), "Top").unwrap();
        let size = ed.text(below).unwrap().box_size();
        assert!(size.x > 100.0 && size.y > 50.0, "{size}");
        let inside = Vec2::new(60.0, 80.0);
        assert_eq!(
            ed.text_at(inside, 0.0),
            Some(above),
            "the one drawn last is on top"
        );
        // outside the box is a miss, and a margin brings a near miss in
        let beside = Vec2::new(40.0 + size.x + 5.0, 80.0);
        assert_eq!(ed.text_at(beside, 0.0), None);
        assert_eq!(ed.text_at(beside, 10.0), Some(below));
        assert_eq!(ed.text_at(Vec2::new(-500.0, -500.0), 6.0), None);
        // a hidden layer is not hit, and the layer underneath is
        ed.set_visible(above, false);
        assert_eq!(ed.text_at(inside, 0.0), Some(below));
        // a layer moved elsewhere is hit where it now is
        ed.set_visible(above, true);
        ed.set_layer_transform(
            above,
            fanimate_core::Transform {
                position: Vec2::new(400.0, 300.0),
                ..fanimate_core::Transform::IDENTITY
            },
        );
        assert_eq!(ed.text_at(Vec2::new(410.0, 320.0), 0.0), Some(above));
        assert_eq!(ed.text_at(inside, 0.0), Some(below));
        // empty text still has a box to click, and text that is not a number never panics
        let empty = ed.add_text_layer(Vec2::new(700.0, 100.0), "").unwrap();
        assert_eq!(ed.text_at(Vec2::new(710.0, 110.0), 0.0), Some(empty));
        assert_eq!(ed.text_at(Vec2::new(f32::NAN, 0.0), 6.0), None);
        // other kinds of layer are never found
        let vector = ed.doc.new_layer("v", LayerKind::vector(vec![]));
        ed.doc.layers.push(vector);
        assert_eq!(ed.text_at(Vec2::new(900.0, 400.0), 6.0), None);
    }
}
