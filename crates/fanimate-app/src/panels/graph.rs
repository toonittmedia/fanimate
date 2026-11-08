//! The graph editor: value against frame for one property of the selected
//! layer, one curve per number it holds (the x and y of a position). Drag a
//! key to move it in time and in value, drag the handles of a key that eases
//! along a custom curve to reshape the motion, and right-click for the
//! interpolation choices. A drag is one undo step (`Editor::checkpoint` once,
//! then `Editor::change_key` and `change_key_curve` per move).

use eframe::egui::{self, Align2, Color32, FontId, Id, Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};
use fanimate_core::easing::Bezier;
use fanimate_core::graph::{CurveKey, component_count, display_scale};
use fanimate_core::{ChannelId, Frame, Interpolation, LayerId};
use fanimate_editor::{Action, KeyRef, KeySelect};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus;

/// The frames and values a graph shows: from frame `f0` to `f1` along the
/// bottom, value `v0` to `v1` up the side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphView {
    pub f0: f32,
    pub f1: f32,
    pub v0: f32,
    pub v1: f32,
}

/// The least and most a graph may show along either axis, so zooming cannot
/// collapse the plot to nothing or run it off to infinity.
const MIN_FRAMES: f32 = 3.0;
const MAX_FRAMES: f32 = 100_000.0;
const MIN_VALUES: f32 = 0.5;
const MAX_VALUES: f32 = 1.0e7;

/// Zoom `lo..hi` by `factor` about the point `at` of the way along it (0 to 1),
/// keeping that point where it is.
fn zoom_span(lo: f32, hi: f32, factor: f32, at: f32, least: f32, most: f32) -> (f32, f32) {
    if !(factor.is_finite() && factor > 0.0) {
        return (lo, hi);
    }
    let span = hi - lo;
    let new = (span / factor).clamp(least, most);
    let anchor = lo + span * at;
    let lo = anchor - new * at;
    (lo, lo + new)
}

impl GraphView {
    /// Zoom along the frames about `at` (0 at the left edge, 1 at the right).
    pub fn zoom_frames(&mut self, factor: f32, at: f32) {
        (self.f0, self.f1) = zoom_span(self.f0, self.f1, factor, at, MIN_FRAMES, MAX_FRAMES);
    }

    /// Zoom along the values about `at` (0 at the bottom edge, 1 at the top).
    pub fn zoom_values(&mut self, factor: f32, at: f32) {
        (self.v0, self.v1) = zoom_span(self.v0, self.v1, factor, at, MIN_VALUES, MAX_VALUES);
    }

    /// Move the view by `by` frames.
    pub fn pan_frames(&mut self, by: f32) {
        self.f0 += by;
        self.f1 += by;
    }

    /// Move the view by `by` in value.
    pub fn pan_values(&mut self, by: f32) {
        self.v0 += by;
        self.v1 += by;
    }

    /// Keep the view inside `bounds`: no wider or taller than they are, and
    /// slid back in when it has gone past an edge.
    fn clamp_to(&mut self, bounds: &GraphView) {
        let frames = (self.f1 - self.f0).min(bounds.f1 - bounds.f0);
        self.f0 = self.f0.clamp(bounds.f0, bounds.f1 - frames);
        self.f1 = self.f0 + frames;
        let values = (self.v1 - self.v0).min(bounds.v1 - bounds.v0);
        self.v0 = self.v0.clamp(bounds.v0, bounds.v1 - values);
        self.v1 = self.v0 + values;
    }

    fn is_sound(&self) -> bool {
        [self.f0, self.f1, self.v0, self.v1].iter().all(|v| v.is_finite())
            && self.f1 > self.f0
            && self.v1 > self.v0
    }
}

/// Zoom the graph on show, in or out about its middle (the zoom buttons and shortcuts).
pub fn zoom_by(shell: &mut Shell, factor: f32) {
    if let Some((layer, ch, mut view)) = shell.ui.graph_shown {
        view.zoom_frames(factor, 0.5);
        view.zoom_values(factor, 0.5);
        shell.ui.graph_views.insert((layer, ch), view);
    }
}

