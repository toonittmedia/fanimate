//! The Text tool: click the canvas to type new words, or click text to edit
//! it where it stands.
//!
//! The words are drawn by the renderer as always, so what is typed looks
//! exactly as it will in the export. An egui text box sits over them with its
//! own text made see-through, so the caret, the selection, copy and paste and
//! the system's input methods all work, and every change goes to the layer at
//! once through the editor's commands (one undo step for a burst of typing).
//! A new text layer is only made when the first character is typed, so a click
//! that types nothing leaves no empty layer behind.

use eframe::egui::{self, Color32, Pos2, Rect};
use fanimate_core::{LayerId, Vec2};
use fanimate_editor::{SelectMode, Tool};

use crate::app::Shell;
use crate::panels::properties::begin_edit;

/// How far from the words a click still counts as on them, in screen pixels.
const HIT_SLOP_PX: f32 = 6.0;

/// Text being typed on the canvas.
pub struct TextEdit {
    /// The layer it goes to; `None` until the first character makes one.
    pub layer: Option<LayerId>,
    /// Where a new layer's top left goes, in the document.
    pub at: Vec2,
    pub content: String,
    /// Give the box focus on the next frame.
    pub focus: bool,
    /// Whether this edit made the layer (so an empty result removes it again).
    pub made_layer: bool,
}

/// A click with the Text tool: start editing the text under it, or new text.
pub fn interact(response: &egui::Response, shell: &mut Shell, to_doc: &impl Fn(Vec2, f32, Pos2) -> Vec2) {
    if shell.editor.tool != Tool::Text || !response.clicked_by(egui::PointerButton::Primary) {
        return;
    }
    let Some(pos) = response.interact_pointer_pos() else {
        return;
    };
    let p = to_doc(shell.view.center, shell.view.zoom, pos);
    finish(shell);
    shell.ui.text_edit = Some(match shell.editor.text_at(p, HIT_SLOP_PX / shell.view.zoom) {
        Some(id) => {
            shell.editor.select(id, SelectMode::Replace);
            TextEdit {
                layer: Some(id),
                at: Vec2::ZERO,
                content: shell
                    .editor
                    .text(id)
                    .map(|t| t.look().content.clone())
                    .unwrap_or_default(),
                focus: true,
                made_layer: false,
            }
        }
        None => TextEdit {
            layer: None,
            at: p,
            content: String::new(),
            focus: true,
            made_layer: false,
        },
    });
}

/// End the edit in progress. A layer this edit made, left with nothing in it,
/// is taken away again (one undo step, so it can come back).
pub fn finish(shell: &mut Shell) {
    let Some(edit) = shell.ui.text_edit.take() else {
        return;
    };
    if let (true, Some(id)) = (edit.made_layer, edit.layer)
        && edit.content.trim().is_empty()
    {
        shell.editor.checkpoint("Remove empty text");
        shell.editor.doc.remove(id);
        shell.editor.selection.retain(|s| *s != id);
    }
}

/// The text box over the canvas while editing.
pub fn overlay(ui: &egui::Ui, shell: &mut Shell, rect: Rect, to_screen: &impl Fn(Vec2, f32, Vec2) -> Pos2) {
    if shell.ui.text_edit.is_some() && (shell.editor.tool != Tool::Text || shell.editor.playing) {
        finish(shell);
        return;
    }
    let Some(mut edit) = shell.ui.text_edit.take() else {
        return;
    };
    let (center, zoom) = (shell.view.center, shell.view.zoom);
    // where the words sit: the layer's own origin, or the click for new text
    let (origin, size, wrap) = match edit.layer.and_then(|id| Some((id, shell.editor.text(id)?))) {
        Some((id, text)) => (
            shell.editor.world(id).transform_point2(Vec2::ZERO),
            text.look().size,
            text.look().width,
        ),
        None => (edit.at, fanimate_core::Look::default().size, None),
    };
    let top_left = to_screen(center, zoom, origin);
    let font = egui::FontId::proportional((size * zoom).clamp(6.0, 800.0));
    let width = wrap
        .map_or(rect.right() - top_left.x - 8.0, |w| w * zoom)
        .max(40.0);
    let before = edit.content.clone();
    let mut response = None;
    egui::Area::new(egui::Id::new("canvas-text-edit"))
        .fixed_pos(top_left)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            let r = ui.add(
                egui::TextEdit::multiline(&mut edit.content)
                    .font(font)
                    .text_color(Color32::TRANSPARENT)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(width)
                    .desired_rows(1)
                    .lock_focus(true),
            );
            response = Some(r);
        });
    let Some(r) = response else {
        shell.ui.text_edit = Some(edit);
        return;
    };
    if edit.focus {
        r.request_focus();
        edit.focus = false;
    }
    if edit.content != before {
        match edit.layer {
            Some(id) => {
                begin_edit(shell, &r, "Edit text");
                let words = edit.content.clone();
                shell.editor.change_text(id, |look| look.content = words);
            }
            None if !edit.content.is_empty() => {
                if let Some(id) = shell.editor.add_text_layer(edit.at, &edit.content) {
                    edit.layer = Some(id);
                    edit.made_layer = true;
                    // the typing that follows is a burst of its own, not a part of the layer's making
                    shell.ui.active_edit = Some(r.id);
                }
            }
            None => {}
        }
    }
    // Escape, or a click away from the box, ends the edit
    let ended = r.lost_focus() && !edit.focus;
    shell.ui.text_edit = Some(edit);
    if ended {
        finish(shell);
    }
}
