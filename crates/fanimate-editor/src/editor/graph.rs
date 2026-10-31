//! The graph editor's commands: a key of a curve moved along the frames and
//! up or down, and a key's curve reshaped. The graph shows one number at a
//! time (the x of a position, an angle), so these work on one curve of a
//! property; moving a key sideways moves every curve of the property, because
//! they share their keys.
//!
//! Dragging is a gesture: the panel calls `Editor::checkpoint` once when the
//! drag starts and then these `change_*` commands for every pointer move, so
//! the whole drag is one undo step.

use fanimate_core::graph::{Component, CurveKey, component_count};
use fanimate_core::{ChannelId, Frame, Interpolation, LayerId};

use super::Editor;

impl Editor {
    /// The properties of `layer` the graph can draw, with how many curves each
    /// has.
    pub fn graph_channels(&self, layer: LayerId) -> Vec<(ChannelId, usize)> {
        self.doc
            .find(layer)
            .map(|l| l.graph_channels())
            .unwrap_or_default()
    }

    /// The keys of one curve.
    pub fn curve_keys(&self, layer: LayerId, ch: ChannelId, component: Component) -> Vec<CurveKey> {
        self.doc
            .find(layer)
            .map(|l| l.curve_keys(ch, component))
            .unwrap_or_default()
    }

    /// The value of one curve on `frame`, which may fall between frames.
    pub fn curve_value(
        &self,
        layer: LayerId,
        ch: ChannelId,
        component: Component,
        frame: f32,
    ) -> Option<f32> {
        self.doc.find(layer)?.curve_value(ch, component, frame)
    }