/// Let the graph on show fit its curves again.
pub fn fit(shell: &mut Shell) {
    if let Some((layer, ch, _)) = shell.ui.graph_shown {
        shell.ui.graph_views.remove(&(layer, ch));
    }
}

/// How close a click must be to a key or handle to take hold of it.
const GRAB: f32 = 9.0;
const LEFT_MARGIN: f32 = 52.0;

/// What a drag in the plot has hold of.
#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Curve `component`'s key, now on `frame`.
    Key { component: usize, frame: Frame },
    /// One of the two handles (0 leaves the key, 1 arrives at the next) of the
    /// key on `frame`.
    Handle { which: usize, frame: Frame },
    /// Empty space: the playhead follows the pointer, as on the timeline's ruler.
    Scrub,
}

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    let Some(layer) = shell.editor.primary() else {
        ui.add_space(8.0);
        ui.label(RichText::new("Select a layer to see the curves of its keys.").color(p.text_muted));
        return;
    };
    let keyed: Vec<ChannelId> = shell
        .editor
        .graph_channels(layer)
        .into_iter()
        .filter(|(ch, n)| (0..*n).any(|c| !shell.editor.curve_keys(layer, *ch, c).is_empty()))
        .map(|(ch, _)| ch)
        .collect();
    if keyed.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new("This layer has no keys yet. Move, turn or fade it on a later frame to key it.")
                .color(p.text_muted),
        );
        return;
    }
    let property_id = Id::new(("graph-property", layer));
    let mut active: ChannelId = ui
        .ctx()
        .data(|d| d.get_temp(property_id))
        .filter(|c| keyed.contains(c))
        .unwrap_or(keyed[0]);
    ui.horizontal(|ui| {
        for ch in &keyed {
            let label = shell
                .editor
                .doc
                .find(layer)
                .map_or_else(String::new, |l| l.channel_label(*ch));
            if ui.selectable_label(active == *ch, label).clicked() {
                active = *ch;
            }
        }
        // zoom, at the right: shortcuts and the scroll wheel work too
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let tip = "Scroll up and down; Shift+scroll sideways; Ctrl+scroll zooms the frames and Ctrl+Shift+scroll the values";
            if icons::button(ui, Icon::ZoomFit, 24.0, false, false)
                .on_hover_text(format!("Fit the curves. {tip}."))
                .clicked()
            {
                fit(shell);
            }
            if icons::button(ui, Icon::ZoomIn, 24.0, false, false)
                .on_hover_text(format!("Zoom in on the graph. {tip}."))
                .clicked()
            {
                zoom_by(shell, 1.25);
            }
            if icons::button(ui, Icon::ZoomOut, 24.0, false, false)
                .on_hover_text(format!("Zoom out of the graph. {tip}."))
                .clicked()
            {
                zoom_by(shell, 0.8);
            }
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(property_id, active));
    plot(ui, shell, layer, active);
}

