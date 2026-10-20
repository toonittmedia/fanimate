//! The Timeline panel: transport controls, a scrubbable ruler, and a row of
//! keyframes per layer.

use eframe::egui::{self, Id, Rangef, Rect, RichText, Sense, Shape, Stroke, pos2, vec2};
use fanimate_core::{ChannelId, Frame, Interpolation, LayerId};
use fanimate_editor::{Action, SelectMode};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;

use super::properties::{begin_edit, end_edits};

/// Stands for the camera where a row is named by a layer: it is not a layer,
/// and no layer has this id.
pub const CAMERA_ROW: LayerId = LayerId(u32::MAX);

const NAMES_WIDTH: f32 = 170.0;
const RULER_HEIGHT: f32 = 22.0;
const ROW_HEIGHT: f32 = 22.0;
const DEFAULT_FRAME_WIDTH: f32 = 12.0;
const MIN_FRAME_WIDTH: f32 = 2.0;
const MAX_FRAME_WIDTH: f32 = 60.0;

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    action_banner(ui, shell);
    transport(ui, shell);
    ui.add_space(2.0);

    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    if shell.ui.timeline_frame_width <= 0.0 {
        shell.ui.timeline_frame_width = DEFAULT_FRAME_WIDTH;
    }
    let p = shell.palette;
    let (start, end) = (shell.editor.doc.start, shell.editor.doc.end);
    let fps = shell.editor.doc.fps.max(1.0);

    let frames_rect = Rect::from_min_max(pos2(full.left() + NAMES_WIDTH, full.top()), full.max);
    let ruler = Rect::from_min_max(
        frames_rect.min,
        pos2(frames_rect.right(), frames_rect.top() + RULER_HEIGHT),
    );

    shell.ui.timeline_view_width = frames_rect.width();
    shell.ui.timeline_hovered = ui.rect_contains_pointer(full);
    zoom_and_scroll(ui, shell, frames_rect);
    // when a step, a key jump or the Frame field moves the playhead out of
    // view, bring it back; a scrub drag scrolls at the edge instead (below)
    let now = shell.editor.current_frame();
    let was_scrubbing = std::mem::take(&mut shell.ui.timeline_scrubbing);
    if !shell.editor.playing && !was_scrubbing && shell.ui.timeline_seen_frame != now {
        shell.ui.timeline_scroll = reveal(
            shell.ui.timeline_scroll,
            start,
            now,
            shell.ui.timeline_frame_width,
            frames_rect.width(),
        );
    }
    shell.ui.timeline_seen_frame = now;
    let fw = shell.ui.timeline_frame_width;
    let scroll = shell.ui.timeline_scroll;
    let x_of = |f: f32| ruler.left() + (f - start as f32 - scroll) * fw + fw / 2.0;
    let frame_at = |x: f32| -> Frame { (start as f32 + scroll + (x - ruler.left()) / fw).floor() as Frame };

    // keep the playhead in view while playing
    if shell.editor.playing {
        let x = x_of(shell.editor.frame);
        if x > frames_rect.right() - 20.0 || x < frames_rect.left() {
            shell.ui.timeline_scroll = (shell.editor.frame - start as f32 - 2.0).max(0.0);
        }
    }

    // ruler: click or drag to scrub
    let painter = ui.painter_at(full);
    painter.rect_filled(ruler, 0.0, p.bg);
    // the playing range is lit; what lies beyond it is the future, dimmed
    let range_left = x_of(start as f32) - fw / 2.0;
    let range_right = x_of(end as f32) + fw / 2.0;
    painter.rect_filled(
        Rect::from_min_max(
            pos2(range_left.max(ruler.left()), ruler.top()),
            pos2(range_right.min(ruler.right()), ruler.bottom()),
        ),
        0.0,
        p.accent.gamma_multiply(0.18),
    );
    let ruler_resp = ui.interact(ruler, Id::new("timeline-ruler"), Sense::click_and_drag());
    if ruler_resp.clicked() || ruler_resp.dragged() {
        shell.ui.timeline_scrubbing = true;
        if let Some(pos) = ruler_resp.interact_pointer_pos() {
            shell.editor.go_to(frame_at(pos.x));
            if ruler_resp.dragged() {
                edge_scroll(ui, shell, frames_rect, pos.x);
            }
        }
    }
    let label_every = nice_step(48.0 / fw);
    let first = frame_at(ruler.left()).max(start);
    // the ruler runs on past the end frame
    let last = frame_at(ruler.right());
    for f in first..=last {
        let x = x_of(f as f32);
        let tick = if f > end {
            p.text_muted.gamma_multiply(0.5)
        } else {
            p.text_muted
        };
        let second = (f - start) as f32 % fps == 0.0;
        let h = if second {
            10.0
        } else if f % label_every == 0 {
            6.0
        } else {
            3.0
        };
        painter.line_segment(
            [pos2(x, ruler.bottom() - h), pos2(x, ruler.bottom())],
            Stroke::new(1.0, tick),
        );
        if f % label_every == 0 {
            painter.text(
                pos2(x + 2.0, ruler.top() + 2.0),
                egui::Align2::LEFT_TOP,
                f.to_string(),
                egui::FontId::proportional(10.0),
                tick,
            );
        }
    }

    // rows
    let rows_rect = Rect::from_min_max(pos2(full.left(), ruler.bottom()), full.max);
    let keys_clip = Rect::from_min_max(pos2(frames_rect.left(), rows_rect.top()), rows_rect.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rows_rect));
    egui::ScrollArea::vertical()
        .id_salt("timeline-rows")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // the camera is the project's, pinned above the layers
            camera_row(ui, shell, keys_clip, &x_of, &frame_at);
            if shell.ui.timeline_expanded.contains(&CAMERA_ROW) {
                for ch in ChannelId::CAMERA {
                    property_row(ui, shell, CAMERA_ROW, ch, 0, keys_clip, &x_of, &frame_at);
                }
            }
            for (id, depth) in shell.editor.display_rows() {
                row(ui, shell, id, depth, keys_clip, &x_of, &frame_at);
                // a layer opened with a double-click lists its properties
                if shell.ui.timeline_expanded.contains(&id) {
                    let channels = shell
                        .editor
                        .doc
                        .find(id)
                        .map(|l| l.channels())
                        .unwrap_or_default();
                    for ch in channels {
                        property_row(ui, shell, id, ch, depth, keys_clip, &x_of, &frame_at);
                    }
                }
            }
        });

    // frame range end marker and playhead on top
    let painter = ui.painter_at(Rect::from_min_max(frames_rect.min, full.max));
    let end_x = x_of(end as f32) + fw / 2.0;
    if end_x < full.right() {
        // dim the rows past the end; keys there stay visible but quiet
        painter.rect_filled(
            Rect::from_min_max(pos2(end_x.max(frames_rect.left()), ruler.bottom()), full.max),
            0.0,
            p.bg.gamma_multiply(0.45),
        );
    }
    let boundary = Stroke::new(1.5, p.text_muted);
    // the range starts at a clear line on the left edge of its first frame
    let start_x = x_of(start as f32) - fw / 2.0;
    if start_x >= frames_rect.left() - 0.5 {
        let x = start_x.max(frames_rect.left()) + 0.75;
        painter.line_segment([pos2(x, ruler.top()), pos2(x, full.bottom())], boundary);
    }
    let x = x_of(shell.editor.frame.floor());
    painter.line_segment(
        [pos2(x, ruler.top()), pos2(x, full.bottom())],
        Stroke::new(1.5, p.keyframe),
    );
    painter.add(Shape::convex_polygon(
        vec![
            pos2(x - 5.0, ruler.top()),
            pos2(x + 5.0, ruler.top()),
            pos2(x, ruler.top() + 7.0),
        ],
        p.keyframe,
        Stroke::NONE,
    ));
}

