//! The classical assist tools (M13): line clean-up, inbetween assist and
//! auto-rig.
//!
//! None of them uses a model or the network. Each makes ordinary shapes,
//! frames or bones the artist edits like any other, in one undo step, and
//! each marks the layer it changed or made with a provenance record
//! (`line-clean-up`, `inbetween-assist`, `auto-rig`) unless the layer already
//! has one, so a studio can tell what was assisted. Undoing is the way to
//! reject a result.
//!
//! The pure methods live in `fanimate_core` (`cleanup`, `inbetween`,
//! `autorig`); this is the editing around them: what is targeted, what is
//! skipped and why, and what the status line says.

use fanimate_core::autorig::{self, Piece};
use fanimate_core::cleanup::{self, CleanOptions};
use fanimate_core::frames::{Cel, Part, cel_at};
use fanimate_core::inbetween;
use fanimate_core::provenance;
use fanimate_core::{Frame, LayerId, LayerKind, Provenance, Vec2};

use super::{Editor, StatusKind, plural};

/// What line clean-up did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CleanSummary {
    /// Shapes refitted.
    pub shapes: usize,
    /// Shapes left as they were: already clean, animated, hiding edges or bound by a skin.
    pub skipped: usize,
    /// On-curve points before and after, over the shapes refitted.
    pub before: usize,
    pub after: usize,
}

/// What inbetween assist made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InbetweenSummary {
    /// The frame the new drawing starts on.
    pub frame: Frame,
    /// Shapes blended with a partner.
    pub blended: usize,
    /// Shapes with no partner, held as drawn.
    pub held: usize,
    /// Shapes of the later drawing that nothing matched, left out.
    pub left_out: usize,
}

/// What auto-rig made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RigSummary {
    /// The new bone layer.
    pub layer: LayerId,
    pub bones: usize,
    /// Layers bound to a bone.
    pub bound: usize,
    /// Layers whose names said nothing: left where they were, not in the rig.
    pub unbound: Vec<String>,
}

impl Editor {
    /// Mark `layer` as made or changed by a tool, unless it already says so.
    fn mark_assisted(&mut self, layer: LayerId, tool: &str) {
        if let Some(l) = self.doc.find_mut(layer)
            && l.provenance.is_none()
        {
            let mut record = Provenance::new(tool);
            record.version = env!("CARGO_PKG_VERSION").to_owned();
            record.date = provenance::today();
            l.provenance = Some(record);
        }
    }

    // ---- line clean-up ----------------------------------------------------

    /// Refit the lines of `layer`'s shapes (those in `only`, or all) with the
    /// fewest smooth curves that stay within `options.tolerance`. A shape that
    /// is animated by point keys, hides edges, or is bound by a skin is left
    /// alone (refitting would move keys or weights off their points); a brush
    /// stroke keeps its width profile. One undo step; nothing changes when no
    /// shape gets smaller.
    pub fn clean_up_lines(
        &mut self,
        layer: LayerId,
        only: Option<&[usize]>,
        options: CleanOptions,
    ) -> Result<CleanSummary, String> {
        if !self.is_drawable(layer) {
            return Err("Line clean-up works on a vector or frame-by-frame layer".into());
        }
        if self.is_locked(layer) {
            return Err("The layer is locked".into());
        }
        let shapes = self.layer_shapes(layer).ok_or("That layer has no shapes here")?;
        let indices: Vec<usize> = match only {
            Some(list) => list.iter().copied().filter(|&i| i < shapes.len()).collect(),
            None => (0..shapes.len()).collect(),
        };
        let mut summary = CleanSummary::default();
        let mut changes = Vec::new();
        for index in indices {
            let shape = &shapes[index];
            let fixed =
                !shape.path_keys.is_empty() || !shape.hidden_edge_flags.is_empty() || shape.skin.is_some();
            if fixed || shape.path.cmds.is_empty() {
                summary.skipped += 1;
                continue;
            }
            let (path, report) = cleanup::clean_path(&shape.path, options);
            if report.after >= report.before {
                summary.skipped += 1;
                continue;
            }
            let widths = match &shape.widths {
                Some(old) => match cleanup::resample_widths(&shape.path, old, &path) {
                    Some(w) => Some(w),
                    None => {
                        summary.skipped += 1;
                        continue;
                    }
                },
                None => None,
            };
            summary.shapes += 1;
            summary.before += report.before;
            summary.after += report.after;
            changes.push((index, path, widths));
        }
        if changes.is_empty() {
            return Ok(summary);
        }
        self.checkpoint("Clean up lines");
        if let Some(shapes) = self.layer_shapes_mut(layer) {
            for (index, path, widths) in changes {
                let shape = &mut shapes[index];
                shape.path = path;
                shape.widths = widths;
            }
        }
        self.mark_assisted(layer, "line-clean-up");
        self.dirty = true;
        self.points.clear();
        Ok(summary)
    }