fn plot(ui: &mut egui::Ui, shell: &mut Shell, layer: LayerId, ch: ChannelId) {
    let p = shell.palette;
    let scale = display_scale(ch);
    let comps = component_count(ch);
    let curves: Vec<Vec<CurveKey>> = (0..comps)
        .map(|c| shell.editor.curve_keys(layer, ch, c))
        .collect();
    let (start, end) = (shell.editor.doc.start as f32, shell.editor.doc.end as f32);

    // the frames and values the plot shows: the document's range and the
    // keys; the values stay put during a drag so the key under the pointer
    // does not slide away as the range refits
    let range_id = Id::new(("graph-range", layer));
    let dragging_id = Id::new(("graph-drag", layer));
    let drag: Option<Drag> = ui.ctx().data(|d| d.get_temp(dragging_id));
    let (mut f0, mut f1) = (start.min(0.0).min(start), end);
    for k in curves.iter().flatten() {
        f0 = f0.min(k.frame as f32);
        f1 = f1.max(k.frame as f32);
    }
    let (f0, f1) = (f0 - 1.0, f1 + 1.0);
    let manual = shell
        .ui
        .graph_views
        .get(&(layer, ch))
        .copied()
        .filter(GraphView::is_sound);
    // fitted to the curves, not only the keys: an overshoot or a bounce goes past them
    let (lo, hi) = {
        let mut lo_hi = curves
            .iter()
            .flatten()
            .map(|k| k.value * scale)
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        for c in 0..comps {
            for s in 0..=160 {
                let f = f0 + (f1 - f0) * s as f32 / 160.0;
                if let Some(v) = shell.editor.curve_value(layer, ch, c, f).map(|v| v * scale)
                    && v.is_finite()
                {
                    lo_hi = (lo_hi.0.min(v), lo_hi.1.max(v));
                }
            }
        }
        lo_hi
    };
    let span = (hi - lo).max(1.0);
    let fitted = (lo - span * 0.1, hi + span * 0.1);
    let (v0, v1) = match (drag, ui.ctx().data(|d| d.get_temp::<(f32, f32)>(range_id))) {
        (Some(_), Some(held)) if manual.is_none() => held,
        _ => fitted,
    };
    // how far the view may go: the frames and values there are, and a little more
    let pad = (f1 - f0) * 0.05;
    let room = (fitted.1 - fitted.0) * 0.3;
    let bounds = GraphView {
        f0: f0 - pad,
        f1: f1 + pad,
        v0: fitted.0 - room,
        v1: fitted.1 + room,
    };
    let mut view = manual.unwrap_or(GraphView { f0, f1, v0, v1 });
    if manual.is_some() {
        view.clamp_to(&bounds);
        if Some(view) != manual {
            shell.ui.graph_views.insert((layer, ch), view);
        }
    }

    let (outer, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let rect = Rect::from_min_max(
        pos2(outer.left() + LEFT_MARGIN, outer.top() + 6.0),
        pos2(outer.right() - 8.0, outer.bottom() - 6.0),
    );
    if rect.width() < 40.0 || rect.height() < 30.0 {
        return;
    }
    // scrolling and zooming: the wheel moves the view up and down, with Shift
    // sideways, and Ctrl zooms the frames (with Shift the values) about the pointer
    if ui.rect_contains_pointer(rect) {
        shell.ui.graph_hovered = true;
        let (zoom, scroll, mods, pointer) = ui.input(|i| {
            (
                i.zoom_delta(),
                i.smooth_scroll_delta,
                i.modifiers,
                i.pointer.hover_pos(),
            )
        });
        let at = pointer.unwrap_or(rect.center());
        let across = ((at.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let up = ((rect.bottom() - at.y) / rect.height()).clamp(0.0, 1.0);
        let before = view;
        if zoom != 1.0 {
            if mods.shift {
                view.zoom_values(zoom, up);
            } else {
                view.zoom_frames(zoom, across);
            }
        } else if scroll.x != 0.0 || (mods.shift && scroll.y != 0.0) {
            let dx = if scroll.x != 0.0 { scroll.x } else { scroll.y };
            view.pan_frames(-dx / rect.width() * (view.f1 - view.f0));
        } else if scroll.y != 0.0 {
            view.pan_values(scroll.y / rect.height() * (view.v1 - view.v0));
        }
        view.clamp_to(&bounds);
        if view != before && view.is_sound() {
            shell.ui.graph_views.insert((layer, ch), view);
        } else {
            view = before;
        }
    }
    shell.ui.graph_shown = Some((layer, ch, view));
    let GraphView { f0, f1, v0, v1 } = view;
    let to_screen = |f: f32, v: f32| {
        pos2(
            rect.left() + (f - f0) / (f1 - f0) * rect.width(),
            rect.bottom() - (v - v0) / (v1 - v0) * rect.height(),
        )
    };
    let from_screen = |pos: Pos2| {
        (
            f0 + (pos.x - rect.left()) / rect.width() * (f1 - f0),
            v0 + (rect.bottom() - pos.y) / rect.height() * (v1 - v0),
        )
    };
    // everything in the plot is clipped to it: a key that has been scrolled off
    // must not be drawn in the margin
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, p.panel);
    draw_grid(
        &painter,
        &ui.painter_at(outer),
        rect,
        (f0, f1),
        (v0, v1),
        p.border,
        p.text_muted,
    );
    // the playhead
    let head = to_screen(shell.editor.frame, v0).x;
    if (rect.left()..=rect.right()).contains(&head) {
        painter.vline(head, rect.y_range(), Stroke::new(1.0, p.keyframe));
    }

    let colors = [p.accent, p.bone];
    // the curves, sampled along the plot
    let steps = (rect.width() / 2.0).ceil().max(2.0) as usize;
    for (c, _) in curves.iter().enumerate() {
        let mut line = Vec::with_capacity(steps + 1);
        for s in 0..=steps {
            let f = f0 + (f1 - f0) * s as f32 / steps as f32;
            if let Some(v) = shell.editor.curve_value(layer, ch, c, f) {
                line.push(to_screen(f, (v * scale).clamp(v0 - (v1 - v0), v1 + (v1 - v0))));
            }
        }
        painter.add(egui::Shape::line(line, Stroke::new(1.5, colors[c.min(1)])));
    }

    // the picked key of this property, with its handles when it is on a custom curve
    let picked: Option<Frame> = shell
        .editor
        .selected_keys()
        .iter()
        .find(|k| k.layer == Some(layer) && k.channel == Some(ch))
        .map(|k| k.frame);
    let handles = picked.and_then(|frame| handle_points(&curves, frame, scale));
    let handle_pos = |h: &Handles, which: usize| {
        let (f, v) = h.points[which];
        to_screen(f, v)
    };
    if let Some(h) = &handles {
        for which in 0..2 {
            let anchor = if which == 0 { h.from } else { h.to };
            let a = to_screen(anchor.0, anchor.1);
            let b = handle_pos(h, which);
            painter.line_segment([a, b], Stroke::new(1.0, p.text_muted));
            painter.circle_filled(b, 4.0, p.keyframe);
        }
    }
    for (c, keys) in curves.iter().enumerate() {
        for k in keys {
            let at = to_screen(k.frame as f32, k.value * scale);
            let is_picked = picked == Some(k.frame);
            let r = if is_picked { 5.5 } else { 4.0 };
            let fill = if is_picked { p.keyframe } else { colors[c.min(1)] };
            painter.rect_filled(Rect::from_center_size(at, vec2(r * 2.0, r * 2.0)), 1.5, fill);
        }
    }
    // taking hold, dragging, letting go
    let pointer = response.interact_pointer_pos();
    // a drag is reported once the pointer has moved off, so take hold of what
    // was under it when it was pressed
    if response.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin()).or(pointer)
    {
        let grabbed = handles
            .as_ref()
            .and_then(|h| {
                (0..2)
                    .find(|w| handle_pos(h, *w).distance(pos) <= GRAB)
                    .map(|which| Drag::Handle {
                        which,
                        frame: h.frame,
                    })
            })
            .or_else(|| nearest_key(&curves, scale, pos, &to_screen));
        if grabbed.is_none() {
            // nothing to take hold of: scrub, with no undo step, and let go of the picked key
            ui.ctx().data_mut(|d| d.insert_temp(dragging_id, Drag::Scrub));
            shell.editor.clear_key_selection();
        }
        if let Some(drag) = grabbed {
            shell.editor.checkpoint("Edit key");
            ui.ctx().data_mut(|d| {
                d.insert_temp(dragging_id, drag);
                d.insert_temp(range_id, (v0, v1));
            });
            if let Drag::Key { frame, .. } = drag {
                shell
                    .editor
                    .select_key(KeyRef::new(Some(layer), Some(ch), frame), KeySelect::Replace);
            }
        }
    }
    if response.dragged()
        && let (Some(drag), Some(pos)) = (drag, pointer)
    {
        let (f, v) = from_screen(pos);
        match drag {
            Drag::Scrub => shell
                .editor
                .go_to((f.round() as Frame).max(fanimate_core::REST_FRAME)),
            Drag::Key { component, frame } => {
                if let Ok(to) =
                    shell
                        .editor
                        .change_key(layer, ch, component, frame, f.round() as Frame, v / scale)
                {
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(dragging_id, Drag::Key { component, frame: to }));
                }
            }
            Drag::Handle { which, frame } => {
                if let Some(h) = handle_points(&curves, frame, scale) {
                    let (dt, dv) = (h.to.0 - h.from.0, h.to.1 - h.from.1);
                    if dt > 0.0 && dv != 0.0 {
                        let x = ((f - h.from.0) / dt).clamp(0.0, 1.0);
                        let y = (v - h.from.1) / dv;
                        let mut b = h.bezier.0.map(f32::from);
                        let at = which * 2;
                        b[at] = x * 1000.0;
                        b[at + 1] = y * 1000.0;
                        let curve = Interpolation::Bezier(Bezier::new(
                            b[0] / 1000.0,
                            b[1] / 1000.0,
                            b[2] / 1000.0,
                            b[3] / 1000.0,
                        ));
                        // only fails while playing, when the panel is disabled
                        let _ = shell.editor.change_key_curve(layer, ch, frame, curve);
                    }
                }
            }
        }
    }
    // a key, handle or the playhead taken to the edge of the plot pulls the view after it
    if response.dragged()
        && drag.is_some()
        && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
    {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let (px, py) = (
            edge_push(pos.x, rect.left(), rect.right()),
            edge_push(pos.y, rect.top(), rect.bottom()),
        );
        let mut moved = view;
        moved.pan_frames(px * 0.6 * (view.f1 - view.f0) * dt);
        if !matches!(drag, Some(Drag::Scrub)) {
            moved.pan_values(-py * 0.6 * (view.v1 - view.v0) * dt);
        }
        moved.clamp_to(&bounds);
        if moved != view && moved.is_sound() {
            shell.ui.graph_views.insert((layer, ch), moved);
            ui.ctx().request_repaint();
        }
    }
    if response.drag_stopped() {
        ui.ctx().data_mut(|d| {
            d.remove::<Drag>(dragging_id);
            d.remove::<(f32, f32)>(range_id);
        });
    }
    if response.clicked()
        && let Some(pos) = pointer
    {
        match nearest_key(&curves, scale, pos, &to_screen) {
            Some(Drag::Key { frame, .. }) => shell
                .editor
                .select_key(KeyRef::new(Some(layer), Some(ch), frame), KeySelect::Replace),
            _ => {
                // empty space: let go of the key and go to that frame
                shell.editor.clear_key_selection();
                let (f, _) = from_screen(pos);
                shell
                    .editor
                    .go_to((f.round() as Frame).max(fanimate_core::REST_FRAME));
            }
        }
    }
    if response.secondary_clicked()
        && let Some(pos) = pointer
        && let Some(Drag::Key { frame, .. }) = nearest_key(&curves, scale, pos, &to_screen)
    {
        // right-clicking an unpicked key picks it first, as everywhere
        if !shell
            .editor
            .is_key_selected(&KeyRef::new(Some(layer), Some(ch), frame))
        {
            shell
                .editor
                .select_key(KeyRef::new(Some(layer), Some(ch), frame), KeySelect::Replace);
        }
    }
    menus::context_menu(&response, |ui| {
        ui.menu_button("Interpolation", |ui| {
            ui.set_min_width(menus::MENU_MIN_WIDTH);
            for interp in Interpolation::ALL {
                menus::item(ui, shell, Action::SetInterpolation(interp));
            }
            // a custom curve starts as ease in and out; its handles then show on the key
            ui.separator();
            menus::item(
                ui,
                shell,
                Action::SetInterpolation(Interpolation::Bezier(Bezier([420, 0, 580, 1000]))),
            );
        });
        ui.separator();
        menus::item(ui, shell, Action::DeleteKey);
    });
    super::properties::end_edits(ui, shell);
}

