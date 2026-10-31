//! The Timeline panel: transport controls, a scrubbable ruler, and a row of
//! keyframes per layer.

use std::collections::BTreeSet;

use eframe::egui::{self, CursorIcon, Id, Pos2, Rangef, Rect, RichText, Sense, Shape, Stroke, pos2, vec2};
use fanimate_core::anim::KeyList;
use fanimate_core::{
    Camera, ChannelId, Frame, Interpolation, Layer, LayerId, LayerKind, LoopMode, REST_FRAME,
};
use fanimate_editor::{Action, Focus, KeyRef, KeySelect, SelectMode};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;
use crate::theme::{self, Palette};
use crate::timecode;

use super::properties::{begin_edit, end_edits};

/// Stands for the camera where a row is named by a layer: it is not a layer,
/// and no layer has this id.
pub const CAMERA_ROW: LayerId = LayerId(u32::MAX);

/// The names column is this wide until the divider is dragged.
const DEFAULT_NAMES_WIDTH: f32 = 170.0;
const MIN_NAMES_WIDTH: f32 = 100.0;
/// Width of the divider's hit area, which draws as a hairline.
const DIVIDER_GRAB: f32 = 6.0;
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

    // the names column's width is the tab's; rows read it back from the shell
    let names_w = resolve_names_width(shell.ui.timeline_names_width, full.width());
    shell.ui.timeline_names_width = names_w;
    let frames_rect = Rect::from_min_max(pos2(full.left() + names_w, full.top()), full.max);
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
            REST_FRAME,
            now,
            shell.ui.timeline_frame_width,
            frames_rect.width(),
        );
    }
    shell.ui.timeline_seen_frame = now;
    let fw = shell.ui.timeline_frame_width;
    let scroll = shell.ui.timeline_scroll;
    // the view starts at the rest pose, frame 0, so frame 1 is the second cell
    let x_of = |f: f32| ruler.left() + (f - REST_FRAME as f32 - scroll) * fw + fw / 2.0;
    let frame_at = |x: f32| -> Frame { frame_at_x(x, &x_of) };

    // keep the playhead in view while playing
    if shell.editor.playing {
        let x = x_of(shell.editor.frame);
        if x > frames_rect.right() - 20.0 || x < frames_rect.left() {
            shell.ui.timeline_scroll = (shell.editor.frame - REST_FRAME as f32 - 2.0).max(0.0);
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
    // the rest pose is a frame of its own: say so when the pointer rests on it
    let rest_cell = Rect::from_min_max(
        pos2(x_of(REST_FRAME as f32) - fw / 2.0, ruler.top()),
        pos2(x_of(REST_FRAME as f32) + fw / 2.0, ruler.bottom()),
    );
    if let Some(pos) = ruler_resp.hover_pos()
        && rest_cell.contains(pos)
    {
        ruler_resp.clone().on_hover_text("Rest pose");
    }
    let label_every = nice_step(48.0 / fw);
    let second_every = second_label_every(fw, fps);
    let per_second = frames_per_second(fps);
    let first = frame_at(ruler.left()).max(REST_FRAME);
    // the ruler runs on past the end frame
    let last = frame_at(ruler.right());
    for f in first..=last {
        let x = x_of(f as f32);
        let tick = if f > end {
            p.text_muted.gamma_multiply(0.5)
        } else {
            p.text_muted
        };
        let second = timecode::is_second_start(f, fps);
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
                theme::mono(10.0),
                tick,
            );
        }
        // whole seconds, on the major ticks and only as often as they fit
        if second && (f / per_second) % second_every == 0 {
            painter.text(
                pos2(x + 3.0, ruler.bottom() - 1.0),
                egui::Align2::LEFT_BOTTOM,
                timecode::seconds_label(f, fps),
                theme::mono(9.0),
                tick.gamma_multiply(0.7),
            );
        }
    }

    // rows
    let rows_rect = Rect::from_min_max(pos2(full.left(), ruler.bottom()), full.max);
    let keys_clip = Rect::from_min_max(pos2(frames_rect.left(), rows_rect.top()), rows_rect.max);
    let shown = visible_rows(&shell.editor, shell.hide_unanimated_layers);
    // the camera shows once it has keys, or while looking through it
    let show_camera = shell.editor.doc.camera.is_animated() || shell.look_through_camera;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rows_rect));
    // where each row's keys are this frame, for box select and the scale grips
    let mut key_rows: Vec<KeyRow> = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("timeline-rows")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // the camera is the project's, pinned above the layers
            if show_camera {
                camera_row(ui, shell, keys_clip, &x_of, &frame_at, &mut key_rows);
                if shell.ui.timeline_expanded.contains(&CAMERA_ROW) {
                    for ch in ChannelId::CAMERA {
                        property_row(
                            ui,
                            shell,
                            CAMERA_ROW,
                            ch,
                            0,
                            keys_clip,
                            &x_of,
                            &frame_at,
                            &mut key_rows,
                        );
                    }
                }
            }
            for (id, depth) in shown {
                row(ui, shell, id, depth, keys_clip, &x_of, &frame_at, &mut key_rows);
                // a layer opened with a double-click lists its properties
                if shell.ui.timeline_expanded.contains(&id) {
                    let channels = shell
                        .editor
                        .doc
                        .find(id)
                        .map(|l| l.channels())
                        .unwrap_or_default();
                    for ch in channels {
                        property_row(
                            ui,
                            shell,
                            id,
                            ch,
                            depth,
                            keys_clip,
                            &x_of,
                            &frame_at,
                            &mut key_rows,
                        );
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
    // the rest pose column is a little dimmer than the frames that play, and
    // a hairline closes it
    let rest_right = x_of(REST_FRAME as f32) + fw / 2.0;
    if rest_right > frames_rect.left() {
        painter.rect_filled(
            Rect::from_min_max(
                pos2(rest_cell.left().max(frames_rect.left()), ruler.top()),
                pos2(rest_right, full.bottom()),
            ),
            0.0,
            p.bg.gamma_multiply(0.3),
        );
        painter.vline(
            rest_right,
            Rangef::new(ruler.top(), full.bottom()),
            Stroke::new(1.0, p.border),
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

    // the empty space below the last row: a click lets go of the picked keys
    // and a drag from it draws a box, as from the empty part of a row
    let rows_bottom = key_rows
        .iter()
        .map(|r| r.rect.bottom())
        .fold(rows_rect.top(), f32::max);
    let below = Rect::from_min_max(pos2(keys_clip.left(), rows_bottom), keys_clip.max);
    if below.height() > 2.0 && !grip_busy(ui.ctx()) {
        let resp = ui.interact(below, Id::new("timeline-below-rows"), Sense::click_and_drag());
        let extend = ui.input(|i| i.modifiers.command || i.modifiers.shift);
        if resp.clicked()
            && !extend
            && let Some(pos) = resp.interact_pointer_pos()
        {
            // as on the empty part of a row: let go of the keys and go there
            shell.editor.set_focus(Focus::Timeline);
            shell.ui.timeline_scrubbing = true;
            shell.editor.clear_key_selection();
            shell.editor.go_to(frame_at(pos.x));
        }
        if resp.drag_started_by(egui::PointerButton::Primary)
            && let Some(start) = ui.input(|i| i.pointer.press_origin())
        {
            shell.editor.set_focus(Focus::Timeline);
            let base: Vec<KeyRef> = if extend {
                shell.editor.selected_keys().iter().copied().collect()
            } else {
                shell.editor.clear_key_selection();
                Vec::new()
            };
            let gesture = Gesture::Boxing {
                origin_cell: cell_at(start.x, &x_of),
                origin_y: start.y,
                base,
            };
            set_gesture(ui.ctx(), Some(gesture));
        }
    }

    key_gestures(ui, shell, &key_rows, keys_clip, &x_of, &frame_at);
    names_divider(ui, shell, full);
}

/// The width of the names column for a stored `width` (nothing stored yet is
/// 0): the default if unset, else between the minimum and half the panel.
fn resolve_names_width(width: f32, panel_width: f32) -> f32 {
    if !width.is_finite() || width <= 0.0 {
        return DEFAULT_NAMES_WIDTH.min((panel_width / 2.0).max(MIN_NAMES_WIDTH));
    }
    width.clamp(MIN_NAMES_WIDTH, (panel_width / 2.0).max(MIN_NAMES_WIDTH))
}

/// The line between the names and the frames: a hairline you can drag to
/// give the names more or less room, and double-click to reset. Made after
/// the rows so it is on top of them.
fn names_divider(ui: &mut egui::Ui, shell: &mut Shell, full: Rect) {
    let p = shell.palette;
    let x = full.left() + shell.ui.timeline_names_width;
    let grab = Rect::from_min_max(
        pos2(x - DIVIDER_GRAB / 2.0, full.top()),
        pos2(x + DIVIDER_GRAB / 2.0, full.bottom()),
    );
    let resp = ui.interact(grab, Id::new("timeline-names-divider"), Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if resp.double_clicked() {
        shell.ui.timeline_names_width = DEFAULT_NAMES_WIDTH;
    } else if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        shell.ui.timeline_names_width = resolve_names_width(pos.x - full.left(), full.width());
    }
    let color = if resp.hovered() || resp.dragged() {
        p.accent
    } else {
        p.border
    };
    ui.painter_at(full)
        .vline(x, Rangef::new(full.top(), full.bottom()), Stroke::new(1.0, color));
}

/// Label spacing in frames that is at least `min` and a "round" number.
fn nice_step(min: f32) -> Frame {
    [1, 2, 4, 6, 12, 24, 48, 96, 240, 480, 960]
        .into_iter()
        .find(|s| *s as f32 >= min)
        .unwrap_or(1920)
}

/// Whole frames in a second, as the ruler counts them (see `timecode`).
fn frames_per_second(fps: f32) -> Frame {
    if fps.is_finite() && fps >= 1.0 {
        (fps.round() as Frame).max(1)
    } else {
        1
    }
}

/// Every how many seconds the ruler names one: a label needs about 40 px, so
/// zoomed out it skips seconds, to a round count.
fn second_label_every(frame_width: f32, fps: f32) -> Frame {
    let second_px = frame_width * frames_per_second(fps) as f32;
    [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800]
        .into_iter()
        .find(|s| *s as f32 * second_px >= 40.0)
        .unwrap_or(3600)
}

/// Cut `text` to the longest start of it that, with a "…" after it, is no
/// wider than `max_width` by `measure`. Text that already fits is kept.
fn ellipsize(text: &str, max_width: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |n: usize| {
        let head: String = chars[..n].iter().collect();
        format!("{}…", head.trim_end())
    };
    // the widest cut that fits; the width only grows with the length
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if measure(cut(mid).as_str()) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
}

/// `text` as it fits in `max_width` points when drawn with `font`: cut with
/// a "…" when it is too long.
fn fit_text(painter: &egui::Painter, text: &str, font: &egui::FontId, max_width: f32) -> String {
    ellipsize(text, max_width, |s| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    })
}

/// Draw a row's name starting at `x`, cut to end before `right`, and say
/// whether it was cut (so the full name can go in a tooltip).
fn name_label(
    painter: &egui::Painter,
    (x, y): (f32, f32),
    right: f32,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) -> bool {
    let shown = fit_text(painter, text, &font, (right - x).max(0.0));
    let cut = shown != text;
    painter.text(pos2(x, y), egui::Align2::LEFT_CENTER, shown, font, color);
    cut
}

/// The rows to list, from `rows` (every row, in display order with its
/// depth): all of them, or with `hide` only those `keep` says to keep and
/// the groups they sit in.
fn filter_rows(
    rows: Vec<(LayerId, usize)>,
    hide: bool,
    keep: impl Fn(LayerId) -> bool,
) -> Vec<(LayerId, usize)> {
    if !hide {
        return rows;
    }
    // rows come parent first, then its children one level deeper: the
    // parent of a row is the nearest row above it that is shallower
    let mut parent: Vec<Option<usize>> = Vec::with_capacity(rows.len());
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for (i, &(_, depth)) in rows.iter().enumerate() {
        while stack.last().is_some_and(|&(d, _)| d >= depth) {
            stack.pop();
        }
        parent.push(stack.last().map(|&(_, j)| j));
        stack.push((depth, i));
    }
    let mut kept = vec![false; rows.len()];
    for (i, &(id, _)) in rows.iter().enumerate() {
        if !keep(id) {
            continue;
        }
        // the row and every group around it, up to one already kept
        let mut at = Some(i);
        while let Some(j) = at {
            if kept[j] {
                break;
            }
            kept[j] = true;
            at = parent[j];
        }
    }
    rows.into_iter()
        .zip(kept)
        .filter_map(|(row, kept)| kept.then_some(row))
        .collect()
}

/// Whether the layer, or any layer inside it, has keys.
fn has_motion(layer: &Layer) -> bool {
    layer.is_animated() || layer.kind.children().is_some_and(|c| c.iter().any(has_motion))
}

/// The layer rows to list: every layer, or with `hide_unanimated` only the
/// selected layers, those with keys, and the groups around them.
fn visible_rows(ed: &fanimate_editor::Editor, hide_unanimated: bool) -> Vec<(LayerId, usize)> {
    filter_rows(ed.display_rows(), hide_unanimated, |id| {
        ed.is_selected(id) || ed.doc.find(id).is_some_and(has_motion)
    })
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

/// The furthest the view may scroll, in frames from the rest pose: until
/// the last frame there is anything at sits at the left edge.
fn max_scroll(shell: &Shell) -> f32 {
    (last_frame(shell) - REST_FRAME).max(0) as f32
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
    let origin = REST_FRAME as f32;
    let max = max_scroll(shell);
    let ui = &mut shell.ui;
    let old = ui.timeline_frame_width.clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
    let new = width.clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
    ui.timeline_scroll = zoomed_scroll(anchor, origin, ui.timeline_scroll, old, new).clamp(0.0, max);
    ui.timeline_frame_width = new;
}

/// Where zoom buttons and shortcuts zoom around: the playhead if it is on
/// screen, else the middle of the view.
fn zoom_anchor(shell: &Shell) -> f32 {
    let ui = &shell.ui;
    let fw = ui.timeline_frame_width.max(MIN_FRAME_WIDTH);
    let first = REST_FRAME as f32 + ui.timeline_scroll;
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
    // the rest pose is in view too
    let frames = (shell.editor.doc.end - REST_FRAME + 1).max(1) as f32;
    let width = (shell.ui.timeline_view_width / frames).clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
    shell.ui.timeline_frame_width = width;
    shell.ui.timeline_scroll = 0.0;
}

fn zoom_and_scroll(ui: &egui::Ui, shell: &mut Shell, area: Rect) {
    let hovered = ui.rect_contains_pointer(area);
    if !hovered {
        return;
    }
    let (zoom, scroll, mods) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers));
    if zoom != 1.0 {
        // zoom around the frame under the pointer
        let fw = shell.ui.timeline_frame_width;
        let pointer_x = ui
            .input(|i| i.pointer.hover_pos())
            .map_or(area.center().x, |p| p.x);
        let anchor = REST_FRAME as f32 + shell.ui.timeline_scroll + (pointer_x - area.left() - fw / 2.0) / fw;
        set_frame_width(shell, fw * zoom, anchor);
    } else if scroll.x != 0.0 || (mods.shift && scroll.y != 0.0) {
        let dx = if scroll.x != 0.0 { scroll.x } else { scroll.y };
        let max = max_scroll(shell);
        shell.ui.timeline_scroll =
            (shell.ui.timeline_scroll - dx / shell.ui.timeline_frame_width).clamp(0.0, max);
    }
}

// ----------------------------------------------------------------------
// Picking, moving and scaling keys

/// How far outside the end key of the picked keys the scale grip is grabbed,
/// from just past the key's own grab zone to this far.
const GRIP_NEAR: f32 = KEY_GRAB_PX;
const GRIP_FAR: f32 = 14.0;

/// The keys of one row as drawn this frame, for box select and the grips.
struct KeyRow {
    /// The row's key area, clipped to the view.
    rect: Rect,
    /// The layer, or `None` for the camera.
    layer: Option<LayerId>,
    /// The property, or `None` for the summary row of the layer or camera.
    channel: Option<ChannelId>,
    frames: Vec<Frame>,
}

/// An end of the picked keys, where a grip scales them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GripEnd {
    Left,
    Right,
}

/// The grip under the pointer: which end, the picked keys' vertical extent
/// and their first and last frame.
type GripHover = (GripEnd, (f32, f32), (Frame, Frame));

/// A drag that goes on across the rows, kept between frames (a drag that
/// starts in one row is followed in all of them).
#[derive(Clone)]
enum Gesture {
    /// A box over empty key area. `base` is what was picked before, kept when
    /// Shift or Ctrl began the box.
    Boxing {
        origin_cell: f32,
        origin_y: f32,
        base: Vec<KeyRef>,
    },
    /// Moving (or with Alt copying) the picked keys by whole frames, from the
    /// frame the pointer pressed on. `last` is the offset shown.
    Moving { start: Frame, last: Frame },
    /// Stretching the picked keys about `anchor`, from the grabbed end that
    /// began on `from_edge`; `grab_dx` is how far the pointer was from that
    /// key, so it does not jump.
    Scaling {
        end: GripEnd,
        anchor: Frame,
        from_edge: Frame,
        grab_dx: f32,
        last: Frame,
    },
}

const GESTURE_KEY: &str = "timeline-key-gesture";
const GRIP_KEY: &str = "timeline-key-grip";

fn gesture(ctx: &egui::Context) -> Option<Gesture> {
    ctx.data(|d| d.get_temp(Id::new(GESTURE_KEY)))
}

fn set_gesture(ctx: &egui::Context, g: Option<Gesture>) {
    let id = Id::new(GESTURE_KEY);
    ctx.data_mut(|d| match g {
        Some(g) => {
            d.insert_temp(id, g);
        }
        None => d.remove::<Gesture>(id),
    });
}

/// The grip the pointer was over last frame, if any.
fn armed_grip(ctx: &egui::Context) -> Option<GripEnd> {
    ctx.data(|d| d.get_temp::<Option<GripEnd>>(Id::new(GRIP_KEY)))
        .flatten()
}

fn arm_grip(ctx: &egui::Context, grip: Option<GripEnd>) {
    ctx.data_mut(|d| d.insert_temp(Id::new(GRIP_KEY), grip));
}

/// Whether a grip is under the pointer or being dragged, so rows leave the
/// press alone.
fn grip_busy(ctx: &egui::Context) -> bool {
    armed_grip(ctx).is_some() || matches!(gesture(ctx), Some(Gesture::Scaling { .. }))
}

/// How a key is outlined: a purple ring when picked.
fn key_outline(p: &Palette, picked: bool) -> Stroke {
    if picked {
        Stroke::new(1.5, p.accent)
    } else {
        Stroke::new(1.0, p.bg)
    }
}

/// Where along the frames `x` falls, as a number of frames: frame `n`'s cell
/// runs from `n` to `n + 1` and its key sits in the middle. `x_of` is the
/// view's frame to x map, which is a straight line.
fn cell_at(x: f32, x_of: &impl Fn(f32) -> f32) -> f32 {
    let rest = REST_FRAME as f32;
    let (x0, x1) = (x_of(rest), x_of(rest + 1.0));
    let width = x1 - x0;
    if width.abs() < 1e-6 {
        return rest + 0.5;
    }
    rest + 0.5 + (x - x0) / width
}

/// The x of a position `cell` along the frames (the inverse of [`cell_at`]).
fn x_at_cell(cell: f32, x_of: &impl Fn(f32) -> f32) -> f32 {
    let rest = REST_FRAME as f32;
    let (x0, x1) = (x_of(rest), x_of(rest + 1.0));
    x0 + (cell - rest - 0.5) * (x1 - x0)
}

/// The frame under `x`: the cell it falls in, never before the rest pose.
fn frame_at_x(x: f32, x_of: &impl Fn(f32) -> f32) -> Frame {
    (cell_at(x, x_of).floor() as Frame).max(REST_FRAME)
}

/// The first and last frame a box from cell `a` to cell `b` touches.
fn box_frames(a: f32, b: f32) -> (Frame, Frame) {
    let at = |c: f32| (c.floor() as Frame).max(REST_FRAME);
    (at(a.min(b)), at(a.max(b)))
}

/// The rows a band from `top` to `bottom` touches, as `select_keys_in` takes
/// them. Rows out of view (no height left) are not touched.
fn rows_in_band(rows: &[KeyRow], top: f32, bottom: f32) -> Vec<(Option<LayerId>, Option<ChannelId>)> {
    let (top, bottom) = (top.min(bottom), top.max(bottom));
    rows.iter()
        .filter(|r| r.rect.height() > 0.0 && r.rect.bottom() > top && r.rect.top() < bottom)
        .map(|r| (r.layer, r.channel))
        .collect()
}

/// The key of `frames` nearest `x`, if one is within `grab` of it.
fn key_under(frames: &[Frame], x: f32, x_of: &impl Fn(f32) -> f32, grab: f32) -> Option<Frame> {
    frames
        .iter()
        .copied()
        .map(|f| (f, (x_of(f as f32) - x).abs()))
        .filter(|(_, d)| *d <= grab)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(f, _)| f)
}

