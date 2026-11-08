//! Text animators on the timeline, as range sticks.
//!
//! A text layer's own keys (move, fade or scale the whole text) are the
//! diamonds on its row, as for any layer. Each animator's keys are one white
//! stick under it instead, from the animator's first key to its last, so what
//! belongs to the text as a whole and what belongs to an animator are told
//! apart at a glance. Sticks that overlap in time sit in separate lanes; a lane
//! is used again by a later stick once the earlier one has ended, so there are
//! only as many lanes as animators that run at the same moment.
//!
//! Click a stick to pick the animator (its controls open in the Text panel,
//! which a double-click brings to the front), drag it to move all its keys,
//! drag either end to stretch or squeeze them. Right-click for the menu.

use eframe::egui::{self, Id, Rect, Sense, Stroke, pos2, vec2};
use fanimate_core::{Frame, Interpolation, LayerId, Text};
use fanimate_editor::{Action, SelectMode};

use super::Tab;
use super::timeline::key_outline;
use crate::app::Shell;
use crate::menus;

/// Height of one lane of sticks, and of a stick in it.
const LANE_HEIGHT: f32 = 12.0;
const STICK_HEIGHT: f32 = 8.0;
/// The ends of a stick that stretch it, and the room a stick has beyond its
/// first and last key so a one-frame range can still be taken hold of.
const GRIP: f32 = 6.0;
const CAP: f32 = 4.0;
/// Wide enough to show a name inside it.
const NAME_ROOM: f32 = 48.0;

/// One animator's stick: its place in the layer's list, the frames it runs over
/// and the lane it sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stick {
    pub index: usize,
    pub first: Frame,
    pub last: Frame,
    pub lane: usize,
}

/// The lane of each range (first and last frame, both included) so that no two
/// in one lane overlap, using as few lanes as there can be: the most ranges
/// that cover one frame. Ranges are placed by their start, each in the first
/// lane whose last range has ended.
pub fn pack(ranges: &[(Frame, Frame)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..ranges.len()).collect();
    order.sort_by_key(|&i| (ranges[i].0, ranges[i].1, i));
    let mut lane_ends: Vec<Frame> = Vec::new();
    let mut lanes = vec![0; ranges.len()];
    for i in order {
        let (first, last) = ranges[i];
        match lane_ends.iter().position(|end| *end < first) {
            Some(lane) => {
                lane_ends[lane] = last;
                lanes[i] = lane;
            }
            None => {
                lane_ends.push(last);
                lanes[i] = lane_ends.len() - 1;
            }
        }
    }
    lanes
}

/// The sticks of a text layer: one for each animator that has keys.
pub fn sticks(text: &Text) -> Vec<Stick> {
    let ranges: Vec<(usize, (Frame, Frame))> = text
        .animators
        .iter()
        .enumerate()
        .filter_map(|(i, a)| Some((i, a.key_range()?)))
        .collect();
    let lanes = pack(&ranges.iter().map(|(_, r)| *r).collect::<Vec<_>>());
    ranges
        .into_iter()
        .zip(lanes)
        .map(|((index, (first, last)), lane)| Stick {
            index,
            first,
            last,
            lane,
        })
        .collect()
}

/// Which part of a stick a press is on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Body,
    Start,
    End,
}

