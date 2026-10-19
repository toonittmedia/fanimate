//! The canvas: the GPU-rendered document, plus navigation, the drawing
//! and Transform Layer tools, click-to-select and overlays.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke};
use fanimate_core::{Color, LayerId, LayerKind, Vec2, evaluate};
use fanimate_editor::{Action, SelectMode, Tool};
use fanimate_render::View;

use crate::app::Shell;
use crate::menus;
use crate::panels::handles::{self, Hit, Screen};

/// Apply snapping to a document-space point. Stores the hint line in
/// `shell.ui.snap_hint` so the overlay can draw it. Returns the snapped point.
fn do_snap(shell: &mut Shell, p: Vec2) -> Vec2 {
    // Only snap while a drag is active (not on hover)
    let result = fanimate_editor::snap::snap(
        p,
        &shell.snap,
        &shell.grid,
        &shell.guides,
        shell.editor.doc.size(),
        shell.view.zoom,
    );
    shell.ui.snap_hint = result.hint;
    result.position
}

const FIT_MARGIN: f32 = 32.0;

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    // Clear the snap hint from the previous frame; tool gestures set it again
    // during this frame if a drag is active.
    shell.ui.snap_hint = None;

    let full_rect = ui.available_rect_before_wrap();

    // Reserve ruler strips along the top and left.
    let ruler_size = if shell.show_rulers {
        crate::panels::rulers::RULER_SIZE
    } else {
        0.0
    };
    let rect = egui::Rect::from_min_max(full_rect.min + egui::vec2(ruler_size, ruler_size), full_rect.max);

    let response = ui.allocate_rect(full_rect, Sense::click_and_drag());
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let doc_size = shell.editor.doc.size();

    if shell.view.fit_pending && rect.width() > 1.0 {
        let avail = (Vec2::new(rect.width(), rect.height()) - Vec2::splat(2.0 * FIT_MARGIN)).max(Vec2::ONE);
        shell.view.zoom = (avail / doc_size.max(Vec2::ONE)).min_element();
        shell.view.center = doc_size / 2.0;
        shell.view.fit_pending = false;
    }

    let to_screen = |center: Vec2, zoom: f32, p: Vec2| -> Pos2 {
        let s = (p - center) * zoom;
        rect.center() + egui::vec2(s.x, s.y)
    };
    let to_doc = |center: Vec2, zoom: f32, p: Pos2| -> Vec2 {
        let d = p - rect.center();
        center + Vec2::new(d.x, d.y) / zoom
    };

    // Rulers and guides (before the canvas content so guides appear on top
    // of the grid but under the artwork).
    {
        let (center, zoom) = (shell.view.center, shell.view.zoom);
        let to_screen_v = |p: Vec2| to_screen(center, zoom, p);
        let to_doc_z = |z: f32, p: Pos2| to_doc(center, z, p);
        crate::panels::rulers::rulers(ui, shell, full_rect, &to_doc_z, &to_screen_v);
        // Grid (drawn before the GPU texture)
        let painter = ui.painter_at(rect);
        crate::panels::rulers::grid(&painter, shell, rect, &to_screen_v);
    }

    navigate(&ctx, &response, shell, rect, &to_doc);

    // The selection frame and the corner radius handles come first: a handle
    // takes the pointer before the tool does. They are found where they were
    // drawn last frame, so what you click is what you see.
    let screen = Screen {
        origin: rect.center(),
        center: shell.view.center,
        zoom: shell.view.zoom,
    };
    let tool = shell.editor.tool;
    let on_canvas = matches!(tool, Tool::TransformLayer | Tool::SelectPoints);
    let frame_seen = shell.ui.frame.clone();
    let radius_seen = if on_canvas {
        shell.editor.radius_target()
    } else {
        None
    };
    let handle_busy = on_canvas && handles::interact(&ctx, &response, shell, &screen, frame_seen.as_ref());
    let on_handle = response.clicked_by(egui::PointerButton::Primary)
        && response.interact_pointer_pos().is_some_and(|p| {
            handles::hit_test(&screen, frame_seen.as_ref(), radius_seen.as_ref(), p).is_some()
        });
    let tools_free = !handle_busy && !on_handle;

    if tools_free {
        transform_tool(&ctx, &response, shell, &to_doc);
    }
    freehand_tool(&ctx, &response, shell, &to_doc);
    eraser_tool(&ctx, &response, shell, &to_doc);
    let shape_preview = shape_tools(&ctx, &response, shell, &to_doc);
    let shape_e_preview = shape_tools_e(&ctx, &response, shell, &to_doc);
    click_tools(&response, shell, &to_doc);
    click_tools_e(&ctx, &response, shell, &to_doc);
    if tools_free {
        point_tools(&ctx, &response, shell, &to_doc);
    }
    editing_tools_e(&ctx, &response, shell, &to_doc);
    add_point_tool(&response, shell, &to_doc);
    bone_tools(&ctx, &response, shell, &to_doc);
    pen_tool(&ctx, &response, shell, &to_doc);
    curvature_tool(&ctx, &response, shell, &to_doc);

    // what is under the pointer decides the cursor: a handle, else the tool
    let hover_hit = response
        .hover_pos()
        .filter(|_| !handle_busy)
        .and_then(|p| handles::hit_test(&screen, frame_seen.as_ref(), radius_seen.as_ref(), p));
    if let Some(hit) = hover_hit {
        ctx.set_cursor_icon(handles::cursor(&screen, frame_seen.as_ref(), hit));
    } else if response.hovered() || response.dragged() {
        ctx.set_cursor_icon(tool_cursor(&ctx, &response, shell.editor.tool));
    }
    // the frame as it is now, for drawing and for the next frame's clicks
    shell.ui.frame = (tool == Tool::TransformLayer)
        .then(|| shell.editor.selection_frame())
        .flatten();

    // render the document into a texture and show it
    let aids = canvas_overlays(shell);
    if let (Some(canvas), Some(rs)) = (&mut shell.canvas, &shell.render_state) {
        let size_px = [
            (rect.width() * ppp).round() as u32,
            (rect.height() * ppp).round() as u32,
        ];
        let view = View::looking_at(
            shell.view.center,
            Vec2::new(size_px[0] as f32, size_px[1] as f32),
            shell.view.zoom * ppp,
        );
        let p = shell.palette.pasteboard;
        let pasteboard = Color::rgb(p.r() as f32 / 255.0, p.g() as f32 / 255.0, p.b() as f32 / 255.0);
        let pointer = response
            .hover_pos()
            .map(|p| to_doc(shell.view.center, shell.view.zoom, p));
        let preview = shell
            .editor
            .stroke_preview()
            .or(shape_preview)
            .or(shape_e_preview)
            .or_else(|| shell.editor.pen_preview(pointer))
            .or_else(|| shell.editor.curvature_preview(pointer));
        let id = canvas.draw(
            rs,
            &shell.editor.doc,
            shell.editor.frame,
            size_px,
            &view,
            pasteboard,
            preview.as_ref(),
            &aids,
        );
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        ui.painter().image(id, rect, uv, Color32::WHITE);
    } else {
        ui.painter().rect_filled(rect, 0.0, shell.palette.pasteboard);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No GPU available for the canvas",
            egui::FontId::proportional(14.0),
            shell.palette.error,
        );
    }

    // Snap hint line over the artwork
    if let Some((a, b)) = shell.ui.snap_hint {
        let sa = to_screen(shell.view.center, shell.view.zoom, a);
        let sb = to_screen(shell.view.center, shell.view.zoom, b);
        let painter = ui.painter_at(rect);
        painter.line_segment(
            [sa, sb],
            Stroke::new(1.0, shell.palette.accent.gamma_multiply(0.6)),
        );
    }

    overlays(ui, shell, rect, &to_screen);
    selection_overlay(ui, shell, &response, &screen, hover_hit);
    crate::panels::selbar::show(ui.ctx(), shell, rect, &screen);
    point_overlay(ui, shell, rect, &to_screen);
    pen_overlay(ui, shell, rect, &to_screen);
    curvature_overlay(ui, shell, rect, &to_screen);
    weight_overlay(ui, shell, rect, &to_screen);
    brush_cursor(ui, &response, shell);

    menus::context_menu(&response, |ui| {
        menus::item(ui, shell, Action::FitView);
        menus::item(ui, shell, Action::ActualSize);
        menus::item(ui, shell, Action::ZoomIn);
        menus::item(ui, shell, Action::ZoomOut);
        ui.separator();
        if shell.editor.tool.edits_points() {
            menus::shape_items(ui, shell);
            ui.separator();
        }
        menus::layer_context(ui, shell);
    });
}

