//! Frame evaluation: turn a document at a frame into a flat draw list.

use std::borrow::Cow;

use glam::Affine2;

use crate::compositing::{Blend, Ink, MaskRole, ROOT_SCOPE, Scope, ScopeId, ScopeKind};
use crate::path::Shape;
use crate::scene::{Document, Layer, LayerId, LayerKind};

/// One shape to draw, with everything resolved for the frame.
#[derive(Clone, Debug)]
pub struct DrawItem<'a> {
    pub layer: LayerId,
    /// The shape's index in its layer.
    pub index: usize,
    /// The shape as drawn on this frame: borrowed from the document, or,
    /// for a shape with point keys, a copy with the path for the frame.
    /// For an image, its outline, filled with nothing, so it can be picked.
    pub shape: Cow<'a, Shape>,
    /// For an image layer, the picture and its mesh.
    pub image: Option<ImageItem<'a>>,
    /// For a drawing on a frame-by-frame layer, its sub-layer; `index` is
    /// then the shape's index in that sub-layer.
    pub part: Option<crate::frames::Part>,
    /// Shape space to document space.
    pub transform: Affine2,
    /// Combined opacity of the shape's layer and all its ancestors, up to the
    /// scope it is drawn in (a flattened group carries the rest).
    pub opacity: f32,
    /// The picture it is drawn into: `ROOT_SCOPE`, the whole frame, unless the
    /// document uses compositing (see [`Evaluator::scopes`]).
    pub scope: ScopeId,
    /// How it is laid down in that picture.
    pub ink: Ink,
}

/// An image to draw: the media file's name and the mesh, its vertices in
/// the layer's space (bent by bones on this frame) with texture
/// coordinates, row by row.
#[derive(Clone, Debug)]
pub struct ImageItem<'a> {
    pub media: &'a str,
    pub cells: [u32; 2],
    pub vertices: Vec<(glam::Vec2, glam::Vec2)>,
}

/// Make a frame's items drawable without scopes, for the places that cannot
/// composite (exports to vector formats, onion skins): items that only make a
/// mask are left out, and the rest are put in the root and laid down over each
/// other in order, as if no layer were masked, clipped, mixed or flattened.
pub fn flatten_scopes(items: &mut Vec<DrawItem<'_>>, scopes: &[Scope]) {
    let in_mask = |mut s: ScopeId| {
        // a parent is an earlier scope; anything else ends the walk
        while let Some(scope) = scopes.get(s as usize) {
            match scope.kind {
                ScopeKind::Mask => return true,
                ScopeKind::Root => return false,
                ScopeKind::Layer { .. } if scope.parent < s => s = scope.parent,
                ScopeKind::Layer { .. } => return false,
            }
        }
        false
    };
    if scopes.len() > 1 {
        items.retain(|i| !in_mask(i.scope));
    }
    for item in items {
        item.scope = ROOT_SCOPE;
        item.ink = Ink::Over;
    }
}

/// Evaluate `doc` at `frame` (which may be fractional, for motion blur or
/// smooth playback) into draw items, back to front.
pub fn evaluate(doc: &Document, frame: f32) -> Vec<DrawItem<'_>> {
    let mut out = Vec::new();
    Evaluator::default().evaluate_into(doc, frame, &mut out);
    out
}

/// The most a camera may zoom out or in, so a stray value cannot collapse the
/// view to nothing or a point.
pub const CAMERA_ZOOM: std::ops::RangeInclusive<f32> = 0.05..=50.0;

/// What the document's camera does to the canvas at `frame`: document space
/// to the space of the picture the camera takes (the same size as the
/// canvas). The camera sits over the middle of the canvas, moved by its
/// position, rolled by its roll (positive is clockwise, as everywhere) and
/// zoomed by its zoom; the canvas turns the other way and grows. `None` when
/// the camera is looking at the whole canvas, as it does with no animation.
pub fn camera_at(doc: &Document, frame: f32) -> Option<Affine2> {
    let cam = &doc.camera;
    let zoom = cam.zoom.sample(frame);
    let zoom = if zoom.is_finite() && zoom != 0.0 {
        zoom.abs().clamp(*CAMERA_ZOOM.start(), *CAMERA_ZOOM.end())
    } else {
        1.0
    };
    let (position, roll) = (cam.position.sample(frame), cam.roll.sample(frame));
    let (position, roll) = if position.is_finite() && roll.is_finite() {
        (position, roll)
    } else {
        (glam::Vec2::ZERO, 0.0)
    };
    let centre = doc.size() / 2.0;
    let m = Affine2::from_translation(centre)
        * Affine2::from_scale(glam::Vec2::splat(zoom))
        * Affine2::from_angle(-roll)
        * Affine2::from_translation(-(centre + position));
    // the camera that sees everything is no camera
    (m != Affine2::IDENTITY).then_some(m)
}

/// The part of the canvas the camera shows at `frame`, as its four corners in
/// document space (top left, top right, bottom right, bottom left), for the
/// frame drawn on the canvas.
pub fn camera_frame(doc: &Document, frame: f32) -> Option<[glam::Vec2; 4]> {
    let inverse = camera_at(doc, frame).unwrap_or(Affine2::IDENTITY).inverse();
    if !inverse.is_finite() {
        return None;
    }
    let (w, h) = (doc.width, doc.height);
    Some(
        [
            glam::Vec2::ZERO,
            glam::Vec2::new(w, 0.0),
            glam::Vec2::new(w, h),
            glam::Vec2::new(0.0, h),
        ]
        .map(|c| inverse.transform_point2(c)),
    )
}

/// Reusable evaluator. Keeps its scratch buffers between frames so playback
/// does not allocate.
#[derive(Default)]
pub struct Evaluator {
    /// Per bone-layer nesting depth: skinning matrix for each bone
    /// (current pose times inverse rest pose).
    skins: Vec<Vec<Affine2>>,
    /// Per bone-layer nesting depth: the frame each of its actions plays
    /// at (Smart Bones), or NaN for actions that are not playing.
    smart: Vec<Vec<f32>>,
    pose: Vec<Affine2>,
    /// Buffers text layers use each frame.
    text: crate::text::Scratch,
    rest: Vec<Affine2>,
    angles: Vec<f32>,
    /// The bone layer whose action is being edited: its Smart Bones are not
    /// applied on top of the action's own keys.
    editing: Option<crate::scene::LayerId>,
    /// Leave rough sketches out, as renders do.
    pub skip_rough: bool,
    /// Look at the canvas through the document's camera, as renders do.
    /// The canvas the animator works on shows the whole page instead.
    pub camera: bool,
    /// The pictures of the frame just evaluated, drawn on their own and laid
    /// over their parents: scope 0 is the whole frame, and a document with no
    /// masks, clips, blend modes or flattened groups has no other. Each
    /// item's `scope` indexes this.
    pub scopes: Vec<Scope>,
    /// The scope and ink that items are being made in.
    scope: ScopeId,
    ink: Ink,
}

/// The bone layer a subtree is inside.
#[derive(Clone, Copy)]
struct Rig<'a> {
    /// Bone layer space to document space.
    world: Affine2,
    /// Index into `Evaluator::skins` and `Evaluator::smart`.
    depth: usize,
    actions: &'a [crate::actions::RigAction],
}

