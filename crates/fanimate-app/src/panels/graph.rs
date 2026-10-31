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
use crate::menus;

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
    ui.horizontal_wrapped(|ui| {
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
    let fitted = {
        let values = curves.iter().flatten().map(|k| k.value * scale);
        let (lo, hi) = values.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let span = (hi - lo).max(1.0);
        (lo - span * 0.15, hi + span * 0.15)
    };
    let (v0, v1) = match (drag, ui.ctx().data(|d| d.get_temp::<(f32, f32)>(range_id))) {
        (Some(_), Some(held)) => held,
        _ => fitted,
    };
    let (f0, f1) = (f0 - 1.0, f1 + 1.0);

    let (outer, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let rect = Rect::from_min_max(
        pos2(outer.left() + LEFT_MARGIN, outer.top() + 6.0),
        pos2(outer.right() - 8.0, outer.bottom() - 6.0),
    );
    if rect.width() < 40.0 || rect.height() < 30.0 {
        return;
    }
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
    let painter = ui.painter_at(outer);
    painter.rect_filled(rect, 3.0, p.panel);
    draw_grid(&painter, rect, (f0, f1), (v0, v1), p.border, p.text_muted);
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
    painter.text(
        pos2(outer.left() + 4.0, rect.top()),
        Align2::LEFT_TOP,
        shell
            .editor
            .doc
            .find(layer)
            .map_or_else(String::new, |l| l.channel_label(ch)),
        FontId::proportional(11.0),
        p.text_muted,
    );

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
            _ => shell.editor.clear_key_selection(),
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
fn draw_grid(
    painter: &egui::Painter,
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
        painter.text(
            pos2(rect.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            format!("{}", (v * 100.0).round() / 100.0),
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