/// Label spacing in frames that is at least `min` and a "round" number.
fn nice_step(min: f32) -> Frame {
    [1, 2, 4, 6, 12, 24, 48, 96, 240, 480, 960]
        .into_iter()
        .find(|s| *s as f32 >= min)
        .unwrap_or(1920)
}

/// The scroll (in frames from the start) that keeps `anchor`, a frame, at
/// the same place on screen when the frame width changes from `old` to `new`.
fn zoomed_scroll(anchor: f32, start: f32, scroll: f32, old: f32, new: f32) -> f32 {
    // a frame is drawn centred in its cell, so its x is (offset + 0.5) cells
    let x = (anchor - start - scroll) * old + old / 2.0;
    anchor - start - (x - new / 2.0) / new
}

/// The scroll (in frames from `start`) that shows `frame` with a little
/// room either side, changing the scroll as little as it can.
fn reveal(scroll: f32, start: Frame, frame: Frame, frame_width: f32, view_width: f32) -> f32 {
    let margin = (frame_width * 2.0).min(view_width / 4.0);
    // pixels from the left of the view to the frame's cell
    let left = (frame - start) as f32 * frame_width - scroll * frame_width;
    let right = left + frame_width;
    let scroll = if left < margin {
        scroll - (margin - left) / frame_width
    } else if right > view_width - margin {
        scroll + (right - (view_width - margin)) / frame_width
    } else {
        scroll
    };
    scroll.max(0.0)
}

/// How fast the view scrolls, in pixels per second (to the right when
/// positive), while the pointer holds a scrub drag at `x` in a view from
/// `left` to `right`: nothing inside, faster the further outside.
fn edge_speed(x: f32, left: f32, right: f32) -> f32 {
    const ZONE: f32 = 28.0;
    const MAX_DEPTH: f32 = 120.0;
    const PER_PIXEL: f32 = 9.0;
    if x > right - ZONE {
        (x - (right - ZONE)).min(MAX_DEPTH) * PER_PIXEL
    } else if x < left + ZONE {
        -(left + ZONE - x).min(MAX_DEPTH) * PER_PIXEL
    } else {
        0.0
    }
}

/// The last frame there is something at: the end of the animation, the
/// latest key on any row, or the playhead.
fn last_frame(shell: &Shell) -> Frame {
    let ed = &shell.editor;
    let latest_key = ed
        .display_rows()
        .into_iter()
        .filter_map(|(id, _)| ed.key_frames(id).last().copied())
        .max()
        .unwrap_or(0);
    let camera = ed.doc.camera.key_frames().last().copied().unwrap_or(0);
    ed.doc.end.max(latest_key).max(camera).max(ed.current_frame())
}

/// The furthest the view may scroll, in frames from the start: until the
/// last frame there is anything at sits at the left edge.
fn max_scroll(shell: &Shell) -> f32 {
    (last_frame(shell) - shell.editor.doc.start).max(0) as f32
}

/// Scroll while a scrub drag is held near or past the edge of `area`.
fn edge_scroll(ui: &egui::Ui, shell: &mut Shell, area: Rect, x: f32) {
    let speed = edge_speed(x, area.left(), area.right());
    if speed == 0.0 {
        return;
    }
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let max = max_scroll(shell);
    let state = &mut shell.ui;
    let fw = state.timeline_frame_width.max(MIN_FRAME_WIDTH);
    state.timeline_scroll = (state.timeline_scroll + speed * dt / fw).clamp(0.0, max);
    ui.ctx().request_repaint();
}