impl Evaluator {
    /// An evaluator for renders: it leaves out rough sketches, which are
    /// for the animator.
    pub fn for_render() -> Self {
        Self {
            skip_rough: true,
            camera: true,
            ..Self::default()
        }
    }

    pub fn evaluate_into<'a>(&mut self, doc: &'a Document, frame: f32, out: &mut Vec<DrawItem<'a>>) {
        out.clear();
        self.editing = doc.action_edit.as_ref().map(|e| e.layer);
        let root = if self.camera {
            camera_at(doc, frame).unwrap_or(Affine2::IDENTITY)
        } else {
            Affine2::IDENTITY
        };
        self.scopes.clear();
        self.scopes.push(Scope::ROOT);
        self.scope = ROOT_SCOPE;
        self.ink = Ink::Over;
        self.visit(&doc.layers, frame, root, 1.0, None, out);
    }

    /// Start a scope inside the current one and make it the current one.
    /// Returns the scope to go back to.
    fn open(&mut self, kind: ScopeKind) -> ScopeId {
        let id = self.scopes.len() as ScopeId;
        self.scopes.push(Scope {
            parent: self.scope,
            kind,
        });
        std::mem::replace(&mut self.scope, id)
    }

    /// `world` is the parent's space to document space. Inside a bone layer,
    /// `rig` is set and `world` is still kept up to date; `rel` (parent
    /// space to bone layer space) is recovered from both when needed.
    fn visit<'a>(
        &mut self,
        layers: &'a [Layer],
        frame: f32,
        world: Affine2,
        opacity: f32,
        rig: Option<(Rig<'a>, Affine2)>,
        out: &mut Vec<DrawItem<'a>>,
    ) {
        // Siblings are drawn in list order unless one has Order keys. The
        // common case allocates nothing: the sorted slots exist only then.
        let keyed = layers.len() > 1 && layers.iter().any(|l| l.order.is_animated());
        let slots = if keyed {
            draw_order(layers, frame)
        } else {
            Vec::new()
        };
        let index_at = |n: usize| if keyed { slots[n].1 } else { n };
        // masks, clips and the like need the siblings looked at together; a
        // document that uses none draws as it always has
        if layers.iter().any(|l| !l.composite.is_plain()) {
            let order: Vec<usize> = (0..layers.len()).map(index_at).collect();
            self.visit_composited(layers, &order, frame, world, opacity, rig, out);
            return;
        }
        for n in 0..layers.len() {
            self.visit_layer(&layers[index_at(n)], frame, world, opacity, rig, false, out);
        }
    }

    /// Siblings with compositing settings among them: the mask is drawn
    /// first into a scope of its own, then each layer in turn, with the
    /// layers clipped to it, held to the mask unless it is marked unmasked.
    #[allow(clippy::too_many_arguments)]
    fn visit_composited<'a>(
        &mut self,
        layers: &'a [Layer],
        order: &[usize],
        frame: f32,
        world: Affine2,
        opacity: f32,
        rig: Option<(Rig<'a>, Affine2)>,
        out: &mut Vec<DrawItem<'a>>,
    ) {
        // Every mask layer of the group, in list order, drawn into one scope.
        // Hiding all of them turns the mask off, so it can be looked past.
        let has_mask = order
            .iter()
            .any(|&i| layers[i].composite.mask != MaskRole::None && layers[i].visible);
        let mask = has_mask.then(|| {
            let prev = self.open(ScopeKind::Mask);
            let id = self.scope;
            for &i in order {
                let layer = &layers[i];
                let role = layer.composite.mask;
                if role == MaskRole::None {
                    continue;
                }
                let before = self.ink;
                self.ink = if role == MaskRole::Subtract {
                    Ink::Erase
                } else {
                    Ink::Over
                };
                // what the group is faded by is not the mask's business
                self.visit_layer(layer, frame, world, 1.0, rig, true, out);
                self.ink = before;
            }
            self.scope = prev;
            id
        });
        let mut n = 0;
        while n < order.len() {
            let layer = &layers[order[n]];
            n += 1;
            if layer.composite.mask != MaskRole::None {
                continue;
            }
            // the layers above it that are clipped to it belong to it
            let mut end = n;
            while end < order.len() {
                let c = &layers[order[end]].composite;
                if c.clip && c.mask == MaskRole::None {
                    end += 1;
                } else {
                    break;
                }
            }
            let clipped = &order[n..end];
            n = end;
            // hiding the base hides what is clipped to it
            if !layer.visible {
                continue;
            }
            let held = mask.filter(|_| !layer.composite.unmasked);
            let outer = held.map(|mask| {
                self.open(ScopeKind::Layer {
                    opacity: 1.0,
                    blend: Blend::Normal,
                    mask: Some(mask),
                })
            });
            if clipped.is_empty() {
                self.visit_layer(layer, frame, world, opacity, rig, false, out);
            } else {
                // the base and what is clipped to it are one picture, which takes the base's blend
                let prev = self.open(ScopeKind::Layer {
                    opacity: 1.0,
                    blend: layer.composite.blend,
                    mask: None,
                });
                self.visit_layer(layer, frame, world, opacity, rig, true, out);
                let before = self.ink;
                self.ink = Ink::Atop;
                for &i in clipped {
                    self.visit_layer(&layers[i], frame, world, opacity, rig, true, out);
                }
                self.ink = before;
                self.scope = prev;
            }
            if let Some(prev) = outer {
                self.scope = prev;
            }
        }
    }

    /// One layer and what is inside it. `plain` leaves out the scope the
    /// layer's own blend or flatten would open, for a layer that is a mask or
    /// is clipped to another and so is drawn into someone else's picture.
    #[allow(clippy::too_many_arguments)]
    fn visit_layer<'a>(
        &mut self,
        layer: &'a Layer,
        frame: f32,
        world: Affine2,
        opacity: f32,
        rig: Option<(Rig<'a>, Affine2)>,
        plain: bool,
        out: &mut Vec<DrawItem<'a>>,
    ) {
        if !layer.visible {
            return;
        }
        let opacity = opacity * layer.opacity.sample(frame).clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return;
        }
        // A layer that is mixed in some way, or a group that is faded as
        // one picture, is drawn on its own and laid over its parent; what
        // is inside it is then at full strength, the picture carries the
        // fade. Layers clipped to another or inside a mask are only drawn.
        let composite = layer.composite;
        let container = matches!(
            layer.kind,
            LayerKind::Group { .. } | LayerKind::Switch { .. } | LayerKind::Bone { .. }
        );
        let (opacity, restore) = if !plain
            && self.ink == Ink::Over
            && (composite.blend != Blend::Normal || (composite.flatten && container))
        {
            let prev = self.open(ScopeKind::Layer {
                opacity,
                blend: composite.blend,
                mask: None,
            });
            (1.0, Some(prev))
        } else {
            (opacity, None)
        };
        let local = layer.transform.sample(frame).to_affine();
        let world = world * local;
        // rel: this layer's space to the enclosing bone layer's space
        let rig = rig.map(|(r, rel)| (r, rel * local));

        match &layer.kind {
            LayerKind::Vector { shapes } => {
                for (index, shape) in shapes.iter().enumerate() {
                    if !shape.visible {
                        continue;
                    }
                    let item = self.shape_item(layer.id, index, None, shape, frame, world, opacity, rig);
                    out.push(item);
                }
            }
            LayerKind::Frames { cels } => {
                let Some(cel) = crate::frames::cel_at(cels, frame).map(|i| &cels[i]) else {
                    if let Some(prev) = restore {
                        self.scope = prev;
                    }
                    return;
                };
                for (part, index, shape) in cel.shapes() {
                    if !shape.visible || (self.skip_rough && part == crate::frames::Part::Rough) {
                        continue;
                    }
                    let item =
                        self.shape_item(layer.id, index, Some(part), shape, frame, world, opacity, rig);
                    out.push(item);
                }
            }
            LayerKind::Group { children } => {
                self.visit(children, frame, world, opacity, rig, out);
            }
            LayerKind::Switch { children, switch } => {
                if let Some(i) = crate::scene::switch_choice(children, switch, frame) {
                    self.visit(&children[i..=i], frame, world, opacity, rig, out);
                }
            }
            LayerKind::Audio(_) => {}
            LayerKind::Text(text) => {
                let start = out.len();
                text.items(layer.id, frame, world, opacity, &mut self.text, out);
                for item in &mut out[start..] {
                    item.scope = self.scope;
                    item.ink = self.ink;
                }
            }
            LayerKind::Image(image) => {
                let mut vertices = image.vertices();
                let mut transform = world;
                if let Some((r, rel)) = rig {
                    let skins = &self.skins[r.depth];
                    if let Some(weights) = image.valid_skin() {
                        let inv = rel.inverse();
                        for ((p, _), w) in vertices.iter_mut().zip(weights) {
                            *p = (inv * crate::bones::blend(skins, w) * rel).transform_point2(*p);
                        }
                    } else if let Some(skin) = image.bone.and_then(|b| skins.get(b)) {
                        transform = r.world * *skin * rel;
                    }
                }
                out.push(DrawItem {
                    layer: layer.id,
                    index: 0,
                    part: None,
                    shape: Cow::Owned(image_outline(&vertices, image.cells())),
                    image: Some(ImageItem {
                        media: &image.media,
                        cells: image.cells(),
                        vertices,
                    }),
                    transform,
                    opacity,
                    scope: self.scope,
                    ink: self.ink,
                });
            }
            LayerKind::Bone { skeleton, children } => {
                let depth = rig.map_or(0, |(r, _)| r.depth + 1);
                if self.skins.len() <= depth {
                    self.skins.resize_with(depth + 1, Vec::new);
                    self.smart.resize_with(depth + 1, Vec::new);
                }
                skeleton.angles(frame, &mut self.angles);
                // Smart Bones: each driver, as the timeline poses it,
                // picks a frame of its action, which turns other bones
                let smart = &mut self.smart[depth];
                smart.clear();
                let on = self.editing != Some(layer.id);
                for act in &skeleton.actions {
                    let f = act
                        .driver
                        .filter(|_| on)
                        .and_then(|d| self.angles.get(d).copied())
                        .and_then(|a| act.frame_for(a));
                    smart.push(f.unwrap_or(f32::NAN));
                }
                // master controllers play regular actions by their values on this frame
                if on {
                    for controller in &skeleton.controllers {
                        for link in &controller.links {
                            if let Some(act) = skeleton.actions.get(link.action)
                                && let Some(f) = controller.action_frame(link, act, frame)
                            {
                                smart[link.action] = f;
                            }
                        }
                    }
                }
                for (act, &f) in skeleton.actions.iter().zip(smart.iter()) {
                    if f.is_nan() {
                        continue;
                    }
                    for b in &act.bones {
                        if Some(b.bone) != act.driver
                            && let Some(a) = self.angles.get_mut(b.bone)
                        {
                            *a += act.bone_offset(b.bone, f);
                        }
                    }
                }
                for (a, bone) in self.angles.iter_mut().zip(&skeleton.bones) {
                    *a = bone.clamp(*a);
                }
                skeleton.pose_from(&self.angles, &mut self.pose);
                skeleton.rest_pose(&mut self.rest);
                let skins = &mut self.skins[depth];
                skins.clear();
                skins.extend(self.pose.iter().zip(&self.rest).map(|(p, r)| *p * r.inverse()));
                let inner = Rig {
                    world,
                    depth,
                    actions: &skeleton.actions,
                };
                self.visit(
                    children,
                    frame,
                    world,
                    opacity,
                    Some((inner, Affine2::IDENTITY)),
                    out,
                );
            }
        }
        if let Some(prev) = restore {
            self.scope = prev;
        }
    }
}

