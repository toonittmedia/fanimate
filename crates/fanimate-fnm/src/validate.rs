//! Project validator and self-repair.
//!
//! Catches the kinds of damage that silently corrupt or slow down projects
//! in other tools (pain point P13 in the product vision): keyframes on
//! negative frames, duplicate or unsorted keys, broken bone references,
//! duplicate layer ids and non-finite numbers.
//!
//! [`validate`] only reports. [`repair`] fixes everything it safely can and
//! marks each fixed issue as repaired; anything left is for the user.

use std::collections::HashSet;
use std::fmt;

use fanimate_core::anim::Animatable;
use fanimate_core::{Channel, Document, Interpolation, Key, Layer, LayerId, LayerKind, LoopMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing; the project still behaves correctly.
    Warning,
    /// The project is damaged and may misbehave until fixed.
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub severity: Severity,
    /// Where the problem is, e.g. `layer 3 "arm" > transform.rotation`.
    pub location: String,
    pub message: String,
    pub repaired: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub issues: Vec<Issue>,
}

impl Report {
    /// No issues at all.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    /// Errors that were not repaired.
    pub fn unresolved_errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error && !i.repaired)
    }

    pub fn repaired_count(&self) -> usize {
        self.issues.iter().filter(|i| i.repaired).count()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.issues.is_empty() {
            return f.write_str("no issues");
        }
        for i in &self.issues {
            let sev = match i.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            let fixed = if i.repaired { " (repaired)" } else { "" };
            writeln!(f, "{sev}: {}: {}{fixed}", i.location, i.message)?;
        }
        Ok(())
    }
}

/// The highest layer id a sound project holds. No project makes a billion
/// layers, and the room above keeps new ids from overflowing.
pub const MAX_LAYER_ID: u32 = 1 << 30;

/// The most guides a project may hold. Nobody places this many.
pub const MAX_GUIDES: usize = 1000;

/// The farthest a layer's pivot may be from its origin, in layer units.
const PIVOT_LIMIT: f32 = 1.0e7;

/// Check a document without changing it.
pub fn validate(doc: &Document) -> Report {
    // The checks share one code path with repair; run it on a scratch copy.
    let mut scratch = doc.clone();
    Checker::new(false).run(&mut scratch)
}

/// Check a document and fix everything that can be fixed safely.
pub fn repair(doc: &mut Document) -> Report {
    Checker::new(true).run(doc)
}

struct Checker {
    repair: bool,
    report: Report,
    /// Names of the pictures the document holds.
    media: std::collections::BTreeSet<String>,
}

impl Checker {
    fn new(repair: bool) -> Self {
        Self {
            repair,
            report: Report::default(),
            media: Default::default(),
        }
    }

    fn issue(&mut self, severity: Severity, location: &str, message: String, fixable: bool) {
        self.report.issues.push(Issue {
            severity,
            location: location.to_owned(),
            message,
            repaired: fixable && self.repair,
        });
    }

    fn run(mut self, doc: &mut Document) -> Report {
        self.media = doc.media.keys().cloned().collect();
        self.document(doc);
        self.layer_ids(doc);
        let end = doc.end;
        let mut layers = std::mem::take(&mut doc.layers);
        let siblings = layers.len();
        for layer in &mut layers {
            self.layer(layer, end, None, siblings);
        }
        doc.layers = layers;
        self.report
    }

    fn document(&mut self, doc: &mut Document) {
        let loc = "document";
        if !(doc.fps.is_finite() && doc.fps > 0.0) {
            self.issue(
                Severity::Error,
                loc,
                format!("frame rate {} is not positive", doc.fps),
                true,
            );
            if self.repair {
                doc.fps = 24.0;
            }
        }
        if !(doc.width.is_finite() && doc.width > 0.0 && doc.height.is_finite() && doc.height > 0.0) {
            self.issue(
                Severity::Error,
                loc,
                format!("canvas size {}x{} is not positive", doc.width, doc.height),
                true,
            );
            if self.repair {
                doc.width = 1920.0;
                doc.height = 1080.0;
            }
        }
        if doc.start < 0 {
            self.issue(
                Severity::Error,
                loc,
                format!("start frame {} is negative", doc.start),
                true,
            );
            if self.repair {
                // frames count from 1; frame 0 is the rest pose, not part of the range
                doc.start = 1;
            }
        }
        if doc.end < doc.start {
            self.issue(
                Severity::Error,
                loc,
                format!("end frame {} is before start frame {}", doc.end, doc.start),
                true,
            );
            if self.repair {
                doc.end = doc.start;
            }
        }
        // the camera's channels are checked like a layer's
        let end = doc.end;
        self.channel(&mut doc.camera.position, loc, "camera.position", end);
        self.channel(&mut doc.camera.zoom, loc, "camera.zoom", end);
        self.channel(&mut doc.camera.roll, loc, "camera.roll", end);
        let range = fanimate_core::CAMERA_ZOOM;
        let outside = std::iter::once(doc.camera.zoom.base)
            .chain(doc.camera.zoom.keys.iter().map(|k| k.value))
            .any(|z| z.is_finite() && !range.contains(&z));
        if outside {
            self.issue(
                Severity::Warning,
                loc,
                format!(
                    "the camera's zoom is outside {} to {}; it is held within",
                    range.start(),
                    range.end()
                ),
                false,
            );
        }
        // a guide that is not a place on the canvas, or one too many
        let bad = doc.guides.iter().filter(|g| !g.position.is_finite()).count();
        if bad > 0 {
            self.issue(
                Severity::Warning,
                loc,
                format!("{bad} guide(s) have no position; they are removed"),
                true,
            );
        }
        if doc.guides.len() - bad > MAX_GUIDES {
            self.issue(
                Severity::Warning,
                loc,
                format!("more than {MAX_GUIDES} guides; the extra ones are removed"),
                true,
            );
        }
        if self.repair {
            doc.guides.retain(|g| g.position.is_finite());
            doc.guides.truncate(MAX_GUIDES);
        }
        // saved styles: usable numbers, a name each, no repeats, not too many
        let mut names = HashSet::new();
        let mut kept: Vec<fanimate_core::StylePreset> = Vec::new();
        for style in &doc.styles {
            let name = fanimate_core::StylePreset::tidy_name(&style.name);
            if style.is_sound()
                && !name.is_empty()
                && kept.len() < fanimate_core::MAX_STYLES
                && names.insert(name.clone())
            {
                kept.push(fanimate_core::StylePreset {
                    name,
                    ..style.clone()
                });
            }
        }
        if kept != doc.styles {
            self.issue(
                Severity::Warning,
                loc,
                format!(
                    "{} saved style(s) are unusable, repeated or too many; they are removed or renamed",
                    doc.styles.len().saturating_sub(kept.len()).max(1)
                ),
                true,
            );
            if self.repair {
                doc.styles = kept;
            }
        }
    }

    /// Layer ids must be unique and at most [`MAX_LAYER_ID`], and
    /// `next_layer_id` must be above all of them (and at most one more).
    fn layer_ids(&mut self, doc: &mut Document) {
        let mut seen = HashSet::new();
        let mut dupes = Vec::new();
        let mut huge = Vec::new();
        let mut max = 0;
        doc.walk(|l| {
            if l.id.0 > MAX_LAYER_ID {
                huge.push(l.id);
            } else {
                max = max.max(l.id.0);
            }
            if !seen.insert(l.id) {
                dupes.push(l.id);
            }
        });
        if doc.next_layer_id <= max || doc.next_layer_id > MAX_LAYER_ID + 1 {
            self.issue(
                Severity::Error,
                "document",
                format!(
                    "next layer id {} is not just above the highest id {max}",
                    doc.next_layer_id
                ),
                true,
            );
        }
        for id in &huge {
            self.issue(
                Severity::Error,
                &format!("layer {}", id.0),
                "layer id is too large".into(),
                true,
            );
        }
        for id in dupes.iter().filter(|id| id.0 <= MAX_LAYER_ID) {
            self.issue(
                Severity::Error,
                &format!("layer {}", id.0),
                "layer id is used more than once".into(),
                true,
            );
        }
        if self.repair {
            // keep the first use of each sound id, renumber the rest
            let mut seen = HashSet::new();
            let mut next = max + 1;
            doc.walk_mut(|l| {
                if l.id.0 > MAX_LAYER_ID || !seen.insert(l.id) {
                    l.id = LayerId(next);
                    next += 1;
                }
            });
            if !(max + 1..=MAX_LAYER_ID + 1).contains(&doc.next_layer_id) || doc.next_layer_id < next {
                doc.next_layer_id = next;
            }
        }
    }

