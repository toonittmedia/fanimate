use super::*;
use crate::Action;
use fanimate_core::{Color, Document, Path, Shape, Style, evaluate};

fn style() -> Style {
    Style::default().with_stroke(Color::BLACK, 3.0)
}

/// A wobbly circle of many points, as a hand draws one.
fn rough_circle(centre: Vec2, radius: f32) -> Path {
    let mut p = Path::new();
    for i in 0..200 {
        let a = i as f32 / 200.0 * std::f32::consts::TAU;
        let r = radius + ((i * 7919 % 13) as f32 - 6.0) * 0.08;
        let v = centre + Vec2::new(r * a.cos(), r * a.sin());
        p = if i == 0 { p.move_to(v) } else { p.line_to(v) };
    }
    p.close()
}

fn vector(ed: &mut Editor, name: &str, shapes: Vec<Shape>) -> LayerId {
    let layer = ed.doc.new_layer(name, LayerKind::vector(shapes));
    let id = layer.id;
    ed.doc.layers.push(layer);
    id
}

fn editor() -> Editor {
    Editor::new(Document::new("t", 800.0, 800.0, 24.0))
}

fn shapes_of(ed: &Editor, id: LayerId) -> Vec<Shape> {
    ed.layer_shapes(id).unwrap().clone()
}

fn tool_of(ed: &Editor, id: LayerId) -> Option<String> {
    ed.doc
        .find(id)
        .unwrap()
        .provenance
        .as_ref()
        .map(|p| p.tool.clone())
}

// ---- clean-up ----

#[test]
fn clean_up_refits_rough_lines_in_one_undo_step_and_labels_the_layer() {
    let mut ed = editor();
    let rough = rough_circle(Vec2::new(200.0, 200.0), 100.0);
    let id = vector(&mut ed, "ink", vec![Shape::new("circle", rough.clone(), style())]);
    ed.selection = vec![id];
    ed.run(Action::CleanUpLines);
    let after = shapes_of(&ed, id);
    assert!(
        after[0].path.on_curve_count() < 30,
        "{}",
        after[0].path.on_curve_count()
    );
    assert_eq!(tool_of(&ed, id).as_deref(), Some("line-clean-up"));
    // one undo restores the drawn line and the layer's lack of a record
    ed.run(Action::Undo);
    assert_eq!(shapes_of(&ed, id)[0].path, rough);
    assert!(tool_of(&ed, id).is_none());
}

#[test]
fn clean_up_keeps_a_brush_stroke_width_profile_in_step() {
    let mut ed = editor();
    let mut p = Path::new().move_to(Vec2::new(0.0, 0.0));
    for i in 1..=80 {
        p = p.line_to(Vec2::new(i as f32 * 4.0, (i as f32 * 0.2).sin() * 30.0));
    }
    let mut shape = Shape::new("stroke", p, style());
    shape.widths = Some((0..=80).map(|i| 1.0 + i as f32 * 0.1).collect());
    let id = vector(&mut ed, "ink", vec![shape]);
    let s = ed.clean_up_lines(id, None, CleanOptions::default()).unwrap();
    assert_eq!(s.shapes, 1);
    let shape = &shapes_of(&ed, id)[0];
    let widths = shape.widths.as_ref().unwrap();
    assert_eq!(widths.len(), shape.path.on_curve_count());
    assert!(widths.first().unwrap() < widths.last().unwrap());
}