/// Onion skins and the light table, from the view settings. Not while
/// playing: they would only flicker.
fn canvas_overlays(shell: &Shell) -> crate::canvas_gpu::Overlays {
    let ed = &shell.editor;
    let mut out = crate::canvas_gpu::Overlays::default();
    if shell.light_table && !ed.selection.is_empty() {
        out.light_table = Some(ed.selection.clone());
    }
    let o = shell.onion;
    if !o.on || ed.playing {
        return out;
    }
    let to_color = |c: egui::Color32| {
        fanimate_core::Color::rgb(c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0)
    };
    let (past, future) = ed.onion_frames(o.before, o.after, o.keys);
    let fade = |k: usize, n: usize| 0.45 * (1.0 - k as f32 / (n as f32 + 1.0));
    // farthest first, so nearer skins are on top
    for (k, f) in past.iter().enumerate().rev() {
        out.onion
            .push((*f as f32, to_color(shell.palette.onion_past), fade(k, past.len())));
    }
    for (k, f) in future.iter().enumerate().rev() {
        out.onion.push((
            *f as f32,
            to_color(shell.palette.onion_future),
            fade(k, future.len()),
        ));
    }
    if !o.all_layers && !ed.selection.is_empty() {
        out.onion_layers = Some(ed.selection.clone());
    }
    out
}

/// The pointer over the canvas says what the active tool will do.
fn tool_cursor(ctx: &egui::Context, response: &egui::Response, tool: Tool) -> egui::CursorIcon {
    use egui::CursorIcon;
    if response.dragged_by(egui::PointerButton::Middle) {
        return CursorIcon::Move;
    }
    match tool {
        // Windows has no hand cursors, so panning shows the move arrows
        Tool::Pan | Tool::TransformLayer | Tool::TranslatePoints => CursorIcon::Move,
        Tool::SelectPoints | Tool::SelectBone | Tool::ManipulateBones => CursorIcon::Default,
        Tool::Zoom if ctx.input(|i| i.modifiers.alt) => CursorIcon::ZoomOut,
        Tool::Zoom => CursorIcon::ZoomIn,
        Tool::Freehand
        | Tool::FreehandWidth
        | Tool::AddPoint
        | Tool::Pen
        | Tool::Curvature
        | Tool::AddBone
        | Tool::PaintWeights
        | Tool::Eraser
        | Tool::Rectangle
        | Tool::Ellipse
        | Tool::Line
        | Tool::Polygon
        | Tool::Star
        | Tool::Triangle
        | Tool::DeleteEdge
        | Tool::Magnet
        | Tool::ScalePoints
        | Tool::RotatePoints
        | Tool::DrawShape
        | Tool::LineWidth
        | Tool::StrokeExposure
        | Tool::ShapeBuilder
        | Tool::PaintBucket
        | Tool::Eyedropper => CursorIcon::Crosshair,
    }
}

fn navigate(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    rect: Rect,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    let view = &mut shell.view;
    // pan: middle-drag with any tool, or the Pan tool
    let panning = response.dragged_by(egui::PointerButton::Middle)
        || (shell.editor.tool == Tool::Pan && response.dragged_by(egui::PointerButton::Primary));
    if panning {
        let d = response.drag_delta();
        view.center -= Vec2::new(d.x, d.y) / view.zoom;
    }
    if !response.hovered() {
        return;
    }
    // Ctrl+scroll or pinch zooms about the pointer; plain scroll pans
    let (zoom_delta, scroll, pointer) =
        ctx.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
    let pointer = pointer.unwrap_or(rect.center());
    if zoom_delta != 1.0 {
        zoom_about(view, pointer, zoom_delta, to_doc);
    } else if scroll != egui::Vec2::ZERO {
        view.center -= Vec2::new(scroll.x, scroll.y) / view.zoom;
    }
    // Zoom tool: click in, Alt-click out
    if shell.editor.tool == Tool::Zoom && response.clicked() {
        let factor = if ctx.input(|i| i.modifiers.alt) { 0.8 } else { 1.25 };
        zoom_about(view, pointer, factor, to_doc);
    }
}