    /// The Clean Up Lines command: the selected shapes, or else every shape
    /// of the selected layer.
    pub(super) fn clean_up_lines_command(&mut self) {
        let (layer, only) = match self.offset_targets() {
            Some((layer, shapes)) => (layer, Some(shapes)),
            None => match self.primary() {
                Some(layer) => (layer, None),
                None => {
                    self.set_status(StatusKind::Info, "Select a layer or shapes to clean up");
                    return;
                }
            },
        };
        let options = self.clean_options;
        match self.clean_up_lines(layer, only.as_deref(), options) {
            Ok(s) if s.shapes == 0 => self.set_status(
                StatusKind::Info,
                if s.skipped == 0 {
                    "Nothing to clean up here".to_owned()
                } else {
                    format!(
                        "Nothing to clean up: {} shape{} already clean or not changeable",
                        s.skipped,
                        plural(s.skipped)
                    )
                },
            ),
            Ok(s) => self.set_status(
                StatusKind::Success,
                format!(
                    "Cleaned {} shape{}: {} points became {}{}",
                    s.shapes,
                    plural(s.shapes),
                    s.before,
                    s.after,
                    if s.skipped > 0 {
                        format!(" ({} left as they were)", s.skipped)
                    } else {
                        String::new()
                    }
                ),
            ),
            Err(e) => self.set_status(StatusKind::Info, e),
        }
    }

    // ---- inbetween assist -------------------------------------------------

    /// Suggest a drawing between the one shown now and the next one of the
    /// frame-by-frame `layer`, and add it as a new drawing. At the first
    /// drawing's own frame it goes halfway between the two; on a later frame
    /// it goes on that frame. Shapes are paired by position and size and
    /// blended along arcs (see `fanimate_core::inbetween`); every drawing
    /// part is done. One undo step, and the playhead moves to the new drawing.
    pub fn suggest_inbetween(&mut self, layer: LayerId) -> Result<InbetweenSummary, String> {
        if self.is_locked(layer) {
            return Err("The layer is locked".into());
        }
        let here = self.current_frame();
        let Some(LayerKind::Frames { cels }) = self.doc.find(layer).map(|l| &l.kind) else {
            return Err("Inbetween assist works on a frame-by-frame layer".into());
        };
        let from = cel_at(cels, here as f32).ok_or("Go to a frame that shows a drawing")?;
        let (a, b) = match (cels.get(from), cels.get(from + 1)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err("There is no later drawing to go toward".into()),
        };
        if b.frame <= a.frame + 1 {
            return Err("There is no frame between these two drawings".into());
        }
        let frame = if here > a.frame {
            here
        } else {
            a.frame + (b.frame - a.frame) / 2
        };
        let t = (frame - a.frame) as f32 / (b.frame - a.frame) as f32;
        let mut cel = Cel::new(frame);
        let mut total = inbetween::Stats::default();
        for part in Part::ALL {
            let (shapes, stats) = inbetween::blend_shapes(a.part(part), b.part(part), t);
            *cel.part_mut(part) = shapes;
            total.blended += stats.blended;
            total.held += stats.held;
            total.left_out += stats.left_out;
        }
        if total.blended + total.held == 0 {
            return Err("The two drawings have nothing to blend".into());
        }
        self.checkpoint("Suggest inbetween");
        if let Some(LayerKind::Frames { cels }) = self.doc.find_mut(layer).map(|l| &mut l.kind) {
            cels.insert(from + 1, cel);
        }
        self.mark_assisted(layer, "inbetween-assist");
        self.dirty = true;
        self.points.clear();
        self.go_to(frame);
        Ok(InbetweenSummary {
            frame,
            blended: total.blended,
            held: total.held,
            left_out: total.left_out,
        })
    }

    pub(super) fn suggest_inbetween_command(&mut self) {
        let Some(layer) = self.primary() else {
            self.set_status(StatusKind::Info, "Select a frame-by-frame layer first");
            return;
        };
        match self.suggest_inbetween(layer) {
            Ok(s) => {
                let mut message = format!(
                    "Suggested an inbetween on frame {} ({} shape{} blended",
                    s.frame,
                    s.blended,
                    plural(s.blended)
                );
                if s.held > 0 {
                    message.push_str(&format!(", {} held", s.held));
                }
                if s.left_out > 0 {
                    message.push_str(&format!(", {} of the next drawing left out", s.left_out));
                }
                message.push_str("). Undo to reject it");
                self.set_status(StatusKind::Success, message);
            }
            Err(e) => self.set_status(StatusKind::Info, e),
        }
    }

    // ---- auto-rig ---------------------------------------------------------