    /// `bones`: bone count of the enclosing bone layer, if any. `siblings`:
    /// how many layers share the list this one is in, counting itself.
    /// A text layer: its settings within range, a font that exists, a sane
    /// number of animators, and each animator's values and keys like any other
    /// channel's.
    fn text(&mut self, text: &mut fanimate_core::Text, loc: &str, end: i32) {
        if !text.look().is_sound() {
            self.issue(
                Severity::Warning,
                loc,
                "the text's size, weight, spacing, wrapping, language, features, colour or path are outside what is allowed; they are held within it".into(),
                true,
            );
            if self.repair {
                text.look_mut().repair();
            }
        }
        if !text.look().font_is_available() {
            self.issue(
                Severity::Warning,
                loc,
                format!(
                    "the font \"{}\" is not installed on this computer and is not bundled; the text is set in {}",
                    text.look().family,
                    fanimate_core::text::DEFAULT_FONT
                ),
                false,
            );
        }
        if text.animators.len() > fanimate_core::text::MAX_ANIMATORS {
            self.issue(
                Severity::Warning,
                loc,
                format!(
                    "{} animators; only the first {} are used",
                    text.animators.len(),
                    fanimate_core::text::MAX_ANIMATORS
                ),
                true,
            );
            if self.repair {
                text.animators.truncate(fanimate_core::text::MAX_ANIMATORS);
            }
        }
        self.channel(&mut text.path_offset, loc, "path offset", end);
        for (i, a) in text.animators.iter_mut().enumerate() {
            let aloc = format!("{loc} > animator {i} \"{}\"", a.name);
            self.channel(&mut a.selector.start, &aloc, "range start", end);
            self.channel(&mut a.selector.end, &aloc, "range end", end);
            self.channel(&mut a.selector.offset, &aloc, "range offset", end);
            self.channel(&mut a.position, &aloc, "position", end);
            self.channel(&mut a.rotation, &aloc, "rotation", end);
            self.channel(&mut a.scale, &aloc, "scale", end);
            self.channel(&mut a.opacity, &aloc, "opacity", end);
            if let Some(fill) = &mut a.fill {
                self.channel(fill, &aloc, "fill", end);
            }
            self.channel(&mut a.anchor, &aloc, "anchor", end);
            self.channel(&mut a.tracking, &aloc, "tracking", end);
            self.channel(&mut a.stroke_width, &aloc, "stroke width", end);
            if let Some(c) = &mut a.stroke_color {
                self.channel(c, &aloc, "stroke colour", end);
            }
        }
    }

    /// Every bone needs an id of its own, and the counter has to be above them
    /// all, or a script holding an id could find another bone (version 7).
    fn bone_ids(&mut self, skeleton: &mut fanimate_core::Skeleton, loc: &str) {
        if skeleton.ids_are_sound() {
            return;
        }
        self.issue(
            Severity::Error,
            loc,
            "bones share an id, have none or have one out of range; they get new ids".into(),
            true,
        );
        if self.repair {
            skeleton.ensure_ids();
        }
    }

    fn layer(&mut self, layer: &mut Layer, end: i32, bones: Option<usize>, siblings: usize) {
        let loc = format!("layer {} \"{}\"", layer.id.0, layer.name);
        self.channel(&mut layer.opacity, &loc, "opacity", end);
        self.order(layer, &loc, end, siblings);
        if layer.opacity.base < 0.0 || layer.opacity.base > 1.0 {
            self.issue(
                Severity::Warning,
                &loc,
                format!(
                    "opacity {} is outside 0..1 and will be clamped",
                    layer.opacity.base
                ),
                false,
            );
        }
        self.channel(&mut layer.transform.position, &loc, "transform.position", end);
        self.channel(&mut layer.transform.rotation, &loc, "transform.rotation", end);
        self.channel(&mut layer.transform.scale, &loc, "transform.scale", end);
        if let Some(pivot) = layer.transform.pivot
            && !(pivot.is_finite() && pivot.abs().max_element() <= PIVOT_LIMIT)
        {
            self.issue(
                Severity::Error,
                &loc,
                format!(
                    "transform.pivot {pivot} is not a point in the layer; the layer turns about its origin"
                ),
                true,
            );
            if self.repair {
                layer.transform.pivot = None;
            }
        }
        // the record of what made the layer is kept, never dropped, so that
        // generated content stays labelled; only what is unusable in it is fixed
        if let Some(provenance) = &mut layer.provenance
            && !provenance.is_sound()
        {
            self.issue(
                Severity::Warning,
                &loc,
                "the record of what made this layer is incomplete or too long".into(),
                true,
            );
            if self.repair {
                provenance.repair();
            }
        }

        match &mut layer.kind {
            LayerKind::Vector { shapes } => {
                for (i, shape) in shapes.iter_mut().enumerate() {
                    let sloc = format!("{loc} > shape {i} \"{}\"", shape.name);
                    self.shape(shape, &sloc, end, bones);
                }
            }
            LayerKind::Frames { cels } => {
                let mut cels_keys = std::mem::take(cels);
                self.cels(&mut cels_keys, &loc);
                for cel in &mut cels_keys {
                    let at = cel.frame;
                    for part in fanimate_core::Part::ALL {
                        for (i, shape) in cel.part_mut(part).iter_mut().enumerate() {
                            let sloc = format!(
                                "{loc} > drawing on frame {} > {} shape {i} \"{}\"",
                                at,
                                part.label(),
                                shape.name
                            );
                            self.shape(shape, &sloc, end, bones);
                        }
                    }
                }
                *cels = cels_keys;
            }
            LayerKind::Group { children } => {
                let siblings = children.len();
                for child in children {
                    self.layer(child, end, bones, siblings);
                }
            }
            LayerKind::Image(img) => self.image(img, &loc, bones),
            LayerKind::Switch { children, switch } => {
                self.keys(switch, |_| true, &format!("{loc} > switch keys"), end);
                let siblings = children.len();
                for child in children {
                    self.layer(child, end, bones, siblings);
                }
            }
            LayerKind::Text(text) => self.text(text, &loc, end),
            LayerKind::Audio(audio) => {
                self.media_file(&audio.media, &loc, "sound");
                if !(audio.volume.is_finite() && audio.volume >= 0.0) {
                    self.issue(
                        Severity::Error,
                        &loc,
                        format!("volume {} is not valid", audio.volume),
                        true,
                    );
                    if self.repair {
                        audio.volume = 1.0;
                    }
                }
            }
            LayerKind::Bone { skeleton, children } => {
                self.bone_ids(skeleton, &loc);
                for i in 0..skeleton.bones.len() {
                    let bone = &mut skeleton.bones[i];
                    let bloc = format!("{loc} > bone {i} \"{}\"", bone.name);
                    if let Some(p) = bone.parent.filter(|&p| p >= i) {
                        self.issue(
                            Severity::Error,
                            &bloc,
                            format!("parent {p} does not come before this bone; it is treated as a root"),
                            true,
                        );
                        if self.repair {
                            bone.parent = None;
                        }
                    }
                    if !(bone.origin.is_finite() && bone.length.is_finite()) {
                        self.issue(
                            Severity::Error,
                            &bloc,
                            "origin or length is not a finite number".into(),
                            true,
                        );
                        if self.repair {
                            if !bone.origin.is_finite() {
                                bone.origin = fanimate_core::Vec2::ZERO;
                            }
                            if !bone.length.is_finite() {
                                bone.length = 0.0;
                            }
                        }
                    }
                    if bone
                        .limits
                        .is_some_and(|[lo, hi]| !(lo.is_finite() && hi.is_finite() && lo <= hi))
                    {
                        self.issue(
                            Severity::Error,
                            &bloc,
                            "angle limits are not a valid range; they are removed".into(),
                            true,
                        );
                        if self.repair {
                            bone.limits = None;
                        }
                    }
                    if bone.strength.is_some_and(|s| !(s.is_finite() && s > 0.0)) {
                        self.issue(
                            Severity::Warning,
                            &bloc,
                            "strength is not a positive number; the default is used".into(),
                            true,
                        );
                        if self.repair {
                            bone.strength = None;
                        }
                    }
                    let bone = &mut skeleton.bones[i];
                    let mut angle = std::mem::replace(&mut bone.angle, Channel::new(0.0));
                    self.channel(&mut angle, &bloc, "angle", end);
                    skeleton.bones[i].angle = angle;
                }
                let n = skeleton.bones.len();
                let mut actions = std::mem::take(&mut skeleton.actions);
                for (a, act) in actions.iter_mut().enumerate() {
                    let aloc = format!("{loc} > action {a} \"{}\"", act.name);
                    self.action(act, &aloc, n);
                }
                // an action whose driver is missing cannot play; the actions after it move up one,
                // so controllers holding them are renumbered
                let kept: Vec<bool> = actions
                    .iter()
                    .map(|act| act.driver.is_none_or(|d| d < n))
                    .collect();
                if self.repair {
                    let mut at = 0;
                    let renumber: Vec<Option<usize>> = kept
                        .iter()
                        .map(|k| {
                            k.then(|| {
                                at += 1;
                                at - 1
                            })
                        })
                        .collect();
                    actions.retain(|act| act.driver.is_none_or(|d| d < n));
                    for c in &mut skeleton.controllers {
                        for link in &mut c.links {
                            if let Some(Some(new)) = renumber.get(link.action) {
                                link.action = *new;
                            } else {
                                link.action = usize::MAX;
                            }
                        }
                    }
                }
                let action_count = actions.len();
                skeleton.actions = actions;
                self.controllers(&mut skeleton.controllers, &loc, action_count, end);
                let siblings = children.len();
                for child in children {
                    self.layer(child, end, Some(n), siblings);
                }
            }
        }
    }
    /// A stroke's width profile: one finite, non-negative value per point.
    fn widths(&mut self, shape: &mut fanimate_core::Shape, loc: &str) {
        let points = shape.path.on_curve_count();
        let Some(widths) = &mut shape.widths else { return };
        if widths.len() != points {
            self.issue(
                Severity::Error,
                loc,
                format!(
                    "width profile has {} values for {points} points; the stroke is drawn at one width",
                    widths.len()
                ),
                true,
            );
            if self.repair {
                shape.widths = None;
            }
            return;
        }
        let bad = widths.iter().filter(|w| !(w.is_finite() && **w >= 0.0)).count();
        if bad > 0 {
            self.issue(
                Severity::Error,
                loc,
                format!("width profile has {bad} negative or non-finite value(s)"),
                true,
            );
            if self.repair {
                for w in widths.iter_mut() {
                    if !w.is_finite() {
                        *w = 1.0;
                    } else if *w < 0.0 {
                        *w = 0.0;
                    }
                }
            }
        }
    }