fn zoom_about(
    view: &mut crate::app::CanvasView,
    pointer: Pos2,
    factor: f32,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    let before = to_doc(view.center, view.zoom, pointer);
    view.zoom = (view.zoom * factor).clamp(0.02, 64.0);
    let after = to_doc(view.center, view.zoom, pointer);
    // keep the document point under the pointer still
    view.center += before - after;
}

fn transform_tool(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    if shell.editor.tool != Tool::TransformLayer {
        return;
    }
    let mods = ctx.input(|i| i.modifiers);

    // click: select the topmost shape under the pointer
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let p = to_doc(shell.view.center, shell.view.zoom, pos);
        match hit_test(shell, p) {
            Some(id) => {
                let mode = if mods.command {
                    SelectMode::Toggle
                } else if mods.shift {
                    SelectMode::Range
                } else {
                    SelectMode::Replace
                };
                shell.editor.select(id, mode);
            }
            None if !mods.command && !mods.shift => shell.editor.selection.clear(),
            None => {}
        }
    }

    // drag: move the selected layer (pick one under the pointer if none)
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        if shell.editor.primary().is_none()
            && let Some(id) = hit_test(shell, to_doc(shell.view.center, shell.view.zoom, pos))
        {
            shell.editor.select(id, SelectMode::Replace);
        }
        if let Some(id) = shell.editor.primary()
            && let Some(t) = shell.editor.layer_transform(id)
        {
            shell.editor.checkpoint("Move layer");
            shell.ui.drag_origin = Some((id, t));
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some((id, origin)) = shell.ui.drag_origin
    {
        let total = response.total_drag_delta().unwrap_or_default();
        let raw_delta = Vec2::new(total.x, total.y) / shell.view.zoom;
        let snapped_pos = do_snap(shell, origin.position + raw_delta);
        let doc_delta = snapped_pos - origin.position;
        // drag in document space -> movement in the layer's parent space
        let parent = shell.editor.parent_world(id);
        let local = parent.matrix2.inverse() * doc_delta;
        let mut t = origin;
        t.position += if local.is_finite() { local } else { doc_delta };
        shell.editor.set_layer_transform(id, t);
    }
    if response.drag_stopped() {
        shell.ui.drag_origin = None;
    }
}

/// Brush strokes. Every pointer position since the last frame is used, not
/// just the latest, so fast strokes keep their shape. A pen reports touch
/// events with pressure; a mouse has none and draws at full pressure.
fn freehand_tool(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    if !matches!(shell.editor.tool, Tool::Freehand | Tool::FreehandWidth) {
        if shell.editor.is_drawing() {
            shell.editor.cancel_stroke();
        }
        return;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let doc = |p: Pos2| to_doc(center, zoom, p);
    let (moves, press_origin) = pointer_moves(ctx);
    let pressure = moves.first().map_or(1.0, |m| m.1);

    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        // a tap leaves a dot
        if shell.editor.begin_stroke(doc(pos), pressure, zoom) {
            shell.editor.finish_stroke();
        }
        return;
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        // start where the button went down: the drag only registers after
        // the pointer has moved a little
        let start = press_origin.or(response.interact_pointer_pos());
        if let Some(start) = start
            && !shell.editor.begin_stroke(doc(start), pressure, zoom)
        {
            return;
        }
    }
    if shell.editor.is_drawing() {
        for (pos, pressure) in &moves {
            shell.editor.continue_stroke(doc(*pos), *pressure);
        }
        if response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
            shell.editor.finish_stroke();
        }
    }
}

/// Every pointer position since the last frame, with pen pressure (1 for
/// a mouse), and where the button went down.
fn pointer_moves(ctx: &egui::Context) -> (Vec<(Pos2, f32)>, Option<Pos2>) {
    ctx.input(|i| {
        let (mut pen, mut mouse) = (Vec::new(), Vec::new());
        for event in &i.events {
            match event {
                egui::Event::Touch {
                    pos,
                    force,
                    phase: egui::TouchPhase::Start | egui::TouchPhase::Move,
                    ..
                } => pen.push((*pos, force.unwrap_or(1.0))),
                egui::Event::PointerMoved(pos) => mouse.push((*pos, 1.0)),
                _ => {}
            }
        }
        // egui also turns pen input into pointer moves; prefer the pen's own
        (if pen.is_empty() { mouse } else { pen }, i.pointer.press_origin())
    })
}

/// The Eraser: one drag is one undo step.
fn eraser_tool(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    if shell.editor.tool != Tool::Eraser {
        shell.editor.end_erase();
        return;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let doc = |p: Pos2| to_doc(center, zoom, p);
    let (moves, press_origin) = pointer_moves(ctx);
    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        shell.editor.begin_erase(doc(pos), zoom);
        shell.editor.end_erase();
        return;
    }
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = press_origin.or(response.interact_pointer_pos())
        && !shell.editor.begin_erase(doc(start), zoom)
    {
        return;
    }
    if shell.editor.is_erasing() {
        for (pos, _) in &moves {
            shell.editor.erase_to(doc(*pos));
        }
        if response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
            shell.editor.end_erase();
        }
    }
}

/// A circle the size of the brush or eraser around the pointer, drawn in a
/// dark and a light ring so it shows on any artwork.
fn brush_cursor(ui: &egui::Ui, response: &egui::Response, shell: &Shell) {
    let size = match shell.editor.tool {
        Tool::Freehand => shell.editor.draw.width,
        Tool::Eraser => shell.editor.draw.eraser,
        Tool::PaintWeights => shell.editor.draw.weight_brush,
        _ => return,
    };
    let Some(pos) = response.hover_pos().or(response.interact_pointer_pos()) else {
        return;
    };
    let r = size * shell.view.zoom / 2.0;
    if r >= 2.0 {
        let painter = ui.painter_at(response.rect);
        painter.circle_stroke(pos, r, Stroke::new(1.0, shell.palette.bg));
        painter.circle_stroke(pos, r + 1.0, Stroke::new(1.0, shell.palette.text));
    }
}

/// How close, in screen pixels, a click must be to a shape to hit it.
const HIT_SLOP_PX: f32 = 4.0;