/// The first and last frame of the picked keys.
fn selection_span(keys: &BTreeSet<KeyRef>) -> Option<(Frame, Frame)> {
    let first = keys.iter().map(|k| k.frame).min()?;
    let last = keys.iter().map(|k| k.frame).max()?;
    Some((first, last))
}

/// Whether a picked key is drawn in `row`: a summary row shows every key of
/// its layer (or the camera).
fn row_holds(row: &KeyRow, key: &KeyRef) -> bool {
    row.layer == key.layer && (row.channel.is_none() || row.channel == key.channel)
}

/// The top and bottom of the rows that show picked keys.
fn selection_extent(rows: &[KeyRow], keys: &BTreeSet<KeyRef>) -> Option<(f32, f32)> {
    rows.iter()
        .filter(|r| r.rect.height() > 0.0 && keys.iter().any(|k| row_holds(r, k)))
        .map(|r| (r.rect.top(), r.rect.bottom()))
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

/// Which end of the picked keys the pointer is at the grip of: just outside
/// the end key's own grab zone, level with the rows that hold picked keys.
/// Only keys on different frames have ends.
fn grip_hit(
    span: (Frame, Frame),
    extent: (f32, f32),
    pointer: Pos2,
    x_of: &impl Fn(f32) -> f32,
) -> Option<GripEnd> {
    if span.0 >= span.1 || pointer.y < extent.0 || pointer.y > extent.1 {
        return None;
    }
    let (left, right) = (x_of(span.0 as f32), x_of(span.1 as f32));
    if (left - GRIP_FAR..=left - GRIP_NEAR).contains(&pointer.x) {
        Some(GripEnd::Left)
    } else if (right + GRIP_NEAR..=right + GRIP_FAR).contains(&pointer.x) {
        Some(GripEnd::Right)
    } else {
        None
    }
}

/// Whether `x` is on the bar a loop draws after its last key.
fn on_loop_bar(spans: &[LoopSpan], x: f32, x_of: &impl Fn(f32) -> f32) -> bool {
    spans.iter().any(|s| x >= x_of(s.last as f32) + KEY_GRAB_PX)
}

/// A row's key area and what the pointer does in it. Returns the response,
/// and whether a right-click landed on empty space (not on a key).
///
/// A click on a key picks it (Shift adds, Ctrl toggles), a click on empty
/// space lets go of the picked keys and goes to that frame. A drag from a key
/// moves all the picked keys (Alt copies); a drag from empty space starts a
/// box. The drag itself is followed by [`key_gestures`], after every row.
/// `select` is the layer a click also selects.
#[allow(clippy::too_many_arguments)]
fn key_area(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    salt: Id,
    row: KeyRow,
    select: Option<LayerId>,
    x_of: &impl Fn(f32) -> f32,
    frame_at: &impl Fn(f32) -> Frame,
    key_rows: &mut Vec<KeyRow>,
) -> (egui::Response, bool) {
    let resp = ui.interact(row.rect, salt, Sense::click_and_drag());
    let ctx = ui.ctx().clone();
    let mods = ui.input(|i| i.modifiers);
    let extend = mods.command || mods.shift;
    let (layer, channel) = (row.layer, row.channel);
    let grab = |x: f32| key_under(&row.frames, x, x_of, KEY_GRAB_PX);
    let key_ref = |f: Frame| KeyRef::new(layer, channel, f);
    let layer_picked = select.is_some_and(|id| shell.editor.is_selected(id));
    let pick_layer = |shell: &mut Shell| {
        if let Some(id) = select {
            shell.editor.select(id, SelectMode::Replace);
        }
    };
    let mut on_empty = false;

    // a grip under the pointer takes the press, and its release is no click
    if !grip_busy(&ctx) {
        if resp.clicked()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            shell.editor.set_focus(Focus::Timeline);
            shell.ui.timeline_scrubbing = true;
            match grab(pos.x) {
                Some(f) => {
                    let how = if mods.command {
                        KeySelect::Toggle
                    } else if mods.shift {
                        KeySelect::Add
                    } else {
                        KeySelect::Replace
                    };
                    shell.editor.select_key(key_ref(f), how);
                    if !extend {
                        shell.editor.go_to(f);
                        pick_layer(shell);
                    }
                }
                None if !extend => {
                    shell.editor.clear_key_selection();
                    shell.editor.go_to(frame_at(pos.x));
                    pick_layer(shell);
                }
                None => {}
            }
        }
        if resp.secondary_clicked()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            // right-clicking a key picks it unless it is already picked, so a
            // menu on a picked key acts on all the picked keys
            shell.editor.set_focus(Focus::Timeline);
            shell.ui.timeline_scrubbing = true;
            match grab(pos.x) {
                Some(f) => {
                    if !shell.editor.is_key_selected(&key_ref(f)) {
                        shell.editor.select_key(key_ref(f), KeySelect::Replace);
                    }
                    shell.editor.go_to(f);
                }
                None => {
                    on_empty = true;
                    shell.editor.go_to(frame_at(pos.x));
                }
            }
            if !layer_picked {
                pick_layer(shell);
            }
        }
        if resp.drag_started_by(egui::PointerButton::Primary)
            && let Some(start) = ui.input(|i| i.pointer.press_origin())
        {
            shell.editor.set_focus(Focus::Timeline);
            match grab(start.x) {
                Some(f) => {
                    // a key that is not picked is picked first, alone
                    if !shell.editor.is_key_selected(&key_ref(f)) {
                        let how = if extend {
                            KeySelect::Add
                        } else {
                            KeySelect::Replace
                        };
                        shell.editor.select_key(key_ref(f), how);
                    }
                    if !layer_picked {
                        pick_layer(shell);
                    }
                    if shell.editor.begin_selected_key_drag(mods.alt) {
                        let start = frame_at(start.x);
                        set_gesture(&ctx, Some(Gesture::Moving { start, last: 0 }));
                    }
                }
                None => {
                    let base: Vec<KeyRef> = if extend {
                        shell.editor.selected_keys().iter().copied().collect()
                    } else {
                        shell.editor.clear_key_selection();
                        Vec::new()
                    };
                    let gesture = Gesture::Boxing {
                        origin_cell: cell_at(start.x, x_of),
                        origin_y: start.y,
                        base,
                    };
                    set_gesture(&ctx, Some(gesture));
                }
            }
        }
        if let Some(pos) = resp.hover_pos()
            && grab(pos.x).is_some()
        {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
    }
    key_rows.push(row);
    (resp, on_empty)
}