/// Change the zoom to `width` pixels per frame, keeping `anchor` in place.
fn set_frame_width(shell: &mut Shell, width: f32, anchor: f32) {
    let start = shell.editor.doc.start as f32;
    let max = max_scroll(shell);
    let ui = &mut shell.ui;
    let old = ui.timeline_frame_width.clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
    let new = width.clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
    ui.timeline_scroll = zoomed_scroll(anchor, start, ui.timeline_scroll, old, new).clamp(0.0, max);
    ui.timeline_frame_width = new;
}

/// Where zoom buttons and shortcuts zoom around: the playhead if it is on
/// screen, else the middle of the view.
fn zoom_anchor(shell: &Shell) -> f32 {
    let ui = &shell.ui;
    let fw = ui.timeline_frame_width.max(MIN_FRAME_WIDTH);
    let first = shell.editor.doc.start as f32 + ui.timeline_scroll;
    let last = first + ui.timeline_view_width / fw;
    let now = shell.editor.frame.floor();
    // a frame is on screen when its centre is
    if (first..=last).contains(&(now + 0.5)) {
        now
    } else {
        (first + last) / 2.0
    }
}

pub fn zoom_by(shell: &mut Shell, factor: f32) {
    let anchor = zoom_anchor(shell);
    set_frame_width(shell, shell.ui.timeline_frame_width * factor, anchor);
}

pub fn reset_zoom(shell: &mut Shell) {
    let anchor = zoom_anchor(shell);
    set_frame_width(shell, DEFAULT_FRAME_WIDTH, anchor);
}

/// Zoom so the whole frame range fits the view.
pub fn fit_all(shell: &mut Shell) {
    let frames = (shell.editor.doc.end - shell.editor.doc.start + 1).max(1) as f32;
    let width = (shell.ui.timeline_view_width / frames).clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
    shell.ui.timeline_frame_width = width;
    shell.ui.timeline_scroll = 0.0;
}

