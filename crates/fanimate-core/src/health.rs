//! The rig health check: problems that make a rig confusing or slow
//! before they become bugs, such as bones that move nothing, Smart Bones
//! that fight, and keyframes nobody can see.
//!
//! Unlike the file validator, nothing here is damage: every finding is a
//! rig that works but probably not as meant.

use crate::actions::RigAction;
use crate::scene::{Document, Layer, LayerId, LayerKind};

/// One finding: the layer it is about and what to tell the animator.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub layer: LayerId,
    pub message: String,
}

/// Check every bone layer in `doc`, and keys on hidden layers.
pub fn check(doc: &Document) -> Vec<Finding> {
    let mut out = Vec::new();
    doc.walk(|layer| {
        if !layer.visible && !layer.key_frames().is_empty() {
            out.push(Finding {
                layer: layer.id,
                message: format!("\"{}\" is hidden but has keyframes; they play unseen", layer.name),
            });
        }
        if let LayerKind::Bone { skeleton, children } = &layer.kind {
            check_rig(layer, skeleton, children, &mut out);
        }
    });
    out
}

fn check_rig(layer: &Layer, skeleton: &crate::Skeleton, children: &[Layer], out: &mut Vec<Finding>) {
    let mut push = |message: String| {
        out.push(Finding {
            layer: layer.id,
            message,
        })
    };
    let n = skeleton.bones.len();
    // what each bone moves: artwork bound to it, child bones, actions
    let mut used = vec![false; n];
    for b in &skeleton.bones {
        if let Some(p) = b.parent.filter(|&p| p < n) {
            used[p] = true;
        }
    }
    for act in &skeleton.actions {
        if let Some(d) = act.driver.filter(|&d| d < n) {
            used[d] = true;
        }
        for b in &act.bones {
            if b.bone < n {
                used[b.bone] = true;
            }
        }
    }
    mark_bound(children, &mut used);
    for (i, b) in skeleton.bones.iter().enumerate() {
        if !used[i] {
            push(format!(
                "Bone \"{}\" moves nothing: no artwork is bound to it",
                b.name
            ));
        }
        if b.length <= 0.0 {
            push(format!("Bone \"{}\" has no length", b.name));
        }
    }
    for act in skeleton.actions.iter().filter(|a| a.driver.is_some()) {
        let keys = act
            .driver
            .and_then(|d| act.bones.iter().find(|b| b.bone == d))
            .map_or(0, |b| b.angle.keys.len());
        if keys < 2 {
            push(format!(
                "Smart Bone action \"{}\" does nothing yet: turn its bone on a later frame of the action",
                act.name
            ));
        }
        for s in &act.shapes {
            if let Some(shape) = find_shape(children, s.layer, s.shape)
                && s.path_keys.iter().any(|k| !k.value.same_structure(&shape.path))
            {
                push(format!(
                    "Smart Bone action \"{}\" no longer fits \"{}\": its points changed since, so it is skipped",
                    act.name, shape.name
                ));
            }
        }
    }
    // two Smart Bones on the same driver both reshaping the same thing add
    // up, which is rarely what was meant
    let smart: Vec<&RigAction> = skeleton.actions.iter().filter(|a| a.driver.is_some()).collect();
    for (i, a) in smart.iter().enumerate() {
        for b in &smart[i + 1..] {
            if a.driver != b.driver {
                continue;
            }
            let shared_bone = a
                .bones
                .iter()
                .any(|x| Some(x.bone) != a.driver && b.bones.iter().any(|y| y.bone == x.bone));
            let shared_shape = a
                .shapes
                .iter()
                .any(|x| b.shapes.iter().any(|y| y.layer == x.layer && y.shape == x.shape));
            if shared_bone || shared_shape {
                push(format!(
                    "Smart Bone actions \"{}\" and \"{}\" share a bone and change the same things; their changes add up",
                    a.name, b.name
                ));
            }
        }
    }
}

/// Mark the bones that artwork inside `layers` follows, stopping at nested
/// bone layers (their artwork follows their own bones).
fn mark_bound(layers: &[Layer], used: &mut [bool]) {
    fn mark(used: &mut [bool], b: usize) {
        if let Some(u) = used.get_mut(b) {
            *u = true;
        }
    }
    for l in layers {
        match &l.kind {
            LayerKind::Vector { shapes } => {
                for s in shapes {
                    if let Some(b) = s.bone {
                        mark(used, b);
                    }
                    for w in s.skin.iter().flatten().flatten() {
                        if w.1 > 0.0 {
                            mark(used, w.0);
                        }
                    }
                }
            }
            LayerKind::Image(img) => {
                if let Some(b) = img.bone {
                    mark(used, b);
                }
                for w in img.skin.iter().flatten().flatten() {
                    if w.1 > 0.0 {
                        mark(used, w.0);
                    }
                }
            }
            LayerKind::Frames { cels } => {
                for (_, _, s) in cels.iter().flat_map(|c| c.shapes()) {
                    if let Some(b) = s.bone {
                        mark(used, b);
                    }
                    for w in s.skin.iter().flatten().flatten() {
                        if w.1 > 0.0 {
                            mark(used, w.0);
                        }
                    }
                }
            }
            LayerKind::Group { children } | LayerKind::Switch { children, .. } => mark_bound(children, used),
            LayerKind::Bone { .. } | LayerKind::Audio(_) | LayerKind::Text(_) => {}
        }
    }
}

fn find_shape(layers: &[Layer], id: LayerId, index: usize) -> Option<&crate::Shape> {
    layers.iter().find_map(|l| match &l.kind {
        LayerKind::Vector { shapes } if l.id == id => shapes.get(index),
        LayerKind::Group { children } => find_shape(children, id, index),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::BoneKeys;
    use crate::samples;
    use crate::{Channel, Vec2};

    #[test]
    fn healthy_samples_have_no_findings() {
        assert!(
            check(&samples::hello()).is_empty(),
            "{:?}",
            check(&samples::hello())
        );
        assert!(check(&samples::benchmark(40)).is_empty());
    }

    #[test]
    fn finds_idle_bones_hidden_keys_and_clashing_smart_bones() {
        let mut doc = samples::hello();
        doc.layers[0].visible = false;
        let LayerKind::Bone { skeleton, .. } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        skeleton.add("spare", None, Vec2::ZERO, 10.0, 0.0);
        let mut a = RigAction::new("a", Some(0), 10);
        a.bones.push(BoneKeys {
            bone: 1,
            angle: Channel::new(0.0),
        });
        let b = a.clone();
        skeleton.actions.extend([a, b]);
        let messages: Vec<String> = check(&doc).into_iter().map(|f| f.message).collect();
        let has = |s: &str| messages.iter().any(|m| m.contains(s));
        assert!(has("is hidden but has keyframes"), "{messages:?}");
        assert!(
            !has("Bone \"spare\" moves nothing"),
            "used by an action: {messages:?}"
        );
        assert!(has("does nothing yet"));
        assert!(has("add up"));
        let LayerKind::Bone { skeleton, .. } = &mut doc.layers[0].kind else {
            unreachable!()
        };
        skeleton.actions.clear();
        assert!(
            check(&doc)
                .iter()
                .any(|f| f.message.contains("\"spare\" moves nothing"))
        );
    }
}