/// Follows a key drag across the rows once they are all drawn: the box, the
/// keys being moved, and the scale grips (shown only with the pointer at an
/// end of two or more picked keys on different frames).
fn key_gestures(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    rows: &[KeyRow],
    area: Rect,
    x_of: &impl Fn(f32) -> f32,
    frame_at: &impl Fn(f32) -> Frame,
) {
    let ctx = ui.ctx().clone();
    let p = shell.palette;
    let (pos, down, pressed, mods) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.modifiers,
        )
    });
    let mut current = gesture(&ctx);
    let painter = ui.painter_at(area);

    // the grip under the pointer, if any: not while over a key, which moves
    let mut hover: Option<GripHover> = None;
    if current.is_none()
        && let Some(pos) = pos
        && let Some(span) = selection_span(shell.editor.selected_keys()).filter(|s| s.0 < s.1)
        && ui.rect_contains_pointer(area)
    {
        let on_key = rows.iter().any(|r| {
            r.rect.top() <= pos.y
                && pos.y < r.rect.bottom()
                && key_under(&r.frames, pos.x, x_of, KEY_GRAB_PX).is_some()
        });
        if !on_key
            && let Some(extent) = selection_extent(rows, shell.editor.selected_keys())
            && let Some(end) = grip_hit(span, extent, pos, x_of)
        {
            hover = Some((end, extent, span));
        }
    }
    arm_grip(&ctx, hover.map(|h| h.0));

    if pressed
        && let Some((end, _, span)) = hover
        && let Some(pos) = pos
    {
        let (from_edge, opposite) = match end {
            GripEnd::Left => (span.0, span.1),
            GripEnd::Right => (span.1, span.0),
        };
        // Alt stretches about the playhead instead of the far end
        let anchor = if mods.alt {
            shell.editor.current_frame()
        } else {
            opposite
        };
        shell.editor.set_focus(Focus::Timeline);
        if shell.editor.begin_key_scale(anchor) {
            let g = Gesture::Scaling {
                end,
                anchor,
                from_edge,
                grab_dx: x_of(from_edge as f32) - pos.x,
                last: from_edge,
            };
            set_gesture(&ctx, Some(g.clone()));
            current = Some(g);
        }
    }

    let Some(g) = current else {
        if let Some((end, extent, span)) = hover {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
            let key = if end == GripEnd::Left { span.0 } else { span.1 };
            draw_grip(&painter, &p, end, x_of(key as f32), (extent.0 + extent.1) / 2.0);
        }
        return;
    };
    ctx.request_repaint();
    if !down {
        match g {
            Gesture::Moving { .. } => shell.editor.end_selected_key_drag(),
            Gesture::Scaling { .. } => shell.editor.end_key_scale(),
            Gesture::Boxing { .. } => {}
        }
        set_gesture(&ctx, None);
        return;
    }
    let Some(pos) = pos else {
        return;
    };
    match g {
        Gesture::Moving { start, last } => {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
            let delta = frame_at(pos.x) - start;
            if delta != last {
                shell.editor.drag_selected_keys_by(delta);
                set_gesture(&ctx, Some(Gesture::Moving { start, last: delta }));
            }
            edge_scroll(ui, shell, area, pos.x);
        }
        Gesture::Scaling {
            end,
            anchor,
            from_edge,
            grab_dx,
            last,
        } => {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
            let to = frame_at(pos.x + grab_dx);
            if to != last {
                shell.editor.scale_selected_keys(anchor, from_edge, to);
                set_gesture(
                    &ctx,
                    Some(Gesture::Scaling {
                        end,
                        anchor,
                        from_edge,
                        grab_dx,
                        last: to,
                    }),
                );
            }
            if let Some((first, last_key)) = selection_span(shell.editor.selected_keys()) {
                if let Some(extent) = selection_extent(rows, shell.editor.selected_keys()) {
                    let key = if end == GripEnd::Left { first } else { last_key };
                    draw_grip(&painter, &p, end, x_of(key as f32), (extent.0 + extent.1) / 2.0);
                }
                let n = last_key - first;
                let fps = shell.editor.doc.fps;
                let text = format!(
                    "{n} frame{} ({})",
                    if n == 1 { "" } else { "s" },
                    timecode::clock(n, fps)
                );
                scale_readout(ui, &p, pos, text);
            }
            edge_scroll(ui, shell, area, pos.x);
        }
        Gesture::Boxing {
            origin_cell,
            origin_y,
            base,
        } => {
            let band = Rect::from_two_pos(pos2(x_at_cell(origin_cell, x_of), origin_y), pos);
            let (from, to) = box_frames(origin_cell, cell_at(pos.x, x_of));
            let hit = rows_in_band(rows, band.top(), band.bottom());
            // what was picked before the box (with Shift or Ctrl), then what is in it
            shell.editor.clear_key_selection();
            for k in &base {
                shell.editor.select_key(*k, KeySelect::Add);
            }
            shell.editor.select_keys_in(&hit, from, to, true);
            painter.rect(
                band,
                0.0,
                p.accent.gamma_multiply(0.15),
                Stroke::new(1.0, p.accent),
                egui::StrokeKind::Inside,
            );
            edge_scroll(ui, shell, area, pos.x);
        }
    }
}

