//! A numeric view of a layer's keyed properties, for the graph editor: each
//! number a property holds is a curve of value against frame. A position has
//! two curves (x and y), a rotation one, a scale two, an opacity one and a
//! bone's angle one. Colours, points and switch choices are not numbers on a
//! curve, so they are not here.
//!
//! The values are in the file's units: radians for angles. The graph editor
//! shows degrees ([`Curve::display_scale`]).

use crate::anim::{Channel, Frame, Interpolation};
use crate::scene::{ChannelId, Layer, LayerKind};
use glam::Vec2;

/// One key of a curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveKey {
    pub frame: Frame,
    pub value: f32,
    /// How the curve moves from this key to the next.
    pub interp: Interpolation,
}

/// Which number of a property: its only one, or x or y of a pair.
pub type Component = usize;

/// How many curves a property has: 2 for a pair, 1 for a number, 0 when it
/// is not a number the graph can show.
pub fn component_count(ch: ChannelId) -> usize {
    match ch {
        ChannelId::Position | ChannelId::Scale => 2,
        ChannelId::Rotation | ChannelId::Opacity | ChannelId::Order | ChannelId::Bone(_) => 1,
        _ => 0,
    }
}

/// The factor that turns a curve's values into the numbers shown (radians
/// into degrees, a fraction into a percentage; 1 for the rest).
pub fn display_scale(ch: ChannelId) -> f32 {
    match ch {
        ChannelId::Rotation | ChannelId::Bone(_) => 180.0 / std::f32::consts::PI,
        ChannelId::Opacity => 100.0,
        _ => 1.0,
    }
}

fn pick(v: Vec2, c: Component) -> f32 {
    if c == 0 { v.x } else { v.y }
}

fn put(v: Vec2, c: Component, n: f32) -> Vec2 {
    if c == 0 {
        Vec2::new(n, v.y)
    } else {
        Vec2::new(v.x, n)
    }
}