fn zoom_and_scroll(ui: &egui::Ui, shell: &mut Shell, area: Rect) {
    let hovered = ui.rect_contains_pointer(area);
    if !hovered {
        return;
    }
    let start = shell.editor.doc.start;
    let (zoom, scroll, mods) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers));
    if zoom != 1.0 {
        // zoom around the frame under the pointer
        let fw = shell.ui.timeline_frame_width;
        let pointer_x = ui
            .input(|i| i.pointer.hover_pos())
            .map_or(area.center().x, |p| p.x);
        let anchor = start as f32 + shell.ui.timeline_scroll + (pointer_x - area.left() - fw / 2.0) / fw;
        set_frame_width(shell, fw * zoom, anchor);
    } else if scroll.x != 0.0 || (mods.shift && scroll.y != 0.0) {
        let dx = if scroll.x != 0.0 { scroll.x } else { scroll.y };
        let max = max_scroll(shell);
        shell.ui.timeline_scroll =
            (shell.ui.timeline_scroll - dx / shell.ui.timeline_frame_width).clamp(0.0, max);
    }
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    depth: usize,
    keys_clip: Rect,
    x_of: &impl Fn(f32) -> f32,
    frame_at: &impl Fn(f32) -> Frame,
) {
    let Some(layer) = shell.editor.doc.find(id) else {
        return;
    };
    let name = layer.name.clone();
    let visible = layer.visible;
    // a frame-by-frame layer's drawings, as blocks across their exposure
    let exposures: Vec<(Frame, Frame)> = match &layer.kind {
        fanimate_core::LayerKind::Frames { cels } => cels
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    c.frame,
                    cels.get(i + 1).map_or(shell.editor.doc.end + 1, |n| n.frame),
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    // a switch layer names the drawing each key shows
    let switch_names: Vec<(Frame, String)> = match &layer.kind {
        fanimate_core::LayerKind::Switch { switch, .. } => {
            switch.iter().map(|k| (k.frame, k.value.clone())).collect()
        }
        _ => Vec::new(),
    };
    let sound = match &layer.kind {
        fanimate_core::LayerKind::Audio(a) => Some((a.media.clone(), a.start, a.volume)),
        _ => None,
    };
    let label = crate::theme::label_color(layer.shown_label());
    let glyphs = key_glyphs(layer);
    let p = shell.palette;
    let selected = shell.editor.is_selected(id);

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let names = Rect::from_min_max(rect.min, pos2(rect.left() + NAMES_WIDTH, rect.bottom()));
    let keys = Rect::from_min_max(pos2(names.right(), rect.top()), rect.max);

    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 0.0, p.accent.gamma_multiply(0.2));
    }
    // the layer's label colour: a strip by the name, and a bar across the
    // frame range, as in compositing apps; keys are drawn on top
    let strip = Rect::from_min_size(rect.min, vec2(3.0, rect.height()));
    painter.rect_filled(strip, 0.0, label);
    let half = (x_of(1.0) - x_of(0.0)) / 2.0;
    let (start, end) = (shell.editor.doc.start as f32, shell.editor.doc.end as f32);
    let bar = Rect::from_min_max(
        pos2(x_of(start) - half, rect.top() + 4.0),
        pos2(x_of(end) + half, rect.bottom() - 4.0),
    );
    let keys_painter = ui.painter_at(keys.intersect(keys_clip));
    keys_painter.rect_filled(bar, 2.0, label.gamma_multiply(0.3));
    for (i, (from, to)) in exposures.iter().enumerate() {
        let block = Rect::from_min_max(
            pos2(x_of(*from as f32) - half + 1.0, rect.top() + 3.0),
            pos2(x_of(*to as f32) - half - 1.0, rect.bottom() - 3.0),
        );
        let shade = if i % 2 == 0 { 0.55 } else { 0.4 };
        keys_painter.rect_filled(block, 2.0, label.gamma_multiply(shade));
    }
    if let Some((media, from, volume)) = sound
        && let Some(peaks) = shell.audio.peaks(&shell.editor.doc, &media)
    {
        // only the frames in view, one line each
        let clip = keys.intersect(keys_clip);
        let first = (frame_at(clip.left()) - from - 1).max(0) as usize;
        let last = ((frame_at(clip.right()) - from + 1).max(0) as usize).min(peaks.len());
        let mid = rect.center().y;
        let reach = (ROW_HEIGHT / 2.0 - 3.0) * volume.min(1.0);
        let width = (2.0 * half - 1.0).clamp(1.0, 3.0);
        for (i, (lo, hi)) in peaks.iter().enumerate().take(last).skip(first) {
            let x = x_of((from + i as Frame) as f32);
            let span = Rangef::new(mid - hi * reach, mid - lo * reach + 1.0);
            keys_painter.vline(x, span, Stroke::new(width, p.text.gamma_multiply(0.7)));
        }
    }
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, p.border.gamma_multiply(0.5)),
    );
    painter.text(
        pos2(names.left() + 8.0 + depth as f32 * 12.0, names.center().y),
        egui::Align2::LEFT_CENTER,
        &name,
        egui::FontId::proportional(12.0),
        if visible { p.text } else { p.text_muted },
    );

    // name column: select, right-click for the layer menu
    let name_resp = ui.interact(names, Id::new(("tl-name", id)), Sense::click());
    if name_resp.clicked() {
        let m = ui.input(|i| i.modifiers);
        let mode = if m.command {
            SelectMode::Toggle
        } else if m.shift {
            SelectMode::Range
        } else {
            SelectMode::Replace
        };
        shell.editor.select(id, mode);
    }
    if name_resp.secondary_clicked() && !selected {
        shell.editor.select(id, SelectMode::Replace);
    }
    if name_resp.double_clicked() {
        toggle_expanded(shell, id);
    }
    menus::context_menu(&name_resp, |ui| menus::layer_context(ui, shell));
    // the arrow at the end of the name opens the layer's property rows too
    let open = shell.ui.timeline_expanded.contains(&id);
    let arrow = Rect::from_center_size(pos2(names.right() - 12.0, names.center().y), vec2(12.0, 12.0));
    icons::image(
        ui.ctx(),
        if open { Icon::Collapse } else { Icon::Expand },
        12.0,
        p.text_muted,
    )
    .paint_at(ui, arrow);
    let arrow_resp = ui.interact(arrow.expand(4.0), Id::new(("tl-arrow", id)), Sense::click());
    if arrow_resp
        .on_hover_text("Show this layer's properties, to key each on its own (or double-click the layer)")
        .clicked()
    {
        toggle_expanded(shell, id);
    }

    // keyframes, shaped by how they move on: smooth round, linear
    // diamond, step square
    let now = shell.editor.current_frame();
    let key_painter = ui.painter_at(keys.intersect(keys_clip));
    for (f, interp) in glyphs {
        let c = pos2(x_of(f as f32), keys.center().y);
        let fill = p.key_color(interp);
        // the key under the playhead gets a purple ring, so its colour (the
        // way it moves on) still reads
        let outline = if f == now {
            Stroke::new(1.5, p.accent_strong)
        } else {
            Stroke::new(1.0, p.bg)
        };
        key_shape(&key_painter, c, interp, fill, outline);
    }

    for (f, name) in &switch_names {
        key_painter.text(
            pos2(x_of(*f as f32) + 7.0, keys.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(10.0),
            p.text,
        );
    }

    // key area: click to select the layer and jump to that frame; drag a
    // key to retime it (with the keys of the other selected layers on that
    // frame), or drag elsewhere to scrub; right-click for the key menu
    let key_resp = ui.interact(
        keys.intersect(keys_clip),
        Id::new(("tl-keys", id)),
        Sense::click_and_drag(),
    );
    if key_resp.clicked() || key_resp.secondary_clicked() {
        shell.ui.timeline_scrubbing = true;
        if let Some(pos) = key_resp.interact_pointer_pos() {
            shell.editor.go_to(frame_at(pos.x));
        }
        if !selected || key_resp.clicked() {
            shell.editor.select(id, SelectMode::Replace);
        }
    }
    if key_resp.double_clicked() {
        toggle_expanded(shell, id);
    }
    if key_resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = ui.input(|i| i.pointer.press_origin())
    {
        let from = frame_at(start.x);
        let on_key = (x_of(from as f32) - start.x).abs() <= KEY_GRAB_PX;
        if on_key && shell.editor.key_frames(id).contains(&from) {
            if !selected {
                shell.editor.select(id, SelectMode::Replace);
            }
            shell.editor.begin_key_drag(from);
        }
    }
    if key_resp.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = key_resp.interact_pointer_pos()
    {
        if shell.editor.is_dragging_keys() {
            shell.editor.drag_keys_to(frame_at(pos.x));
        } else {
            shell.ui.timeline_scrubbing = true;
            shell.editor.go_to(frame_at(pos.x));
            edge_scroll(ui, shell, keys_clip, pos.x);
        }
    }
    if key_resp.drag_stopped() {
        shell.editor.end_key_drag();
    }
    if key_resp.hovered()
        && let Some(pos) = key_resp.hover_pos()
    {
        let f = frame_at(pos.x);
        let on_key = (x_of(f as f32) - pos.x).abs() <= KEY_GRAB_PX;
        if shell.editor.is_dragging_keys() || on_key && shell.editor.key_frames(id).contains(&f) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    menus::context_menu(&key_resp, |ui| {
        let f = shell.editor.current_frame();
        ui.label(RichText::new(format!("Frame {f}")).color(p.text_muted));
        menus::item(ui, shell, Action::AddKey);
        menus::item(ui, shell, Action::DeleteKey);
        menus::interpolation_items(ui, shell);
        menus::switch_items(ui, shell);
        ui.separator();
        menus::item(ui, shell, Action::PrevKey);
        menus::item(ui, shell, Action::NextKey);
    });
}

/// A key's mark: round for smooth, a diamond for linear (and for keys that
/// differ), square for step.
pub fn key_shape(
    painter: &egui::Painter,
    c: egui::Pos2,
    interp: Option<Interpolation>,
    fill: egui::Color32,
    outline: Stroke,
) {
    match interp {
        Some(Interpolation::Smooth) => {
            painter.circle(c, 4.5, fill, outline);
        }
        Some(Interpolation::Step) => {
            let r = Rect::from_center_size(c, vec2(8.0, 8.0));
            painter.rect(r, 1.0, fill, outline, egui::StrokeKind::Outside);
        }
        _ => {
            let s = 5.0;
            painter.add(Shape::convex_polygon(
                vec![
                    c + vec2(0.0, -s),
                    c + vec2(s, 0.0),
                    c + vec2(0.0, s),
                    c + vec2(-s, 0.0),
                ],
                fill,
                outline,
            ));
        }
    }
}

fn toggle_expanded(shell: &mut Shell, id: LayerId) {
    let open = &mut shell.ui.timeline_expanded;
    if !open.remove(&id) {
        open.insert(id);
    }
}

/// The camera's row, above the layers: its keys (all three properties
/// together), and the arrow or a double-click to open Position, Zoom and Roll
/// as rows of their own. It is the project's, not a layer's.
fn camera_row(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    keys_clip: Rect,
    x_of: &impl Fn(f32) -> f32,
    frame_at: &impl Fn(f32) -> Frame,
) {
    let p = shell.palette;
    let now = shell.editor.current_frame();
    // each key frame, with the way it moves on when the properties agree
    let camera = &shell.editor.doc.camera;
    let glyphs: Vec<(Frame, Option<Interpolation>)> = camera
        .key_frames()
        .into_iter()
        .map(|f| {
            let mut interp: Option<Option<Interpolation>> = None;
            for ch in ChannelId::CAMERA {
                if let Some(i) = camera.keys(ch).and_then(|k| k.interp_at(f)) {
                    interp = Some(match interp {
                        None => Some(i),
                        Some(Some(j)) if j == i => Some(i),
                        Some(_) => None,
                    });
                }
            }
            (f, interp.flatten())
        })
        .collect();
    let open = shell.ui.timeline_expanded.contains(&CAMERA_ROW);

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let names = Rect::from_min_max(rect.min, pos2(rect.left() + NAMES_WIDTH, rect.bottom()));
    let keys = Rect::from_min_max(pos2(names.right(), rect.top()), rect.max);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, p.bone.gamma_multiply(0.12));
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
        0.0,
        p.bone,
    );
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, p.border.gamma_multiply(0.5)),
    );
    painter.text(
        pos2(names.left() + 8.0, names.center().y),
        egui::Align2::LEFT_CENTER,
        "Camera",
        egui::FontId::proportional(12.0),
        p.text,
    );
    let arrow = Rect::from_center_size(pos2(names.right() - 12.0, names.center().y), vec2(12.0, 12.0));
    icons::image(
        ui.ctx(),
        if open { Icon::Collapse } else { Icon::Expand },
        12.0,
        p.text_muted,
    )
    .paint_at(ui, arrow);

    let key_painter = ui.painter_at(keys.intersect(keys_clip));
    for (f, interp) in &glyphs {
        let c = pos2(x_of(*f as f32), keys.center().y);
        let outline = if *f == now {
            Stroke::new(1.5, p.accent_strong)
        } else {
            Stroke::new(1.0, p.bg)
        };
        key_shape(&key_painter, c, *interp, p.key_color(*interp), outline);
    }

    let name_resp = ui.interact(names, Id::new("tl-camera-name"), Sense::click());
    // choosing the camera shows its settings: with no layer selected, the
    // Properties panel is the project's, camera included
    if name_resp.clicked() {
        shell.editor.selection.clear();
        shell.ui.focus_properties = true;
    }
    let mut toggle = name_resp.double_clicked();
    let arrow_resp = ui.interact(arrow.expand(4.0), Id::new("tl-camera-arrow"), Sense::click());
    toggle |= arrow_resp
        .on_hover_text("Show the camera's Position, Zoom and Roll, to key each on its own (or double-click)")
        .clicked();
    // click to go to a frame; drag a key to retime all of the camera's keys
    // on it, or drag elsewhere to scrub
    let key_resp = ui.interact(
        keys.intersect(keys_clip),
        Id::new("tl-camera-keys"),
        Sense::click_and_drag(),
    );
    if key_resp.clicked() || key_resp.secondary_clicked() {
        shell.ui.timeline_scrubbing = true;
        if let Some(pos) = key_resp.interact_pointer_pos() {
            shell.editor.go_to(frame_at(pos.x));
        }
    }
    toggle |= key_resp.double_clicked();
    if key_resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = ui.input(|i| i.pointer.press_origin())
    {
        let from = frame_at(start.x);
        if (x_of(from as f32) - start.x).abs() <= KEY_GRAB_PX {
            shell.editor.begin_camera_key_drag(None, from);
        }
    }
    if key_resp.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = key_resp.interact_pointer_pos()
    {
        if shell.editor.is_dragging_keys() {
            shell.editor.drag_keys_to(frame_at(pos.x));
        } else {
            shell.ui.timeline_scrubbing = true;
            shell.editor.go_to(frame_at(pos.x));
            edge_scroll(ui, shell, keys_clip, pos.x);
        }
    }
    if key_resp.drag_stopped() {
        shell.editor.end_key_drag();
    }
    menus::context_menu(&key_resp, |ui| {
        ui.label(RichText::new(format!("Camera, frame {now}")).color(p.text_muted));
        for ch in ChannelId::CAMERA {
            menus::item(ui, shell, Action::AddChannelKey(ch));
        }
        ui.separator();
        menus::item(ui, shell, Action::ResetCamera);
        menus::item(ui, shell, Action::LookThroughCamera);
    });
    if toggle {
        let open = &mut shell.ui.timeline_expanded;
        if !open.remove(&CAMERA_ROW) {
            open.insert(CAMERA_ROW);
        }
    }
}