/// The layer of the topmost shape under `p`.
fn hit_test(shell: &Shell, p: Vec2) -> Option<LayerId> {
    shell
        .editor
        .shape_at(p, HIT_SLOP_PX / shell.view.zoom)
        .map(|(layer, _)| layer)
}

/// Rectangle and Ellipse: drag out a shape. Shift keeps the sides equal;
/// Alt grows it from where the drag started. Returns the shape being
/// dragged, to preview.
fn shape_tools(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) -> Option<fanimate_core::Shape> {
    let tool = shell.editor.tool;
    if !matches!(tool, Tool::Rectangle | Tool::Ellipse) {
        shell.ui.shape_drag = None;
        return None;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let (mods, press_origin) = ctx.input(|i| (i.modifiers, i.pointer.press_origin()));
    if response.drag_started_by(egui::PointerButton::Primary) {
        shell.ui.shape_drag = press_origin.map(|p| to_doc(center, zoom, p));
    }
    let from = shell.ui.shape_drag?;
    let to = do_snap(shell, to_doc(center, zoom, response.interact_pointer_pos()?));
    if response.drag_stopped() {
        shell.ui.shape_drag = None;
        shell
            .editor
            .add_drag_shape(tool, from, to, mods.shift, mods.alt, zoom);
        return None;
    }
    shell
        .editor
        .drag_shape(tool, from, to, mods.shift, mods.alt, zoom)
}

/// A point gesture on the canvas.
#[derive(Clone, Copy, Debug)]
pub enum PointGesture {
    /// Moving the selected points; where the pointer was last frame.
    Move(Vec2),
    /// Dragging a selection box from this screen position.
    Box(Pos2),
}

/// Select Points and Transform Points. Clicking a point selects it (Shift
/// or Ctrl adds). With Select Points, dragging a selected point moves the
/// selection and dragging elsewhere draws a selection box; Transform
/// Points moves the selection from anywhere.
fn point_tools(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    let tool = shell.editor.tool;
    if !matches!(tool, Tool::SelectPoints | Tool::TranslatePoints) {
        // Add Point keeps its own drag in the same place
        if tool != Tool::AddPoint {
            shell.ui.point_gesture = None;
        }
        return;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let doc = |p: Pos2| to_doc(center, zoom, p);
    let slop = POINT_SLOP_PX / zoom;
    let (mods, press_origin) = ctx.input(|i| (i.modifiers, i.pointer.press_origin()));
    let adding = mods.shift || mods.command;
    let mode = if mods.command {
        SelectMode::Toggle
    } else if mods.shift {
        SelectMode::Range
    } else {
        SelectMode::Replace
    };

    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        match shell.editor.point_at(doc(pos), slop) {
            Some(point) => shell.editor.select_point(point, mode),
            // not on a point: on a shape, which is chosen whole
            None => match shell.editor.shape_to_select_at(doc(pos), HIT_SLOP_PX / zoom) {
                Some((layer, index)) => shell.editor.select_shape(layer, index, mode),
                None if !adding => shell.editor.points.clear(),
                None => {}
            },
        }
        return;
    }

    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = press_origin
    {
        let hit = shell.editor.point_at(doc(start), slop);
        if let Some(point) = hit
            && !shell.editor.points.contains(&point)
        {
            shell
                .editor
                .select_point(point, if adding { mode } else { SelectMode::Replace });
        }
        // pressing on a shape's body takes the whole shape along (choosing
        // it first if it is not chosen), so it can be dragged without
        // choosing every point
        let body = if hit.is_none() && tool == Tool::SelectPoints {
            shell.editor.shape_to_select_at(doc(start), HIT_SLOP_PX / zoom)
        } else {
            None
        };
        if let Some((layer, index)) = body
            && !shell.editor.shape_selected(layer, index)
        {
            shell
                .editor
                .select_shape(layer, index, if adding { mode } else { SelectMode::Replace });
        }
        let moving = hit.is_some() || body.is_some() || tool == Tool::TranslatePoints;
        if tool == Tool::TranslatePoints && hit.is_none() && shell.editor.points.is_empty() {
            // nothing selected: move the nearest point, if one is close
            if let Some(point) = shell.editor.point_at(doc(start), slop * 4.0) {
                shell.editor.select_point(point, SelectMode::Replace);
            }
        }
        shell.ui.point_gesture = if moving {
            shell
                .editor
                .begin_point_drag()
                .then_some(PointGesture::Move(doc(start)))
        } else {
            Some(PointGesture::Box(start))
        };
    }

    let Some(gesture) = shell.ui.point_gesture else {
        return;
    };
    let Some(pos) = response.interact_pointer_pos() else {
        return;
    };
    match gesture {
        PointGesture::Move(last) => {
            let raw = doc(pos);
            let now = do_snap(shell, raw);
            shell.editor.drag_points(now - last);
            shell.ui.point_gesture = Some(PointGesture::Move(now));
        }
        PointGesture::Box(start) if response.drag_stopped() => {
            // a box picks the shapes it touches; with Alt, only the points inside it
            if mods.alt {
                shell.editor.select_points_in(doc(start), doc(pos), adding);
            } else {
                shell.editor.select_shapes_touching(doc(start), doc(pos), adding);
            }
        }
        PointGesture::Box(_) => {}
    }
    if response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
        shell.ui.point_gesture = None;
    }
}

/// How close, in screen pixels, a click must be to a point to pick it.
const POINT_SLOP_PX: f32 = 6.0;