#[test]
fn clean_up_leaves_alone_what_it_cannot_refit_safely() {
    let mut ed = editor();
    let mut keyed = Shape::new("keyed", rough_circle(Vec2::new(100.0, 100.0), 50.0), style());
    keyed.path_keys = vec![fanimate_core::Key {
        frame: 5,
        value: keyed.path.clone(),
        interp: Default::default(),
    }];
    let mut hidden = Shape::new("hidden", rough_circle(Vec2::new(300.0, 100.0), 50.0), style());
    hidden.hidden_edge_flags = vec![false; hidden.path.cmds.len()];
    let clean = Shape::new(
        "clean",
        Path::line(Vec2::new(400.0, 100.0), Vec2::new(500.0, 100.0)),
        style(),
    );
    let id = vector(&mut ed, "ink", vec![keyed.clone(), hidden.clone(), clean]);
    let s = ed.clean_up_lines(id, None, CleanOptions::default()).unwrap();
    assert_eq!((s.shapes, s.skipped), (0, 3));
    let after = shapes_of(&ed, id);
    assert_eq!(
        (after[0].path.clone(), after[1].path.clone()),
        (keyed.path, hidden.path)
    );
    // nothing changed, so no undo step and no record
    assert!(tool_of(&ed, id).is_none());
    assert!(ed.history.undo_label().is_none());
}

#[test]
fn clean_up_refuses_a_locked_layer_and_the_wrong_kind() {
    let mut ed = editor();
    let id = vector(
        &mut ed,
        "ink",
        vec![Shape::new("c", rough_circle(Vec2::ZERO, 50.0), style())],
    );
    ed.doc.find_mut(id).unwrap().locked = true;
    assert!(
        ed.clean_up_lines(id, None, CleanOptions::default())
            .unwrap_err()
            .contains("locked")
    );
    let group = ed.doc.new_layer("g", LayerKind::Group { children: vec![] });
    let gid = group.id;
    ed.doc.layers.push(group);
    assert!(ed.clean_up_lines(gid, None, CleanOptions::default()).is_err());
}

// ---- inbetween ----

fn drawing(frame: Frame, x: f32) -> Cel {
    let mut cel = Cel::new(frame);
    cel.line.push(Shape::new(
        "line",
        Path::line(Vec2::new(x, 0.0), Vec2::new(x + 100.0, 0.0)),
        style(),
    ));
    cel.colour.push(Shape::new(
        "paint",
        Path::rect(Vec2::new(x, 20.0), Vec2::new(40.0, 40.0), 0.0),
        Style::fill(Color::BLACK),
    ));
    cel
}

fn frames_editor() -> (Editor, LayerId) {
    let mut ed = editor();
    let layer = ed.doc.new_layer(
        "anim",
        LayerKind::Frames {
            cels: vec![drawing(1, 0.0), drawing(9, 80.0)],
        },
    );
    let id = layer.id;
    ed.doc.layers.push(layer);
    ed.selection = vec![id];
    ed.go_to(1);
    (ed, id)
}

fn cels(ed: &Editor, id: LayerId) -> Vec<Cel> {
    match &ed.doc.find(id).unwrap().kind {
        LayerKind::Frames { cels } => cels.clone(),
        _ => panic!(),
    }
}

#[test]
fn an_inbetween_goes_halfway_and_is_one_undo_step() {
    let (mut ed, id) = frames_editor();
    ed.run(Action::SuggestInbetween);
    let c = cels(&ed, id);
    assert_eq!(c.iter().map(|c| c.frame).collect::<Vec<_>>(), [1, 5, 9]);
    // halfway between a line at x 0 and one at x 80 is at x 40, on both parts
    let mid = &c[1];
    let first = mid.line[0].path.on_curve_points()[0];
    assert!(first.distance(Vec2::new(40.0, 0.0)) < 1.5, "{first:?}");
    assert_eq!((mid.line.len(), mid.colour.len()), (1, 1));
    assert_eq!(ed.current_frame(), 5);
    assert_eq!(tool_of(&ed, id).as_deref(), Some("inbetween-assist"));
    ed.run(Action::Undo);
    assert_eq!(cels(&ed, id).len(), 2);
}