/// One property of an open layer: its own row of keys, to key, retime and
/// set the movement of one property without touching the others.
#[allow(clippy::too_many_arguments)]
fn property_row(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    ch: ChannelId,
    depth: usize,
    keys_clip: Rect,
    x_of: &impl Fn(f32) -> f32,
    frame_at: &impl Fn(f32) -> Frame,
) {
    let camera = ch.is_camera();
    let layer = shell.editor.doc.find(id);
    if layer.is_none() && !camera {
        return;
    }
    let label = if camera {
        ch.camera_label().to_owned()
    } else {
        layer.map(|l| l.channel_label(ch)).unwrap_or_default()
    };
    let frames: Vec<(Frame, Interpolation)> = {
        let mut frames = Vec::new();
        let keys = if camera {
            shell.editor.doc.camera.keys(ch)
        } else {
            layer.and_then(|l| l.channel_keys(ch))
        };
        if let Some(keys) = keys {
            keys.frames_into(&mut frames);
            frames.sort_unstable();
            frames
                .into_iter()
                .filter_map(|f| Some((f, keys.interp_at(f)?)))
                .collect()
        } else {
            Vec::new()
        }
    };
    let p = shell.palette;
    let selected = !camera && shell.editor.is_selected(id);
    let now = shell.editor.current_frame();

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let names = Rect::from_min_max(rect.min, pos2(rect.left() + NAMES_WIDTH, rect.bottom()));
    let keys = Rect::from_min_max(pos2(names.right(), rect.top()), rect.max);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, p.bg.gamma_multiply(0.5));
    if selected {
        painter.rect_filled(rect, 0.0, p.accent.gamma_multiply(0.1));
    }
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, p.border.gamma_multiply(0.35)),
    );
    painter.text(
        pos2(names.left() + 8.0 + (depth + 1) as f32 * 12.0, names.center().y),
        egui::Align2::LEFT_CENTER,
        &label,
        egui::FontId::proportional(11.5),
        p.text_muted,
    );

    let key_painter = ui.painter_at(keys.intersect(keys_clip));
    for (f, interp) in &frames {
        let c = pos2(x_of(*f as f32), keys.center().y);
        let outline = if *f == now {
            Stroke::new(1.5, p.accent_strong)
        } else {
            Stroke::new(1.0, p.bg)
        };
        key_shape(
            &key_painter,
            c,
            Some(*interp),
            p.key_color(Some(*interp)),
            outline,
        );
    }

    // click to select the layer and go to a frame; drag a key to retime it,
    // or drag elsewhere to scrub; right-click for this property's key menu
    let name_resp = ui.interact(names, Id::new(("tl-prop-name", id, ch)), Sense::click());
    if !camera && (name_resp.clicked() || name_resp.secondary_clicked()) {
        shell.editor.select(id, SelectMode::Replace);
    }
    let key_resp = ui.interact(
        keys.intersect(keys_clip),
        Id::new(("tl-prop-keys", id, ch)),
        Sense::click_and_drag(),
    );
    if key_resp.clicked() || key_resp.secondary_clicked() {
        shell.ui.timeline_scrubbing = true;
        if let Some(pos) = key_resp.interact_pointer_pos() {
            shell.editor.go_to(frame_at(pos.x));
        }
        if !camera {
            shell.editor.select(id, SelectMode::Replace);
        }
    }
    if key_resp.drag_started_by(egui::PointerButton::Primary)
        && let Some(start) = ui.input(|i| i.pointer.press_origin())
    {
        let from = frame_at(start.x);
        if (x_of(from as f32) - start.x).abs() <= KEY_GRAB_PX && frames.iter().any(|(f, _)| *f == from) {
            if !camera {
                shell.editor.select(id, SelectMode::Replace);
            }
            shell.editor.begin_channel_key_drag(id, ch, from);
        }
    }
    if key_resp.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = key_resp.interact_pointer_pos()
    {
        if shell.editor.is_dragging_keys() {
            shell.editor.drag_keys_to(frame_at(pos.x));
        } else {
            shell.ui.timeline_scrubbing = true;
            shell.editor.go_to(frame_at(pos.x));
            edge_scroll(ui, shell, keys_clip, pos.x);
        }
    }
    if key_resp.drag_stopped() {
        shell.editor.end_key_drag();
    }
    if key_resp.hovered()
        && let Some(pos) = key_resp.hover_pos()
    {
        let f = frame_at(pos.x);
        let on_key = (x_of(f as f32) - pos.x).abs() <= KEY_GRAB_PX && frames.iter().any(|(k, _)| *k == f);
        if shell.editor.is_dragging_keys() || on_key {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    menus::context_menu(&key_resp, |ui| {
        ui.label(
            RichText::new(format!("{label}, frame {}", shell.editor.current_frame())).color(p.text_muted),
        );
        menus::item(ui, shell, Action::AddChannelKey(ch));
        menus::item(ui, shell, Action::DeleteChannelKey(ch));
        for interp in Interpolation::ALL {
            menus::item(ui, shell, Action::SetChannelInterpolation(ch, interp));
        }
    });
}

/// Each key frame of the layer, with the interpolation its keys share
/// (`None` when they differ).
fn key_glyphs(layer: &fanimate_core::Layer) -> Vec<(Frame, Option<Interpolation>)> {
    layer
        .key_frames()
        .into_iter()
        .map(|f| {
            let mut interp: Option<Option<Interpolation>> = None;
            layer.key_lists(|k| {
                if let Some(i) = k.interp_at(f) {
                    interp = Some(match interp {
                        None => Some(i),
                        Some(Some(j)) if j == i => Some(i),
                        Some(_) => None,
                    });
                }
            });
            (f, interp.flatten())
        })
        .collect()
}

/// How close, in points, the pointer must be to a key to drag it.
const KEY_GRAB_PX: f32 = 6.0;

/// While an action is open, say so above the timeline: its keys are being
/// edited, not the main timeline's.
fn action_banner(ui: &mut egui::Ui, shell: &mut Shell) {
    let Some((layer, index)) = shell.editor.editing_action() else {
        return;
    };
    let Some(act) = shell.editor.rig_actions(layer).get(index) else {
        return;
    };
    let p = shell.palette;
    let text = format!(
        "Editing action \"{}\" (frames 0 to {}). Changes go into the action, not the timeline.",
        act.name, act.length
    );
    egui::Frame::NONE
        .fill(p.accent.gamma_multiply(0.25))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .corner_radius(3.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(text).size(12.0).color(p.text));
                if ui.button("Finish editing").clicked() {
                    shell.pending.push(Action::FinishAction);
                }
            });
        });
}