enum Chan<'a> {
    Number(&'a Channel<f32>),
    Pair(&'a Channel<Vec2>),
}

enum ChanMut<'a> {
    Number(&'a mut Channel<f32>),
    Pair(&'a mut Channel<Vec2>),
}

impl Layer {
    /// The properties of this layer the graph editor can draw, with how many
    /// curves each has: the transform and opacity, the layer's place among its
    /// siblings once it is keyed, and a bone's angle once it has keys.
    pub fn graph_channels(&self) -> Vec<(ChannelId, usize)> {
        let mut v = vec![
            (ChannelId::Position, 2),
            (ChannelId::Rotation, 1),
            (ChannelId::Scale, 2),
            (ChannelId::Opacity, 1),
        ];
        if self.order.is_animated() {
            v.push((ChannelId::Order, 1));
        }
        if let LayerKind::Bone { skeleton, .. } = &self.kind {
            for (i, b) in skeleton.bones.iter().enumerate() {
                if b.angle.is_animated() {
                    v.push((ChannelId::Bone(i), 1));
                }
            }
        }
        v
    }

    fn chan(&self, ch: ChannelId) -> Option<Chan<'_>> {
        Some(match (ch, &self.kind) {
            (ChannelId::Position, _) => Chan::Pair(&self.transform.position),
            (ChannelId::Scale, _) => Chan::Pair(&self.transform.scale),
            (ChannelId::Rotation, _) => Chan::Number(&self.transform.rotation),
            (ChannelId::Opacity, _) => Chan::Number(&self.opacity),
            (ChannelId::Order, _) => Chan::Number(&self.order),
            (ChannelId::Bone(i), LayerKind::Bone { skeleton, .. }) => {
                Chan::Number(&skeleton.bones.get(i)?.angle)
            }
            _ => return None,
        })
    }

    fn chan_mut(&mut self, ch: ChannelId) -> Option<ChanMut<'_>> {
        Some(match (ch, &mut self.kind) {
            (ChannelId::Position, _) => ChanMut::Pair(&mut self.transform.position),
            (ChannelId::Scale, _) => ChanMut::Pair(&mut self.transform.scale),
            (ChannelId::Rotation, _) => ChanMut::Number(&mut self.transform.rotation),
            (ChannelId::Opacity, _) => ChanMut::Number(&mut self.opacity),
            (ChannelId::Order, _) => ChanMut::Number(&mut self.order),
            (ChannelId::Bone(i), LayerKind::Bone { skeleton, .. }) => {
                ChanMut::Number(&mut skeleton.bones.get_mut(i)?.angle)
            }
            _ => return None,
        })
    }

    /// The keys of curve `component` of property `ch`, in frame order. Empty
    /// when the property has no keys or is not one the graph shows.
    pub fn curve_keys(&self, ch: ChannelId, component: Component) -> Vec<CurveKey> {
        match self.chan(ch) {
            Some(Chan::Number(c)) if component == 0 => c
                .keys
                .iter()
                .map(|k| CurveKey {
                    frame: k.frame,
                    value: k.value,
                    interp: k.interp,
                })
                .collect(),
            Some(Chan::Pair(c)) if component < 2 => c
                .keys
                .iter()
                .map(|k| CurveKey {
                    frame: k.frame,
                    value: pick(k.value, component),
                    interp: k.interp,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The value of curve `component` of `ch` on `frame` (which may fall
    /// between frames), the way the animation plays it; `None` when this
    /// layer has no such property.
    pub fn curve_value(&self, ch: ChannelId, component: Component, frame: f32) -> Option<f32> {
        match self.chan(ch)? {
            Chan::Number(c) if component == 0 => Some(c.sample(frame)),
            Chan::Pair(c) if component < 2 => Some(pick(c.sample(frame), component)),
            _ => None,
        }
    }

    /// Set the value of the existing key of curve `component` on `frame`.
    /// The other component of a pair is kept. Returns whether there was a key
    /// and its value changed. A value that is not a number is refused.
    pub fn set_curve_value(&mut self, ch: ChannelId, component: Component, frame: Frame, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        match self.chan_mut(ch) {
            Some(ChanMut::Number(c)) if component == 0 => c
                .keys
                .binary_search_by_key(&frame, |k| k.frame)
                .ok()
                .is_some_and(|i| std::mem::replace(&mut c.keys[i].value, value) != value),
            Some(ChanMut::Pair(c)) if component < 2 => match c.keys.binary_search_by_key(&frame, |k| k.frame)
            {
                Ok(i) => {
                    let old = c.keys[i].value;
                    let new = put(old, component, value);
                    c.keys[i].value = new;
                    new != old
                }
                Err(_) => false,
            },
            _ => false,
        }
    }

    /// The span of frames strictly between the neighbours of the key on
    /// `frame` in property `ch`: a key dragged sideways stays inside it, so it
    /// never lands on or crosses another. `None` when there is no such key.
    pub fn key_room(&self, ch: ChannelId, frame: Frame) -> Option<(Frame, Frame)> {
        let frames: Vec<Frame> = match self.chan(ch)? {
            Chan::Number(c) => c.keys.iter().map(|k| k.frame).collect(),
            Chan::Pair(c) => c.keys.iter().map(|k| k.frame).collect(),
        };
        let i = frames.iter().position(|f| *f == frame)?;
        let lo = if i == 0 { Frame::MIN / 2 } else { frames[i - 1] + 1 };
        let hi = frames.get(i + 1).map_or(Frame::MAX / 2, |f| *f - 1);
        Some((lo, hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    fn layer() -> (Document, crate::scene::LayerId) {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut l = doc.new_layer("g", LayerKind::Group { children: vec![] });
        l.transform.position = l
            .transform
            .position
            .clone()
            .with_key(1, Vec2::new(0.0, 10.0), Interpolation::Linear)
            .with_key(11, Vec2::new(100.0, 30.0), Interpolation::Linear)
            .with_key(21, Vec2::new(0.0, 10.0), Interpolation::Linear);
        l.opacity = Channel::new(1.0).with_key(1, 1.0, Interpolation::Linear);
        let id = l.id;
        doc.layers.push(l);
        (doc, id)
    }

    #[test]
    fn a_pair_has_two_curves_and_a_number_one() {
        assert_eq!(component_count(ChannelId::Position), 2);
        assert_eq!(component_count(ChannelId::Rotation), 1);
        assert_eq!(component_count(ChannelId::FillColor(0)), 0);
        let (doc, id) = layer();
        let l = doc.find(id).unwrap();
        let x = l.curve_keys(ChannelId::Position, 0);
        let y = l.curve_keys(ChannelId::Position, 1);
        assert_eq!(x.iter().map(|k| k.value).collect::<Vec<_>>(), [0.0, 100.0, 0.0]);
        assert_eq!(y.iter().map(|k| k.value).collect::<Vec<_>>(), [10.0, 30.0, 10.0]);
        assert!(l.curve_keys(ChannelId::Position, 2).is_empty());
        assert!(l.curve_keys(ChannelId::FillColor(0), 0).is_empty());
        assert_eq!(l.curve_value(ChannelId::Position, 0, 6.0), Some(50.0));
        assert_eq!(l.curve_value(ChannelId::Position, 1, 6.0), Some(20.0));
        assert_eq!(l.curve_value(ChannelId::Shape(0), 0, 6.0), None);
    }

    #[test]
    fn display_units_turn_angles_into_degrees_and_opacity_into_percent() {
        assert!((display_scale(ChannelId::Rotation) * std::f32::consts::PI - 180.0).abs() < 1e-3);
        assert_eq!(display_scale(ChannelId::Opacity), 100.0);
        assert_eq!(display_scale(ChannelId::Position), 1.0);
    }

    #[test]
    fn setting_a_value_changes_one_component_of_an_existing_key_only() {
        let (mut doc, id) = layer();
        let l = doc.find_mut(id).unwrap();
        assert!(l.set_curve_value(ChannelId::Position, 1, 11, 80.0));
        assert_eq!(
            l.transform.position.keys[1].value,
            Vec2::new(100.0, 80.0),
            "x kept"
        );
        assert!(!l.set_curve_value(ChannelId::Position, 1, 11, 80.0), "no change");
        assert!(
            !l.set_curve_value(ChannelId::Position, 0, 5, 1.0),
            "no key on frame 5"
        );
        assert!(!l.set_curve_value(ChannelId::Position, 0, 11, f32::NAN));
        assert!(!l.set_curve_value(ChannelId::Position, 3, 11, 1.0));
        assert!(!l.set_curve_value(ChannelId::Trim(0), 0, 11, 1.0));
        assert!(l.set_curve_value(ChannelId::Opacity, 0, 1, 0.5));
        assert_eq!(l.opacity.keys[0].value, 0.5);
    }

    #[test]
    fn a_key_keeps_to_the_room_between_its_neighbours() {
        let (doc, id) = layer();
        let l = doc.find(id).unwrap();
        assert_eq!(l.key_room(ChannelId::Position, 11), Some((2, 20)));
        let (lo, hi) = l.key_room(ChannelId::Position, 1).unwrap();
        assert!(
            lo < -1000 && hi == 10,
            "the first key can go back as far as it likes"
        );
        assert!(l.key_room(ChannelId::Position, 12).is_none());
        assert!(l.key_room(ChannelId::Shape(0), 1).is_none());
    }

    #[test]
    fn the_graph_lists_the_transform_and_only_bones_with_keys() {
        let (mut doc, id) = layer();
        let l = doc.find_mut(id).unwrap();
        let ids: Vec<ChannelId> = l.graph_channels().iter().map(|c| c.0).collect();
        assert_eq!(
            ids,
            [
                ChannelId::Position,
                ChannelId::Rotation,
                ChannelId::Scale,
                ChannelId::Opacity
            ]
        );
        let mut skeleton = crate::Skeleton::default();
        let a = skeleton.add("a", None, Vec2::ZERO, 10.0, 0.0);
        let _b = skeleton.add("b", Some(a), Vec2::ZERO, 10.0, 0.0);
        skeleton.bones[1].angle = Channel::new(0.0).with_key(1, 0.5, Interpolation::Linear);
        let mut rig = doc.new_layer(
            "rig",
            LayerKind::Bone {
                skeleton,
                children: vec![],
            },
        );
        rig.opacity = Channel::new(1.0);
        let channels = rig.graph_channels();
        assert!(channels.contains(&(ChannelId::Bone(1), 1)));
        assert!(
            !channels.iter().any(|c| c.0 == ChannelId::Bone(0)),
            "an unkeyed bone has no curve"
        );
    }
}
