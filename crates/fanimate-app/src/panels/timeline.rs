//! The Timeline panel: transport controls, a scrubbable ruler, and a row of
//! keyframes per layer.

use eframe::egui::{self, Id, Rangef, Rect, RichText, Sense, Shape, Stroke, pos2, vec2};
use fanimate_core::{Frame, Interpolation, LayerId};
use fanimate_editor::{Action, SelectMode};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;

use super::properties::{begin_edit, end_edits};

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
    zoom_and_scroll(ui, shell, frames_rect, start, end);
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
    if (ruler_resp.clicked() || ruler_resp.dragged())
        && let Some(pos) = ruler_resp.interact_pointer_pos()
    {
        shell.editor.go_to(frame_at(pos.x));
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
            for (id, depth) in shell.editor.display_rows() {
                row(ui, shell, id, depth, keys_clip, &x_of, &frame_at);
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

/// Change the zoom to `width` pixels per frame, keeping `anchor` in place.
fn set_frame_width(shell: &mut Shell, width: f32, anchor: f32) {
    let (start, end) = (shell.editor.doc.start as f32, shell.editor.doc.end as f32);
    let ui = &mut shell.ui;
    let old = ui.timeline_frame_width.clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
    let new = width.clamp(MIN_FRAME_WIDTH, MAX_FRAME_WIDTH);
    ui.timeline_scroll = zoomed_scroll(anchor, start, ui.timeline_scroll, old, new).clamp(0.0, end - start);
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

fn zoom_and_scroll(ui: &egui::Ui, shell: &mut Shell, area: Rect, start: Frame, end: Frame) {
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
        let anchor = start as f32 + shell.ui.timeline_scroll + (pointer_x - area.left() - fw / 2.0) / fw;
        set_frame_width(shell, fw * zoom, anchor);
    } else if scroll.x != 0.0 || (mods.shift && scroll.y != 0.0) {
        let dx = if scroll.x != 0.0 { scroll.x } else { scroll.y };
        let max = (end - start) as f32;
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
    menus::context_menu(&name_resp, |ui| menus::layer_context(ui, shell));

    // keyframes, shaped by how they move on: smooth round, linear
    // diamond, step square
    let now = shell.editor.current_frame();
    let key_painter = ui.painter_at(keys.intersect(keys_clip));
    for (f, interp) in glyphs {
        let c = pos2(x_of(f as f32), keys.center().y);
        let fill = if f == now { p.accent_strong } else { p.keyframe };
        let outline = Stroke::new(1.0, p.bg);
        match interp {
            Some(Interpolation::Smooth) => {
                key_painter.circle(c, 4.5, fill, outline);
            }
            Some(Interpolation::Step) => {
                let r = Rect::from_center_size(c, vec2(8.0, 8.0));
                key_painter.rect(r, 1.0, fill, outline, egui::StrokeKind::Outside);
            }
            _ => {
                let s = 5.0;
                key_painter.add(Shape::convex_polygon(
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
        if let Some(pos) = key_resp.interact_pointer_pos() {
            shell.editor.go_to(frame_at(pos.x));
        }
        if !selected || key_resp.clicked() {
            shell.editor.select(id, SelectMode::Replace);
        }
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
            shell.editor.go_to(frame_at(pos.x));
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
            .add(egui::DragValue::new(&mut f).range(start..=end).speed(0.2))
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
            shell.editor.frame = shell.editor.frame.min(new_end as f32);
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
