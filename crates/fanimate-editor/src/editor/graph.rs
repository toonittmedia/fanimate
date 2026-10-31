//! The graph editor's commands: a key of a curve moved along the frames and
//! up or down, and a key's curve reshaped. The graph shows one number at a
//! time (the x of a position, an angle), so these work on one curve of a
//! property; moving a key sideways moves every curve of the property, because
//! they share their keys.
//!
//! Dragging is a gesture: the panel calls `Editor::checkpoint` once when the
//! drag starts and then these `change_*` commands for every pointer move, so
//! the whole drag is one undo step.

use fanimate_core::expr::{Context, Expr};
use fanimate_core::graph::{Component, CurveKey, component_count, display_scale};
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

/// The most frames one bake writes.
pub const MAX_BAKE_FRAMES: i32 = 20_000;

impl Editor {
    /// Work out `expression` (see [`fanimate_core::expr`]) on every frame from
    /// `first` to `last` and write the answers as the keys of curve `component`
    /// of property `ch` of `layer`, replacing the keys in that range. `value`
    /// in the expression is what the curve has on that frame before the bake
    /// (in the units the graph shows: degrees, percent), and the answer is in
    /// the same units. Frames outside the range keep their keys, or hold the
    /// nearest baked value if there were none. One undo step; if anything is
    /// wrong (a bad expression, a frame whose answer is not a number, a range
    /// of more than [`MAX_BAKE_FRAMES`] frames, a property the layer has not
    /// got) nothing changes. Returns how many keys were written.
    pub fn bake_expression(
        &mut self,
        layer: LayerId,
        ch: ChannelId,
        component: Component,
        expression: &str,
        first: Frame,
        last: Frame,
    ) -> Result<usize, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to bake.".to_owned());
        }
        if last < first || i64::from(last) - i64::from(first) >= i64::from(MAX_BAKE_FRAMES) {
            return Err(format!(
                "Bake between 1 and {MAX_BAKE_FRAMES} frames: the last frame must not come before the first."
            ));
        }
        let program = Expr::parse(expression).map_err(|e| e.to_string())?;
        let Some(target) = self.doc.find(layer) else {
            return Err("There is no such layer.".to_owned());
        };
        if component >= component_count(ch) || target.curve_value(ch, component, first as f32).is_none() {
            return Err("That layer does not have that property.".to_owned());
        }
        let scale = f64::from(display_scale(ch));
        let (start, end, fps) = (self.doc.start, self.doc.end, f64::from(self.doc.fps));
        let span = f64::from((last - first).max(1));
        let mut values = Vec::with_capacity((last - first) as usize + 1);
        for frame in first..=last {
            let now = target.curve_value(ch, component, frame as f32).unwrap_or(0.0);
            let cx = Context {
                frame: f64::from(frame),
                time: f64::from(frame - start) / fps.max(1.0),
                fps,
                start: f64::from(start),
                end: f64::from(end),
                first: f64::from(first),
                last: f64::from(last),
                progress: f64::from(frame - first) / span,
                value: f64::from(now) * scale,
            };
            let answer = program.eval(&cx).map_err(|e| format!("Frame {frame}: {e}."))?;
            let stored = answer / scale;
            if !stored.is_finite() || stored.abs() > 1.0e9 {
                return Err(format!("Frame {frame}: the answer is too large."));
            }
            values.push(stored as f32);
        }
        self.checkpoint("Bake expression");
        let written = self
            .doc
            .find_mut(layer)
            .is_some_and(|l| l.write_curve_range(ch, component, first, &values));
        if !written {
            return Err("That layer does not have that property.".to_owned());
        }
        Ok(values.len())
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

    fn degrees(ed: &Editor, id: LayerId) -> Vec<(Frame, f32)> {
        ed.curve_keys(id, ChannelId::Rotation, 0)
            .iter()
            .map(|k| {
                (
                    k.frame,
                    (k.value * 180.0 / std::f32::consts::PI * 100.0).round() / 100.0,
                )
            })
            .collect()
    }

    #[test]
    fn baking_an_expression_writes_a_key_a_frame_in_the_units_the_graph_shows() {
        let (mut ed, id) = rig();
        // rotation was never keyed: `value` is 0 degrees, the answer is degrees
        let before = ed.revision();
        assert_eq!(
            ed.bake_expression(id, ChannelId::Rotation, 0, "value + frame * 2", 1, 5),
            Ok(5)
        );
        assert_eq!(
            degrees(&ed, id),
            [(1, 2.0), (2, 4.0), (3, 6.0), (4, 8.0), (5, 10.0)]
        );
        assert_eq!(ed.revision(), before + 1, "one undo step");
        // baked again over part of the range, the keys there are replaced and the rest stay
        assert_eq!(
            ed.bake_expression(id, ChannelId::Rotation, 0, "100 + progress", 3, 4),
            Ok(2)
        );
        assert_eq!(
            degrees(&ed, id),
            [(1, 2.0), (2, 4.0), (3, 100.0), (4, 101.0), (5, 10.0)]
        );
        ed.run(crate::Action::Undo);
        assert_eq!(degrees(&ed, id).len(), 5);
        assert_eq!(degrees(&ed, id)[2], (3, 6.0));
        ed.run(crate::Action::Undo);
        assert!(degrees(&ed, id).is_empty(), "back to unkeyed");
    }

    #[test]
    fn baking_one_number_of_a_pair_keeps_the_other_where_it_was() {
        let (mut ed, id) = rig();
        let y_before = ed.curve_value(id, ChannelId::Position, 1, 6.0).unwrap();
        // x from 4 to 8 follows its own curve plus 1: `value` is the old curve
        let x_before = ed.curve_value(id, ChannelId::Position, 0, 6.0).unwrap();
        ed.bake_expression(id, ChannelId::Position, 0, "value + 1", 4, 8)
            .unwrap();
        let x = ed.curve_keys(id, ChannelId::Position, 0);
        assert!(
            x.iter()
                .any(|k| k.frame == 6 && (k.value - (x_before + 1.0)).abs() < 1e-4)
        );
        // the original keys outside 4..=8 are still there
        assert!(
            x.iter().any(|k| k.frame == 1)
                && x.iter().any(|k| k.frame == 11)
                && x.iter().any(|k| k.frame == 21)
        );
        // y still passes through the same value on frame 6
        assert!((ed.curve_value(id, ChannelId::Position, 1, 6.0).unwrap() - y_before).abs() < 1e-3);
    }

    #[test]
    fn a_bake_that_cannot_finish_changes_nothing_and_says_why() {
        let (mut ed, id) = rig();
        let before = (ed.revision(), ed.curve_keys(id, ChannelId::Rotation, 0).len());
        let bad =
            |ed: &mut Editor, ch, c, src: &str, a, b| ed.bake_expression(id, ch, c, src, a, b).unwrap_err();
        assert!(bad(&mut ed, ChannelId::Rotation, 0, "frame +", 1, 5).contains("ends too soon"));
        // a frame in the middle that divides by zero names itself
        let e = bad(&mut ed, ChannelId::Rotation, 0, "10 / (frame - 3)", 1, 5);
        assert!(e.starts_with("Frame 3:") && e.contains("dividing by zero"), "{e}");
        assert!(bad(&mut ed, ChannelId::Rotation, 0, "1", 5, 1).contains("must not come before"));
        assert!(bad(&mut ed, ChannelId::Rotation, 0, "1", 1, 1 + MAX_BAKE_FRAMES).contains("frames"));
        assert!(bad(&mut ed, ChannelId::Rotation, 1, "1", 1, 5).contains("does not have"));
        assert!(bad(&mut ed, ChannelId::Bone(0), 0, "1", 1, 5).contains("does not have"));
        assert!(bad(&mut ed, ChannelId::Rotation, 0, "1000000000 * 1000000000", 1, 5).contains("too large"));
        assert!(
            ed.bake_expression(LayerId(999), ChannelId::Rotation, 0, "1", 1, 5)
                .unwrap_err()
                .contains("no such layer")
        );
        ed.playing = true;
        assert!(bad(&mut ed, ChannelId::Rotation, 0, "1", 1, 5).contains("Pause"));
        ed.playing = false;
        assert_eq!(
            (ed.revision(), ed.curve_keys(id, ChannelId::Rotation, 0).len()),
            before,
            "nothing was written and no undo step was made"
        );
        // the largest allowed range is fine
        assert_eq!(
            ed.bake_expression(id, ChannelId::Opacity, 0, "50", 1, MAX_BAKE_FRAMES),
            Ok(MAX_BAKE_FRAMES as usize)
        );
        // opacity is in percent: 50 is half
        assert!((ed.curve_value(id, ChannelId::Opacity, 0, 7.0).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_random_blink_bakes_the_same_keys_every_time() {
        let (mut ed, id) = rig();
        let src = "blink(frame, 4, 24, 3) * 100";
        ed.bake_expression(id, ChannelId::Opacity, 0, src, 1, 96).unwrap();
        let first = ed.curve_keys(id, ChannelId::Opacity, 0);
        assert_eq!(
            first.iter().filter(|k| k.value > 0.5).count(),
            12,
            "3 frames in each of 4 periods"
        );
        let (mut other, oid) = rig();
        other
            .bake_expression(oid, ChannelId::Opacity, 0, src, 1, 96)
            .unwrap();
        assert_eq!(first, other.curve_keys(oid, ChannelId::Opacity, 0));
    }
}
