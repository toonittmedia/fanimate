//! Frame evaluation: turn a document at a frame into a flat draw list.

use std::borrow::Cow;

use glam::Affine2;

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
    /// Combined opacity of the shape's layer and all its ancestors.
    pub opacity: f32,
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

/// Evaluate `doc` at `frame` (which may be fractional, for motion blur or
/// smooth playback) into draw items, back to front.
pub fn evaluate(doc: &Document, frame: f32) -> Vec<DrawItem<'_>> {
    let mut out = Vec::new();
    Evaluator::default().evaluate_into(doc, frame, &mut out);
    out
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
    rest: Vec<Affine2>,
    angles: Vec<f32>,
    /// The bone layer whose action is being edited: its Smart Bones are not
    /// applied on top of the action's own keys.
    editing: Option<crate::scene::LayerId>,
    /// Leave rough sketches out, as renders do.
    pub skip_rough: bool,
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
            ..Self::default()
        }
    }

    pub fn evaluate_into<'a>(&mut self, doc: &'a Document, frame: f32, out: &mut Vec<DrawItem<'a>>) {
        out.clear();
        self.editing = doc.action_edit.as_ref().map(|e| e.layer);
        self.visit(&doc.layers, frame, Affine2::IDENTITY, 1.0, None, out);
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
        for layer in layers {
            if !layer.visible {
                continue;
            }
            let opacity = opacity * layer.opacity.sample(frame).clamp(0.0, 1.0);
            if opacity <= 0.0 {
                continue;
            }
            let local = layer.transform.sample(frame).to_affine();
            let world = world * local;
            // rel: this layer's space to the enclosing bone layer's space
            let rig = rig.map(|(r, rel)| (r, rel * local));

            match &layer.kind {
                LayerKind::Vector { shapes } => {
                    for (index, shape) in shapes.iter().enumerate() {
                        let item = self.shape_item(layer.id, index, None, shape, frame, world, opacity, rig);
                        out.push(item);
                    }
                }
                LayerKind::Frames { cels } => {
                    let Some(cel) = crate::frames::cel_at(cels, frame).map(|i| &cels[i]) else {
                        continue;
                    };
                    for (part, index, shape) in cel.shapes() {
                        if self.skip_rough && part == crate::frames::Part::Rough {
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
        }
    }
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
        // at rest an action changes nothing, so the shape is not copied
        if f.is_nan() || f == 0.0 {
            continue;
        }
        let Some(keys) = act.shape_keys(layer, index) else {
            continue;
        };
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
    if !shape.is_animated() {
        return Cow::Borrowed(shape);
    }
    match shape.path_at(frame) {
        Cow::Borrowed(p) if std::ptr::eq(p, &shape.path) => Cow::Borrowed(shape),
        path => Cow::Owned(Shape {
            name: shape.name.clone(),
            path: path.into_owned(),
            style: shape.style,
            bone: shape.bone,
            widths: shape.widths.clone(),
            path_keys: Vec::new(),
            skin: shape.skin.clone(),
            rect: shape.rect,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::Interpolation;
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
}