/// The small bar that shows where the picked keys can be grabbed to scale.
fn draw_grip(painter: &egui::Painter, p: &Palette, end: GripEnd, key_x: f32, y: f32) {
    let out = match end {
        GripEnd::Left => -1.0,
        GripEnd::Right => 1.0,
    };
    let x = key_x + out * (GRIP_NEAR + GRIP_FAR) / 2.0;
    let bar = Rect::from_center_size(pos2(x, y), vec2(3.0, 10.0));
    painter.rect_filled(bar, 1.5, p.accent);
}

/// The length of the picked keys, beside the pointer while they are scaled.
fn scale_readout(ui: &egui::Ui, p: &Palette, pointer: Pos2, text: String) {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(text, theme::mono(11.0), p.text);
    let size = galley.size() + vec2(10.0, 6.0);
    let rect = Rect::from_min_size(pointer + vec2(14.0, -size.y - 6.0), size);
    painter.rect(
        rect,
        3.0,
        p.raised,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(5.0, 3.0), galley, p.text);
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
    key_rows: &mut Vec<KeyRow>,
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
    // what repeats after its last key, drawn as a bar with ghost keys
    let loops = layer_loop_spans(layer);
    let p = shell.palette;
    let selected = shell.editor.is_selected(id);
    let names_w = shell.ui.timeline_names_width;

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let names = Rect::from_min_max(rect.min, pos2(rect.left() + names_w, rect.bottom()));
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
    // long names are cut before the arrow; the whole name is in the tooltip
    let name_cut = name_label(
        &painter,
        (names.left() + 8.0 + depth as f32 * 12.0, names.center().y),
        names.right() - ARROW_ROOM,
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
    if name_cut {
        name_resp.on_hover_text(&name);
    }
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
    let key_painter = ui.painter_at(keys.intersect(keys_clip));
    let view = (frame_at(keys_clip.left()), frame_at(keys_clip.right()));
    draw_loops(&key_painter, &p, keys.center().y, view, x_of, &loops);
    let frames: Vec<Frame> = glyphs.iter().map(|g| g.0).collect();
    for (f, interp) in glyphs {
        let c = pos2(x_of(f as f32), keys.center().y);
        let fill = p.key_color(interp);
        // a picked key gets a purple ring, so its colour (the way it moves
        // on) still reads
        let picked = shell.editor.is_key_selected(&KeyRef::new(Some(id), None, f));
        key_shape(&key_painter, c, interp, fill, key_outline(&p, picked));
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

    // key area: click a key to pick it (Shift adds, Ctrl toggles), click
    // empty space to let go; drag a picked key to move all picked keys (Alt
    // copies), drag empty space to pick with a box; right-click for the menu
    let (key_resp, empty_click) = key_area(
        ui,
        shell,
        Id::new(("tl-keys", id)),
        KeyRow {
            rect: keys.intersect(keys_clip),
            layer: Some(id),
            channel: None,
            frames,
        },
        Some(id),
        x_of,
        frame_at,
        key_rows,
    );
    if empty_click {
        // Loop in the menu acts on the picked keys: on the loop bar, the
        // row's repeating properties; elsewhere on empty space, none
        let on_bar = key_resp
            .interact_pointer_pos()
            .is_some_and(|pos| on_loop_bar(&loops, pos.x, x_of));
        if on_bar {
            let channels = loop_channels(shell, id);
            select_keys_of(shell, Some(id), &channels);
        } else {
            shell.editor.clear_key_selection();
        }
    }
    if key_resp.double_clicked() {
        toggle_expanded(shell, id);
    }
    menus::context_menu(&key_resp, |ui| {
        frame_heading(ui, &p, "Frame", shell.editor.current_frame());
        key_clipboard_items(ui, shell);
        menus::item(ui, shell, Action::AddKey);
        menus::item(ui, shell, Action::DeleteKey);
        menus::interpolation_items(ui, shell);
        if !loop_channels(shell, id).is_empty() || shell.editor.has_selected_keys() {
            ui.separator();
            let mode = loops.first().map_or(LoopMode::Off, |s| s.mode);
            menus::loop_menu(ui, shell, mode);
        }
        menus::switch_items(ui, shell);
        ui.separator();
        menus::item(ui, shell, Action::PrevKey);
        menus::item(ui, shell, Action::NextKey);
    });
}

/// Copy, Cut, Paste and Delete for the picked keys, at the top of a key menu.
/// They act on the timeline whichever part of the window was clicked last.
fn key_clipboard_items(ui: &mut egui::Ui, shell: &mut Shell) {
    shell.editor.set_focus(Focus::Timeline);
    key_item(ui, shell, Action::Copy, "Copy Keyframes");
    key_item(ui, shell, Action::Cut, "Cut Keyframes");
    key_item(ui, shell, Action::Paste, "Paste Keyframes");
    key_item(ui, shell, Action::Delete, "Delete Keyframes");
    ui.separator();
}

/// [`menus::item`] with a label of its own: the action is the general Copy,
/// Cut or Paste, and in this menu it is about keyframes.
fn key_item(ui: &mut egui::Ui, shell: &mut Shell, action: Action, label: &str) {
    let button = egui::Button::new(label)
        .shortcut_text(shell.keymap.shortcut_text(action))
        .gap(12.0);
    if ui.add_enabled(shell.editor.can_run(action), button).clicked() {
        shell.pending.push(action);
        ui.close();
    }
}

/// How far from the right edge of the names column the name must stop, so
/// it ends before the expand arrow.
const ARROW_ROOM: f32 = 22.0;

/// A menu's grey heading naming the frame, with the number in the
/// monospace font like every frame number here.
fn frame_heading(ui: &mut egui::Ui, p: &Palette, what: &str, frame: Frame) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(what).color(p.text_muted));
        ui.label(
            RichText::new(frame.to_string())
                .font(theme::mono(12.0))
                .color(p.text_muted),
        );
    });
}