/// The key nearest `pos` among every curve's, if it is within reach.
fn nearest_key(
    curves: &[Vec<CurveKey>],
    scale: f32,
    pos: Pos2,
    to_screen: &impl Fn(f32, f32) -> Pos2,
) -> Option<Drag> {
    curves
        .iter()
        .enumerate()
        .flat_map(|(c, keys)| {
            keys.iter()
                .map(move |k| (c, k.frame, to_screen(k.frame as f32, k.value * scale)))
        })
        .map(|(c, frame, at)| (at.distance(pos), c, frame))
        .filter(|(d, ..)| *d <= GRAB)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, component, frame)| Drag::Key { component, frame })
}

/// The two handles of the key on `frame` when it eases along a custom curve:
/// where they sit in (frame, shown value), and the ends of the segment. Taken
/// on the first curve that changes between this key and the next, since a
/// curve is shared by every number of the property.
struct Handles {
    frame: Frame,
    from: (f32, f32),
    to: (f32, f32),
    bezier: Bezier,
    points: [(f32, f32); 2],
}

fn handle_points(curves: &[Vec<CurveKey>], frame: Frame, scale: f32) -> Option<Handles> {
    for keys in curves {
        let i = keys.iter().position(|k| k.frame == frame)?;
        let (a, b) = (keys[i], *keys.get(i + 1)?);
        let Interpolation::Bezier(bezier) = a.interp else {
            return None;
        };
        let (dt, dv) = (b.frame as f32 - a.frame as f32, (b.value - a.value) * scale);
        if dv == 0.0 {
            continue;
        }
        let from = (a.frame as f32, a.value * scale);
        let to = (b.frame as f32, b.value * scale);
        let [x1, y1, x2, y2] = bezier.0.map(|v| f32::from(v) / 1000.0);
        return Some(Handles {
            frame,
            from,
            to,
            bezier,
            points: [
                (from.0 + x1 * dt, from.1 + y1 * dv),
                (from.0 + x2 * dt, from.1 + y2 * dv),
            ],
        });
    }
    None
}