/// Add Point: click a line to add a point on it; drag from an open end to
/// extend the path, or from empty space to start a line, the new point
/// following the pointer.
fn add_point_tool(response: &egui::Response, shell: &mut Shell, to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2) {
    if shell.editor.tool != Tool::AddPoint {
        return;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let doc = |p: Pos2| to_doc(center, zoom, p);
    let slop = POINT_SLOP_PX / zoom;
    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
        && !shell.editor.insert_point_at(doc(pos), slop)
    {
        shell.editor.set_status(
            fanimate_editor::StatusKind::Info,
            "Click on a line, or drag to draw one",
        );
    }
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = response.ctx.input(|i| i.pointer.press_origin())
        && shell.editor.begin_add_point(doc(start), slop)
    {
        shell.ui.point_gesture = Some(PointGesture::Move(doc(start)));
    }
    if let Some(PointGesture::Move(last)) = shell.ui.point_gesture
        && let Some(pos) = response.interact_pointer_pos()
    {
        let raw = doc(pos);
        let now = do_snap(shell, raw);
        shell.editor.drag_points(now - last);
        shell.ui.point_gesture = Some(PointGesture::Move(now));
    }
    if response.drag_stopped() || (shell.editor.is_adding_point() && !response.dragged()) {
        shell.editor.end_add_point(slop);
        shell.ui.point_gesture = None;
    }
}

/// How close, in screen pixels, a click must be to a bone to pick it.
const BONE_SLOP_PX: f32 = 6.0;

/// Add Bone, Select Bone, Manipulate Bones and Paint Weights.
fn bone_tools(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    let tool = shell.editor.tool;
    if !matches!(
        tool,
        Tool::AddBone | Tool::SelectBone | Tool::ManipulateBones | Tool::PaintWeights
    ) {
        return;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let doc = |p: Pos2| to_doc(center, zoom, p);
    let slop = BONE_SLOP_PX / zoom;
    let (mods, press_origin) = ctx.input(|i| (i.modifiers, i.pointer.press_origin()));

    // a click picks a bone (or clears the choice) with any bone tool but Add
    if response.clicked_by(egui::PointerButton::Primary)
        && tool != Tool::AddBone
        && let Some(pos) = response.interact_pointer_pos()
    {
        if tool == Tool::PaintWeights {
            if shell.editor.begin_paint_weights() {
                shell
                    .editor
                    .paint_weights(doc(pos), shell.editor.draw.weight_brush / 2.0, 0.2, mods.alt);
                shell.editor.end_paint_weights();
            }
        } else {
            let bone = shell.editor.bone_at(doc(pos), slop);
            shell.editor.select_bone(bone);
        }
        return;
    }
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = press_origin.or(response.interact_pointer_pos())
    {
        let at = doc(start);
        match tool {
            Tool::AddBone => {
                shell.editor.begin_add_bone(at);
            }
            Tool::SelectBone => {
                if !shell.editor.begin_bone_edit(at, slop) {
                    let bone = shell.editor.bone_at(at, slop);
                    shell.editor.select_bone(bone);
                    shell.editor.begin_bone_edit(at, slop);
                }
            }
            Tool::ManipulateBones => {
                shell.editor.begin_pose(at, slop, mods.alt);
            }
            _ => {
                shell.editor.begin_paint_weights();
            }
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        if tool == Tool::PaintWeights {
            // per frame, so the brush builds up gently while held
            let amount = (ctx.input(|i| i.stable_dt) * 1.5).min(0.2);
            shell
                .editor
                .paint_weights(doc(pos), shell.editor.draw.weight_brush / 2.0, amount, mods.alt);
            ctx.request_repaint();
        } else if shell.editor.is_dragging_bone() {
            let mut tip = do_snap(shell, doc(pos));
            // Shift + AddBone: snap the bone angle to 15° multiples
            if tool == Tool::AddBone
                && mods.shift
                && let Some(joint) = shell.editor.bone_drag_joint()
            {
                let raw_angle = (tip - joint).y.atan2((tip - joint).x);
                let snapped = fanimate_editor::snap::snap_angle(raw_angle, shell.snap.angle_step);
                let len = (tip - joint).length();
                tip = joint + Vec2::new(snapped.cos(), snapped.sin()) * len;
            }
            shell.editor.drag_bone(tip);
        }
    }
    if response.drag_stopped() {
        shell.editor.end_bone_drag();
        shell.editor.end_paint_weights();
    }
}

/// With Paint Weights, each point of the bound layer as a dot whose
/// strength shows how much it follows the selected bone.
fn weight_overlay(ui: &egui::Ui, shell: &Shell, rect: Rect, to_screen: &impl Fn(Vec2, f32, Vec2) -> Pos2) {
    if shell.editor.tool != Tool::PaintWeights {
        return;
    }
    let painter = ui.painter_at(rect);
    let (c, z, p) = (shell.view.center, shell.view.zoom, shell.palette);
    for (pos, w) in shell.editor.bone_weights() {
        let s = to_screen(c, z, pos);
        painter.circle_filled(s, 5.0, p.panel);
        painter.circle_filled(s, 4.0, p.bone.gamma_multiply(w.clamp(0.0, 1.0)));
        painter.circle_stroke(s, 5.0, Stroke::new(1.0, p.bone));
    }
}

/// The Pen: press to add a point, drag before releasing to make it smooth.
fn pen_tool(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    if shell.editor.tool != Tool::Pen {
        return;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let doc = |p: Pos2| to_doc(center, zoom, p);
    let slop = POINT_SLOP_PX / zoom;
    let (pressed, released, origin) = ctx.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.press_origin(),
        )
    });
    // react to the press itself, not the click, so a drag shapes the curve
    if pressed
        && response.hovered()
        && let Some(at) = origin
    {
        shell.ui.pen_down = shell.editor.pen_down(doc(at), slop);
    }
    if shell.ui.pen_down {
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let snapped = do_snap(shell, doc(pos));
            shell.editor.pen_drag(snapped);
        }
        if released {
            shell.ui.pen_down = false;
            shell.editor.pen_up();
        }
    }
}