/// A property that repeats after its last key: where the keys sit and how
/// they repeat, for the bar and ghost keys the timeline draws after them.
struct LoopSpan {
    mode: LoopMode,
    first: Frame,
    last: Frame,
    /// Every key of the property, with how it moves on.
    keys: Vec<(Frame, Interpolation)>,
}

impl LoopSpan {
    fn new(mode: LoopMode, (first, last): (Frame, Frame), keys: &dyn KeyList) -> Self {
        let mut frames = Vec::new();
        keys.frames_into(&mut frames);
        frames.sort_unstable();
        Self {
            mode,
            first,
            last,
            keys: frames
                .into_iter()
                .filter_map(|f| Some((f, keys.interp_at(f)?)))
                .collect(),
        }
    }
}

/// The properties of a layer that can repeat, without allocating: a point,
/// colour, exposure or switch key list cannot.
fn loop_channel_ids(layer: &Layer) -> impl Iterator<Item = ChannelId> + '_ {
    let bones = match &layer.kind {
        LayerKind::Bone { skeleton, .. } => skeleton.bones.len(),
        _ => 0,
    };
    [
        ChannelId::Position,
        ChannelId::Rotation,
        ChannelId::Scale,
        ChannelId::Opacity,
        ChannelId::Order,
    ]
    .into_iter()
    .chain((0..bones).map(ChannelId::Bone))
}

/// Whether a property can repeat after its last key.
fn loopable(ch: ChannelId) -> bool {
    !matches!(
        ch,
        ChannelId::Shape(_)
            | ChannelId::FillColor(_)
            | ChannelId::StrokeColor(_)
            | ChannelId::Trim(_)
            | ChannelId::Switch
    )
}

/// How one property of a layer repeats, if it does.
fn channel_loop_span(layer: &Layer, ch: ChannelId) -> Option<LoopSpan> {
    let span = layer.loop_span(ch)?;
    Some(LoopSpan::new(
        layer.channel_loop(ch),
        span,
        layer.channel_keys(ch)?,
    ))
}

/// The loops of a layer's properties (usually none).
fn layer_loop_spans(layer: &Layer) -> Vec<LoopSpan> {
    loop_channel_ids(layer)
        .filter_map(|ch| channel_loop_span(layer, ch))
        .collect()
}

fn camera_loop_spans(camera: &Camera, channels: &[ChannelId]) -> Vec<LoopSpan> {
    channels
        .iter()
        .filter_map(|&ch| {
            let span = camera.loop_span(ch)?;
            Some(LoopSpan::new(camera.channel_loop(ch), span, camera.keys(ch)?))
        })
        .collect()
}

/// The properties of layer `id` that could repeat: the ones with at least
/// two keys on different frames.
fn loop_channels(shell: &Shell, id: LayerId) -> Vec<ChannelId> {
    let Some(layer) = shell.editor.doc.find(id) else {
        return Vec::new();
    };
    loop_channel_ids(layer)
        .filter(|&ch| {
            let mut frames = Vec::new();
            if let Some(keys) = layer.channel_keys(ch) {
                keys.frames_into(&mut frames);
            }
            frames.sort_unstable();
            frames.dedup();
            frames.len() >= 2
        })
        .collect()
}

/// Choose all the keys of these properties (of layer `layer`, or of the
/// camera when it is `None`), for a command that acts on chosen keys.
fn select_keys_of(shell: &mut Shell, layer: Option<LayerId>, channels: &[ChannelId]) {
    let mut keys = Vec::new();
    for &ch in channels {
        let list = match layer {
            Some(id) => shell.editor.doc.find(id).and_then(|l| l.channel_keys(ch)),
            None => shell.editor.doc.camera.keys(ch),
        };
        let mut frames = Vec::new();
        if let Some(list) = list {
            list.frames_into(&mut frames);
        }
        keys.extend(frames.into_iter().map(|frame| KeyRef {
            layer,
            channel: Some(ch),
            frame,
        }));
    }
    for (i, key) in keys.into_iter().enumerate() {
        let how = if i == 0 {
            KeySelect::Replace
        } else {
            KeySelect::Add
        };
        shell.editor.select_key(key, how);
    }
}

/// The frames the repeats of a loop put keys on, past its last key and
/// between `from` and `until`: every key except the last, shifted by whole
/// cycles (the first key's repeat is where the last key already is).
fn ghost_frames(
    keys: &[(Frame, Interpolation)],
    first: Frame,
    last: Frame,
    from: Frame,
    until: Frame,
) -> Vec<(Frame, Interpolation)> {
    let len = i64::from(last) - i64::from(first);
    let mut out = Vec::new();
    if len <= 0 || until <= last {
        return out;
    }
    // skip the cycles that end before the view starts
    let mut cycle = ((i64::from(from) - i64::from(last)) / len).max(1);
    while i64::from(first) + cycle * len <= i64::from(until) {
        for &(f, interp) in keys.iter().filter(|(f, _)| *f < last) {
            let g = i64::from(f) + cycle * len;
            if g > i64::from(last) && g >= i64::from(from) && g <= i64::from(until) {
                out.push((g as Frame, interp));
            }
        }
        cycle += 1;
    }
    out
}