/// How keys added from now on move on to the next key, with the shape and
/// colour they have on the timeline: smooth (round, amber), linear (a
/// diamond, blue), step (square, red).
fn key_interp_dropdown(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let current = shell.editor.key_interp;
    let mark = |ui: &mut egui::Ui, interp: Interpolation| {
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        key_shape(
            ui.painter(),
            rect.center(),
            Some(interp),
            p.key_color(Some(interp)),
            Stroke::new(1.0, p.bg),
        );
    };
    mark(ui, current);
    let mut chosen = None;
    egui::ComboBox::from_id_salt("key-interpolation")
        .width(70.0)
        .selected_text(current.label())
        .show_ui(ui, |ui| {
            for interp in Interpolation::ALL {
                ui.horizontal(|ui| {
                    mark(ui, interp);
                    if ui.selectable_label(interp == current, interp.label()).clicked() {
                        chosen = Some(interp);
                    }
                });
            }
        })
        .response
        .on_hover_text(
            "How keys you add move on to the next key. Smooth eases in and out, Linear moves at a \
             steady speed, Step holds then jumps. Keys already on the timeline keep theirs.",
        );
    if let Some(interp) = chosen {
        shell.pending.push(Action::SetKeyInterpolation(interp));
    }
}

fn transport(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.horizontal(|ui| {
        let tip = |shell: &Shell, a: Action| {
            let s = shell.keymap.shortcut_text(a);
            if s.is_empty() {
                a.label().to_owned()
            } else {
                format!("{} ({s})", a.label())
            }
        };
        ui.spacing_mut().item_spacing.x = 2.0;
        let play = if shell.editor.playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        for (icon, action) in [
            (Icon::FirstFrame, Action::FirstFrame),
            (Icon::PrevFrame, Action::PrevFrame),
            (play, Action::PlayPause),
            (Icon::NextFrame, Action::NextFrame),
            (Icon::LastFrame, Action::LastFrame),
        ] {
            if icons::button(ui, icon, 24.0, false, false)
                .on_hover_text(tip(shell, action))
                .clicked()
            {
                shell.pending.push(action);
            }
        }
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.separator();
        ui.label("Frame");
        let mut f = shell.editor.current_frame();
        let (start, end) = (shell.editor.doc.start, shell.editor.doc.end);
        if ui
            // free to go past the last frame: later frames can hold keys too
            .add(egui::DragValue::new(&mut f).range(start..=Frame::MAX).speed(0.2))
            .changed()
        {
            shell.editor.go_to(f);
        }
        // the last frame of the animation, set right here
        ui.label(RichText::new("of").color(shell.palette.text_muted));
        let mut new_end = end;
        let r = ui
            .add(
                egui::DragValue::new(&mut new_end)
                    .range(start..=100_000)
                    .speed(0.2),
            )
            .on_hover_text("Last frame of the animation. Frames after it stay on the timeline.");
        if r.changed() {
            begin_edit(shell, &r, "Change last frame");
            shell.editor.doc.end = new_end;
        }
        ui.label(RichText::new(format!("{} fps", shell.editor.doc.fps)).color(shell.palette.text_muted));
        ui.separator();
        for (icon, action) in [
            (Icon::AddKey, Action::AddKey),
            (Icon::DeleteKey, Action::DeleteKey),
        ] {
            let enabled = shell.editor.can_run(action);
            if icons::button(ui, icon, 24.0, false, !enabled)
                .on_hover_text(tip(shell, action))
                .clicked()
                && enabled
            {
                shell.pending.push(action);
            }
        }
        key_interp_dropdown(ui, shell);
        ui.separator();
        // onion skin: the toggle, and its settings on right-click
        let onion = icons::button(ui, Icon::OnionSkin, 24.0, shell.onion.on, false).on_hover_text(format!(
            "{}. Right-click for settings.",
            tip(shell, Action::ToggleOnionSkin)
        ));
        if onion.clicked() {
            shell.pending.push(Action::ToggleOnionSkin);
        }
        menus::context_menu(&onion, |ui| {
            let o = &mut shell.onion;
            ui.checkbox(&mut o.on, "Show onion skin");
            ui.horizontal(|ui| {
                ui.label("Before");
                ui.add(egui::DragValue::new(&mut o.before).range(0..=10));
                ui.label("After");
                ui.add(egui::DragValue::new(&mut o.after).range(0..=10));
            });
            ui.checkbox(&mut o.keys, "Drawings and keys only")
                .on_hover_text("Show the previous and next drawings, however long they are held");
            ui.checkbox(&mut o.all_layers, "All layers")
                .on_hover_text("Otherwise only the selected layers");
        });
        if icons::button(ui, Icon::LightTable, 24.0, shell.light_table, false)
            .on_hover_text(tip(shell, Action::ToggleLightTable))
            .clicked()
        {
            shell.pending.push(Action::ToggleLightTable);
        }
        if icons::button(ui, Icon::XSheet, 24.0, false, false)
            .on_hover_text("X-Sheet: the same timing, frames down the page")
            .clicked()
        {
            shell.ui.open_tab = Some(crate::panels::Tab::XSheet);
        }
        // zoom, at the right: shortcuts and Ctrl+scroll work too
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let zoom_tip = |shell: &Shell, what: &str, a: Action| {
                let s = shell.keymap.shortcut_text(a);
                let keys = if s.is_empty() {
                    "Ctrl+scroll also zooms".to_owned()
                } else {
                    format!("{s} while the pointer is over the timeline; Ctrl+scroll also zooms")
                };
                format!("{what}. {keys}.")
            };
            let mut zoom = None;
            for (icon, what, action) in [
                (Icon::ZoomFit, "Fit the whole animation", Action::FitView),
                (Icon::ZoomReset, "Reset timeline zoom", Action::ActualSize),
                (Icon::ZoomIn, "Zoom in on the timeline", Action::ZoomIn),
                (Icon::ZoomOut, "Zoom out of the timeline", Action::ZoomOut),
            ] {
                if icons::button(ui, icon, 24.0, false, false)
                    .on_hover_text(zoom_tip(shell, what, action))
                    .clicked()
                {
                    zoom = Some(action);
                }
            }
            match zoom {
                Some(Action::FitView) => fit_all(shell),
                Some(Action::ActualSize) => reset_zoom(shell),
                Some(Action::ZoomIn) => zoom_by(shell, 1.25),
                Some(Action::ZoomOut) => zoom_by(shell, 0.8),
                _ => {}
            }
        });
    });
    end_edits(ui, shell);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a frame is drawn, in pixels from the left of the frame area.
    fn x_of(frame: f32, start: f32, scroll: f32, width: f32) -> f32 {
        (frame - start - scroll) * width + width / 2.0
    }

    #[test]
    fn a_frame_out_of_view_is_scrolled_to_with_a_little_room() {
        // 12 px frames in a 600 px view: 50 frames show
        let (fw, view) = (12.0, 600.0);
        // already in view: nothing moves
        assert_eq!(reveal(0.0, 0, 10, fw, view), 0.0);
        // past the right edge: scrolls just far enough, keeping a margin
        let scroll = reveal(0.0, 0, 80, fw, view);
        let right_edge = (80.0 - scroll) * fw + fw;
        assert!(right_edge <= view - fw, "{right_edge}");
        assert!(
            right_edge >= view - 4.0 * fw,
            "scrolls no further than it must: {right_edge}"
        );
        // before the left edge: scrolls back, never below zero
        // frame 5 comes to two frames from the left edge (the margin)
        assert_eq!(reveal(30.0, 0, 5, fw, view), 3.0);
        assert_eq!(reveal(2.0, 0, 0, fw, view), 0.0);
        // with a first frame that is not zero
        assert_eq!(reveal(0.0, 24, 24, fw, view), 0.0);
    }

    #[test]
    fn a_scrub_held_at_the_edge_scrolls_faster_the_further_out() {
        let (left, right) = (100.0, 700.0);
        // inside the view: still
        assert_eq!(edge_speed(400.0, left, right), 0.0);
        assert_eq!(edge_speed(right - 28.0, left, right), 0.0);
        // at the edge and beyond it: to the right, faster and faster, then capped
        let at_edge = edge_speed(right, left, right);
        let beyond = edge_speed(right + 60.0, left, right);
        let far = edge_speed(right + 5000.0, left, right);
        assert!(0.0 < at_edge && at_edge < beyond && beyond < far);
        assert_eq!(far, edge_speed(right + 1000.0, left, right));
        // and to the left the same, backwards
        assert_eq!(edge_speed(left, left, right), -at_edge);
        assert!(edge_speed(left - 60.0, left, right) < edge_speed(left, left, right));
    }

    #[test]
    fn zooming_keeps_the_anchor_frame_in_place() {
        for (anchor, scroll, old, new) in [
            (30.0, 10.0, 12.0, 24.0),
            (30.0, 0.0, 12.0, 3.0),
            (5.0, 2.0, 40.0, 60.0),
        ] {
            let s = zoomed_scroll(anchor, 0.0, scroll, old, new);
            let before = x_of(anchor, 0.0, scroll, old);
            let after = x_of(anchor, 0.0, s, new);
            assert!((before - after).abs() < 1e-3, "{before} vs {after}");
        }
    }
}