impl Evaluator {
    /// One shape of a vector layer or drawing, posed, morphed and deformed
    /// for the frame.
    #[allow(clippy::too_many_arguments)]
    fn shape_item<'a>(
        &self,
        layer: crate::scene::LayerId,
        index: usize,
        part: Option<crate::frames::Part>,
        shape: &'a Shape,
        frame: f32,
        world: Affine2,
        opacity: f32,
        rig: Option<(Rig<'a>, Affine2)>,
    ) -> DrawItem<'a> {
        let skinned = rig.and_then(|(r, rel)| {
            let weights = shape.valid_skin()?;
            Some((&self.skins[r.depth][..], weights, rel))
        });
        let transform = match (rig, shape.bone, skinned) {
            (_, _, Some(_)) => world,
            (Some((r, rel)), Some(b), None) => match self.skins[r.depth].get(b) {
                Some(skin) => r.world * *skin * rel,
                None => world,
            },
            _ => world,
        };
        let mut drawn = at_frame(shape, frame);
        // Smart Bone actions reshape the shapes of vector layers
        if let Some((r, _)) = rig
            && part.is_none()
        {
            morph(&mut drawn, r.actions, &self.smart[r.depth], layer, index);
        }
        if let Some((skins, weights, rel)) = skinned {
            deform(drawn.to_mut(), skins, weights, rel);
        }
        DrawItem {
            layer,
            index,
            part,
            shape: drawn,
            image: None,
            transform,
            opacity,
            scope: self.scope,
            ink: self.ink,
        }
    }
}

/// Each sibling's slot on `frame` with its index in the list, in the order
/// to draw them: a layer with Order keys sits at the value they hold (they
/// hold between keys, as the editor writes them), any other at its place in
/// the list. A stable sort, so layers in the same slot keep the list order,
/// and `total_cmp`, so a damaged value cannot panic it.
fn draw_order(layers: &[Layer], frame: f32) -> Vec<(f32, usize)> {
    let mut slots: Vec<(f32, usize)> = layers
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let slot = if l.order.is_animated() {
                l.order.sample(frame.floor())
            } else {
                i as f32
            };
            (slot, i)
        })
        .collect();
    slots.sort_by(|a, b| a.0.total_cmp(&b.0));
    slots
}