    /// Move the key on `frame` of property `ch` to `to` and set `component`'s
    /// value there, as one drag step. The key keeps to the room between its
    /// neighbours (it never lands on or crosses one); the frame it ended on is
    /// returned. No undo step of its own: call [`Editor::checkpoint`] first.
    pub fn change_key(
        &mut self,
        layer: LayerId,
        ch: ChannelId,
        component: Component,
        frame: Frame,
        to: Frame,
        value: f32,
    ) -> Result<Frame, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to edit keys.".to_owned());
        }
        if component >= component_count(ch) {
            return Err("That property has no such curve.".to_owned());
        }
        if !value.is_finite() {
            return Err("A key's value must be a number.".to_owned());
        }
        let Some(l) = self.doc.find(layer) else {
            return Err("There is no such layer.".to_owned());
        };
        let Some((lo, hi)) = l.key_room(ch, frame) else {
            return Err("There is no key there.".to_owned());
        };
        let to = to.clamp(lo, hi);
        let Some(l) = self.doc.find_mut(layer) else {
            return Err("There is no such layer.".to_owned());
        };
        if to != frame
            && !l
                .channel_keys_mut(ch)
                .is_some_and(|keys| keys.move_key(frame, to))
        {
            return Err("There is no key there.".to_owned());
        }
        l.set_curve_value(ch, component, to, value);
        // the picked key goes with the one that moved
        if to != frame {
            let from = super::keys::KeyRef::new(Some(layer), Some(ch), frame);
            if self.selected_keys.remove(&from) {
                self.selected_keys
                    .insert(super::keys::KeyRef::new(Some(layer), Some(ch), to));
            }
        }
        Ok(to)
    }

    /// Reshape the curve leaving the key on `frame` of property `ch`. No undo
    /// step of its own (see [`Editor::change_key`]). Returns whether it changed.
    pub fn change_key_curve(
        &mut self,
        layer: LayerId,
        ch: ChannelId,
        frame: Frame,
        interp: Interpolation,
    ) -> Result<bool, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to edit keys.".to_owned());
        }
        let Some(keys) = self.doc.find_mut(layer).and_then(|l| l.channel_keys_mut(ch)) else {
            return Err("There is no such property.".to_owned());
        };
        if keys.interp_at(frame).is_none() {
            return Err("There is no key there.".to_owned());
        }
        Ok(keys.set_interp(frame, interp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::easing::Bezier;
    use fanimate_core::{Channel, Document, LayerKind, Vec2};

    fn rig() -> (Editor, LayerId) {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut l = doc.new_layer("g", LayerKind::Group { children: vec![] });
        l.transform.position = Channel::new(Vec2::ZERO)
            .with_key(1, Vec2::new(0.0, 5.0), Interpolation::Linear)
            .with_key(11, Vec2::new(100.0, 25.0), Interpolation::Linear)
            .with_key(21, Vec2::new(0.0, 5.0), Interpolation::Linear);
        let id = l.id;
        doc.layers.push(l);
        (Editor::new(doc), id)
    }

    #[test]
    fn dragging_a_key_moves_it_and_changes_one_curve_and_is_one_undo_step() {
        let (mut ed, id) = rig();
        let before = ed.revision();
        ed.checkpoint("Move key");
        // x of the middle key to 60, and the key to frame 14
        assert_eq!(ed.change_key(id, ChannelId::Position, 0, 11, 14, 60.0), Ok(14));
        // then on, as the pointer moves: from where the key now is
        assert_eq!(ed.change_key(id, ChannelId::Position, 0, 14, 15, 70.0), Ok(15));
        assert_eq!(ed.revision(), before + 1);
        let x = ed.curve_keys(id, ChannelId::Position, 0);
        let y = ed.curve_keys(id, ChannelId::Position, 1);
        assert_eq!((x[1].frame, x[1].value), (15, 70.0));
        assert_eq!(
            (y[1].frame, y[1].value),
            (15, 25.0),
            "y kept its value and went along in time"
        );
        assert_eq!(x.len(), 3);
        ed.run(crate::Action::Undo);
        assert_eq!(
            ed.curve_keys(id, ChannelId::Position, 0)[1].frame,
            11,
            "one step undoes the whole drag"
        );
        assert_eq!(ed.curve_keys(id, ChannelId::Position, 0)[1].value, 100.0);
    }

    #[test]
    fn a_key_cannot_land_on_or_cross_its_neighbours() {
        let (mut ed, id) = rig();
        assert_eq!(ed.change_key(id, ChannelId::Position, 0, 11, 30, 1.0), Ok(20));
        assert_eq!(ed.change_key(id, ChannelId::Position, 0, 20, -5, 1.0), Ok(2));
        assert_eq!(
            ed.curve_keys(id, ChannelId::Position, 0).len(),
            3,
            "no key was lost"
        );
        // the first key may go before frame 1, and not past the second
        assert_eq!(ed.change_key(id, ChannelId::Position, 0, 1, -40, 0.0), Ok(-40));
        assert_eq!(ed.change_key(id, ChannelId::Position, 0, -40, 99, 0.0), Ok(1));
    }

    #[test]
    fn refused_edits_change_nothing() {
        let (mut ed, id) = rig();
        let before = ed.doc.clone();
        assert!(
            ed.change_key(id, ChannelId::Position, 0, 12, 13, 1.0).is_err(),
            "no key on 12"
        );
        assert!(
            ed.change_key(id, ChannelId::Position, 2, 11, 11, 1.0).is_err(),
            "no third curve"
        );
        assert!(
            ed.change_key(id, ChannelId::Position, 0, 11, 11, f32::NAN)
                .is_err()
        );
        assert!(
            ed.change_key(LayerId(999), ChannelId::Position, 0, 11, 11, 1.0)
                .is_err()
        );
        assert!(
            ed.change_key(id, ChannelId::FillColor(0), 0, 11, 11, 1.0)
                .is_err()
        );
        assert_eq!(ed.doc, before);
        ed.playing = true;
        assert!(ed.change_key(id, ChannelId::Position, 0, 11, 12, 1.0).is_err());
        assert!(
            ed.change_key_curve(id, ChannelId::Position, 11, Interpolation::Step)
                .is_err()
        );
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn a_keys_curve_is_reshaped_and_a_picked_key_follows_a_move() {
        let (mut ed, id) = rig();
        let curve = Interpolation::Bezier(Bezier([250, 0, 750, 1000]));
        assert_eq!(ed.change_key_curve(id, ChannelId::Position, 11, curve), Ok(true));
        assert_eq!(ed.curve_keys(id, ChannelId::Position, 0)[1].interp, curve);
        assert_eq!(
            ed.change_key_curve(id, ChannelId::Position, 11, curve),
            Ok(false),
            "same curve"
        );
        assert!(ed.change_key_curve(id, ChannelId::Position, 12, curve).is_err());
        assert!(ed.change_key_curve(id, ChannelId::Shape(3), 11, curve).is_err());
        let key = crate::KeyRef::new(Some(id), Some(ChannelId::Position), 11);
        ed.select_key(key, crate::KeySelect::Replace);
        ed.change_key(id, ChannelId::Position, 0, 11, 13, 100.0).unwrap();
        assert!(ed.is_key_selected(&crate::KeyRef::new(Some(id), Some(ChannelId::Position), 13)));
        assert!(!ed.is_key_selected(&key));
    }
}