    /// The box a vector layer's artwork fills, in the space of the layer's
    /// parent (the layer's own transform at the rest pose applied).
    fn artwork_box(&self, id: LayerId) -> Option<(Vec2, Vec2)> {
        let layer = self.doc.find(id)?;
        let LayerKind::Vector { shapes } = &layer.kind else {
            return None;
        };
        let to_parent = layer.transform.sample(0.0).to_affine();
        let mut points = shapes
            .iter()
            .filter(|s| s.visible)
            .flat_map(|s| s.path.points())
            .map(|p| to_parent.transform_point2(p));
        let first = points.next()?;
        Some(points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Build a skeleton for the vector `layers` from their names and places
    /// (see `fanimate_core::autorig`), bind each layer to the bone of its
    /// part, and put those layers in a new bone layer where the lowest one was
    /// (a layer named like no body part stays where it is). All
    /// the layers must sit in the same list. A layer that is locked, or has
    /// no artwork, stops the command with a reason. One undo step.
    pub fn auto_rig(&mut self, layers: &[LayerId]) -> Result<RigSummary, String> {
        let first = *layers.first().ok_or("Select the layers of a character first")?;
        let (parent, _) = self.doc.locate(first).ok_or("That layer is gone")?;
        let mut ordered: Vec<(usize, LayerId)> = Vec::new();
        for &id in layers {
            let (p, index) = self.doc.locate(id).ok_or("A selected layer is gone")?;
            if p != parent {
                return Err("The layers must all be in the same group".into());
            }
            if self.is_locked(id) {
                return Err("A selected layer is locked".into());
            }
            if !ordered.iter().any(|(_, i)| *i == id) {
                ordered.push((index, id));
            }
        }
        ordered.sort_by_key(|(index, _)| *index);
        let mut pieces = Vec::new();
        let mut used = Vec::new();
        for (_, id) in &ordered {
            let layer = self.doc.find(*id).ok_or("A selected layer is gone")?;
            if !matches!(layer.kind, LayerKind::Vector { .. }) {
                return Err(format!(
                    "\"{}\" is not a vector layer: auto-rig reads vector layers",
                    layer.name
                ));
            }
            let Some((min, max)) = self.artwork_box(*id) else {
                return Err(format!("\"{}\" has no artwork to place a bone by", layer.name));
            };
            pieces.push(Piece {
                name: layer.name.clone(),
                min,
                max,
            });
            used.push(*id);
        }
        let plan = autorig::plan(&pieces)?;
        let skeleton = plan.skeleton();
        let bones = skeleton.bones.len();
        let unbound: Vec<String> = plan.unbound(&pieces).into_iter().map(str::to_owned).collect();
        let bound = plan.binding.iter().flatten().count();

        self.checkpoint("Auto-rig");
        // the rig goes where its highest layer was; layers named like no body part stay put
        let at = ordered
            .iter()
            .zip(&plan.binding)
            .filter(|(_, bone)| bone.is_some())
            .map(|((index, _), _)| *index)
            .min()
            .unwrap_or(0);
        let mut children = Vec::new();
        for (id, bone) in used.iter().zip(&plan.binding) {
            if bone.is_none() {
                continue;
            }
            if let Some(mut layer) = self.doc.remove(*id) {
                if let (Some(bone), LayerKind::Vector { shapes }) = (bone, &mut layer.kind) {
                    for shape in shapes.iter_mut().filter(|s| s.skin.is_none()) {
                        shape.bone = Some(*bone);
                    }
                }
                children.push(layer);
            }
        }
        let name = format!("{} rig", children.last().map_or("Character", |_| "Character"));
        let mut rig = self.doc.new_layer(name, LayerKind::Bone { skeleton, children });
        rig.label = Some(fanimate_core::Label::for_id(rig.id));
        let id = rig.id;
        self.doc.insert(parent, at, rig);
        self.mark_assisted(id, "auto-rig");
        self.dirty = true;
        self.points.clear();
        self.select(id, super::SelectMode::Replace);
        Ok(RigSummary {
            layer: id,
            bones,
            bound,
            unbound,
        })
    }

    pub(super) fn auto_rig_command(&mut self) {
        let layers: Vec<LayerId> = if self.selection.is_empty() {
            self.doc
                .layers
                .iter()
                .filter(|l| matches!(l.kind, LayerKind::Vector { .. }))
                .map(|l| l.id)
                .collect()
        } else {
            self.selection.clone()
        };
        match self.auto_rig(&layers) {
            Ok(s) => {
                let mut message = format!("Auto-rigged: {} bones, {} layers bound", s.bones, s.bound);
                if !s.unbound.is_empty() {
                    message.push_str(&format!(
                        "; left out of the rig (no body part in the name): {}",
                        s.unbound.join(", ")
                    ));
                }
                self.set_status(StatusKind::Success, message);
            }
            Err(e) => self.set_status(StatusKind::Info, e),
        }
    }
}

#[cfg(test)]
#[path = "assist_tests.rs"]
mod tests;