/// The mesh's border as a closed shape filled with nothing visible, so
/// clicking inside an image picks it.
fn image_outline(vertices: &[(glam::Vec2, glam::Vec2)], [cols, rows]: [u32; 2]) -> Shape {
    let at = |c: u32, r: u32| vertices[(r * (cols + 1) + c) as usize].0;
    let mut border = Vec::new();
    border.extend((0..=cols).map(|c| at(c, 0)));
    border.extend((1..=rows).map(|r| at(cols, r)));
    border.extend((0..cols).rev().map(|c| at(c, rows)));
    border.extend((1..rows).rev().map(|r| at(0, r)));
    let mut path = crate::Path::new().move_to(border[0]);
    for p in &border[1..] {
        path = path.line_to(*p);
    }
    Shape::new(
        "Image",
        path.close(),
        crate::Style::fill(crate::Color::TRANSPARENT),
    )
}

/// Add the point changes of the Smart Bone actions playing on `frames` to
/// shape `index` of `layer`.
fn morph(
    shape: &mut Cow<'_, Shape>,
    actions: &[crate::actions::RigAction],
    frames: &[f32],
    layer: crate::scene::LayerId,
    index: usize,
) {
    for (act, &f) in actions.iter().zip(frames) {
        if f.is_nan() {
            continue;
        }
        let Some(keys) = act.shape_keys(layer, index) else {
            continue;
        };
        // at its first key an action changes nothing, so the shape is not copied
        if keys.first().is_none_or(|first| f <= first.frame as f32) {
            continue;
        }
        let (Some(from), Some(to)) = (
            crate::actions::sample_keys(keys, 0.0),
            crate::actions::sample_keys(keys, f),
        ) else {
            continue;
        };
        crate::actions::add_offset(&mut shape.to_mut().path, &from, &to);
    }
}

/// Move each point of `shape` (in its layer's space) by the blend of its
/// bones' movements. `rel` is the layer's space to the bone layer's.
fn deform(shape: &mut Shape, skins: &[Affine2], weights: &[Vec<(usize, f32)>], rel: Affine2) {
    use crate::PathCmd;
    let inv = rel.inverse();
    let moves: Vec<Affine2> = weights
        .iter()
        .map(|w| inv * crate::bones::blend(skins, w) * rel)
        .collect();
    let mut point = 0;
    let mut prev: Option<usize> = None;
    for cmd in &mut shape.path.cmds {
        match cmd {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => {
                *p = moves[point].transform_point2(*p);
                prev = Some(point);
                point += 1;
            }
            PathCmd::CubicTo(c1, c2, p) => {
                // the handle leaving a point moves with it, the one arriving
                // with the point it arrives at
                let from = prev.unwrap_or(point);
                *c1 = moves[from].transform_point2(*c1);
                *c2 = moves[point].transform_point2(*c2);
                *p = moves[point].transform_point2(*p);
                prev = Some(point);
                point += 1;
            }
            PathCmd::Close => prev = None,
        }
    }
    shape.skin = None;
}