#[test]
fn on_a_later_frame_the_inbetween_goes_there_and_the_ratio_follows() {
    let (mut ed, id) = frames_editor();
    ed.go_to(3);
    let s = ed.suggest_inbetween(id).unwrap();
    assert_eq!((s.frame, s.blended, s.held, s.left_out), (3, 2, 0, 0));
    // 2 of 8 frames of the way: x = 20
    let first = cels(&ed, id)[1].line[0].path.on_curve_points()[0];
    assert!(first.distance(Vec2::new(20.0, 0.0)) < 1.5, "{first:?}");
}

#[test]
fn inbetween_says_why_it_cannot() {
    let (mut ed, id) = frames_editor();
    ed.go_to(9);
    assert!(ed.suggest_inbetween(id).unwrap_err().contains("no later drawing"));
    ed.go_to(0);
    assert!(ed.suggest_inbetween(id).unwrap_err().contains("shows a drawing"));
    // two neighbouring drawings leave no frame between
    let mut tight = editor();
    let layer = tight.doc.new_layer(
        "anim",
        LayerKind::Frames {
            cels: vec![drawing(1, 0.0), drawing(2, 80.0)],
        },
    );
    let tid = layer.id;
    tight.doc.layers.push(layer);
    tight.go_to(1);
    assert!(
        tight
            .suggest_inbetween(tid)
            .unwrap_err()
            .contains("no frame between")
    );
    // a vector layer is the wrong kind
    let v = vector(&mut tight, "v", vec![]);
    assert!(tight.suggest_inbetween(v).unwrap_err().contains("frame-by-frame"));
    assert!(tight.history.undo_label().is_none());
}

// ---- auto-rig ----

fn box_shape(x0: f32, y0: f32, x1: f32, y1: f32) -> Shape {
    Shape::new(
        "box",
        Path::rect(Vec2::new(x0, y0), Vec2::new(x1 - x0, y1 - y0), 0.0),
        Style::fill(Color::BLACK),
    )
}

fn character(ed: &mut Editor) -> Vec<LayerId> {
    [
        ("background", (0.0, 0.0, 800.0, 800.0)),
        ("torso", (350.0, 300.0, 450.0, 520.0)),
        ("head", (360.0, 180.0, 440.0, 300.0)),
        ("arm L", (300.0, 310.0, 350.0, 520.0)),
    ]
    .into_iter()
    .map(|(name, (a, b, c, d))| vector(ed, name, vec![box_shape(a, b, c, d)]))
    .collect()
}

fn corners(ed: &Editor) -> Vec<Vec2> {
    evaluate(&ed.doc, 0.0)
        .iter()
        .map(|i| i.transform.transform_point2(i.shape.path.on_curve_points()[0]))
        .collect()
}

#[test]
fn auto_rig_builds_bones_binds_the_layers_and_keeps_the_art_where_it_was() {
    let mut ed = editor();
    let ids = character(&mut ed);
    let before = corners(&ed);
    ed.selection = ids[1..].to_vec();
    ed.run(Action::AutoRig);
    // the three body layers moved into one bone layer, the background stayed
    assert_eq!(ed.doc.layers.len(), 2);
    let rig = &ed.doc.layers[1];
    let LayerKind::Bone { skeleton, children } = &rig.kind else {
        panic!("not a bone layer");
    };
    assert_eq!(skeleton.bones.len(), 3);
    assert_eq!(
        children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["torso", "head", "arm L"]
    );
    assert_eq!(rig.provenance.as_ref().map(|p| p.tool.as_str()), Some("auto-rig"));
    let bound: Vec<Option<usize>> = children
        .iter()
        .map(|c| match &c.kind {
            LayerKind::Vector { shapes } => shapes[0].bone,
            _ => None,
        })
        .collect();
    assert_eq!(bound, [Some(0), Some(1), Some(2)]);
    // at the rest pose every piece is exactly where it was drawn
    let after = corners(&ed);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!(b.distance(*a) < 1.0e-2, "{b:?} moved to {a:?}");
    }
    // one undo puts the plain layers back
    ed.run(Action::Undo);
    assert_eq!(ed.doc.layers.len(), 4);
    assert!(matches!(ed.doc.layers[1].kind, LayerKind::Vector { .. }));
}

