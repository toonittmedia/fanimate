//! A custom title bar that holds the menus, like modern editors do, so no
//! vertical space is spent on a separate menu row.
//!
//! On Windows and Linux the window has no system decorations: this bar
//! draws the menus, the minimise, maximise and close buttons, drags the
//! window, and [`resize_edges`] makes the window edges resizable. On macOS
//! the native traffic-light buttons stay, inside a full-size content view.

use eframe::egui::{
    self, Align, Color32, CursorIcon, Id, Key, Layout, Pos2, Rect, ResizeDirection, Sense, SetOpenCommand,
    Stroke, Vec2, ViewportCommand, vec2,
};

use crate::app::{Shell, window_title};
use crate::{menus, theme};

pub const HEIGHT: f32 = 34.0;
const BUTTON_WIDTH: f32 = 46.0;
const RESIZE_MARGIN: f32 = 5.0;
/// Menu bar buttons: text plus this much space on each side.
const MENU_PAD_X: f32 = 9.0;
const MENU_BUTTON_HEIGHT: f32 = 26.0;

type MenuContents = fn(&mut egui::Ui, &mut Shell);
const MENUS: [(&str, MenuContents); 7] = [
    ("File", menus::file),
    ("Edit", menus::edit),
    ("Layer", menus::layer),
    ("Animation", menus::animation),
    ("View", menus::view),
    ("Window", menus::window),
    ("Help", menus::help),
];

pub fn uses_custom_frame() -> bool {
    !cfg!(target_os = "macos")
}

/// Where the window sits, for the custom frame's maximise.
#[derive(Default)]
pub struct WindowState {
    /// Set while the window fills the monitor as a normal window instead of
    /// being maximised (see [`track_window`]).
    filled: Option<Filled>,
    /// The last normal placement, to restore to.
    normal: Option<Placement>,
}

#[derive(Clone, Copy)]
struct Filled {
    target: Rect,
    restore: Option<Placement>,
    /// Frames left in which the window may still be moving into `target`.
    settling: u8,
}

/// A normal window's outer position and inner size, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    pos: Pos2,
    size: Vec2,
}

impl WindowState {
    /// Maximised, or filling the monitor in place of maximised.
    pub fn is_maximized(&self, ctx: &egui::Context) -> bool {
        self.filled.is_some() || ctx.input(|i| i.viewport().maximized.unwrap_or(false))
    }
}

/// Does a window of `inner` size cover the whole monitor?
fn covers_monitor(inner: Vec2, monitor: Vec2) -> bool {
    inner.x >= monitor.x - 0.5 && inner.y >= monitor.y - 0.5
}

fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).abs().max_elem() <= 2.0
}

/// Where to restore a filled window when no earlier placement is known:
/// three quarters of the monitor, centred.
fn default_restore(target: Rect) -> Placement {
    let size = target.size() * 0.75;
    Placement {
        pos: target.min + (target.size() - size) / 2.0,
        size,
    }
}

/// Keep a maximised window from counting as fullscreen.
///
/// A borderless window has no frame, so when Windows maximises it, winit
/// sizes it to the monitor's work area. With an auto-hiding taskbar the
/// work area is the whole monitor, and Windows then treats the window as
/// fullscreen: the taskbar no longer slides up, and the window may be shown
/// by direct flip, which leaves Alt+Tab previews and OBS window capture
/// blank. Browsers solve this by stopping one pixel short of the edge; here
/// the window is un-maximised and placed one point short of the monitor's
/// bottom edge, without any platform code.
pub fn track_window(ctx: &egui::Context, w: &mut WindowState) {
    if !cfg!(windows) || !uses_custom_frame() {
        return;
    }
    let (maximized, fullscreen, inner, outer, monitor) = ctx.input(|i| {
        let v = i.viewport();
        (
            v.maximized.unwrap_or(false),
            v.fullscreen.unwrap_or(false),
            v.inner_rect,
            v.outer_rect,
            v.monitor_size,
        )
    });
    let (Some(inner), Some(outer), Some(monitor)) = (inner, outer, monitor) else {
        return;
    };
    if fullscreen {
        return;
    }

    if let Some(f) = &mut w.filled {
        if f.settling > 0 {
            f.settling -= 1;
            ctx.request_repaint();
            return;
        }
        // moved or resized since (Snap, Win+arrow keys, drag): a normal window
        if maximized
            || !near(inner.size(), f.target.size())
            || !near(outer.min.to_vec2(), f.target.min.to_vec2())
        {
            w.filled = None;
        } else {
            return;
        }
    }

    let filled_size = monitor - vec2(0.0, 1.0);
    if maximized && covers_monitor(inner.size(), monitor) {
        let target = Rect::from_min_size(inner.min, filled_size);
        ctx.send_viewport_cmd(ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(target.min));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(target.size()));
        w.filled = Some(Filled {
            target,
            restore: w.normal,
            settling: 3,
        });
    } else if !maximized && near(inner.size(), filled_size) {
        // a filled window from the last session, restored by eframe
        w.filled = Some(Filled {
            target: Rect::from_min_size(outer.min, inner.size()),
            restore: None,
            settling: 0,
        });
    } else if !maximized {
        w.normal = Some(Placement {
            pos: outer.min,
            size: inner.size(),
        });
    }
}