/// Lines and numbers at round values and frames.
/// How hard the pointer pushes on an edge of the plot (negative towards `lo`),
/// 0 when it is well inside, growing as it nears the edge and to twice as much past it.
fn edge_push(pos: f32, lo: f32, hi: f32) -> f32 {
    const BAND: f32 = 24.0;
    if pos < lo + BAND {
        -((lo + BAND - pos) / BAND).min(2.0)
    } else if pos > hi - BAND {
        ((pos - (hi - BAND)) / BAND).min(2.0)
    } else {
        0.0
    }
}

/// A number for a label: at most two decimals and never "-0".
fn label(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == 0.0 { "0".to_owned() } else { format!("{r}") }
}

/// The grid and its numbers: the lines and frame numbers are `painter`'s
/// (clipped to the plot), the value numbers beside it `margin`'s.
fn draw_grid(
    painter: &egui::Painter,
    margin: &egui::Painter,
    rect: Rect,
    (f0, f1): (f32, f32),
    (v0, v1): (f32, f32),
    line: Color32,
    text: Color32,
) {
    let nice = |span: f32, target: f32| {
        let raw = (span / target).max(1e-6);
        let pow = 10f32.powf(raw.log10().floor());
        let m = raw / pow;
        pow * if m < 1.5 {
            1.0
        } else if m < 3.5 {
            2.0
        } else if m < 7.5 {
            5.0
        } else {
            10.0
        }
    };
    let step = nice(v1 - v0, 5.0);
    let mut v = (v0 / step).ceil() * step;
    while v <= v1 {
        let y = rect.bottom() - (v - v0) / (v1 - v0) * rect.height();
        painter.hline(rect.x_range(), y, Stroke::new(1.0, line.gamma_multiply(0.5)));
        margin.text(
            pos2(rect.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            label(v),
            FontId::monospace(10.0),
            text,
        );
        v += step;
    }
    let fstep = nice(f1 - f0, 8.0).max(1.0);
    let mut f = (f0 / fstep).ceil() * fstep;
    while f <= f1 {
        let x = rect.left() + (f - f0) / (f1 - f0) * rect.width();
        painter.vline(x, rect.y_range(), Stroke::new(1.0, line.gamma_multiply(0.35)));
        painter.text(
            pos2(x + 2.0, rect.bottom() - 2.0),
            Align2::LEFT_BOTTOM,
            format!("{}", f.round() as i32),
            FontId::monospace(10.0),
            text,
        );
        f += fstep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> GraphView {
        GraphView {
            f0: 0.0,
            f1: 100.0,
            v0: 0.0,
            v1: 200.0,
        }
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_where_it_is() {
        let mut v = view();
        // a quarter of the way along, twice as close
        v.zoom_frames(2.0, 0.25);
        assert_eq!((v.f0, v.f1), (12.5, 62.5));
        // the point that was a quarter along (frame 25) is still a quarter along
        assert_eq!(v.f0 + (v.f1 - v.f0) * 0.25, 25.0);
        let mut v = view();
        v.zoom_values(0.5, 0.0);
        assert_eq!((v.v0, v.v1), (0.0, 400.0), "zoomed out about the bottom edge");
    }

    #[test]
    fn zoom_is_limited_and_odd_factors_do_nothing() {
        let mut v = view();
        v.zoom_frames(1.0e9, 0.5);
        assert!(v.f1 - v.f0 >= MIN_FRAMES - 1e-3);
        v.zoom_frames(1.0e-9, 0.5);
        assert!(v.f1 - v.f0 <= MAX_FRAMES + 1e-3);
        let mut v = view();
        for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            v.zoom_frames(bad, 0.5);
            v.zoom_values(bad, 0.5);
        }
        assert_eq!(v, view());
        assert!(v.is_sound());
    }

    #[test]
    fn the_view_cannot_leave_its_bounds() {
        let bounds = view();
        // panned far past every edge, then zoomed out far past them
        let mut v = GraphView {
            f0: 450.0,
            f1: 500.0,
            v0: -900.0,
            v1: -700.0,
        };
        v.clamp_to(&bounds);
        // slid back in, keeping its size: the right-hand frames, the lowest values
        assert_eq!((v.f0, v.f1, v.v0, v.v1), (50.0, 100.0, 0.0, 200.0));
        let mut v = GraphView {
            f0: -1000.0,
            f1: 1000.0,
            v0: -1000.0,
            v1: 1000.0,
        };
        v.clamp_to(&bounds);
        assert_eq!(v, bounds, "no wider or taller than the bounds");
        // a view already inside is left alone
        let mut v = GraphView {
            f0: 20.0,
            f1: 60.0,
            v0: 40.0,
            v1: 120.0,
        };
        let before = v;
        v.clamp_to(&bounds);
        assert_eq!(v, before);
    }

    #[test]
    fn the_edges_push_harder_the_nearer_the_pointer_is() {
        assert_eq!(edge_push(50.0, 0.0, 100.0), 0.0);
        assert!(edge_push(95.0, 0.0, 100.0) > 0.0 && edge_push(5.0, 0.0, 100.0) < 0.0);
        assert!(edge_push(99.0, 0.0, 100.0) > edge_push(80.0, 0.0, 100.0));
        assert_eq!(edge_push(900.0, 0.0, 100.0), 2.0, "capped");
    }

    #[test]
    fn labels_never_say_minus_zero() {
        assert_eq!(label(-0.0), "0");
        assert_eq!(label(-0.001), "0");
        assert_eq!(label(2.5), "2.5");
        assert_eq!(label(-100.0), "-100");
    }

    #[test]
    fn panning_moves_both_ends_alike() {
        let mut v = view();
        v.pan_frames(-10.0);
        v.pan_values(50.0);
        assert_eq!((v.f0, v.f1, v.v0, v.v1), (-10.0, 90.0, 50.0, 250.0));
    }
}