/// The strip of sticks under a text layer's row, if it has any.
pub fn rows(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    id: LayerId,
    depth: usize,
    keys_clip: Rect,
    x_of: &impl Fn(f32) -> f32,
    frame_at: &impl Fn(f32) -> Frame,
) {
    let Some(text) = shell.editor.text(id) else {
        return;
    };
    let sticks = sticks(text);
    let Some(lanes) = sticks.iter().map(|s| s.lane + 1).max() else {
        return;
    };
    let names: Vec<String> = text.animators.iter().map(|a| a.name.clone()).collect();
    let label = shell
        .editor
        .doc
        .find(id)
        .map(|l| crate::theme::label_color(l.shown_label()))
        .unwrap_or_default();
    let p = shell.palette;
    let height = lanes as f32 * LANE_HEIGHT + 6.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let names_col = Rect::from_min_max(
        rect.min,
        pos2(rect.left() + shell.ui.timeline_names_width, rect.bottom()),
    );
    let keys = Rect::from_min_max(pos2(names_col.right(), rect.top()), rect.max);
    let painter = ui.painter_at(rect);
    if shell.editor.is_selected(id) {
        painter.rect_filled(rect, 0.0, p.accent.gamma_multiply(0.2));
    }
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
        0.0,
        label,
    );
    painter.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, p.border.gamma_multiply(0.5)),
    );
    painter.text(
        pos2(
            names_col.left() + 8.0 + (depth + 1) as f32 * 12.0,
            names_col.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        "Animators",
        egui::FontId::proportional(10.5),
        p.text_muted,
    );

    let clip = keys.intersect(keys_clip);
    let key_painter = ui.painter_at(clip);
    let layer_picked = shell.editor.is_selected(id);
    for s in &sticks {
        let y = keys.top() + 3.0 + (s.lane as f32 + 0.5) * LANE_HEIGHT;
        let (x0, x1) = (x_of(s.first as f32), x_of(s.last as f32));
        let stick = Rect::from_min_max(
            pos2(x0 - CAP, y - STICK_HEIGHT / 2.0),
            pos2(x1.max(x0) + CAP, y + STICK_HEIGHT / 2.0),
        );
        let picked = layer_picked && shell.ui.text_animator == Some((id, s.index));
        key_painter.rect(
            stick,
            STICK_HEIGHT / 2.0,
            egui::Color32::WHITE,
            key_outline(&p, picked),
            egui::StrokeKind::Inside,
        );
        let name = names.get(s.index).map_or("", String::as_str);
        if stick.width() >= NAME_ROOM {
            let galley = key_painter.layout_no_wrap(name.to_owned(), egui::FontId::proportional(8.5), p.bg);
            if galley.size().x + 10.0 <= stick.width() {
                key_painter.galley(pos2(stick.left() + 6.0, y - galley.size().y / 2.0), galley, p.bg);
            }
        }
        let grip = GRIP.min(stick.width() / 3.0);
        let zones = [
            (Zone::Body, stick),
            (
                Zone::Start,
                Rect::from_min_max(stick.min, pos2(stick.left() + grip, stick.bottom())),
            ),
            (
                Zone::End,
                Rect::from_min_max(pos2(stick.right() - grip, stick.top()), stick.max),
            ),
        ];
        for (zone, area) in zones {
            // the pressable part is as tall as the lane, so a thin stick is easy to hit
            let area = Rect::from_min_max(
                pos2(area.left(), y - LANE_HEIGHT / 2.0),
                pos2(area.right(), y + LANE_HEIGHT / 2.0),
            )
            .intersect(clip);
            if area.is_negative() {
                continue;
            }
            let salt = Id::new(("animator-stick", id, s.index, zone as u8));
            let resp = ui.interact(area, salt, Sense::click_and_drag());
            if zone != Zone::Body && (resp.hovered() || resp.dragged()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            work(ui, shell, &resp, zone, id, s, name, frame_at);
        }
    }
}

/// What a press, a drag or a click on one part of a stick does.
#[allow(clippy::too_many_arguments)]
fn work(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    resp: &egui::Response,
    zone: Zone,
    id: LayerId,
    stick: &Stick,
    name: &str,
    frame_at: &impl Fn(f32) -> Frame,
) {
    let index = stick.index;
    let pick = |shell: &mut Shell| {
        if !shell.editor.is_selected(id) {
            shell.editor.select(id, SelectMode::Replace);
        }
        shell.ui.text_animator = Some((id, index));
    };
    let press_at = Id::new(("animator-stick-press", id, index));
    if resp.drag_started()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        shell.editor.set_focus(fanimate_editor::Focus::Timeline);
        pick(shell);
        if shell.editor.begin_animator_drag(id, index).is_some() {
            let press = frame_at(pos.x);
            ui.data_mut(|d| d.insert_temp(press_at, press));
        }
    }
    if resp.dragged()
        && let Some(from) = shell.editor.animator_drag_origin()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let now = frame_at(pos.x);
        let (first, last) = match zone {
            Zone::Body => {
                let press: Frame = ui.data_mut(|d| d.get_temp(press_at)).unwrap_or(now);
                let by = now - press;
                (from.0 + by, from.1 + by)
            }
            Zone::Start => (now.min(from.1), from.1),
            Zone::End => (from.0, now.max(from.0)),
        };
        shell.editor.drag_animator_range(first, last);
    }
    if resp.drag_stopped() {
        shell.editor.end_animator_drag();
    }
    if resp.clicked() {
        shell.editor.set_focus(fanimate_editor::Focus::Timeline);
        pick(shell);
    }
    if resp.double_clicked() {
        pick(shell);
        shell.ui.open_tab = Some(Tab::Text);
    }
    if resp.secondary_clicked() {
        pick(shell);
    }
    if zone == Zone::Body {
        menu(&resp.clone(), shell, index);
        let tip = format!("{name}: frames {} to {}", stick.first, stick.last);
        resp.clone().on_hover_text(tip);
    }
}