fn place(ctx: &egui::Context, p: Placement) {
    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(p.pos));
    ctx.send_viewport_cmd(ViewportCommand::InnerSize(p.size));
}

fn toggle_maximized(ctx: &egui::Context, w: &mut WindowState) {
    if let Some(f) = w.filled.take() {
        place(ctx, f.restore.unwrap_or_else(|| default_restore(f.target)));
    } else {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
}

/// Start dragging the window by its title bar. A filled window first shrinks
/// back to its normal size under the pointer, as a maximised window does.
fn start_drag(ctx: &egui::Context, w: &mut WindowState, pointer: Option<Pos2>) {
    if let (Some(f), Some(p)) = (w.filled.take(), pointer) {
        let restore = f.restore.unwrap_or_else(|| default_restore(f.target));
        // keep the pointer at the same fraction across the title bar
        let fraction = (p.x / f.target.width()).clamp(0.0, 1.0);
        let screen = f.target.min + p.to_vec2();
        place(
            ctx,
            Placement {
                pos: Pos2::new(screen.x - fraction * restore.size.x, screen.y - p.y),
                size: restore.size,
            },
        );
    }
    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
}

pub fn show(ui: &mut egui::Ui, shell: &mut Shell) {
    let bg = shell.palette.bg;
    egui::Panel::top("titlebar")
        .exact_size(HEIGHT)
        .frame(egui::Frame::NONE.fill(bg))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            // background first, so the menus drawn on top get their clicks
            let drag = ui.interact(rect, Id::new("titlebar-drag"), Sense::click_and_drag());
            if drag.drag_started_by(egui::PointerButton::Primary) {
                start_drag(ui.ctx(), &mut shell.ui.window, drag.interact_pointer_pos());
            }
            if drag.double_clicked() {
                toggle_maximized(ui.ctx(), &mut shell.ui.window);
            }

            // centred document title
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                window_title(shell),
                egui::FontId::proportional(13.0),
                shell.palette.text_muted,
            );

            ui.horizontal_centered(|ui| {
                if cfg!(target_os = "macos") {
                    ui.add_space(72.0); // traffic lights
                } else {
                    ui.add_space(10.0);
                }
                ui.add(egui::Image::new((shell.icon.id(), egui::vec2(18.0, 18.0))));
                ui.add_space(6.0);
                menu_bar(ui, shell);

                if uses_custom_frame() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        window_buttons(ui, shell);
                    });
                }
            });
        });
}

/// The menus. Behaves like a desktop menu bar: click a title to open its
/// menu; while one is open, hovering another title (or pressing the left
/// and right arrow keys) switches to it; click the title again, click
/// elsewhere or press Escape to close.
fn menu_bar(ui: &mut egui::Ui, shell: &mut Shell) {
    let ctx = ui.ctx().clone();
    let ids = MENUS.map(|(title, _)| Id::new(("menu-bar", title)));
    let open = ids.iter().position(|id| egui::Popup::is_id_open(&ctx, *id));
    let arrow_target = open.and_then(|i| {
        let (left, right) =
            ctx.input(|inp| (inp.key_pressed(Key::ArrowLeft), inp.key_pressed(Key::ArrowRight)));
        let n = MENUS.len();
        match (left, right) {
            (true, false) => Some((i + n - 1) % n),
            (false, true) => Some((i + 1) % n),
            _ => None,
        }
    });

    ui.spacing_mut().item_spacing.x = 0.0;
    for (i, (title, contents)) in MENUS.iter().enumerate() {
        let response = menu_title(ui, shell, title, open == Some(i));
        let switch_here = open.is_some_and(|o| o != i) && response.hovered();
        let command = if response.clicked() {
            Some(SetOpenCommand::Toggle)
        } else if switch_here || arrow_target == Some(i) {
            Some(SetOpenCommand::Bool(true))
        } else {
            None
        };
        egui::Popup::menu(&response)
            .id(ids[i])
            .open_memory(command)
            .style(theme::menu_style)
            .show(|ui| {
                ui.set_min_width(menus::MENU_MIN_WIDTH);
                contents(ui, shell);
            });
    }
}