/// `shape` as it is on `frame`. Only shapes with point keys are copied, so
/// playing unanimated artwork allocates nothing.
fn at_frame(shape: &Shape, frame: f32) -> Cow<'_, Shape> {
    let trim_moves = !shape.trim_keys.is_empty();
    if !shape.is_animated() && !shape.colors_animated() && !trim_moves {
        return Cow::Borrowed(shape);
    }
    // a shape whose colours move but whose points do not keeps its own path
    let path = match shape.path_at(frame) {
        Cow::Borrowed(p) if std::ptr::eq(p, &shape.path) && !shape.colors_animated() && !trim_moves => {
            return Cow::Borrowed(shape);
        }
        path => path.into_owned(),
    };
    Cow::Owned(Shape {
        name: shape.name.clone(),
        path,
        style: shape.style_at(frame),
        bone: shape.bone,
        widths: shape.widths.clone(),
        path_keys: Vec::new(),
        skin: shape.skin.clone(),
        rect: shape.rect,
        visible: shape.visible,
        fill_keys: Vec::new(),
        stroke_keys: Vec::new(),
        trim: shape.trim_at(frame),
        trim_keys: Vec::new(),
        hidden_edge_flags: shape.hidden_edge_flags.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::Interpolation;

    #[test]
    fn the_camera_sees_everything_until_it_is_moved() {
        let mut doc = Document::new("c", 200.0, 100.0, 24.0);
        assert!(camera_at(&doc, 0.0).is_none(), "no camera, no change");
        // the frame it draws is the whole canvas
        let all = camera_frame(&doc, 0.0).unwrap();
        assert_eq!(all[0], glam::Vec2::ZERO);
        assert_eq!(all[2], glam::Vec2::new(200.0, 100.0));
        // zoomed in on the middle, twice as close: the middle stays, the corners go off
        doc.camera.zoom.base = 2.0;
        let m = camera_at(&doc, 0.0).unwrap();
        assert!(
            (m.transform_point2(glam::Vec2::new(100.0, 50.0)) - glam::Vec2::new(100.0, 50.0)).length() < 1e-4
        );
        assert!(
            (m.transform_point2(glam::Vec2::new(150.0, 50.0)) - glam::Vec2::new(200.0, 50.0)).length() < 1e-4
        );
        // and its frame on the canvas is the middle half
        let frame = camera_frame(&doc, 0.0).unwrap();
        assert!(
            (frame[0] - glam::Vec2::new(50.0, 25.0)).length() < 1e-3,
            "{:?}",
            frame[0]
        );
        assert!((frame[2] - glam::Vec2::new(150.0, 75.0)).length() < 1e-3);
        // moved right: what was at its middle (the point it looks at) lands in the middle
        doc.camera.zoom.base = 1.0;
        doc.camera.position.base = glam::Vec2::new(40.0, 0.0);
        let m = camera_at(&doc, 0.0).unwrap();
        assert!(
            (m.transform_point2(glam::Vec2::new(140.0, 50.0)) - glam::Vec2::new(100.0, 50.0)).length() < 1e-4
        );
        // rolled a quarter turn clockwise: the canvas turns anticlockwise about the middle
        doc.camera.position.base = glam::Vec2::ZERO;
        doc.camera.roll.base = std::f32::consts::FRAC_PI_2;
        let m = camera_at(&doc, 0.0).unwrap();
        let p = m.transform_point2(glam::Vec2::new(150.0, 50.0)); // right of the middle
        assert!((p - glam::Vec2::new(100.0, 0.0)).length() < 1e-3, "{p:?}"); // goes to the top
        // values that are not numbers or a zoom of nothing cannot break the view
        doc.camera.roll.base = f32::NAN;
        doc.camera.zoom.base = 0.0;
        assert!(camera_at(&doc, 0.0).is_none());
        doc.camera.zoom.base = 1.0e9;
        let huge = camera_at(&doc, 0.0).unwrap();
        assert!(
            (huge.matrix2.x_axis.length() - *CAMERA_ZOOM.end()).abs() < 1e-3,
            "clamped"
        );
    }

    #[test]
    fn the_camera_animates_and_renders_look_through_it_but_the_canvas_does_not() {
        use crate::{Color, Style};
        let mut doc = Document::new("c", 200.0, 100.0, 24.0);
        doc.camera.zoom = crate::Channel::new(1.0)
            .with_key(0, 1.0, Interpolation::Linear)
            .with_key(10, 2.0, Interpolation::Linear);
        let shape = Shape::rectangle(
            "r",
            Vec2::new(90.0, 40.0),
            Vec2::splat(20.0),
            0.0,
            Style::fill(Color::BLACK),
        );
        let layer = doc.new_layer("l", LayerKind::vector(vec![shape]));
        doc.layers.push(layer);
        let mut canvas = Evaluator::default();
        let mut render = Evaluator::for_render();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        canvas.evaluate_into(&doc, 10.0, &mut a);
        render.evaluate_into(&doc, 10.0, &mut b);
        assert_eq!(
            a[0].transform,
            Affine2::IDENTITY,
            "the canvas shows the whole page"
        );
        // the render at frame 10 has the artwork twice as big about the middle
        assert!((b[0].transform.matrix2.x_axis.length() - 2.0).abs() < 1e-4);
        render.evaluate_into(&doc, 0.0, &mut b);
        assert_eq!(b[0].transform, Affine2::IDENTITY, "frame 0 sees it all");
        render.evaluate_into(&doc, 5.0, &mut b);
        assert!(
            (b[0].transform.matrix2.x_axis.length() - 1.5).abs() < 1e-4,
            "halfway"
        );
    }

    #[test]
    fn colours_change_over_time_and_the_path_stays_where_it_is() {
        use crate::{Color, Key, Style};
        let mut doc = Document::new("c", 100.0, 100.0, 24.0);
        let mut shape = Shape::rectangle("r", Vec2::ZERO, Vec2::splat(10.0), 0.0, Style::fill(Color::BLACK));
        shape.style.stroke = Some(crate::Stroke {
            color: Color::BLACK,
            width: 2.0,
        });
        shape.fill_keys = vec![
            Key {
                frame: 0,
                value: Color::BLACK,
                interp: Interpolation::Linear,
            },
            Key {
                frame: 10,
                value: Color::WHITE,
                interp: Interpolation::Linear,
            },
        ];
        let layer = doc.new_layer("l", LayerKind::vector(vec![shape.clone()]));
        doc.layers.push(layer);
        let at = |doc: &Document, f: f32| {
            let items = evaluate(doc, f);
            (items[0].shape.style, items[0].shape.path.clone())
        };
        let (style0, path0) = at(&doc, 0.0);
        let (style5, path5) = at(&doc, 5.0);
        let (style99, _) = at(&doc, 99.0);
        assert_eq!(style0.fill, Some(Color::BLACK));
        let mid = style5.fill.unwrap();
        assert!(
            (mid.r - 0.5).abs() < 1e-4 && (mid.g - 0.5).abs() < 1e-4,
            "{mid:?}"
        );
        assert_eq!(style99.fill, Some(Color::WHITE), "held after the last key");
        assert_eq!(path0, path5, "only the colour moved");
        // the stroke, with no keys, keeps its colour and width
        assert_eq!(style5.stroke, shape.style.stroke);
        // a shape with no colour keys is drawn as it is, not copied
        let plain = Shape::new("p", shape.path.clone(), Style::fill(Color::BLACK));
        assert!(matches!(at_frame(&plain, 3.0), Cow::Borrowed(_)));
        assert!(matches!(at_frame(&shape, 3.0), Cow::Owned(_)));
    }
    use crate::bones::Skeleton;
    use crate::path::{Path, Style};
    use crate::transform::AnimTransform;
    use crate::{Color, Vec2};
    use std::f32::consts::FRAC_PI_2;

    fn square() -> Shape {
        Shape::new(
            "sq",
            Path::rect(Vec2::ZERO, Vec2::splat(1.0), 0.0),
            Style::fill(Color::BLACK),
        )
    }

    #[test]
    fn stroke_exposure_moves_with_its_keys() {
        use crate::anim::{Keying, set_key_in};
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut shape = square();
        shape.style.stroke = Some(crate::Stroke {
            color: Color::BLACK,
            width: 1.0,
        });
        // the stroke grows from nothing to all of it over 10 frames
        set_key_in(
            &mut shape.trim_keys,
            0,
            Vec2::new(0.0, 0.0),
            Interpolation::Linear,
        );
        set_key_in(
            &mut shape.trim_keys,
            10,
            Vec2::new(0.0, 1.0),
            Interpolation::Linear,
        );
        let layer = doc.new_layer("v", LayerKind::vector(vec![shape]));
        doc.layers.push(layer);
        let end_at = |frame: f32| evaluate(&doc, frame)[0].shape.trim.y;
        assert_eq!(end_at(0.0), 0.0);
        assert!((end_at(5.0) - 0.5).abs() < 1e-5);
        assert_eq!(end_at(10.0), 1.0);
        // an edit on a later frame keys it, and on the first frame changes it outright
        let mut still = square();
        still.set_trim_at(0, Vec2::new(0.25, 0.75), Keying::default());
        assert_eq!((still.trim, still.trim_keys.len()), (Vec2::new(0.25, 0.75), 0));
        still.set_trim_at(8, Vec2::new(0.0, 1.0), Keying::default());
        assert_eq!(still.trim_keys.len(), 2, "a key to start from, and the new one");
        // a still shape is not copied to be drawn
        let plain = Shape::new("p", Path::circle(Vec2::ZERO, 1.0), Style::fill(Color::BLACK));
        assert!(matches!(at_frame(&plain, 3.0), Cow::Borrowed(_)));
    }

    #[test]
    fn nested_transforms_compose() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let inner = doc
            .new_layer("v", LayerKind::vector(vec![square()]))
            .with_transform(AnimTransform::at(Vec2::new(5.0, 0.0)));
        let group = doc
            .new_layer(
                "g",
                LayerKind::Group {
                    children: vec![inner],
                },
            )
            .with_transform(AnimTransform::at(Vec2::new(10.0, 0.0)));
        doc.layers.push(group);

        let items = evaluate(&doc, 0.0);
        assert_eq!(items.len(), 1);
        let p = items[0].transform.transform_point2(Vec2::ZERO);
        assert!(p.abs_diff_eq(Vec2::new(15.0, 0.0), 1e-5));
    }

    #[test]
    fn hidden_and_transparent_layers_are_skipped() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut hidden = doc.new_layer("h", LayerKind::vector(vec![square()]));
        hidden.visible = false;
        let mut clear = doc.new_layer("c", LayerKind::vector(vec![square()]));
        clear.opacity.base = 0.0;
        doc.layers.extend([hidden, clear]);
        assert!(evaluate(&doc, 0.0).is_empty());
    }

    #[test]
    fn bound_shape_follows_its_bone() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut skel = Skeleton::default();
        let b = skel.add("b", None, Vec2::new(50.0, 50.0), 10.0, 0.0);
        skel.key_angle(b, 0, 0.0, Interpolation::Linear);
        skel.key_angle(b, 10, FRAC_PI_2, Interpolation::Linear);

        // a point on the bone, 10 units along it, in rest pose
        let dot = Shape::new(
            "dot",
            Path::new().move_to(Vec2::new(60.0, 50.0)),
            Style::default(),
        )
        .bound_to(b);
        let shapes = doc.new_layer("art", LayerKind::vector(vec![dot]));
        let rig = doc.new_layer(
            "rig",
            LayerKind::Bone {
                skeleton: skel,
                children: vec![shapes],
            },
        );
        doc.layers.push(rig);

        let at = |frame: f32| {
            let items = evaluate(&doc, frame);
            items[0].transform.transform_point2(Vec2::new(60.0, 50.0))
        };
        assert!(at(0.0).abs_diff_eq(Vec2::new(60.0, 50.0), 1e-4));
        // quarter turn about the joint at (50, 50) moves the point to (50, 60)
        assert!(at(10.0).abs_diff_eq(Vec2::new(50.0, 60.0), 1e-4));
    }

    #[test]
    fn shapes_with_point_keys_are_drawn_as_on_the_frame() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut shape = square();
        let mut moved = shape.path.clone();
        moved.transform(Affine2::from_translation(Vec2::new(10.0, 0.0)));
        shape.key_path(0, shape.path.clone(), Interpolation::Linear);
        shape.key_path(10, moved, Interpolation::Linear);
        let layer = doc.new_layer("v", LayerKind::vector(vec![square(), shape]));
        doc.layers.push(layer);
        let items = evaluate(&doc, 5.0);
        assert!(matches!(items[0].shape, Cow::Borrowed(_)));
        assert_eq!(items[1].index, 1);
        assert_eq!(items[1].shape.path.on_curve_points()[0], Vec2::new(5.0, 0.0));
        assert!(!items[1].shape.is_animated());
    }

    #[test]
    fn skinned_points_follow_their_bones() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut skel = Skeleton::default();
        let a = skel.add("a", None, Vec2::ZERO, 10.0, 0.0);
        let b = skel.add("b", Some(a), Vec2::new(10.0, 0.0), 10.0, 0.0);
        skel.key_angle(b, 0, FRAC_PI_2, Interpolation::Linear);
        let mut line = Shape::new(
            "l",
            Path::new()
                .move_to(Vec2::ZERO)
                .line_to(Vec2::new(10.0, 0.0))
                .line_to(Vec2::new(20.0, 0.0)),
            Style::default(),
        );
        line.skin = Some(vec![vec![(a, 1.0)], vec![(a, 0.5), (b, 0.5)], vec![(b, 1.0)]]);
        let art = doc.new_layer("art", LayerKind::vector(vec![line]));
        let rig = doc.new_layer(
            "rig",
            LayerKind::Bone {
                skeleton: skel,
                children: vec![art],
            },
        );
        doc.layers.push(rig);
        let items = evaluate(&doc, 0.0);
        let pts = items[0].shape.path.on_curve_points();
        // the elbow stays, the tip swings down with the lower bone
        assert!(pts[1].abs_diff_eq(Vec2::new(10.0, 0.0), 1e-4));
        assert!(pts[2].abs_diff_eq(Vec2::new(10.0, 10.0), 1e-4), "{:?}", pts[2]);
        assert_eq!(items[0].transform, Affine2::IDENTITY);
    }

    #[test]
    fn image_layers_draw_their_mesh_and_can_be_picked() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut img = crate::media::Image::new("media/x.png", 40.0, 20.0);
        img.grid = [2, 2];
        let layer = doc
            .new_layer("pic", LayerKind::Image(img))
            .with_transform(AnimTransform::at(Vec2::new(10.0, 0.0)));
        doc.layers.push(layer);
        let items = evaluate(&doc, 0.0);
        let image = items[0].image.as_ref().unwrap();
        assert_eq!(image.vertices.len(), 9);
        assert_eq!(image.media, "media/x.png");
        assert!(items[0].shape.contains(Vec2::new(20.0, 10.0), 0.0));
        assert!(!items[0].shape.contains(Vec2::new(60.0, 10.0), 0.0));
    }

    #[test]
    fn drawings_show_their_exposure_and_roughs_can_be_left_out() {
        use crate::frames::{Cel, Part};
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let mut a = Cel::new(0);
        a.line.push(square());
        a.rough.push(square());
        let mut b = Cel::new(6);
        b.colour.push(square());
        let layer = doc.new_layer("anim", LayerKind::Frames { cels: vec![a, b] });
        doc.layers.push(layer);
        let parts = |items: Vec<DrawItem>| items.iter().map(|i| i.part).collect::<Vec<_>>();
        assert_eq!(parts(evaluate(&doc, 5.0)), [Some(Part::Rough), Some(Part::Line)]);
        assert_eq!(parts(evaluate(&doc, 6.0)), [Some(Part::Colour)]);
        let mut ev = Evaluator::for_render();
        let mut out = Vec::new();
        ev.evaluate_into(&doc, 0.0, &mut out);
        assert_eq!(parts(out), [Some(Part::Line)]);
    }

    #[test]
    fn a_switch_layer_shows_the_child_its_keys_name() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let a = doc.new_layer("A", LayerKind::vector(vec![square()]));
        let b = doc.new_layer("rest", LayerKind::vector(vec![square(), square()]));
        let (aid, bid) = (a.id, b.id);
        let keys = vec![crate::Key {
            frame: 5,
            value: "A".to_owned(),
            interp: Interpolation::Step,
        }];
        let sw = doc.new_layer(
            "mouth",
            LayerKind::Switch {
                children: vec![a, b],
                switch: keys,
            },
        );
        doc.layers.push(sw);
        let shown = |f: f32| evaluate(&doc, f).iter().map(|i| i.layer).collect::<Vec<_>>();
        // before any key: the top child
        assert_eq!(shown(0.0), [bid, bid]);
        assert_eq!(shown(5.0), [aid]);
        assert_eq!(shown(50.0), [aid]);
    }

    #[test]
    fn missing_bone_falls_back_to_layer_transform() {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let shapes = doc.new_layer("art", LayerKind::vector(vec![square().bound_to(7)]));
        let rig = doc.new_layer(
            "rig",
            LayerKind::Bone {
                skeleton: Skeleton::default(),
                children: vec![shapes],
            },
        );
        doc.layers.push(rig);
        let items = evaluate(&doc, 0.0);
        assert_eq!(items[0].transform, Affine2::IDENTITY);
    }

    /// Three layers a, b, c in the list, one square each, and their ids.
    fn three() -> (Document, [crate::scene::LayerId; 3]) {
        let mut doc = Document::new("t", 100.0, 100.0, 24.0);
        let layers: Vec<_> = ["a", "b", "c"]
            .into_iter()
            .map(|n| doc.new_layer(n, LayerKind::vector(vec![square()])))
            .collect();
        let ids = [layers[0].id, layers[1].id, layers[2].id];
        doc.layers = layers;
        (doc, ids)
    }

    fn drawn(doc: &Document, frame: f32) -> Vec<crate::scene::LayerId> {
        evaluate(doc, frame).iter().map(|i| i.layer).collect()
    }

    /// An item's layer, scope, ink and opacity.
    type Composed = (crate::scene::LayerId, ScopeId, Ink, f32);

    /// Each item's layer, scope and ink, and the scopes of the frame.
    fn composed(doc: &Document) -> (Vec<Composed>, Vec<Scope>) {
        let mut items = Vec::new();
        let mut ev = Evaluator::default();
        ev.evaluate_into(doc, 0.0, &mut items);
        (
            items
                .iter()
                .map(|i| (i.layer, i.scope, i.ink, i.opacity))
                .collect(),
            ev.scopes.clone(),
        )
    }

    #[test]
    fn flattening_leaves_out_what_only_makes_a_mask_and_puts_the_rest_in_the_root() {
        let (mut doc, [a, b, c]) = three();
        doc.layers[0].composite.mask = MaskRole::Add;
        doc.layers[1].composite.blend = Blend::Add;
        doc.layers[2].composite.clip = true;
        let mut items = Vec::new();
        let mut ev = Evaluator::default();
        ev.evaluate_into(&doc, 0.0, &mut items);
        assert!(ev.scopes.len() > 1);
        flatten_scopes(&mut items, &ev.scopes);
        assert_eq!(
            items
                .iter()
                .map(|i| (i.layer, i.scope, i.ink))
                .collect::<Vec<_>>(),
            [(b, 0, Ink::Over), (c, 0, Ink::Over)]
        );
        let _ = a;
        // with no scopes there is nothing to leave out
        let (plain, [x, ..]) = three();
        let mut items = evaluate(&plain, 0.0);
        flatten_scopes(&mut items, &[Scope::ROOT]);
        assert_eq!((items.len(), items[0].layer), (3, x));
    }

    #[test]
    fn a_plain_document_has_one_scope_and_every_item_in_it() {
        let (doc, [a, b, c]) = three();
        let (items, scopes) = composed(&doc);
        assert_eq!(scopes, [Scope::ROOT]);
        assert_eq!(
            items,
            [
                (a, ROOT_SCOPE, Ink::Over, 1.0),
                (b, ROOT_SCOPE, Ink::Over, 1.0),
                (c, ROOT_SCOPE, Ink::Over, 1.0)
            ]
        );
    }

    #[test]
    fn a_layer_with_a_blend_mode_is_a_picture_of_its_own_and_the_others_are_not() {
        let (mut doc, [a, b, c]) = three();
        doc.layers[1].composite.blend = Blend::Multiply;
        doc.layers[1].opacity.base = 0.5;
        let (items, scopes) = composed(&doc);
        assert_eq!(
            scopes[1],
            Scope {
                parent: ROOT_SCOPE,
                kind: ScopeKind::Layer {
                    opacity: 0.5,
                    blend: Blend::Multiply,
                    mask: None
                }
            }
        );
        // inside it the layer is at full strength: the picture carries the fade
        assert_eq!(
            items,
            [
                (a, 0, Ink::Over, 1.0),
                (b, 1, Ink::Over, 1.0),
                (c, 0, Ink::Over, 1.0)
            ]
        );
    }

    #[test]
    fn a_flattened_group_is_faded_as_one_picture_and_a_plain_one_fades_each_shape() {
        let (mut doc, [a, b, _]) = three();
        let children = vec![doc.layers.remove(0), doc.layers.remove(0)];
        let mut group = doc.new_layer("g", LayerKind::Group { children });
        group.opacity.base = 0.5;
        doc.layers.insert(0, group);
        // as it has always been: every shape carries the group's opacity, no scope
        let (items, scopes) = composed(&doc);
        assert_eq!(scopes.len(), 1);
        assert_eq!((items[0].0, items[0].3), (a, 0.5));
        assert_eq!((items[1].0, items[1].3), (b, 0.5));
        // flattened: the shapes are at full strength in a picture that is faded
        doc.layers[0].composite.flatten = true;
        let (items, scopes) = composed(&doc);
        assert_eq!(scopes.len(), 2);
        assert!(matches!(scopes[1].kind, ScopeKind::Layer { opacity, .. } if opacity == 0.5));
        assert!(items[..2].iter().all(|i| i.1 == 1 && i.3 == 1.0), "{items:?}");
        // a flattened group that is not faded or mixed is just drawn
        doc.layers[0].opacity.base = 1.0;
        assert_eq!(composed(&doc).1.len(), 2, "it still is its own picture, as asked");
    }

    #[test]
    fn mask_layers_are_drawn_into_a_scope_of_their_own_and_hold_back_the_others() {
        let (mut doc, [a, b, c]) = three();
        doc.layers[0].composite.mask = MaskRole::Add;
        doc.layers[2].composite.unmasked = true;
        let (items, scopes) = composed(&doc);
        assert_eq!(scopes[1].kind, ScopeKind::Mask);
        assert_eq!(
            scopes[2].kind,
            ScopeKind::Layer {
                opacity: 1.0,
                blend: Blend::Normal,
                mask: Some(1)
            }
        );
        // the mask layer is in the mask, b is held to it, c is not
        assert_eq!(
            items,
            [
                (a, 1, Ink::Over, 1.0),
                (b, 2, Ink::Over, 1.0),
                (c, 0, Ink::Over, 1.0)
            ]
        );
        // one that subtracts takes away what was there
        doc.layers[2].composite.unmasked = false;
        doc.layers[2].composite.mask = MaskRole::Subtract;
        let (items, _) = composed(&doc);
        assert_eq!(items[0], (a, 1, Ink::Over, 1.0));
        assert_eq!(items[1].2, Ink::Erase, "{items:?}");
        assert_eq!(items[1].1, 1);
    }

    #[test]
    fn hiding_every_mask_layer_turns_the_mask_off() {
        let (mut doc, [a, b, c]) = three();
        doc.layers[0].composite.mask = MaskRole::Add;
        doc.layers[0].visible = false;
        let (items, scopes) = composed(&doc);
        assert_eq!(scopes.len(), 1);
        assert_eq!(items.iter().map(|i| i.0).collect::<Vec<_>>(), [b, c]);
        let _ = a;
    }

    #[test]
    fn a_clipped_layer_is_drawn_atop_its_base_in_one_picture() {
        let (mut doc, [a, b, c]) = three();
        doc.layers[1].composite.clip = true;
        doc.layers[0].composite.blend = Blend::Screen;
        let (items, scopes) = composed(&doc);
        // the base and the clipped layer are one picture, mixed as the base is
        assert_eq!(
            scopes[1].kind,
            ScopeKind::Layer {
                opacity: 1.0,
                blend: Blend::Screen,
                mask: None
            }
        );
        assert_eq!(
            items,
            [
                (a, 1, Ink::Over, 1.0),
                (b, 1, Ink::Atop, 1.0),
                (c, 0, Ink::Over, 1.0)
            ]
        );
        // hiding the base hides what is clipped to it
        doc.layers[0].visible = false;
        let (items, _) = composed(&doc);
        assert_eq!(items.iter().map(|i| i.0).collect::<Vec<_>>(), [c]);
        // a clipped layer with nothing under it is just drawn
        let (mut doc, [a, ..]) = three();
        doc.layers[0].composite.clip = true;
        assert_eq!(composed(&doc).0[0], (a, 0, Ink::Over, 1.0));
    }

    #[test]
    fn the_masks_of_a_group_hold_back_what_is_inside_it_and_scopes_nest() {
        let (mut doc, [a, b, c]) = three();
        let kids = vec![doc.layers.remove(1), doc.layers.remove(1)];
        let mut group = doc.new_layer("g", LayerKind::Group { children: kids });
        group.composite.blend = Blend::Add;
        doc.layers.push(group);
        doc.layers[0].composite.mask = MaskRole::Add;
        let (items, scopes) = composed(&doc);
        // scope 1: the mask (a); scope 2: the group, held to it; scope 3: the group as a blended picture inside
        assert_eq!(scopes[1].kind, ScopeKind::Mask);
        assert!(matches!(scopes[2].kind, ScopeKind::Layer { mask: Some(1), .. }));
        assert!(matches!(
            scopes[3].kind,
            ScopeKind::Layer {
                blend: Blend::Add,
                ..
            }
        ));
        assert_eq!(scopes[3].parent, 2);
        assert_eq!(
            items,
            [
                (a, 1, Ink::Over, 1.0),
                (b, 3, Ink::Over, 1.0),
                (c, 3, Ink::Over, 1.0)
            ]
        );
    }

    #[test]
    fn siblings_are_drawn_in_list_order_until_one_has_order_keys() {
        let (mut doc, [a, b, c]) = three();
        for f in [0.0, 40.0] {
            assert_eq!(drawn(&doc, f), [a, b, c]);
        }
        // a sits at its own place until frame 40, then jumps in front of the rest
        doc.layers[0].order.set_key(0, 0.0, Interpolation::Step);
        doc.layers[0].order.set_key(40, 5.0, Interpolation::Step);
        assert_eq!(drawn(&doc, 0.0), [a, b, c]);
        assert_eq!(drawn(&doc, 39.0), [a, b, c]);
        assert_eq!(drawn(&doc, 39.9), [a, b, c], "a step holds until its frame");
        assert_eq!(drawn(&doc, 40.0), [b, c, a]);
        assert_eq!(drawn(&doc, 99.0), [b, c, a]);
        // and the list itself is untouched
        assert_eq!(doc.layers.iter().map(|l| l.id).collect::<Vec<_>>(), [a, b, c]);
    }

    #[test]
    fn layers_in_the_same_slot_keep_the_list_order() {
        let (mut doc, [a, b, c]) = three();
        // a keyed to slot 1 ties with b, which is at 1 by its place in the
        // list, and the list order breaks the tie
        doc.layers[0].order.set_key(0, 1.0, Interpolation::Step);
        assert_eq!(drawn(&doc, 5.0), [a, b, c]);
        doc.layers[2].order.set_key(0, 1.0, Interpolation::Step);
        assert_eq!(drawn(&doc, 5.0), [a, b, c]);
        // c below slot 0 goes behind everything
        doc.layers[2].order.set_key(0, -1.0, Interpolation::Step);
        assert_eq!(drawn(&doc, 5.0), [c, a, b]);
        // a value that is not a number cannot break the sort or lose a layer
        doc.layers[1].order.set_key(0, f32::NAN, Interpolation::Step);
        assert_eq!(drawn(&doc, 5.0).len(), 3);
    }

    #[test]
    fn the_children_of_a_group_are_ordered_too() {
        let (mut doc, [a, b, c]) = three();
        doc.layers[0].order.set_key(10, 5.0, Interpolation::Step);
        let children = std::mem::take(&mut doc.layers);
        let group = doc.new_layer("g", LayerKind::Group { children });
        doc.layers.push(group);
        // a's only key is on 10, held before it, so it is already in front
        assert_eq!(drawn(&doc, 0.0), [b, c, a]);
        assert_eq!(drawn(&doc, 20.0), [b, c, a]);
    }

    #[test]
    fn order_keys_are_listed_as_a_channel_and_looked_up_by_id() {
        use crate::scene::ChannelId;
        let (mut doc, _) = three();
        let layer = &mut doc.layers[0];
        assert!(!layer.channels().contains(&ChannelId::Order));
        assert!(!layer.is_animated());
        layer.order.set_key(3, 1.0, Interpolation::Step);
        assert!(layer.channels().contains(&ChannelId::Order));
        assert_eq!(layer.channel_label(ChannelId::Order), "Order");
        assert_eq!(layer.key_frames(), [3]);
        assert!(layer.is_animated());
        assert_eq!(
            layer.channel_keys(ChannelId::Order).unwrap().interp_at(3),
            Some(Interpolation::Step)
        );
        assert!(layer.channel_keys_mut(ChannelId::Order).unwrap().delete_key(3));
        assert!(!layer.is_animated());
        // a layer with no order keys writes nothing about order
        let json = serde_json::to_string(&doc.layers[1]).unwrap();
        assert!(!json.contains("order"));
        doc.layers[1].order.set_key(3, 1.0, Interpolation::Step);
        let json = serde_json::to_string(&doc.layers[1]).unwrap();
        let back: Layer = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc.layers[1]);
    }

    #[test]
    fn loops_are_reached_without_knowing_the_value_type() {
        use crate::LoopMode;
        use crate::scene::ChannelId;
        let (mut doc, _) = three();
        let layer = &mut doc.layers[0];
        assert_eq!(layer.channel_loop(ChannelId::Position), LoopMode::Off);
        // a channel with a single key cannot repeat, so it has no span yet
        layer
            .transform
            .position
            .set_key(2, Vec2::ZERO, Interpolation::Linear);
        assert!(layer.set_channel_loop(ChannelId::Position, LoopMode::Relative));
        assert_eq!(layer.channel_loop(ChannelId::Position), LoopMode::Relative);
        assert_eq!(layer.loop_span(ChannelId::Position), None);
        layer
            .transform
            .position
            .set_key(8, Vec2::X, Interpolation::Linear);
        assert_eq!(layer.loop_span(ChannelId::Position), Some((2, 8)));
        assert_eq!(layer.loop_span(ChannelId::Rotation), None);
        // point, colour and switch keys are not plain channels
        assert!(!layer.set_channel_loop(ChannelId::Shape(0), LoopMode::Absolute));
        assert!(!layer.set_channel_loop(ChannelId::Switch, LoopMode::Absolute));
        assert!(!layer.set_channel_loop(ChannelId::Bone(0), LoopMode::Absolute));
        assert!(!layer.set_channel_loop(ChannelId::CameraZoom, LoopMode::Absolute));
        assert_eq!(layer.channel_loop(ChannelId::Shape(0)), LoopMode::Off);
        // the camera's three channels
        assert!(
            doc.camera
                .set_channel_loop(ChannelId::CameraZoom, LoopMode::Absolute)
        );
        assert_eq!(doc.camera.channel_loop(ChannelId::CameraZoom), LoopMode::Absolute);
        assert_eq!(doc.camera.channel_loop(ChannelId::CameraRoll), LoopMode::Off);
        assert!(
            !doc.camera
                .set_channel_loop(ChannelId::Position, LoopMode::Absolute)
        );
        doc.camera.zoom.set_key(0, 1.0, Interpolation::Linear);
        doc.camera.zoom.set_key(6, 2.0, Interpolation::Linear);
        assert_eq!(doc.camera.loop_span(ChannelId::CameraZoom), Some((0, 6)));
        assert_eq!(doc.camera.loop_span(ChannelId::CameraRoll), None);
    }
}