#[test]
fn auto_rig_with_nothing_selected_uses_every_vector_layer_and_lists_the_unbound() {
    let mut ed = editor();
    character(&mut ed);
    let all: Vec<LayerId> = ed.doc.layers.iter().map(|l| l.id).collect();
    let s = ed.auto_rig(&all).unwrap();
    assert_eq!((s.bones, s.bound), (3, 3));
    assert_eq!(s.unbound, ["background"]);
    assert_eq!(ed.doc.layers.len(), 2);
}

#[test]
fn auto_rig_refuses_with_a_reason_and_changes_nothing() {
    let mut ed = editor();
    let head = vector(&mut ed, "head", vec![box_shape(0.0, 0.0, 10.0, 10.0)]);
    assert!(ed.auto_rig(&[head]).unwrap_err().contains("torso"));
    assert!(ed.auto_rig(&[]).is_err());
    let torso = vector(&mut ed, "torso", vec![box_shape(0.0, 10.0, 10.0, 30.0)]);
    ed.doc.find_mut(torso).unwrap().locked = true;
    assert!(ed.auto_rig(&[head, torso]).unwrap_err().contains("locked"));
    // an empty layer has nothing to place a bone by
    ed.doc.find_mut(torso).unwrap().locked = false;
    let empty = vector(&mut ed, "arm", vec![]);
    assert!(
        ed.auto_rig(&[head, torso, empty])
            .unwrap_err()
            .contains("no artwork")
    );
    assert_eq!(ed.doc.layers.len(), 3);
    assert!(ed.history.undo_label().is_none());
}

#[test]
fn the_assist_commands_are_edits_and_wait_for_their_layer() {
    assert!(Action::CleanUpLines.edits_document());
    assert!(Action::SuggestInbetween.edits_document());
    assert!(Action::AutoRig.edits_document());
    let mut ed = editor();
    assert!(!ed.can_run(Action::CleanUpLines));
    assert!(!ed.can_run(Action::SuggestInbetween));
    assert!(!ed.can_run(Action::AutoRig));
    let id = vector(&mut ed, "torso", vec![]);
    ed.selection = vec![id];
    assert!(ed.can_run(Action::CleanUpLines));
    assert!(ed.can_run(Action::AutoRig));
    assert!(!ed.can_run(Action::SuggestInbetween));
}

// ---- palette ----

#[test]
fn replacing_a_colour_is_one_undo_step_and_nothing_when_nothing_uses_it() {
    let mut ed = editor();
    let red = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    let shapes = vec![Shape::new("a", rough_circle(Vec2::ZERO, 20.0), Style::fill(red)); 3];
    let id = vector(&mut ed, "art", shapes);
    let steps = ed.history.cursor();
    assert!(ed.can_replace_colour(red, green));
    assert_eq!(ed.replace_colour(red, green), 3);
    assert_eq!(ed.history.cursor(), steps + 1);
    assert!(shapes_of(&ed, id).iter().all(|s| s.style.fill == Some(green)));
    ed.run(Action::Undo);
    assert!(shapes_of(&ed, id).iter().all(|s| s.style.fill == Some(red)));
    // not used, the same colour, or playing: no step
    assert!(!ed.can_replace_colour(Color::rgb(0.3, 0.3, 0.3), green));
    assert_eq!(ed.replace_colour(Color::rgb(0.3, 0.3, 0.3), green), 0);
    assert_eq!(ed.replace_colour(red, red), 0);
    ed.playing = true;
    assert_eq!(ed.replace_colour(red, green), 0);
    ed.playing = false;
    assert_eq!(ed.history.cursor(), steps);
    // the live form changes without a step, for a picker being dragged
    assert_eq!(ed.replace_colour_live(red, green), 3);
    assert_eq!(ed.history.cursor(), steps);
    assert!(ed.dirty);
}