    /// A shape: its bone binding, points, width profile, point keys and
    /// weights.
    fn shape(&mut self, shape: &mut fanimate_core::Shape, sloc: &str, end: i32, bones: Option<usize>) {
        let problem = match (shape.bone, bones) {
            (None, _) => None,
            (Some(_), None) => Some("is bound to a bone but is not inside a bone layer".to_owned()),
            (Some(b), Some(n)) if b >= n => Some(format!("is bound to bone {b}, but the skeleton has {n}")),
            (Some(_), Some(_)) => None,
        };
        if let Some(p) = problem {
            self.issue(
                Severity::Warning,
                sloc,
                format!("{p}; it will not move with the rig"),
                true,
            );
            if self.repair {
                shape.bone = None;
            }
        }
        if shape.path.points().any(|p| !p.is_finite()) {
            // an infinite coordinate would be written as null and the project
            // could never be saved, so it moves to the origin
            self.issue(
                Severity::Error,
                sloc,
                "path has points that are not numbers; they move to the origin".into(),
                true,
            );
            if self.repair {
                use fanimate_core::{PathCmd, Vec2};
                let fix = |p: Vec2| if p.is_finite() { p } else { Vec2::ZERO };
                for cmd in &mut shape.path.cmds {
                    *cmd = match *cmd {
                        PathCmd::MoveTo(p) => PathCmd::MoveTo(fix(p)),
                        PathCmd::LineTo(p) => PathCmd::LineTo(fix(p)),
                        PathCmd::CubicTo(a, b, p) => PathCmd::CubicTo(fix(a), fix(b), fix(p)),
                        PathCmd::Close => PathCmd::Close,
                    };
                }
            }
        }
        if let Some(rect) = shape.rect
            && !rect.is_valid()
        {
            self.issue(
                Severity::Warning,
                sloc,
                "rectangle settings are not a rectangle; the shape is an ordinary path".into(),
                true,
            );
            if self.repair {
                shape.rect = None;
            }
        }
        self.hidden_edges(shape, sloc);
        self.widths(shape, sloc);
        self.path_keys(shape, sloc, end);
        self.color_keys(shape, sloc, end);
        self.exposure(shape, sloc, end);
        self.skin(shape, sloc, bones);
    }

    /// Hidden edges: one flag for every command of the path, and none on a
    /// move, which draws nothing. Flags that do not fit show every edge.
    fn hidden_edges(&mut self, shape: &mut fanimate_core::Shape, loc: &str) {
        use fanimate_core::PathCmd;
        if shape.hidden_edge_flags.is_empty() {
            return;
        }
        if shape.hidden_edge_flags.len() != shape.path.cmds.len() {
            self.issue(
                Severity::Warning,
                loc,
                format!(
                    "{} hidden-edge flags for {} path commands; every edge is shown",
                    shape.hidden_edge_flags.len(),
                    shape.path.cmds.len()
                ),
                true,
            );
            if self.repair {
                shape.hidden_edge_flags.clear();
            }
            return;
        }
        let on_a_move = shape
            .hidden_edge_flags
            .iter()
            .zip(&shape.path.cmds)
            .any(|(hidden, cmd)| *hidden && matches!(cmd, PathCmd::MoveTo(_)));
        if on_a_move {
            self.issue(
                Severity::Warning,
                loc,
                "a hidden edge is on a move, which draws nothing; it is shown".into(),
                true,
            );
            if self.repair {
                for (hidden, cmd) in shape.hidden_edge_flags.iter_mut().zip(&shape.path.cmds) {
                    if matches!(cmd, PathCmd::MoveTo(_)) {
                        *hidden = false;
                    }
                }
            }
        }
    }

    /// Colour keys: ordered, on frames that are not negative, finite, and
    /// only where there is a fill or a stroke to colour.
    fn color_keys(&mut self, shape: &mut fanimate_core::Shape, loc: &str, end: i32) {
        if shape.fill_keys.is_empty() && shape.stroke_keys.is_empty() {
            return;
        }
        let colour = |c: &fanimate_core::Color| [c.r, c.g, c.b, c.a].iter().all(|v| v.is_finite());
        self.keys(&mut shape.fill_keys, colour, &format!("{loc} > fill_keys"), end);
        self.keys(
            &mut shape.stroke_keys,
            colour,
            &format!("{loc} > stroke_keys"),
            end,
        );
        if shape.style.fill.is_none() && !shape.fill_keys.is_empty() {
            self.issue(
                Severity::Warning,
                loc,
                "has fill colour keys but no fill; the keys are removed".into(),
                true,
            );
        }
        if shape.style.stroke.is_none() && !shape.stroke_keys.is_empty() {
            self.issue(
                Severity::Warning,
                loc,
                "has stroke colour keys but no stroke; the keys are removed".into(),
                true,
            );
        }
        if self.repair {
            shape.drop_orphan_color_keys();
        }
    }