fn menu_title(ui: &mut egui::Ui, shell: &Shell, title: &str, open: bool) -> egui::Response {
    let p = shell.palette;
    let galley = ui
        .painter()
        .layout_no_wrap(title.to_owned(), egui::FontId::proportional(13.0), p.text);
    let size = vec2(galley.size().x + 2.0 * MENU_PAD_X, MENU_BUTTON_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if open {
        Some(p.border)
    } else if response.hovered() {
        Some(p.raised)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    let pos = (rect.center() - galley.size() / 2.0).round();
    ui.painter().galley(pos, galley, p.text);
    response
}

#[derive(Clone, Copy)]
enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn window_buttons(ui: &mut egui::Ui, shell: &mut Shell) {
    let maximized = shell.ui.window.is_maximized(ui.ctx());
    let p = shell.palette;
    if caption_button(
        ui,
        Caption::Close,
        p.text,
        Some(Color32::from_rgb(0xC4, 0x2B, 0x1C)),
        p.border,
    )
    .clicked()
    {
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }
    let max = if maximized {
        Caption::Restore
    } else {
        Caption::Maximize
    };
    if caption_button(ui, max, p.text, None, p.border).clicked() {
        toggle_maximized(ui.ctx(), &mut shell.ui.window);
    }
    if caption_button(ui, Caption::Minimize, p.text, None, p.border).clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

/// A Windows-style caption button with a drawn glyph (no font needed).
fn caption_button(
    ui: &mut egui::Ui,
    kind: Caption,
    fg: Color32,
    hover_bg: Option<Color32>,
    default_hover: Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(BUTTON_WIDTH, HEIGHT), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, 0.0, hover_bg.unwrap_or(default_hover));
    }
    let fg = if hovered && hover_bg.is_some() {
        Color32::WHITE
    } else {
        fg
    };
    let stroke = Stroke::new(1.0, fg);
    let c = rect.center();
    let s = 5.0;
    let painter = ui.painter();
    match kind {
        Caption::Minimize => {
            painter.line_segment([c + egui::vec2(-s, 0.0), c + egui::vec2(s, 0.0)], stroke);
        }
        Caption::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(c, egui::vec2(2.0 * s, 2.0 * s)),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Caption::Restore => {
            let back = Rect::from_center_size(c + egui::vec2(2.0, -2.0), egui::vec2(1.6 * s, 1.6 * s));
            let front = Rect::from_center_size(c + egui::vec2(-1.0, 1.0), egui::vec2(1.6 * s, 1.6 * s));
            painter.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Inside);
            painter.rect_filled(front, 1.0, ui.visuals().panel_fill);
            painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Inside);
        }
        Caption::Close => {
            painter.line_segment([c + egui::vec2(-s, -s), c + egui::vec2(s, s)], stroke);
            painter.line_segment([c + egui::vec2(-s, s), c + egui::vec2(s, -s)], stroke);
        }
    }
    response
}

/// Make the edges of an undecorated window resizable.
pub fn resize_edges(ctx: &egui::Context, w: &WindowState) {
    if !uses_custom_frame() {
        return;
    }
    let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
    if w.is_maximized(ctx) || fullscreen {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let r = ctx.content_rect();
    let m = RESIZE_MARGIN;
    let (l, rt, t, b) = (
        pos.x <= r.left() + m,
        pos.x >= r.right() - m,
        pos.y <= r.top() + m,
        pos.y >= r.bottom() - m,
    );
    let dir = match (l, rt, t, b) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (.., true) => ResizeDirection::South,
        _ => return,
    };
    ctx.set_cursor_icon(match dir {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    });
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_monitor_counts_as_covering() {
        let monitor = vec2(1366.0, 768.0);
        assert!(covers_monitor(monitor, monitor));
        // a visible taskbar leaves the work area shorter than the monitor
        assert!(!covers_monitor(vec2(1366.0, 728.0), monitor));
        // the filled size stops short, so it never counts as fullscreen
        assert!(!covers_monitor(monitor - vec2(0.0, 1.0), monitor));
    }

    #[test]
    fn default_restore_is_centred_inside_the_monitor() {
        let target = Rect::from_min_size(Pos2::new(1366.0, 0.0), vec2(1920.0, 1079.0));
        let p = default_restore(target);
        assert!(target.contains_rect(Rect::from_min_size(p.pos, p.size)));
        assert!(near(
            Rect::from_min_size(p.pos, p.size).center().to_vec2(),
            target.center().to_vec2()
        ));
    }
}