/// The loops of a row: from each one's last key a thin tinted bar runs to
/// the end of the view, with faint copies of the keys where they repeat, and
/// a small loop mark at the start of the first (with a "+" when it adds up).
fn draw_loops(
    painter: &egui::Painter,
    p: &Palette,
    y: f32,
    view: (Frame, Frame),
    x_of: &impl Fn(f32) -> f32,
    spans: &[LoopSpan],
) {
    let Some(earliest) = spans.iter().min_by_key(|s| s.last) else {
        return;
    };
    for s in spans {
        let bar = Rect::from_min_max(
            pos2(x_of(s.last as f32), y - 2.0),
            pos2(painter.clip_rect().right(), y + 2.0),
        );
        painter.rect_filled(bar, 2.0, p.keyframe.gamma_multiply(0.22));
        for (f, interp) in ghost_frames(&s.keys, s.first, s.last, view.0, view.1) {
            let fill = p.key_color(Some(interp)).gamma_multiply(0.35);
            key_shape(painter, pos2(x_of(f as f32), y), Some(interp), fill, Stroke::NONE);
        }
    }
    let relative = spans.iter().any(|s| s.mode == LoopMode::Relative);
    loop_mark(
        painter,
        pos2(x_of(earliest.last as f32) + 12.0, y),
        p.keyframe,
        relative,
    );
}

/// A small circular arrow, drawn with lines, and a "+" beside it when `plus`.
fn loop_mark(painter: &egui::Painter, c: egui::Pos2, color: egui::Color32, plus: bool) {
    let r = 3.5;
    let start = -std::f32::consts::FRAC_PI_4;
    let end = start + std::f32::consts::PI * 1.5;
    let at = |a: f32| c + vec2(a.cos(), a.sin()) * r;
    let arc: Vec<egui::Pos2> = (0..=12)
        .map(|i| at(start + (end - start) * i as f32 / 12.0))
        .collect();
    let stroke = Stroke::new(1.25, color);
    painter.add(Shape::line(arc, stroke));
    // the head at the end of the arc, pointing the way it turns on
    let tip = at(end);
    let along = vec2(-end.sin(), end.cos());
    let across = vec2(end.cos(), end.sin());
    painter.line_segment([tip, tip - along * 3.0 + across * 2.0], stroke);
    painter.line_segment([tip, tip - along * 3.0 - across * 2.0], stroke);
    if plus {
        let b = c + vec2(8.0, -2.0);
        painter.line_segment([b - vec2(2.0, 0.0), b + vec2(2.0, 0.0)], stroke);
        painter.line_segment([b - vec2(0.0, 2.0), b + vec2(0.0, 2.0)], stroke);
    }
}