/// The right-click menu of a stick. It acts on the animator the press picked.
fn menu(resp: &egui::Response, shell: &mut Shell, index: usize) {
    menus::context_menu(resp, |ui| {
        if ui.button("Open in Text panel").clicked() {
            shell.ui.open_tab = Some(Tab::Text);
            ui.close();
        }
        menus::item(ui, shell, Action::DuplicateTextAnimator(index));
        menus::item(ui, shell, Action::RemoveTextAnimator(index));
        ui.separator();
        ui.menu_button("Key type", |ui| {
            for interp in Interpolation::ALL {
                menus::item(ui, shell, Action::SetAnimatorInterpolation(index, interp));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::{Animator, TextPreset, TextUnit};

    /// The most ranges that cover one frame: the fewest lanes there can be.
    fn deepest(ranges: &[(Frame, Frame)]) -> usize {
        let last = ranges.iter().map(|r| r.1).max().unwrap_or(0);
        (0..=last)
            .map(|f| ranges.iter().filter(|r| r.0 <= f && f <= r.1).count())
            .max()
            .unwrap_or(0)
    }

    fn valid(ranges: &[(Frame, Frame)], lanes: &[usize]) {
        for i in 0..ranges.len() {
            for j in i + 1..ranges.len() {
                let apart = ranges[i].1 < ranges[j].0 || ranges[j].1 < ranges[i].0;
                assert!(
                    lanes[i] != lanes[j] || apart,
                    "{:?} and {:?} share lane {}",
                    ranges[i],
                    ranges[j],
                    lanes[i]
                );
            }
        }
        assert_eq!(
            lanes.iter().max().map_or(0, |m| m + 1),
            deepest(ranges),
            "not the fewest lanes: {lanes:?}"
        );
    }

    #[test]
    fn a_lane_is_used_again_once_its_range_is_over() {
        // the sketch: Rise in 1 to 30, Tint 20 to 50, Wave 35 to 60, Fade 64 to 80
        let ranges = [(1, 30), (20, 50), (35, 60), (64, 80)];
        let lanes = pack(&ranges);
        assert_eq!(lanes, [0, 1, 0, 0], "four ranges, two lanes");
        valid(&ranges, &lanes);
    }

    #[test]
    fn ranges_that_only_touch_at_a_frame_overlap_and_the_next_frame_does_not() {
        assert_eq!(pack(&[(1, 10), (10, 20)]), [0, 1]);
        assert_eq!(pack(&[(1, 10), (11, 20)]), [0, 0]);
    }

    #[test]
    fn the_lanes_are_the_fewest_whatever_the_order_the_animators_are_in() {
        let ranges = [(40, 80), (1, 100), (5, 30), (35, 50), (60, 70), (75, 90), (2, 3)];
        let lanes = pack(&ranges);
        valid(&ranges, &lanes);
        let mut reversed = ranges;
        reversed.reverse();
        valid(&reversed, &pack(&reversed));
        assert!(pack(&[]).is_empty());
        assert_eq!(pack(&[(5, 5)]), [0]);
    }

    #[test]
    fn only_animators_with_keys_get_a_stick_and_it_keeps_their_place_in_the_list() {
        let mut text = Text::new("Hello");
        text.animators.push(TextPreset::Typewriter.animator(1, 24));
        text.animators.push(Animator::new("Still", TextUnit::Word));
        text.animators.push(TextPreset::Wave.animator(10, 24));
        let sticks = sticks(&text);
        assert_eq!(
            sticks.iter().map(|s| (s.index, s.lane)).collect::<Vec<_>>(),
            [(0, 0), (2, 1)],
            "the one with no keys has none, and the others overlap"
        );
        assert_eq!((sticks[0].first, sticks[0].last), (1, 25));
    }
}