    /// Stroke exposure: keys like any, and a start and end that are numbers
    /// from 0 to 1 (outside that they are held within it).
    fn exposure(&mut self, shape: &mut fanimate_core::Shape, loc: &str, end: i32) {
        use fanimate_core::Vec2;
        if !shape.is_trimmed() {
            return;
        }
        let finite = |v: &Vec2| v.is_finite();
        self.keys(&mut shape.trim_keys, finite, &format!("{loc} > trim_keys"), end);
        let within = |v: &Vec2| (0.0..=1.0).contains(&v.x) && (0.0..=1.0).contains(&v.y);
        let held = |v: Vec2| {
            let one = |c: f32| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 };
            Vec2::new(one(v.x), one(v.y))
        };
        let outside = !within(&shape.trim) || shape.trim_keys.iter().any(|k| !within(&k.value));
        if outside {
            self.issue(
                Severity::Warning,
                loc,
                "stroke exposure is outside 0 to 1; it is held within".into(),
                true,
            );
            if self.repair {
                shape.trim = held(shape.trim);
                for key in &mut shape.trim_keys {
                    key.value = held(key.value);
                }
            }
        }
    }

    /// Drawings: on frames that are not negative, in order, one per frame.
    fn cels(&mut self, cels: &mut Vec<fanimate_core::Cel>, loc: &str) {
        let negative = cels.iter().filter(|c| c.frame < 0).count();
        if negative > 0 {
            self.issue(
                Severity::Error,
                loc,
                format!("{negative} drawing(s) on negative frames; they are removed"),
                true,
            );
            if self.repair {
                cels.retain(|c| c.frame >= 0);
            }
        }
        let sorted = cels.windows(2).all(|w| w[0].frame < w[1].frame);
        if !sorted {
            self.issue(
                Severity::Error,
                loc,
                "drawings are out of order or share a frame".into(),
                true,
            );
            if self.repair {
                // stable: of drawings on one frame, the later in the file wins
                cels.sort_by_key(|c| c.frame);
                let taken = std::mem::take(cels);
                for c in taken {
                    match cels.last_mut() {
                        Some(last) if last.frame == c.frame => *last = c,
                        _ => cels.push(c),
                    }
                }
            }
        }
    }

    /// An image layer: a picture the project holds, a sensible size and
    /// mesh, and bone references that exist.
    /// A media file reference: a safe name, for a file the project holds.
    fn media_file(&mut self, name: &str, loc: &str, what: &str) {
        if !crate::is_media_name(name) {
            self.issue(
                Severity::Error,
                loc,
                format!("{what} name {name:?} is not inside the project's media folder; it is not loaded"),
                false,
            );
        } else if !self.media.contains(name) {
            let shown = if what == "picture" {
                "the layer shows a grey placeholder"
            } else {
                "it is silent"
            };
            self.issue(
                Severity::Warning,
                loc,
                format!("{what} {name} is missing; {shown}"),
                false,
            );
        }
    }

    fn image(&mut self, img: &mut fanimate_core::Image, loc: &str, bones: Option<usize>) {
        self.media_file(&img.media, loc, "picture");
        if !(img.width.is_finite() && img.width > 0.0 && img.height.is_finite() && img.height > 0.0) {
            self.issue(
                Severity::Error,
                loc,
                format!("size {}x{} is not positive", img.width, img.height),
                true,
            );
            if self.repair {
                img.width = 100.0;
                img.height = 100.0;
            }
        }
        if img.grid != img.cells() {
            self.issue(
                Severity::Error,
                loc,
                format!("mesh of {}x{} cells is out of range", img.grid[0], img.grid[1]),
                true,
            );
            if self.repair {
                img.grid = img.cells();
                img.skin = None;
            }
        }
        let n = bones.unwrap_or(0);
        if img.bone.is_some_and(|b| b >= n) {
            self.issue(
                Severity::Warning,
                loc,
                "is bound to a bone that does not exist; it will not move with the rig".into(),
                true,
            );
            if self.repair {
                img.bone = None;
            }
        }
        if let Some(skin) = &img.skin {
            let bad = img.valid_skin().is_none()
                || bones.is_none()
                || skin
                    .iter()
                    .flatten()
                    .any(|(b, w)| *b >= n || !(w.is_finite() && *w >= 0.0));
            if bad {
                self.issue(
                    Severity::Error,
                    loc,
                    "mesh weights do not match the mesh or name missing bones; they are removed".into(),
                    true,
                );
                if self.repair {
                    img.skin = None;
                }
            }
        }
    }

    /// Master controllers: a sane number of them, with names, values that are numbers
    /// and links that point at actions that exist.
    fn controllers(
        &mut self,
        controllers: &mut Vec<fanimate_core::actions::Controller>,
        loc: &str,
        actions: usize,
        end: i32,
    ) {
        use fanimate_core::actions::{MAX_CONTROLLERS, MAX_LINKS};
        if controllers.len() > MAX_CONTROLLERS {
            self.issue(
                Severity::Warning,
                loc,
                format!(
                    "{} controllers; only the first {MAX_CONTROLLERS} are kept",
                    controllers.len()
                ),
                true,
            );
            if self.repair {
                controllers.truncate(MAX_CONTROLLERS);
            }
        }
        for (i, c) in controllers.iter_mut().enumerate() {
            let cloc = format!("{loc} > controller {i} \"{}\"", c.name);
            if c.name.trim().is_empty() || c.name.chars().count() > 200 {
                self.issue(
                    Severity::Warning,
                    &cloc,
                    "its name is empty or too long".into(),
                    true,
                );
                if self.repair {
                    c.name = format!("Controller {}", i + 1);
                }
            }
            self.channel(&mut c.x, &cloc, "x", end);
            self.channel(&mut c.y, &cloc, "y", end);
            let bad = c
                .links
                .iter()
                .filter(|l| l.action >= actions || !l.from.is_finite() || !l.to.is_finite())
                .count();
            if bad > 0 {
                self.issue(
                    Severity::Error,
                    &cloc,
                    format!("{bad} link(s) point at an action that does not exist or have a range that is not a number; they are removed"),
                    true,
                );
                if self.repair {
                    c.links
                        .retain(|l| l.action < actions && l.from.is_finite() && l.to.is_finite());
                }
            }
            if c.links.len() > MAX_LINKS {
                self.issue(
                    Severity::Warning,
                    &cloc,
                    format!("more than {MAX_LINKS} links; the rest are removed"),
                    true,
                );
                if self.repair {
                    c.links.truncate(MAX_LINKS);
                }
            }
        }
    }

    /// An action: its driver and bones exist, its keys are sound.
    fn action(&mut self, act: &mut fanimate_core::RigAction, loc: &str, bones: usize) {
        if act.driver.is_some_and(|d| d >= bones) {
            self.issue(
                Severity::Error,
                loc,
                "its Smart Bone does not exist; the action is removed".into(),
                true,
            );
        }
        if act.length < 1 {
            self.issue(
                Severity::Error,
                loc,
                format!("length {} is not positive", act.length),
                true,
            );
            if self.repair {
                act.length = 1;
            }
        }
        let missing = act.bones.iter().filter(|b| b.bone >= bones).count();
        if missing > 0 {
            self.issue(
                Severity::Error,
                loc,
                format!("animates {missing} bone(s) that do not exist; those keys are removed"),
                true,
            );
            if self.repair {
                act.bones.retain(|b| b.bone < bones);
            }
        }
        for b in &mut act.bones {
            let name = format!("bone {} angle", b.bone);
            self.channel(&mut b.angle, loc, &name, act.length);
        }
        for s in &mut act.shapes {
            let sloc = format!("{loc} > layer {} shape {}", s.layer.0, s.shape);
            self.keys(&mut s.path_keys, |p| p.is_finite(), &sloc, act.length);
        }
    }

    /// Smooth binding: one weight list per point, naming bones that exist,
    /// with finite, non-negative weights.
    fn skin(&mut self, shape: &mut fanimate_core::Shape, loc: &str, bones: Option<usize>) {
        let points = shape.path.on_curve_count();
        let Some(skin) = &mut shape.skin else { return };
        let Some(n) = bones else {
            self.issue(
                Severity::Warning,
                loc,
                "has bone weights but is not inside a bone layer; it will not move with a rig".into(),
                true,
            );
            if self.repair {
                shape.skin = None;
            }
            return;
        };
        if skin.len() != points {
            self.issue(
                Severity::Error,
                loc,
                format!(
                    "has bone weights for {} points but {points} points; the weights are removed",
                    skin.len()
                ),
                true,
            );
            if self.repair {
                shape.skin = None;
            }
            return;
        }
        let bad = skin
            .iter()
            .flatten()
            .filter(|(b, w)| *b >= n || !(w.is_finite() && *w >= 0.0))
            .count();
        if bad > 0 {
            self.issue(
                Severity::Error,
                loc,
                format!("{bad} bone weight(s) name a missing bone or are not a valid number"),
                true,
            );
            if self.repair {
                for point in skin.iter_mut() {
                    point.retain(|(b, w)| *b < n && w.is_finite() && *w >= 0.0);
                }
            }
        }
    }

    /// Point keys: checked like a channel's keys, and each must be made of
    /// the same commands as the shape's path to be drawn.
    fn path_keys(&mut self, shape: &mut fanimate_core::Shape, loc: &str, end: i32) {
        if shape.path_keys.is_empty() {
            return;
        }
        let loc_keys = format!("{loc} > path_keys");
        let mismatched = shape
            .path_keys
            .iter()
            .filter(|k| !k.value.same_structure(&shape.path))
            .count();
        if mismatched > 0 {
            self.issue(
                Severity::Error,
                &loc_keys,
                format!("{mismatched} point keyframe(s) do not match the shape's points"),
                true,
            );
            if self.repair {
                let path = &shape.path;
                shape.path_keys.retain(|k| k.value.same_structure(path));
            }
        }
        self.keys(&mut shape.path_keys, |p| p.is_finite(), &loc_keys, end);
    }

    fn channel<T: Animatable + Default>(&mut self, ch: &mut Channel<T>, loc: &str, name: &str, end: i32) {
        let loc = format!("{loc} > {name}");

        if !ch.base.is_finite() {
            self.issue(Severity::Error, &loc, "value is not a finite number".into(), true);
            if self.repair {
                ch.base = T::default();
            }
        }
        self.keys(&mut ch.keys, T::is_finite, &loc, end);

        // a loop repeats the span between the first and last key, so it
        // needs two; checked after the keys were, as repairs may remove some
        if !matches!(ch.loop_mode, LoopMode::Off) && ch.keys.len() < 2 {
            self.issue(
                Severity::Warning,
                &loc,
                format!(
                    "loops but has {} key(s); a loop needs two, so the loop is turned off",
                    ch.keys.len()
                ),
                true,
            );
            if self.repair {
                ch.loop_mode = LoopMode::Off;
            }
        }
    }

    /// A layer's order channel: keys like any other, whose values are a slot
    /// among the layers that share its list. A slot outside the list is
    /// held within it when drawn, so it needs no repair.
    fn order(&mut self, layer: &mut Layer, loc: &str, end: i32, siblings: usize) {
        self.channel(&mut layer.order, loc, "order", end);
        let last = siblings.saturating_sub(1) as f32;
        let beyond = layer
            .order
            .keys
            .iter()
            .filter(|k| k.value.is_finite() && !(0.0..=last).contains(&k.value))
            .count();
        if beyond > 0 {
            self.issue(
                Severity::Warning,
                &format!("{loc} > order"),
                format!(
                    "{beyond} slot(s) are outside the {siblings} layer(s) in the list; they are held within"
                ),
                false,
            );
        }
    }

    fn keys<T>(&mut self, keys: &mut Vec<Key<T>>, finite: impl Fn(&T) -> bool, loc: &str, end: i32) {
        let negative = keys.iter().filter(|k| k.frame < 0).count();
        if negative > 0 {
            self.issue(
                Severity::Error,
                loc,
                format!("{negative} keyframe(s) on negative frames"),
                true,
            );
            if self.repair {
                keys.retain(|k| k.frame >= 0);
            }
        }

        let bad = keys.iter().filter(|k| !finite(&k.value)).count();
        if bad > 0 {
            self.issue(
                Severity::Error,
                loc,
                format!("{bad} keyframe(s) with non-finite values"),
                true,
            );
            if self.repair {
                keys.retain(|k| finite(&k.value));
            }
        }

        // a custom curve with a control point out of range evaluates as if held
        // within it; repair makes the file say so
        let curves = keys
            .iter()
            .filter(|k| matches!(k.interp, Interpolation::Bezier(b) if !b.is_sound()))
            .count();
        if curves > 0 {
            self.issue(
                Severity::Warning,
                loc,
                format!("{curves} keyframe(s) with a curve outside the allowed range; it is held within"),
                true,
            );
            if self.repair {
                for k in keys.iter_mut() {
                    if let Interpolation::Bezier(b) = k.interp {
                        k.interp = Interpolation::Bezier(b.repaired());
                    }
                }
            }
        }

        if !keys.is_sorted_by_key(|k| k.frame) {
            self.issue(Severity::Error, loc, "keyframes are out of order".into(), true);
            if self.repair {
                // stable sort keeps the original order of same-frame keys,
                // so the later key wins in the dedupe below
                keys.sort_by_key(|k| k.frame);
            }
        }

        let mut frames: Vec<i32> = keys.iter().map(|k| k.frame).collect();
        frames.sort_unstable();
        let dupes = frames.windows(2).filter(|w| w[0] == w[1]).count();
        if dupes > 0 {
            self.issue(
                Severity::Error,
                loc,
                format!("{dupes} duplicate keyframe(s)"),
                true,
            );
            if self.repair {
                // keep the last key on each frame
                let taken = std::mem::take(keys);
                for k in taken {
                    match keys.last_mut() {
                        Some(last) if last.frame == k.frame => *last = k,
                        _ => keys.push(k),
                    }
                }
            }
        }

        let late = keys.iter().filter(|k| k.frame > end).count();
        if late > 0 {
            self.issue(
                Severity::Warning,
                loc,
                format!("{late} keyframe(s) after the last frame ({end})"),
                false,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{Interpolation, Key, samples};

    #[test]
    fn samples_are_clean() {
        assert!(validate(&samples::hello()).is_clean());
        assert!(validate(&samples::benchmark(40)).is_clean());
    }

    fn rigged() -> Document {
        samples::hello()
    }

    fn arm_angle(doc: &mut Document) -> &mut Channel<f32> {
        let LayerKind::Bone { skeleton, .. } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        &mut skeleton.bones[0].angle
    }

    #[test]
    fn negative_keyframes_are_reported_then_removed() {
        let mut doc = rigged();
        arm_angle(&mut doc).keys.insert(
            0,
            Key {
                frame: -40,
                value: 1.0,
                interp: Interpolation::Linear,
            },
        );
        let report = validate(&doc);
        assert_eq!(report.unresolved_errors().count(), 1);
        assert!(report.issues[0].message.contains("negative"));

        let fixed = repair(&mut doc);
        assert_eq!(fixed.repaired_count(), 1);
        assert!(arm_angle(&mut doc).keys.iter().all(|k| k.frame >= 0));
        assert!(validate(&doc).is_clean());
    }

    #[test]
    fn unsorted_and_duplicate_keys_are_repaired_keeping_last() {
        let mut doc = rigged();
        let ch = arm_angle(&mut doc);
        ch.keys = vec![
            Key {
                frame: 10,
                value: 1.0,
                interp: Interpolation::Linear,
            },
            Key {
                frame: 0,
                value: 0.0,
                interp: Interpolation::Linear,
            },
            Key {
                frame: 10,
                value: 2.0,
                interp: Interpolation::Linear,
            },
        ];
        let report = repair(&mut doc);
        assert_eq!(report.unresolved_errors().count(), 0);
        let ch = arm_angle(&mut doc);
        let frames: Vec<_> = ch.keys.iter().map(|k| (k.frame, k.value)).collect();
        assert_eq!(frames, [(0, 0.0), (10, 2.0)]);
        assert!(validate(&doc).is_clean());
    }

    #[test]
    fn broken_bone_binding_is_unbound() {
        let mut doc = rigged();
        let LayerKind::Bone { children, .. } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        let LayerKind::Vector { shapes } = &mut children[0].kind else {
            unreachable!()
        };
        shapes[2].bone = Some(42);
        assert_eq!(validate(&doc).issues.len(), 1);
        repair(&mut doc);
        assert!(validate(&doc).is_clean());
    }

    fn stroke_doc(widths: Vec<f32>) -> Document {
        use fanimate_core::{Color, Path, Shape, Style, Vec2};
        let mut doc = Document::new("w", 100.0, 100.0, 24.0);
        let path = Path::new().move_to(Vec2::ZERO).line_to(Vec2::new(10.0, 0.0));
        let mut shape = Shape::new("ink", path, Style::default().with_stroke(Color::BLACK, 4.0));
        shape.widths = Some(widths);
        let layer = doc.new_layer("l", LayerKind::vector(vec![shape]));
        doc.layers.push(layer);
        doc
    }

    fn widths_of(doc: &Document) -> Option<Vec<f32>> {
        let LayerKind::Vector { shapes } = &doc.layers[0].kind else {
            unreachable!()
        };
        shapes[0].widths.clone()
    }

    #[test]
    fn width_profile_with_wrong_length_is_dropped() {
        let mut doc = stroke_doc(vec![1.0, 0.5, 0.2]);
        let report = repair(&mut doc);
        assert_eq!(report.issues.len(), 1, "{report}");
        assert!(report.issues[0].repaired);
        assert_eq!(widths_of(&doc), None);
        assert!(validate(&doc).is_clean());
    }

    #[test]
    fn points_that_are_not_numbers_move_to_the_origin_so_the_project_can_be_saved() {
        use fanimate_core::{PathCmd, Vec2};
        let mut doc = stroke_doc(vec![1.0, 1.0]);
        let LayerKind::Vector { shapes } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        shapes[0].path.cmds[1] = PathCmd::LineTo(Vec2::new(f32::INFINITY, 4.0));
        assert_eq!(validate(&doc).issues.len(), 1);
        let report = repair(&mut doc);
        assert!(report.issues.iter().all(|i| i.repaired), "{report}");
        assert!(validate(&doc).is_clean());
        let LayerKind::Vector { shapes } = &doc.layers[0].kind else {
            unreachable!()
        };
        assert_eq!(shapes[0].path.cmds[1], PathCmd::LineTo(Vec2::ZERO));
        // the point that was a number is kept
        assert_eq!(shapes[0].path.cmds[0], PathCmd::MoveTo(Vec2::ZERO));
    }

    #[test]
    fn hidden_edge_flags_must_fit_the_path_and_stay_off_moves() {
        let mut doc = stroke_doc(vec![1.0, 1.0]);
        let flags = |doc: &mut Document, f: Vec<bool>| {
            let LayerKind::Vector { shapes } = &mut doc.layers[0].kind else {
                unreachable!()
            };
            shapes[0].hidden_edge_flags = f;
        };
        let read = |doc: &Document| {
            let LayerKind::Vector { shapes } = &doc.layers[0].kind else {
                unreachable!()
            };
            shapes[0].hidden_edge_flags.clone()
        };
        // the path is a move and a line: two commands
        flags(&mut doc, vec![false, true]);
        assert!(validate(&doc).is_clean());
        flags(&mut doc, vec![false, true, true]);
        assert_eq!(validate(&doc).issues.len(), 1);
        repair(&mut doc);
        assert!(read(&doc).is_empty());
        assert!(validate(&doc).is_clean());
        flags(&mut doc, vec![true, true]);
        assert_eq!(validate(&doc).issues.len(), 1);
        repair(&mut doc);
        assert_eq!(read(&doc), [false, true]);
        assert!(validate(&doc).is_clean());
    }

    #[test]
    fn saved_styles_are_checked() {
        use fanimate_core::{Color, StylePreset};
        let style = |name: &str, width: f32| StylePreset {
            name: name.into(),
            line: Color::BLACK,
            width,
            fill: None,
            outline: true,
            smoothing: 0.5,
            taper: 10.0,
            pressure: true,
        };
        let mut doc = Document::new("s", 100.0, 100.0, 24.0);
        doc.styles = vec![style("Ink", 4.0), style("Paint", 8.0)];
        assert!(validate(&doc).is_clean());
        // a repeat, a name with nothing in it, an unusable number, and a name with spaces
        doc.styles = vec![
            style("Ink", 4.0),
            style("Ink", 5.0),
            style("   ", 4.0),
            style("Bad", f32::NAN),
            style("  Paint  ", 8.0),
        ];
        assert_eq!(validate(&doc).issues.len(), 1);
        let report = repair(&mut doc);
        assert!(report.issues.iter().all(|i| i.repaired), "{report}");
        let names: Vec<_> = doc.styles.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Ink", "Paint"]);
        assert!(validate(&doc).is_clean());
        // too many are cut
        doc.styles = (0..fanimate_core::MAX_STYLES + 5)
            .map(|i| style(&format!("s{i}"), 4.0))
            .collect();
        repair(&mut doc);
        assert_eq!(doc.styles.len(), fanimate_core::MAX_STYLES);
    }

    #[test]
    fn bad_width_values_are_repaired() {
        let mut doc = stroke_doc(vec![f32::NAN, -2.0]);
        assert_eq!(validate(&doc).issues.len(), 1);
        repair(&mut doc);
        assert_eq!(widths_of(&doc), Some(vec![1.0, 0.0]));
        assert!(validate(&doc).is_clean());
        assert!(validate(&stroke_doc(vec![0.1, 1.0])).is_clean());
    }

    #[test]
    fn point_keys_that_do_not_match_the_path_are_removed() {
        use fanimate_core::{Path, Vec2};
        let mut doc = stroke_doc(vec![1.0, 1.0]);
        let LayerKind::Vector { shapes } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        let path = shapes[0].path.clone();
        shapes[0].key_path(0, path.clone(), Interpolation::Linear);
        shapes[0].key_path(5, Path::circle(Vec2::ZERO, 1.0), Interpolation::Linear);
        shapes[0].path_keys.push(Key {
            frame: 2,
            value: path,
            interp: Interpolation::Linear,
        });
        let report = validate(&doc);
        assert_eq!(report.unresolved_errors().count(), 2, "{report}");
        repair(&mut doc);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        let LayerKind::Vector { shapes } = &doc.layers[0].kind else {
            unreachable!()
        };
        let frames: Vec<_> = shapes[0].path_keys.iter().map(|k| k.frame).collect();
        assert_eq!(frames, [0, 2]);
    }

    #[test]
    fn bad_bone_settings_and_weights_are_repaired() {
        let mut doc = rigged();
        let LayerKind::Bone { skeleton, children } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        skeleton.bones[0].limits = Some([1.0, -1.0]);
        skeleton.bones[0].strength = Some(f32::NAN);
        let LayerKind::Vector { shapes } = &mut children[0].kind else {
            unreachable!()
        };
        let n = shapes[0].path.on_curve_count();
        shapes[0].skin = Some(vec![vec![(0, 1.0), (9, 0.5)]; n]);
        shapes[1].skin = Some(vec![vec![(0, 1.0)]; 2]);
        let report = validate(&doc);
        assert_eq!(report.issues.len(), 4, "{report}");
        repair(&mut doc);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
    }

    #[test]
    fn broken_actions_are_repaired() {
        let mut doc = rigged();
        let LayerKind::Bone { skeleton, .. } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        let mut good = fanimate_core::RigAction::new("good", Some(0), 10);
        good.bones.push(fanimate_core::actions::BoneKeys {
            bone: 5,
            angle: Channel::new(0.0),
        });
        skeleton.actions.push(good);
        skeleton
            .actions
            .push(fanimate_core::RigAction::new("orphan", Some(9), 10));
        assert_eq!(validate(&doc).unresolved_errors().count(), 2);
        repair(&mut doc);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        let LayerKind::Bone { skeleton, .. } = &doc.layers[0].kind else {
            unreachable!()
        };
        assert_eq!(skeleton.actions.len(), 1);
        assert!(skeleton.actions[0].bones.is_empty());
    }

    #[test]
    fn image_layers_are_checked() {
        let mut doc = rigged();
        let mut img = fanimate_core::Image::new("../outside.png", f32::NAN, 10.0);
        img.grid = [0, 2];
        let layer = doc.new_layer("pic", LayerKind::Image(img));
        doc.layers.push(layer);
        let report = validate(&doc);
        assert_eq!(report.issues.len(), 3, "{report}");
        repair(&mut doc);
        // the bad name stays reported: there is no safe repair
        assert_eq!(validate(&doc).issues.len(), 1);
        let ok = fanimate_core::Image::new("media/a.png", 1.0, 1.0);
        let layer = doc.new_layer("pic2", LayerKind::Image(ok));
        doc.layers.push(layer);
        // missing picture: a warning
        assert!(
            validate(&doc)
                .issues
                .iter()
                .any(|i| i.severity == Severity::Warning)
        );
    }

    #[test]
    fn drawings_are_checked_and_repaired() {
        use fanimate_core::{Cel, Path, Shape, Style, Vec2};
        let mut doc = rigged();
        let mut a = Cel::new(5);
        let mut bad = Shape::new(
            "s",
            Path::new().move_to(Vec2::ZERO).line_to(Vec2::X),
            Style::default(),
        );
        bad.widths = Some(vec![1.0]);
        a.line.push(bad);
        let layer = doc.new_layer(
            "anim",
            LayerKind::Frames {
                cels: vec![a, Cel::new(-1), Cel::new(2)],
            },
        );
        doc.layers.push(layer);
        let report = validate(&doc);
        assert_eq!(report.unresolved_errors().count(), 3, "{report}");
        repair(&mut doc);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        let LayerKind::Frames { cels } = &doc.layers.last().unwrap().kind else {
            unreachable!()
        };
        assert_eq!(cels.iter().map(|c| c.frame).collect::<Vec<_>>(), [2, 5]);
    }

    #[test]
    fn duplicate_layer_ids_are_renumbered() {
        let mut doc = samples::benchmark(8);
        let mut copy = doc.layers[0].clone();
        copy.name = "copy".into();
        doc.layers.push(copy);
        let report = validate(&doc);
        assert!(report.unresolved_errors().count() > 0);
        repair(&mut doc);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
    }

    /// Found by the damaged-project test: ids at the top of the range made
    /// repair, and then the next new layer, overflow.
    #[test]
    fn huge_layer_ids_are_renumbered_with_room_for_more() {
        for (layer_id, next) in [(u32::MAX, u32::MAX), (5, u32::MAX), (u32::MAX, 3)] {
            let mut doc = samples::hello();
            doc.layers[0].id = LayerId(layer_id);
            doc.next_layer_id = next;
            assert!(validate(&doc).unresolved_errors().count() > 0);
            repair(&mut doc);
            assert!(validate(&doc).is_clean(), "{}", validate(&doc));
            let new = doc.new_layer_id();
            let mut ids = Vec::new();
            doc.walk(|l| ids.push(l.id));
            assert!(!ids.contains(&new), "a new layer gets an unused id");
            assert!(doc.next_layer_id <= MAX_LAYER_ID + 2);
        }
    }

    #[test]
    fn bad_document_settings_are_repaired() {
        let mut doc = rigged();
        doc.fps = 0.0;
        doc.end = -5;
        repair(&mut doc);
        // keys past the shortened range remain as warnings, not errors
        assert_eq!(
            validate(&doc).unresolved_errors().count(),
            0,
            "{}",
            validate(&doc)
        );
        assert_eq!(doc.fps, 24.0);
    }

    #[test]
    fn bad_pivots_and_rectangles_are_repaired() {
        use fanimate_core::{Color, Shape, Style, Vec2};
        let mut doc = Document::new("p", 100.0, 100.0, 24.0);
        let mut rect = Shape::rectangle(
            "r",
            Vec2::ZERO,
            Vec2::new(10.0, 10.0),
            2.0,
            Style::fill(Color::BLACK),
        );
        rect.rect.as_mut().unwrap().width = f32::NAN;
        let mut layer = doc.new_layer("l", LayerKind::vector(vec![rect]));
        layer.transform.pivot = Some(Vec2::new(f32::INFINITY, 0.0));
        doc.layers.push(layer);
        let report = validate(&doc);
        assert_eq!(report.issues.len(), 2, "{report}");
        let report = repair(&mut doc);
        assert_eq!(report.repaired_count(), 2, "{report}");
        assert_eq!(doc.layers[0].transform.pivot, None);
        let LayerKind::Vector { shapes } = &doc.layers[0].kind else {
            unreachable!()
        };
        assert!(shapes[0].rect.is_none());
        assert!(validate(&doc).is_clean());
    }

    #[test]
    fn corner_radii_that_do_not_fit_make_the_shape_a_plain_path() {
        use fanimate_core::{Color, Shape, Style, Vec2};
        let mut doc = Document::new("p", 100.0, 100.0, 24.0);
        let mut rect = Shape::rectangle(
            "r",
            Vec2::ZERO,
            Vec2::new(40.0, 20.0),
            0.0,
            Style::fill(Color::BLACK),
        );
        assert!(rect.set_rect_corner_radius(0, 5.0));
        // half the shorter side is 10: a radius of 50 cannot be the rectangle's
        rect.rect.as_mut().unwrap().radii = Some([50.0, 0.0, 0.0, 0.0]);
        let layer = doc.new_layer("l", LayerKind::vector(vec![rect]));
        doc.layers.push(layer);
        let report = validate(&doc);
        assert_eq!(report.issues.len(), 1, "{report}");
        repair(&mut doc);
        let LayerKind::Vector { shapes } = &doc.layers[0].kind else {
            unreachable!()
        };
        assert!(shapes[0].rect.is_none());
        assert!(validate(&doc).is_clean());
    }

    #[test]
    fn stroke_exposure_outside_its_range_is_held_within_it() {
        use fanimate_core::{Color, Shape, Style, Vec2};
        let mut doc = Document::new("p", 100.0, 100.0, 24.0);
        let mut shape = Shape::new(
            "s",
            fanimate_core::Path::circle(Vec2::ZERO, 5.0),
            Style::fill(Color::BLACK).with_stroke(Color::BLACK, 2.0),
        );
        shape.trim = Vec2::new(-0.5, 2.0);
        shape.trim_keys = vec![Key {
            frame: 3,
            value: Vec2::new(0.0, f32::NAN),
            interp: Interpolation::Linear,
        }];
        let layer = doc.new_layer("l", LayerKind::vector(vec![shape]));
        doc.layers.push(layer);
        let report = validate(&doc);
        assert!(report.issues.len() >= 2, "{report}");
        repair(&mut doc);
        let LayerKind::Vector { shapes } = &doc.layers[0].kind else {
            unreachable!()
        };
        assert_eq!(shapes[0].trim, Vec2::new(0.0, 1.0));
        assert!(
            shapes[0].trim_keys.is_empty(),
            "the key that was not a number is removed"
        );
        assert!(validate(&doc).is_clean());
    }

    #[test]
    fn colour_keys_are_checked_like_other_keys() {
        use fanimate_core::{Color, Interpolation, Key, Shape, Style, Vec2};
        let mut doc = Document::new("c", 100.0, 100.0, 24.0);
        let mut shape = Shape::rectangle("r", Vec2::ZERO, Vec2::splat(10.0), 0.0, Style::fill(Color::BLACK));
        shape.fill_keys = vec![
            Key {
                frame: 0,
                value: Color::BLACK,
                interp: Interpolation::Smooth,
            },
            Key {
                frame: 10,
                value: Color::WHITE,
                interp: Interpolation::Linear,
            },
        ];
        let layer = doc.new_layer("l", LayerKind::vector(vec![shape.clone()]));
        doc.layers.push(layer);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        // keys out of order, a colour that is not a number, and keys with nothing to colour
        shape.fill_keys.push(Key {
            frame: 5,
            value: Color::rgba(f32::NAN, 0.0, 0.0, 1.0),
            interp: Interpolation::Smooth,
        });
        shape.stroke_keys = vec![Key {
            frame: 3,
            value: Color::BLACK,
            interp: Interpolation::Smooth,
        }];
        doc.layers[0].kind = LayerKind::vector(vec![shape]);
        let report = repair(&mut doc);
        assert!(report.repaired_count() >= 2, "{report}");
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        let LayerKind::Vector { shapes } = &doc.layers[0].kind else {
            unreachable!()
        };
        assert!(shapes[0].stroke_keys.is_empty(), "no stroke, no stroke keys");
        assert!(shapes[0].fill_keys.windows(2).all(|w| w[0].frame < w[1].frame));
    }

    #[test]
    fn the_cameras_channels_are_checked() {
        use fanimate_core::{Interpolation, Key, Vec2};
        let mut doc = Document::new("c", 100.0, 100.0, 24.0);
        doc.camera.zoom.keys = vec![Key {
            frame: 2,
            value: 2.0,
            interp: Interpolation::Linear,
        }];
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        // keys out of order, and a position that is not a number
        doc.camera.zoom.keys.push(Key {
            frame: 1,
            value: 3.0,
            interp: Interpolation::Linear,
        });
        doc.camera.position.keys.push(Key {
            frame: 0,
            value: Vec2::new(f32::NAN, 0.0),
            interp: Interpolation::Smooth,
        });
        let report = repair(&mut doc);
        assert!(report.repaired_count() >= 2, "{report}");
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        // a zoom far outside what a camera can do is a warning: it is held within range
        doc.camera.zoom.base = 1.0e6;
        assert_eq!(validate(&doc).issues.len(), 1);
    }

    #[test]
    fn guides_without_a_position_are_removed() {
        use fanimate_core::Guide;
        let mut doc = Document::new("g", 100.0, 100.0, 24.0);
        doc.guides = vec![
            Guide {
                vertical: true,
                position: 20.0,
                locked: false,
            },
            Guide {
                vertical: false,
                position: f32::NAN,
                locked: false,
            },
        ];
        let report = validate(&doc);
        assert_eq!(report.issues.len(), 1, "{report}");
        assert_eq!(repair(&mut doc).repaired_count(), 1);
        assert_eq!(doc.guides.len(), 1);
        assert!(validate(&doc).is_clean());
        doc.guides = vec![
            Guide {
                vertical: true,
                position: 1.0,
                locked: false
            };
            MAX_GUIDES + 5
        ];
        assert_eq!(repair(&mut doc).repaired_count(), 1);
        assert_eq!(doc.guides.len(), MAX_GUIDES);
    }

    #[test]
    fn good_pivots_and_rectangles_are_clean() {
        use fanimate_core::{Color, Shape, Style, Vec2};
        let mut doc = Document::new("p", 100.0, 100.0, 24.0);
        let rect = Shape::rectangle(
            "r",
            Vec2::ZERO,
            Vec2::new(10.0, 10.0),
            2.0,
            Style::fill(Color::BLACK),
        );
        let mut layer = doc.new_layer("l", LayerKind::vector(vec![rect]));
        layer.transform.pivot = Some(Vec2::new(5.0, 5.0));
        doc.layers.push(layer);
        assert!(validate(&doc).is_clean());
    }

    fn key(frame: i32, value: f32) -> Key<f32> {
        Key {
            frame,
            value,
            interp: Interpolation::Linear,
        }
    }

    /// A group of two layers, to put order keys on.
    fn grouped() -> Document {
        let mut doc = Document::new("g", 100.0, 100.0, 24.0);
        let kids = ["a", "b"].map(|n| doc.new_layer(n, LayerKind::vector(vec![])));
        let group = doc.new_layer(
            "group",
            LayerKind::Group {
                children: kids.to_vec(),
            },
        );
        doc.layers.push(group);
        doc
    }

    fn kids(doc: &mut Document) -> &mut Vec<Layer> {
        let LayerKind::Group { children } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        children
    }

    /// Frame 0 is the rest pose: a key there is sound, and so is a project
    /// that starts there. Only a negative start is a problem.
    #[test]
    fn frame_zero_is_valid_and_a_negative_start_is_not() {
        let mut doc = rigged();
        let ch = arm_angle(&mut doc);
        ch.keys = vec![key(0, 0.0), key(12, 1.0)];
        doc.start = 0;
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        doc.start = -3;
        let report = validate(&doc);
        assert_eq!(report.unresolved_errors().count(), 1, "{report}");
        repair(&mut doc);
        assert_eq!(doc.start, 1);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
    }

    #[test]
    fn a_loop_needs_two_keys() {
        let mut doc = rigged();
        arm_angle(&mut doc).keys = vec![key(1, 0.5)];
        arm_angle(&mut doc).loop_mode = LoopMode::Relative;
        let report = validate(&doc);
        assert_eq!(report.issues.len(), 1, "{report}");
        assert_eq!(report.issues[0].severity, Severity::Warning);
        assert!(report.issues[0].message.contains("loop"));
        repair(&mut doc);
        assert_eq!(arm_angle(&mut doc).loop_mode, LoopMode::Off);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        // two keys are enough, and a channel that does not loop is left alone
        arm_angle(&mut doc).keys = vec![key(1, 0.5), key(9, 1.5)];
        arm_angle(&mut doc).loop_mode = LoopMode::Absolute;
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        // a repair that removes keys can leave a loop with too few
        arm_angle(&mut doc).keys.push(key(20, f32::NAN));
        arm_angle(&mut doc).keys.remove(0);
        let report = repair(&mut doc);
        assert_eq!(report.repaired_count(), 2, "{report}");
        assert_eq!(arm_angle(&mut doc).loop_mode, LoopMode::Off);
    }

    #[test]
    fn order_keys_are_checked() {
        let mut doc = grouped();
        kids(&mut doc)[0].order.keys = vec![key(1, 1.0), key(8, 0.0)];
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));

        // a slot past the end of the list is a warning, kept as it is
        kids(&mut doc)[1].order.keys = vec![key(1, 7.0)];
        let report = validate(&doc);
        assert_eq!(report.issues.len(), 1, "{report}");
        assert_eq!(report.issues[0].severity, Severity::Warning);
        assert!(!report.issues[0].repaired);
        let report = repair(&mut doc);
        assert_eq!(report.repaired_count(), 0, "{report}");
        assert_eq!(kids(&mut doc)[1].order.keys, vec![key(1, 7.0)]);

        // a slot that is not a number is an error, repaired by removing the key
        kids(&mut doc)[1].order.keys = vec![key(1, 1.0), key(4, f32::NAN)];
        let report = validate(&doc);
        assert_eq!(report.unresolved_errors().count(), 1, "{report}");
        repair(&mut doc);
        assert_eq!(kids(&mut doc)[1].order.keys, vec![key(1, 1.0)]);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
    }

    #[test]
    fn damaged_controllers_are_repaired_and_links_follow_the_actions_that_stay() {
        use fanimate_core::actions::{Controller, ControllerAxis, ControllerKind, ControllerLink, RigAction};
        let mut doc = rigged();
        let LayerKind::Bone { skeleton, .. } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        // action 0 is Smart Bone action whose driver bone does not exist (it will be dropped),
        // action 1 and 2 are fine
        skeleton.actions.push(RigAction::new("lost", Some(99), 5));
        skeleton.actions.push(RigAction::new("one", None, 5));
        skeleton.actions.push(RigAction::new("two", None, 5));
        let link = |action: usize| ControllerLink {
            action,
            axis: ControllerAxis::X,
            from: 0.0,
            to: 1.0,
        };
        let mut c = Controller::new("  ", ControllerKind::Slider);
        c.links = vec![link(0), link(1), link(2), link(77)];
        c.x.base = f32::NAN;
        let mut wild = Controller::new("Wild", ControllerKind::Pad);
        wild.links = vec![ControllerLink {
            from: f32::NAN,
            ..link(1)
        }];
        skeleton.controllers = vec![c, wild];
        let report = validate(&doc);
        assert!(report.unresolved_errors().count() >= 1, "{report}");
        repair(&mut doc);
        assert!(validate(&doc).is_clean(), "{}", validate(&doc));
        let LayerKind::Bone { skeleton, .. } = &doc.layers[0].kind else {
            unreachable!()
        };
        let names: Vec<_> = skeleton.actions.iter().map(|a| a.name.as_str()).collect();
        assert!(!names.contains(&"lost"), "{names:?}");
        let first = &skeleton.controllers[0];
        assert_eq!(first.name, "Controller 1", "a blank name is replaced");
        assert!(first.x.base.is_finite());
        // "one" and "two" moved up by one: the links that held them moved with them, the one to the
        // lost action and the one past the end are gone
        let held: Vec<_> = first
            .links
            .iter()
            .map(|l| skeleton.actions[l.action].name.as_str())
            .collect();
        assert_eq!(held, ["one", "two"]);
        assert!(
            skeleton.controllers[1].links.is_empty(),
            "a range that is not a number is removed"
        );
        // too many controllers are cut
        let LayerKind::Bone { skeleton, .. } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        skeleton.controllers = (0..fanimate_core::actions::MAX_CONTROLLERS + 4)
            .map(|i| Controller::new(format!("c{i}"), ControllerKind::Slider))
            .collect();
        repair(&mut doc);
        let LayerKind::Bone { skeleton, .. } = &doc.layers[0].kind else {
            unreachable!()
        };
        assert_eq!(
            skeleton.controllers.len(),
            fanimate_core::actions::MAX_CONTROLLERS
        );
    }
}