/// The Pen path's points and the handles of smooth ones.
fn pen_overlay(ui: &egui::Ui, shell: &Shell, rect: Rect, to_screen: &impl Fn(Vec2, f32, Vec2) -> Pos2) {
    if shell.editor.tool != Tool::Pen {
        return;
    }
    let painter = ui.painter_at(rect);
    let (c, z, p) = (shell.view.center, shell.view.zoom, shell.palette);
    for (i, (at, handle)) in shell.editor.pen_points().into_iter().enumerate() {
        let s = to_screen(c, z, at);
        if handle != Vec2::ZERO {
            for h in [at + handle, at - handle] {
                let hs = to_screen(c, z, h);
                painter.line_segment([s, hs], Stroke::new(1.0, p.accent));
                painter.circle_filled(hs, 3.0, p.accent);
            }
        }
        let r = Rect::from_center_size(s, egui::vec2(7.0, 7.0));
        // the first point is filled: clicking it closes the shape
        painter.rect_filled(r, 1.0, if i == 0 { p.accent } else { p.panel });
        painter.rect_stroke(r, 1.0, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
    }
}

/// The point layer's points as small squares, filled when selected, and
/// the selection box being dragged.
fn point_overlay(ui: &egui::Ui, shell: &Shell, rect: Rect, to_screen: &impl Fn(Vec2, f32, Vec2) -> Pos2) {
    if !shell.editor.tool.edits_points() {
        return;
    }
    let painter = ui.painter_at(rect);
    let (c, z, p) = (shell.view.center, shell.view.zoom, shell.palette);
    // the points take the colour of their layer's label, like the frame
    let color = shell
        .editor
        .point_layer()
        .and_then(|id| shell.editor.doc.find(id))
        .map_or(p.accent, |l| crate::theme::label_color(l.shown_label()));
    // the shapes chosen, outlined
    let screen = Screen {
        origin: rect.center(),
        center: c,
        zoom: z,
    };
    for outline in shell.editor.selected_shape_outlines() {
        handles::paint_outline(&painter, &screen, &outline, Stroke::new(1.5, color));
    }
    for (point, pos) in shell.editor.point_positions() {
        let r = Rect::from_center_size(to_screen(c, z, pos), egui::vec2(7.0, 7.0));
        let selected = shell.editor.points.contains(&point);
        painter.rect_filled(r, 1.0, if selected { color } else { p.panel });
        painter.rect_stroke(
            r,
            1.0,
            Stroke::new(1.0, if selected { p.text } else { color }),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(PointGesture::Box(start)) = shell.ui.point_gesture
        && let Some(now) = ui.ctx().pointer_latest_pos()
    {
        let r = Rect::from_two_pos(start, now);
        painter.rect_filled(r, 0.0, p.accent.gamma_multiply(0.12));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
    }
}

/// Paint Bucket and Eyedropper act on the shape clicked.
fn click_tools(response: &egui::Response, shell: &mut Shell, to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2) {
    let tool = shell.editor.tool;
    if !matches!(tool, Tool::PaintBucket | Tool::Eyedropper)
        || !response.clicked_by(egui::PointerButton::Primary)
    {
        return;
    }
    let Some(pos) = response.interact_pointer_pos() else {
        return;
    };
    let p = to_doc(shell.view.center, shell.view.zoom, pos);
    let slop = HIT_SLOP_PX / shell.view.zoom;
    let hit = if tool == Tool::PaintBucket {
        shell.editor.paint_at(p, slop) || shell.editor.shape_at(p, slop).is_some()
    } else {
        shell.editor.pick_style_at(p, slop)
    };
    if !hit {
        shell
            .editor
            .set_status(fanimate_editor::StatusKind::Info, "Click on a shape");
    }
}

/// Phase E: click-based tools (DrawShape).
fn click_tools_e(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    let tool = shell.editor.tool;
    if !matches!(tool, Tool::DrawShape) {
        return;
    }
    // Fire on a click (no movement) OR a very short primary-button drag
    // (stylus / mouse jitter). `clicked_by` requires the pointer to stay
    // within egui's own click-distance threshold; the drag_stopped path
    // catches releases that are just outside it.
    let (press_origin, primary_released) =
        ctx.input(|i| (i.pointer.press_origin(), i.pointer.primary_released()));
    let is_tap = response.clicked_by(egui::PointerButton::Primary);
    let is_short_drag = primary_released
        && response
            .interact_pointer_pos()
            .zip(press_origin)
            .is_some_and(|(end, start)| (end - start).length() <= HIT_SLOP_PX * 3.0);
    if !is_tap && !is_short_drag {
        return;
    }
    let Some(pos) = response.interact_pointer_pos() else {
        return;
    };
    let _ = ctx; // modifiers available if needed in future
    let p = to_doc(shell.view.center, shell.view.zoom, pos);
    let slop = HIT_SLOP_PX / shell.view.zoom;
    if !shell.editor.pick_draw_style(p, slop) {
        shell.editor.set_status(
            fanimate_editor::StatusKind::Info,
            "Click on a shape to copy its colours",
        );
    }
}

/// A bone layer's bones as tapered shapes from joint to tip, the selected
/// one in the interaction colour, pinned ones with a square joint.
fn bone_overlay(
    painter: &egui::Painter,
    shell: &Shell,
    rig: LayerId,
    to_screen: &impl Fn(Vec2, f32, Vec2) -> Pos2,
) {
    let ed = &shell.editor;
    let (c, z, p) = (shell.view.center, shell.view.zoom, shell.palette);
    let selected = ed.selected_bone().filter(|b| b.layer == rig).map(|b| b.bone);
    for (i, joint, tip) in ed.bone_positions(rig) {
        let (sa, sb) = (to_screen(c, z, joint), to_screen(c, z, tip));
        let color = if selected == Some(i) { p.accent } else { p.bone };
        let d = sb - sa;
        let len = d.length();
        if len > 1.0 {
            // a kite: wide near the joint, to a point at the tip
            let n = egui::vec2(-d.y, d.x) / len * (len * 0.12).clamp(3.0, 9.0);
            let m = sa + d * 0.15;
            painter.add(egui::Shape::convex_polygon(
                vec![sa, m + n, sb, m - n],
                color.gamma_multiply(0.55),
                Stroke::new(1.5, color),
            ));
        }
        let pinned = ed
            .bone(fanimate_editor::BoneRef { layer: rig, bone: i })
            .is_some_and(|b| b.pinned);
        if pinned {
            let r = Rect::from_center_size(sa, egui::vec2(9.0, 9.0));
            painter.rect_filled(r, 1.0, color);
            painter.rect_stroke(r, 1.0, Stroke::new(1.0, p.bg), egui::StrokeKind::Outside);
        } else {
            painter.circle_filled(sa, 4.5, color);
            painter.circle_stroke(sa, 4.5, Stroke::new(1.0, p.bg));
        }
    }
}

/// The Transform tool's frame and handles, a rectangle's radius handles,
/// the label beside a dragged handle, and the outline of the shape the
/// pointer is over, which says what a click would choose.
fn selection_overlay(
    ui: &egui::Ui,
    shell: &mut Shell,
    response: &egui::Response,
    screen: &Screen,
    hover: Option<Hit>,
) {
    let tool = shell.editor.tool;
    if !matches!(tool, Tool::TransformLayer | Tool::SelectPoints) {
        shell.ui.hover = None;
        return;
    }
    let painter = ui.painter_at(response.rect);
    let p = shell.palette;

    // the outline of what a click would choose
    let busy = shell.ui.handle_gesture.is_some()
        || shell.ui.point_gesture.is_some()
        || shell.ui.drag_origin.is_some()
        || shell.editor.playing;
    let pointer = response.hover_pos().filter(|_| !busy && hover.is_none());
    if let Some(outline) = hover_outline(shell, screen, pointer) {
        handles::paint_outline(&painter, screen, &outline, Stroke::new(1.0, p.text_muted));
    }

    if let Some(frame) = shell.ui.frame.clone() {
        handles::paint_frame(&painter, shell, screen, &frame, hover);
    }
    if let Some(target) = shell.editor.radius_target() {
        handles::paint_radius(&painter, shell, screen, &target, hover);
    }
    if shell.ui.handle_gesture.is_some()
        && let Some(pos) = ui.ctx().pointer_latest_pos()
    {
        handles::paint_readout(&painter, shell, pos);
    }
}

/// The outline of the shape under `pointer`, kept between frames: it is only
/// worked out again when the pointer, the document, the frame or the view
/// has changed, so hovering costs nothing while the mouse is still.
fn hover_outline(
    shell: &mut Shell,
    screen: &Screen,
    pointer: Option<Pos2>,
) -> Option<Vec<(Vec<Vec2>, bool)>> {
    let Some(pos) = pointer else {
        shell.ui.hover = None;
        return None;
    };
    let key = (
        pos,
        shell.editor.revision(),
        shell.editor.current_frame(),
        screen.zoom.to_bits(),
        screen.center.x.to_bits() ^ screen.center.y.to_bits().rotate_left(7),
    );
    if let Some(cache) = &shell.ui.hover
        && cache.key == key
    {
        return (!cache.outline.is_empty()).then(|| cache.outline.clone());
    }
    let hit = shell.editor.shape_at(screen.doc(pos), HIT_SLOP_PX / screen.zoom);
    let outline = hit.map_or_else(Vec::new, |(layer, index)| {
        shell.editor.shape_outline(layer, index)
    });
    shell.ui.hover = Some(crate::panels::handles::HoverCache {
        key,
        outline: outline.clone(),
    });
    (!outline.is_empty()).then_some(outline)
}

fn bounds(points: impl Iterator<Item = Vec2>) -> Option<(Vec2, Vec2)> {
    points.fold(None, |acc, q| match acc {
        None => Some((q, q)),
        Some((lo, hi)) => Some((lo.min(q), hi.max(q))),
    })
}

/// Outlines of the selected layers (in each one's label colour, as the
/// Transform tool's frame draws them) and the bones of selected bone layers.
fn overlays(ui: &egui::Ui, shell: &Shell, rect: Rect, to_screen: &impl Fn(Vec2, f32, Vec2) -> Pos2) {
    let ed = &shell.editor;
    if ed.selection.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    let (c, z) = (shell.view.center, shell.view.zoom);

    // the Transform tool draws a frame with handles instead of a bare box
    let outlines = ed.tool != Tool::TransformLayer;
    let items = if outlines {
        evaluate(&ed.doc, ed.frame)
    } else {
        Vec::new()
    };
    let mut drawn = Vec::new();
    for sel in &ed.selection {
        let pts = items
            .iter()
            .filter(|i| ed.doc.is_within(i.layer, *sel))
            .flat_map(|i| {
                i.shape
                    .path
                    .points()
                    .map(move |q| i.transform.transform_point2(q))
            });
        if let Some((lo, hi)) = bounds(pts) {
            let r = Rect::from_two_pos(to_screen(c, z, lo), to_screen(c, z, hi)).expand(3.0);
            let color = ed.doc.find(*sel).map_or(shell.palette.accent, |l| {
                crate::theme::label_color(l.shown_label())
            });
            painter.rect_stroke(r, 2.0, Stroke::new(1.5, color), egui::StrokeKind::Outside);
        }

        // bones of a selected bone layer, or with a bone tool, of the bone
        // layer around the selection
        let rig = match ed.doc.find(*sel).map(|l| &l.kind) {
            Some(LayerKind::Bone { .. }) => Some(*sel),
            _ if ed.tool.group() == fanimate_editor::ToolGroup::Bone => ed.enclosing_rig(*sel),
            _ => None,
        };
        if let Some(rig) = rig
            && !drawn.contains(&rig)
        {
            drawn.push(rig);
            bone_overlay(&painter, shell, rig, to_screen);
        }
    }
}

// ===========================================================================
// Phase E tool interactions
// ===========================================================================

/// Shape tools E: Line, Polygon, Star, Triangle. Drag to preview; release to
/// commit. Scroll wheel adjusts the side/point count while dragging.
fn shape_tools_e(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) -> Option<fanimate_core::Shape> {
    let tool = shell.editor.tool;
    if !matches!(tool, Tool::Line | Tool::Polygon | Tool::Star | Tool::Triangle) {
        shell.ui.shape_drag_e = None;
        return None;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let (mods, scroll, press_origin) =
        ctx.input(|i| (i.modifiers, i.smooth_scroll_delta, i.pointer.press_origin()));

    // Scroll wheel adjusts sides / point count
    if response.hovered() && scroll.y != 0.0 {
        let delta = if scroll.y > 0.0 { 1 } else { -1 };
        if mods.alt && tool == Tool::Star {
            shell.editor.adjust_star_inner(delta as f32 * 0.05);
        } else {
            shell.editor.adjust_shape_sides(delta);
        }
    }

    if response.drag_started_by(egui::PointerButton::Primary) {
        shell.ui.shape_drag_e = press_origin.map(|p| to_doc(center, zoom, p));
    }
    let from = shell.ui.shape_drag_e?;
    let to = do_snap(shell, to_doc(center, zoom, response.interact_pointer_pos()?));
    if response.drag_stopped() {
        shell.ui.shape_drag_e = None;
        shell
            .editor
            .add_drag_shape_e(tool, from, to, mods.shift, mods.alt, zoom);
        return None;
    }
    shell
        .editor
        .drag_shape_e(tool, from, to, mods.shift, mods.alt, zoom)
}

/// Editing tools E: Delete Edge, Magnet, Scale Points, Rotate Points,
/// Line Width, Stroke Exposure, Shape Builder.
fn editing_tools_e(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    let tool = shell.editor.tool;
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let doc = |p: egui::Pos2| to_doc(center, zoom, p);
    let slop = POINT_SLOP_PX / zoom;
    let mods = ctx.input(|i| i.modifiers);

    match tool {
        // ---- Delete Edge ----
        Tool::DeleteEdge => {
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(pos) = response.interact_pointer_pos()
            {
                let p = doc(pos);
                if !shell.editor.delete_edge_at(p, slop * 2.0) {
                    shell.editor.set_status(
                        fanimate_editor::StatusKind::Info,
                        "Click on a line segment to delete it",
                    );
                }
            }
        }

        // ---- Magnet ----
        Tool::Magnet => {
            let radius = shell.editor.draw.width * zoom / 2.0;
            if response.drag_started_by(egui::PointerButton::Primary) {
                shell.editor.checkpoint_magnet();
                shell.ui.magnet_active = true;
            }
            if shell.ui.magnet_active {
                if let Some(pos) = response.interact_pointer_pos() {
                    shell
                        .editor
                        .apply_magnet(doc(pos), shell.editor.draw.width * 3.0, 0.15, mods.alt);
                    ctx.request_repaint();
                }
                if response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
                    shell.ui.magnet_active = false;
                }
            }
            let _ = radius;
        }

        // ---- Scale Points ----
        Tool::ScalePoints => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(start) = ctx.input(|i| i.pointer.press_origin())
            {
                shell.editor.checkpoint("Scale points");
                shell.ui.points_transform_origin = Some((doc(start), doc(start)));
            }
            if let Some((anchor, last)) = shell.ui.points_transform_origin
                && let Some(pos) = response.interact_pointer_pos()
            {
                let now = doc(pos);
                let (old_len, new_len) = (
                    (last - anchor).length().max(1e-6),
                    (now - anchor).length().max(1e-6),
                );
                let factor = new_len / old_len;
                if (factor - 1.0).abs() > 1e-4 {
                    // Shift = scale uniformly; without Shift = scale both axes equally
                    shell.editor.scale_selected_points(factor, factor);
                    shell.ui.points_transform_origin = Some((anchor, now));
                }
            }
            if response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
                shell.ui.points_transform_origin = None;
            }
        }

        // ---- Rotate Points ----
        Tool::RotatePoints => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(start) = ctx.input(|i| i.pointer.press_origin())
            {
                shell.editor.checkpoint("Rotate points");
                let centre = shell.editor.selected_points_centroid();
                shell.ui.points_transform_origin = Some((centre, doc(start)));
            }
            if let Some((centre, last)) = shell.ui.points_transform_origin
                && let Some(pos) = response.interact_pointer_pos()
            {
                let now = doc(pos);
                let old_angle = (last - centre).y.atan2((last - centre).x);
                let new_angle = (now - centre).y.atan2((now - centre).x);
                let mut delta = new_angle - old_angle;
                if mods.shift {
                    let step = 15.0_f32.to_radians();
                    delta = (delta / step).round() * step;
                }
                if delta.abs() > 1e-4 {
                    shell.editor.rotate_selected_points(delta);
                    shell.ui.points_transform_origin = Some((centre, now));
                }
            }
            if response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
                shell.ui.points_transform_origin = None;
            }
        }

        // ---- Line Width ----
        Tool::LineWidth => {
            if response.drag_started_by(egui::PointerButton::Primary) {
                shell.editor.checkpoint("Line width");
                shell.ui.magnet_active = true;
            }
            if shell.ui.magnet_active {
                if let Some(pos) = response.interact_pointer_pos() {
                    let drag_speed = response.drag_delta().length() * zoom;
                    let new_width = (shell.editor.draw.width + drag_speed * 0.1).max(0.5);
                    shell.editor.apply_line_width(doc(pos), slop * 6.0, new_width);
                }
                if response.drag_stopped() || !response.dragged_by(egui::PointerButton::Primary) {
                    shell.ui.magnet_active = false;
                }
            }
        }

        // ---- Shape Builder ----
        Tool::ShapeBuilder => {
            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(start) = ctx.input(|i| i.pointer.press_origin())
            {
                shell.editor.shape_builder = Some(fanimate_editor::ShapeBuilderState {
                    from: doc(start),
                    subtract: mods.alt,
                });
            }
            if response.drag_stopped()
                && let Some(state) = shell.editor.shape_builder.take()
                && let Some(pos) = response.interact_pointer_pos()
            {
                shell
                    .editor
                    .apply_shape_builder(state.from, doc(pos), state.subtract);
            }
            if !response.dragged_by(egui::PointerButton::Primary) {
                shell.editor.shape_builder = None;
            }
        }

        _ => {}
    }
}