/// A key's mark: round for smooth and for the eased and custom curves (all of
/// them curved motion), a diamond for linear (and for keys that differ), square
/// for step.
pub fn key_shape(
    painter: &egui::Painter,
    c: egui::Pos2,
    interp: Option<Interpolation>,
    fill: egui::Color32,
    outline: Stroke,
) {
    match interp {
        Some(Interpolation::Smooth | Interpolation::Ease(_) | Interpolation::Bezier(_)) => {
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
    key_rows: &mut Vec<KeyRow>,
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
    let loops = camera_loop_spans(camera, &ChannelId::CAMERA);
    let open = shell.ui.timeline_expanded.contains(&CAMERA_ROW);

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let names = Rect::from_min_max(
        rect.min,
        pos2(rect.left() + shell.ui.timeline_names_width, rect.bottom()),
    );
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
    let name_cut = name_label(
        &painter,
        (names.left() + 8.0, names.center().y),
        names.right() - ARROW_ROOM,
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
    let view = (frame_at(keys_clip.left()), frame_at(keys_clip.right()));
    draw_loops(&key_painter, &p, keys.center().y, view, x_of, &loops);
    for (f, interp) in &glyphs {
        let c = pos2(x_of(*f as f32), keys.center().y);
        let picked = shell.editor.is_key_selected(&KeyRef::new(None, None, *f));
        key_shape(
            &key_painter,
            c,
            *interp,
            p.key_color(*interp),
            key_outline(&p, picked),
        );
    }

    let name_resp = ui.interact(names, Id::new("tl-camera-name"), Sense::click());
    // choosing the camera shows its settings: with no layer selected, the
    // Properties panel is the project's, camera included
    if name_resp.clicked() {
        shell.editor.selection.clear();
        shell.ui.focus_properties = true;
    }
    let mut toggle = name_resp.double_clicked();
    if name_cut {
        name_resp.on_hover_text("Camera");
    }
    let arrow_resp = ui.interact(arrow.expand(4.0), Id::new("tl-camera-arrow"), Sense::click());
    toggle |= arrow_resp
        .on_hover_text("Show the camera's Position, Zoom and Roll, to key each on its own (or double-click)")
        .clicked();
    // the camera's keys are picked, moved and boxed like a layer's
    let (key_resp, empty_click) = key_area(
        ui,
        shell,
        Id::new("tl-camera-keys"),
        KeyRow {
            rect: keys.intersect(keys_clip),
            layer: None,
            channel: None,
            frames: glyphs.iter().map(|g| g.0).collect(),
        },
        None,
        x_of,
        frame_at,
        key_rows,
    );
    if empty_click {
        // Loop in the menu acts on the picked keys: on the loop bar, the
        // camera's repeating properties; elsewhere on empty space, none
        let on_bar = key_resp
            .interact_pointer_pos()
            .is_some_and(|pos| on_loop_bar(&loops, pos.x, x_of));
        if on_bar {
            select_keys_of(shell, None, &ChannelId::CAMERA);
        } else {
            shell.editor.clear_key_selection();
        }
    }
    toggle |= key_resp.double_clicked();
    menus::context_menu(&key_resp, |ui| {
        frame_heading(ui, &p, "Camera, frame", now);
        key_clipboard_items(ui, shell);
        if shell.editor.has_selected_keys() {
            menus::item(ui, shell, Action::DeleteKey);
        }
        for ch in ChannelId::CAMERA {
            menus::item(ui, shell, Action::AddChannelKey(ch));
        }
        ui.separator();
        let mode = loops.first().map_or(LoopMode::Off, |s| s.mode);
        menus::loop_menu(ui, shell, mode);
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
    key_rows: &mut Vec<KeyRow>,
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
    // how this property repeats after its last key, if it does
    let loops: Vec<LoopSpan> = if camera {
        camera_loop_spans(&shell.editor.doc.camera, &[ch])
    } else {
        layer.and_then(|l| channel_loop_span(l, ch)).into_iter().collect()
    };
    let p = shell.palette;
    let selected = !camera && shell.editor.is_selected(id);

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let names = Rect::from_min_max(
        rect.min,
        pos2(rect.left() + shell.ui.timeline_names_width, rect.bottom()),
    );
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
    let name_cut = name_label(
        &painter,
        (names.left() + 8.0 + (depth + 1) as f32 * 12.0, names.center().y),
        names.right() - 8.0,
        &label,
        egui::FontId::proportional(11.5),
        p.text_muted,
    );

    let key_painter = ui.painter_at(keys.intersect(keys_clip));
    let view = (frame_at(keys_clip.left()), frame_at(keys_clip.right()));
    draw_loops(&key_painter, &p, keys.center().y, view, x_of, &loops);
    let owner = (!camera).then_some(id);
    for (f, interp) in &frames {
        let c = pos2(x_of(*f as f32), keys.center().y);
        let picked = shell.editor.is_key_selected(&KeyRef::new(owner, Some(ch), *f));
        key_shape(
            &key_painter,
            c,
            Some(*interp),
            p.key_color(Some(*interp)),
            key_outline(&p, picked),
        );
    }

    // click to select the layer and go to a frame; drag a key to retime it,
    // or drag elsewhere to scrub; right-click for this property's key menu
    let name_resp = ui.interact(names, Id::new(("tl-prop-name", id, ch)), Sense::click());
    if !camera && (name_resp.clicked() || name_resp.secondary_clicked()) {
        shell.editor.select(id, SelectMode::Replace);
    }
    if name_cut {
        name_resp.on_hover_text(&label);
    }
    let (key_resp, empty_click) = key_area(
        ui,
        shell,
        Id::new(("tl-prop-keys", id, ch)),
        KeyRow {
            rect: keys.intersect(keys_clip),
            layer: owner,
            channel: Some(ch),
            frames: frames.iter().map(|(f, _)| *f).collect(),
        },
        owner,
        x_of,
        frame_at,
        key_rows,
    );
    if empty_click {
        // Loop in the menu acts on the picked keys: this property's
        if loopable(ch) {
            select_keys_of(shell, owner, &[ch]);
        } else {
            shell.editor.clear_key_selection();
        }
    }
    menus::context_menu(&key_resp, |ui| {
        frame_heading(ui, &p, &format!("{label}, frame"), shell.editor.current_frame());
        key_clipboard_items(ui, shell);
        menus::item(ui, shell, Action::AddChannelKey(ch));
        if shell.editor.has_selected_keys() {
            menus::item(ui, shell, Action::DeleteKey);
        } else {
            menus::item(ui, shell, Action::DeleteChannelKey(ch));
        }
        for interp in Interpolation::ALL {
            menus::item(ui, shell, Action::SetChannelInterpolation(ch, interp));
        }
        if loopable(ch) {
            ui.separator();
            let mode = loops.first().map_or(LoopMode::Off, |s| s.mode);
            menus::loop_menu(ui, shell, mode);
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

const INTERP_ROW_WIDTH: f32 = 104.0;
const INTERP_ROW_HEIGHT: f32 = 22.0;

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
            // a fixed width, so the list does not change size as it opens
            ui.set_min_width(INTERP_ROW_WIDTH);
            ui.spacing_mut().item_spacing.y = 0.0;
            for interp in Interpolation::ALL {
                // each choice is one row across the whole list: the mark and
                // the label are ours, and a click anywhere on the row counts
                let row = ui.allocate_response(vec2(INTERP_ROW_WIDTH, INTERP_ROW_HEIGHT), Sense::click());
                let painter = ui.painter();
                if row.hovered() {
                    painter.rect_filled(row.rect, 2.0, p.accent.gamma_multiply(0.35));
                } else if interp == current {
                    painter.rect_filled(row.rect, 2.0, p.accent.gamma_multiply(0.18));
                }
                key_shape(
                    painter,
                    pos2(row.rect.left() + 16.0, row.rect.center().y),
                    Some(interp),
                    p.key_color(Some(interp)),
                    Stroke::new(1.0, p.bg),
                );
                painter.text(
                    pos2(row.rect.left() + 32.0, row.rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    interp.label(),
                    egui::FontId::proportional(13.0),
                    p.text,
                );
                if row.clicked() {
                    chosen = Some(interp);
                }
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
            (Icon::RestPose, Action::GoToRestPose),
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
        // frame numbers are monospace, so the fields do not jitter as they count
        ui.scope(|ui| {
            ui.style_mut().override_font_id = Some(theme::mono(12.0));
            if ui
                // from the rest pose on, and free to go past the last frame:
                // later frames can hold keys too
                .add(
                    egui::DragValue::new(&mut f)
                        .range(REST_FRAME..=Frame::MAX)
                        .speed(0.2),
                )
                .changed()
            {
                shell.editor.go_to(f);
            }
        });
        // the last frame of the animation, set right here
        ui.label(RichText::new("of").color(shell.palette.text_muted));
        let mut new_end = end;
        let r = ui
            .scope(|ui| {
                ui.style_mut().override_font_id = Some(theme::mono(12.0));
                ui.add(
                    egui::DragValue::new(&mut new_end)
                        .range(start..=100_000)
                        .speed(0.2),
                )
            })
            .inner
            .on_hover_text("Last frame of the animation. Frames after it stay on the timeline.");
        if r.changed() {
            begin_edit(shell, &r, "Change last frame");
            shell.editor.doc.end = new_end.max(start);
        }
        let fps = shell.editor.doc.fps;
        ui.label(
            RichText::new(timecode::clock(shell.editor.current_frame(), fps))
                .font(theme::mono(12.0))
                .color(shell.palette.text_muted),
        )
        .on_hover_text("Seconds and frames");
        ui.label(RichText::new(format!("{fps} fps")).color(shell.palette.text_muted));
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

    /// Every character is 10 points wide.
    fn ten_per_char(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    #[test]
    fn long_names_are_cut_with_an_ellipsis_and_short_ones_kept() {
        assert_eq!(ellipsize("Arm", 100.0, ten_per_char), "Arm");
        assert_eq!(ellipsize("abcdefghij", 100.0, ten_per_char), "abcdefghij");
        // four letters and the ellipsis fit in 55
        assert_eq!(ellipsize("abcdefghij", 55.0, ten_per_char), "abcd…");
        // never wider than the room, however little there is
        for room in [0.0, 5.0, 10.0, 25.0, 99.0] {
            let cut = ellipsize("abcdefghij", room, ten_per_char);
            assert!(cut.ends_with('…'), "{cut}");
            assert!(ten_per_char(&cut) <= room.max(10.0), "{cut} in {room}");
        }
        // a space before the cut is not left hanging
        assert_eq!(ellipsize("ab cdefgh", 45.0, ten_per_char), "ab…");
        assert_eq!(ellipsize("", 0.0, ten_per_char), "");
    }

    fn rows(depths: &[usize]) -> Vec<(LayerId, usize)> {
        depths
            .iter()
            .enumerate()
            .map(|(i, d)| (LayerId(i as u32), *d))
            .collect()
    }

    fn ids(rows: &[(LayerId, usize)]) -> Vec<u32> {
        rows.iter().map(|(id, _)| id.0).collect()
    }

    #[test]
    fn hiding_unanimated_layers_keeps_the_groups_around_the_kept_ones() {
        // 0 group { 1, 2 group { 3 } }, 4, 5 group { 6 }
        let all = rows(&[0, 1, 1, 2, 0, 0, 1]);
        // off: everything
        assert_eq!(
            ids(&filter_rows(all.clone(), false, |_| false)),
            [0, 1, 2, 3, 4, 5, 6]
        );
        // nothing kept: nothing listed
        assert!(filter_rows(all.clone(), true, |_| false).is_empty());
        // a layer deep inside keeps the groups it sits in, and only those
        let kept = filter_rows(all.clone(), true, |id| id.0 == 3);
        assert_eq!(ids(&kept), [0, 2, 3]);
        // a group's other children are not dragged along
        let kept = filter_rows(all.clone(), true, |id| id.0 == 1 || id.0 == 6);
        assert_eq!(ids(&kept), [0, 1, 5, 6]);
        // a kept group does not keep its children
        let kept = filter_rows(all, true, |id| id.0 == 0);
        assert_eq!(ids(&kept), [0]);
    }

    #[test]
    fn a_kept_child_after_a_sibling_group_still_finds_its_own_parent() {
        // 0 group { 1 group { 2* }, 3 group { 4* } }
        let all = rows(&[0, 1, 2, 1, 2]);
        let kept = filter_rows(all, true, |id| id.0 == 2 || id.0 == 4);
        assert_eq!(ids(&kept), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn the_repeats_of_a_loop_land_on_the_keys_shifted_by_whole_cycles() {
        use Interpolation::{Linear, Smooth};
        let keys = [(0, Smooth), (5, Linear), (10, Smooth)];
        let at = |from, until| {
            ghost_frames(&keys, 0, 10, from, until)
                .into_iter()
                .map(|(f, _)| f)
                .collect::<Vec<_>>()
        };
        // the first key's repeat is the last key, which is already there
        assert_eq!(at(0, 30), [15, 20, 25, 30]);
        assert_eq!(at(0, 10), Vec::<Frame>::new());
        // only what is in view
        assert_eq!(at(21, 36), [25, 30, 35]);
        // a repeat keeps how its key moves on
        let ghosts = ghost_frames(&keys, 0, 10, 0, 16);
        assert_eq!(ghosts, [(15, Linear)]);
        // a loop that is not one (all keys on one frame) draws nothing
        assert!(ghost_frames(&keys, 4, 4, 0, 100).is_empty());
    }

    #[test]
    fn the_names_column_keeps_to_its_limits() {
        // nothing stored yet: the default
        assert_eq!(resolve_names_width(0.0, 1000.0), DEFAULT_NAMES_WIDTH);
        assert_eq!(resolve_names_width(f32::NAN, 1000.0), DEFAULT_NAMES_WIDTH);
        assert_eq!(resolve_names_width(240.0, 1000.0), 240.0);
        // not narrower than 100, not wider than half the panel
        assert_eq!(resolve_names_width(30.0, 1000.0), MIN_NAMES_WIDTH);
        assert_eq!(resolve_names_width(900.0, 1000.0), 500.0);
        // a panel too small for both: the minimum wins
        assert_eq!(resolve_names_width(150.0, 120.0), MIN_NAMES_WIDTH);
    }

    #[test]
    fn seconds_are_named_only_as_often_as_they_fit() {
        // 12 px frames at 24 fps: a second is 288 px, name every one
        assert_eq!(second_label_every(12.0, 24.0), 1);
        // 2 px frames: 48 px a second, still every one
        assert_eq!(second_label_every(2.0, 24.0), 1);
        // zoomed out further it skips, to a round count
        assert_eq!(second_label_every(0.5, 24.0), 5);
        assert_eq!(frames_per_second(24.0), 24);
        assert_eq!(frames_per_second(0.0), 1);
    }

    #[test]
    fn only_plain_channels_loop() {
        for ch in [
            ChannelId::Position,
            ChannelId::Order,
            ChannelId::Bone(2),
            ChannelId::CameraZoom,
        ] {
            assert!(loopable(ch), "{ch:?}");
        }
        for ch in [
            ChannelId::Shape(0),
            ChannelId::FillColor(0),
            ChannelId::StrokeColor(1),
            ChannelId::Trim(0),
            ChannelId::Switch,
        ] {
            assert!(!loopable(ch), "{ch:?}");
        }
    }

    /// A view starting at the rest pose with `width` pixel frames, scrolled
    /// `scroll` frames, the frame area starting at x = 100.
    fn view_x(scroll: f32, width: f32) -> impl Fn(f32) -> f32 {
        move |f| 100.0 + (f - REST_FRAME as f32 - scroll) * width + width / 2.0
    }

    #[test]
    fn a_frame_is_found_from_x_and_back() {
        let x_of = view_x(2.0, 10.0);
        // frame 5 is centred at 100 + 30 + 5
        assert_eq!(x_of(5.0), 135.0);
        assert_eq!(frame_at_x(135.0, &x_of), 5);
        assert_eq!(frame_at_x(130.5, &x_of), 5);
        assert_eq!(frame_at_x(139.5, &x_of), 5);
        assert_eq!(frame_at_x(129.5, &x_of), 4);
        // never before the rest pose
        assert_eq!(frame_at_x(-500.0, &x_of), REST_FRAME);
        // between: a cell is a frame wide, its key in the middle
        assert_eq!(cell_at(135.0, &x_of), 5.5);
        assert_eq!(x_at_cell(5.5, &x_of), 135.0);
        assert!((x_at_cell(cell_at(171.0, &x_of), &x_of) - 171.0).abs() < 1e-3);
    }

    #[test]
    fn a_box_touches_the_frames_from_its_start_to_its_end() {
        assert_eq!(box_frames(3.2, 7.8), (3, 7));
        // dragged backwards
        assert_eq!(box_frames(7.8, 3.2), (3, 7));
        // a box inside one cell touches that frame
        assert_eq!(box_frames(4.1, 4.9), (4, 4));
        // not before the rest pose
        assert_eq!(box_frames(-3.0, 2.5), (REST_FRAME, 2));
    }

    fn key_row(layer: Option<u32>, channel: Option<ChannelId>, top: f32, frames: &[Frame]) -> KeyRow {
        KeyRow {
            rect: Rect::from_min_max(pos2(100.0, top), pos2(600.0, top + ROW_HEIGHT)),
            layer: layer.map(LayerId),
            channel,
            frames: frames.to_vec(),
        }
    }

    #[test]
    fn a_band_over_the_rows_touches_the_rows_it_overlaps() {
        let rows = [
            key_row(None, None, 0.0, &[1]),
            key_row(Some(1), None, 22.0, &[1]),
            key_row(Some(1), Some(ChannelId::Rotation), 44.0, &[1]),
            key_row(Some(2), None, 66.0, &[1]),
        ];
        let layers = |hit: Vec<(Option<LayerId>, Option<ChannelId>)>| -> Vec<Option<u32>> {
            hit.into_iter().map(|(l, _)| l.map(|l| l.0)).collect()
        };
        // from the middle of one row to the middle of the next
        assert_eq!(layers(rows_in_band(&rows, 30.0, 50.0)), [Some(1), Some(1)]);
        // either way round
        assert_eq!(layers(rows_in_band(&rows, 50.0, 30.0)), [Some(1), Some(1)]);
        // the channel row is named by its channel
        let hit = rows_in_band(&rows, 50.0, 55.0);
        assert_eq!(hit, [(Some(LayerId(1)), Some(ChannelId::Rotation))]);
        // a band that ends exactly on a row's top does not touch it
        assert_eq!(layers(rows_in_band(&rows, 0.0, 22.0)), [None]);
        // outside every row
        assert!(rows_in_band(&rows, 200.0, 300.0).is_empty());
        // a row scrolled out of view has no height left and is never touched
        let mut hidden = key_row(Some(3), None, 0.0, &[1]);
        hidden.rect = Rect::from_min_max(pos2(100.0, 40.0), pos2(600.0, 20.0));
        assert!(rows_in_band(&[hidden], 0.0, 100.0).is_empty());
    }

    #[test]
    fn the_key_under_the_pointer_is_the_nearest_within_reach() {
        let x_of = view_x(0.0, 12.0);
        // keys at 3 and 4 are 12 px apart
        let frames = [3, 4, 9];
        let x3 = x_of(3.0);
        assert_eq!(key_under(&frames, x3, &x_of, KEY_GRAB_PX), Some(3));
        assert_eq!(key_under(&frames, x3 + 4.0, &x_of, KEY_GRAB_PX), Some(3));
        // nearer the next key than this one, and past this one's reach
        assert_eq!(key_under(&frames, x3 + 7.0, &x_of, KEY_GRAB_PX), Some(4));
        // between two keys, reaching both: the nearer
        assert_eq!(key_under(&frames, x3 + 5.5, &x_of, 9.0), Some(3));
        assert_eq!(key_under(&frames, x3 + 6.5, &x_of, 9.0), Some(4));
        // between two keys but within reach of neither
        assert_eq!(key_under(&[3, 9], x_of(6.0), &x_of, KEY_GRAB_PX), None);
        assert_eq!(key_under(&[], x3, &x_of, KEY_GRAB_PX), None);
    }

    fn picked(layer: u32, channel: Option<ChannelId>, frame: Frame) -> KeyRef {
        KeyRef::new(Some(LayerId(layer)), channel, frame)
    }

    #[test]
    fn the_span_of_the_picked_keys_runs_from_the_first_to_the_last() {
        let mut keys = BTreeSet::new();
        assert_eq!(selection_span(&keys), None);
        keys.insert(picked(1, Some(ChannelId::Position), 12));
        assert_eq!(selection_span(&keys), Some((12, 12)));
        keys.insert(picked(2, None, 4));
        keys.insert(picked(1, Some(ChannelId::Rotation), 30));
        assert_eq!(selection_span(&keys), Some((4, 30)));
    }

    #[test]
    fn the_rows_with_picked_keys_set_the_height_of_the_grip() {
        let rows = [
            key_row(Some(1), None, 0.0, &[2, 8]),
            key_row(Some(1), Some(ChannelId::Rotation), 22.0, &[2]),
            key_row(Some(1), Some(ChannelId::Position), 44.0, &[8]),
            key_row(Some(2), None, 66.0, &[2]),
        ];
        let mut keys = BTreeSet::new();
        assert_eq!(selection_extent(&rows, &keys), None);
        // a rotation key: its own row, and the layer's summary row above it
        keys.insert(picked(1, Some(ChannelId::Rotation), 2));
        assert_eq!(selection_extent(&rows, &keys), Some((0.0, 44.0)));
        // a key of another layer adds that layer's rows
        keys.insert(picked(2, None, 2));
        assert_eq!(selection_extent(&rows, &keys), Some((0.0, 88.0)));
        // only another layer's keys: only its rows
        let mut other = BTreeSet::new();
        other.insert(picked(2, None, 2));
        assert_eq!(selection_extent(&rows, &other), Some((66.0, 88.0)));
        // the camera's keys are in no layer's row
        let mut camera = BTreeSet::new();
        camera.insert(KeyRef::new(None, Some(ChannelId::CameraZoom), 2));
        assert_eq!(selection_extent(&rows, &camera), None);
    }

    #[test]
    fn a_grip_sits_just_outside_each_end_of_two_or_more_picked_frames() {
        let x_of = view_x(0.0, 12.0);
        let span = (4, 10);
        let extent = (22.0, 66.0);
        let (left, right) = (x_of(4.0), x_of(10.0));
        let at = |x: f32, y: f32| grip_hit(span, extent, pos2(x, y), &x_of);
        assert_eq!(at(left - 10.0, 40.0), Some(GripEnd::Left));
        assert_eq!(at(right + 10.0, 40.0), Some(GripEnd::Right));
        // on the end key itself is a move, not a scale
        assert_eq!(at(left, 40.0), None);
        assert_eq!(at(right + 3.0, 40.0), None);
        // too far out
        assert_eq!(at(left - GRIP_FAR - 1.0, 40.0), None);
        assert_eq!(at(right + GRIP_FAR + 1.0, 40.0), None);
        // level with the rows that hold the keys
        assert_eq!(at(right + 10.0, 10.0), None);
        assert_eq!(at(right + 10.0, 80.0), None);
        // inside the span there is nothing to grab
        assert_eq!(at((left + right) / 2.0, 40.0), None);
        // keys on one frame have no ends
        assert_eq!(grip_hit((4, 4), extent, pos2(left - 10.0, 40.0), &x_of), None);
    }

    #[test]
    fn the_loop_bar_starts_a_key_grab_past_the_last_key() {
        let x_of = view_x(0.0, 12.0);
        let span = LoopSpan {
            mode: LoopMode::Absolute,
            first: 0,
            last: 10,
            keys: Vec::new(),
        };
        assert!(on_loop_bar(std::slice::from_ref(&span), x_of(10.0) + 20.0, &x_of));
        assert!(!on_loop_bar(std::slice::from_ref(&span), x_of(10.0), &x_of));
        assert!(!on_loop_bar(std::slice::from_ref(&span), x_of(3.0), &x_of));
        assert!(!on_loop_bar(&[], x_of(30.0), &x_of));
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
