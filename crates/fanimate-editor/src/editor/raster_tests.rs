use super::*;
use crate::{Action, Tool};
use fanimate_core::{Color, Document, LayerKind, Vec2};

fn editor() -> Editor {
    Editor::new(Document::new("t", 1000.0, 800.0, 24.0))
}

fn paint_layer(ed: &mut Editor) -> LayerId {
    ed.run(Action::NewRasterLayer);
    ed.primary().unwrap()
}

fn tiles(ed: &Editor, id: LayerId) -> Vec<String> {
    match &ed.doc.find(id).unwrap().kind {
        LayerKind::Group { children } => children.iter().map(|c| c.name.clone()).collect(),
        _ => panic!("not a group"),
    }
}

/// The RGBA at a layer-space point, read back from the project's own pictures.
fn pixel(ed: &Editor, id: LayerId, p: Vec2) -> [u8; 4] {
    let (key, x, y) = raster::locate(p);
    let px = ed.tile_pixels(id, key);
    let i = ((y * TILE + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn stroke(ed: &mut Editor, points: &[Vec2]) {
    assert!(ed.begin_stroke(points[0], 1.0, 1.0));
    for p in &points[1..] {
        ed.continue_stroke(*p, 1.0);
    }
    ed.finish_stroke();
}

#[test]
fn a_new_raster_layer_is_a_marked_empty_group_in_one_undo_step() {
    let mut ed = editor();
    let id = paint_layer(&mut ed);
    let layer = ed.doc.find(id).unwrap();
    assert!(layer.raster.is_some() && matches!(layer.kind, LayerKind::Group { .. }));
    assert_eq!(layer.name, "Paint 1");
    assert_eq!(ed.raster_group_of(id), Some(id));
    ed.run(Action::Undo);
    assert!(ed.doc.layers.is_empty());
    assert!(Action::NewRasterLayer.edits_document());
}

#[test]
fn the_brush_paints_pixels_in_the_selected_colour_as_one_undo_step() {
    let mut ed = editor();
    let id = paint_layer(&mut ed);
    ed.draw.line = Color::rgb(1.0, 0.0, 0.0);
    ed.draw.width = 20.0;
    let before = ed.history.cursor();
    stroke(&mut ed, &[Vec2::new(50.0, 50.0), Vec2::new(150.0, 50.0)]);
    assert_eq!(tiles(&ed, id), ["0,0"]);
    assert_eq!(pixel(&ed, id, Vec2::new(100.0, 50.0)), [255, 0, 0, 255]);
    assert_eq!(pixel(&ed, id, Vec2::new(100.0, 90.0))[3], 0, "off the line");
    // the stroke is one step, and the picture it left is the only one in the store
    assert_eq!(ed.history.cursor(), before + 1);
    assert_eq!(ed.doc.media.len(), 1);
    ed.run(Action::Undo);
    assert!(tiles(&ed, id).is_empty());
    assert!(ed.doc.media.is_empty());
    ed.run(Action::Redo);
    assert_eq!(pixel(&ed, id, Vec2::new(100.0, 50.0)), [255, 0, 0, 255]);
}

#[test]
fn a_stroke_over_tile_edges_makes_each_tile_and_a_second_stroke_replaces_pictures_not_adds() {
    let mut ed = editor();
    let id = paint_layer(&mut ed);
    ed.draw.width = 30.0;
    stroke(&mut ed, &[Vec2::new(200.0, 100.0), Vec2::new(330.0, 100.0)]);
    let mut names = tiles(&ed, id);
    names.sort();
    assert_eq!(names, ["0,0", "1,0"]);
    assert_eq!(ed.doc.media.len(), 2);
    // painting again over the same tiles swaps their pictures, leaving none behind
    stroke(&mut ed, &[Vec2::new(210.0, 120.0), Vec2::new(320.0, 120.0)]);
    assert_eq!(
        ed.doc.media.len(),
        2,
        "the old pictures are dropped from the store"
    );
    for name in tiles(&ed, id) {
        // each tile sits where its name says
        let key = raster::parse_tile_name(&name).unwrap();
        let LayerKind::Group { children } = &ed.doc.find(id).unwrap().kind else {
            unreachable!()
        };
        let child = children.iter().find(|c| c.name == name).unwrap();
        assert_eq!(child.transform.position.base, raster::tile_origin(key));
    }
}

#[test]
fn paint_lands_where_the_layer_is_not_where_the_page_is() {
    let mut ed = editor();
    let id = paint_layer(&mut ed);
    ed.doc.find_mut(id).unwrap().transform.position.base = Vec2::new(300.0, 0.0);
    ed.draw.width = 20.0;
    stroke(&mut ed, &[Vec2::new(350.0, 40.0), Vec2::new(380.0, 40.0)]);
    // the document point (350, 40) is (50, 40) in the layer
    assert_eq!(pixel(&ed, id, Vec2::new(50.0, 40.0))[3], 255);
    assert_eq!(pixel(&ed, id, Vec2::new(350.0, 40.0))[3], 0);
}

#[test]
fn the_eraser_rubs_out_pixels_and_an_emptied_tile_goes_away_with_its_picture() {
    let mut ed = editor();
    let id = paint_layer(&mut ed);
    ed.draw.width = 10.0;
    stroke(&mut ed, &[Vec2::new(100.0, 100.0), Vec2::new(110.0, 100.0)]);
    assert_eq!(pixel(&ed, id, Vec2::new(105.0, 100.0))[3], 255);
    ed.tool = Tool::Eraser;
    ed.draw.eraser = 12.0;
    let before = ed.history.cursor();
    assert!(ed.begin_erase(Vec2::new(95.0, 100.0), 1.0));
    ed.erase_to(Vec2::new(118.0, 100.0));
    assert!(ed.is_erasing());
    ed.end_erase();
    assert!(!ed.is_erasing());
    assert_eq!(ed.history.cursor(), before + 1);
    assert!(tiles(&ed, id).is_empty(), "nothing left of the tile");
    assert!(ed.doc.media.is_empty());
    // erasing where nothing is painted is not an undo step
    let steps = ed.history.cursor();
    assert!(ed.begin_erase(Vec2::new(600.0, 600.0), 1.0));
    ed.end_erase();
    assert_eq!(ed.history.cursor(), steps);
}

#[test]
fn a_hidden_or_locked_raster_layer_is_not_painted_on_and_a_vector_layer_still_gets_vectors() {
    let mut ed = editor();
    let id = paint_layer(&mut ed);
    ed.doc.find_mut(id).unwrap().locked = true;
    assert_eq!(ed.raster_target(), None);
    ed.doc.find_mut(id).unwrap().locked = false;
    ed.doc.find_mut(id).unwrap().visible = false;
    assert_eq!(ed.raster_target(), None);
    ed.doc.find_mut(id).unwrap().visible = true;
    assert_eq!(ed.raster_target(), Some(id));
    // with a vector layer selected the brush makes a shape, not pixels
    ed.run(Action::NewVectorLayer);
    let vector = ed.primary().unwrap();
    assert_eq!(ed.raster_target(), None);
    stroke(
        &mut ed,
        &[
            Vec2::new(10.0, 10.0),
            Vec2::new(80.0, 30.0),
            Vec2::new(150.0, 10.0),
        ],
    );
    let LayerKind::Vector { shapes } = &ed.doc.find(vector).unwrap().kind else {
        panic!()
    };
    assert_eq!(shapes.len(), 1);
    assert!(tiles(&ed, id).is_empty());
}

#[test]
fn a_painting_survives_saving_and_opening_and_old_readers_see_a_group_of_pictures() {
    let mut ed = editor();
    let id = paint_layer(&mut ed);
    ed.draw.line = Color::rgb(0.0, 0.0, 1.0);
    ed.draw.width = 24.0;
    stroke(&mut ed, &[Vec2::new(240.0, 60.0), Vec2::new(300.0, 80.0)]);
    let file = std::env::temp_dir().join(format!("fanimate-raster-{}.fnm", std::process::id()));
    fanimate_fnm::save(&ed.doc.mainline(), &file).unwrap();
    let (doc, report) = fanimate_fnm::load(&file).unwrap();
    std::fs::remove_file(&file).ok();
    assert!(report.is_clean(), "{report}");
    let back = Editor::new(doc);
    assert!(back.doc.find(id).unwrap().raster.is_some());
    assert_eq!(pixel(&back, id, Vec2::new(270.0, 70.0)), [0, 0, 255, 255]);
    let (mut a, mut b) = (tiles(&back, id), tiles(&ed, id));
    a.sort();
    b.sort();
    assert_eq!(a, b);
    // an older reader ignores the marker and finds image layers: that is the format
    let LayerKind::Group { children } = &back.doc.find(id).unwrap().kind else {
        panic!()
    };
    assert!(children.iter().all(|c| matches!(c.kind, LayerKind::Image(_))));
}

#[test]
fn hostile_pointer_positions_never_panic_or_grow_without_bound() {
    let mut ed = editor();
    let id = paint_layer(&mut ed);
    ed.draw.width = 2000.0;
    assert!(ed.begin_stroke(Vec2::new(f32::NAN, 0.0), 1.0, 1.0));
    for p in [
        Vec2::new(1.0e30, 5.0),
        Vec2::new(f32::INFINITY, 0.0),
        Vec2::new(0.0, 0.0),
        Vec2::new(900.0, 700.0),
    ] {
        ed.continue_stroke(p, f32::NAN);
    }
    ed.finish_stroke();
    assert!(tiles(&ed, id).len() <= raster::MAX_TILES);
}