/// Curvature tool: click to add smooth/corner points; Esc to finish.
fn curvature_tool(
    ctx: &egui::Context,
    response: &egui::Response,
    shell: &mut Shell,
    to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2,
) {
    if shell.editor.tool != Tool::Curvature {
        if shell.editor.is_drawing_curvature() {
            shell.editor.cancel_curvature();
        }
        return;
    }
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    let doc = |p: egui::Pos2| to_doc(center, zoom, p);
    let slop = POINT_SLOP_PX / zoom;
    let (pressed, released, mods, origin) = ctx.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.modifiers,
            i.pointer.press_origin(),
        )
    });
    if pressed
        && response.hovered()
        && let Some(at) = origin
    {
        shell.ui.curvature_down = shell.editor.curvature_down(doc(at), mods.alt, slop);
    }
    if shell.ui.curvature_down && released {
        shell.ui.curvature_down = false;
        shell.editor.curvature_up();
    }
}

/// The Curvature path's on-canvas points (dots at each committed position).
fn curvature_overlay(
    ui: &egui::Ui,
    shell: &Shell,
    rect: Rect,
    to_screen: &impl Fn(Vec2, f32, Vec2) -> egui::Pos2,
) {
    if shell.editor.tool != Tool::Curvature {
        return;
    }
    let painter = ui.painter_at(rect);
    let (c, z, p) = (shell.view.center, shell.view.zoom, shell.palette);
    for (i, at) in shell.editor.curvature_points().into_iter().enumerate() {
        let s = to_screen(c, z, at);
        let r = Rect::from_center_size(s, egui::vec2(7.0, 7.0));
        painter.rect_filled(r, 1.0, if i == 0 { p.accent } else { p.panel });
        painter.rect_stroke(r, 1.0, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
    }
}
