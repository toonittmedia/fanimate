//! Menu contents, shared by the title-bar menus and right-click menus so
//! the same command always looks and behaves the same.

use eframe::egui;
use fanimate_editor::{Action, KeymapKind, Preset, Sample, Tool};

use crate::app::Shell;
use crate::panels::Tab;
use crate::theme::{self, Theme};

/// Menus are at least this wide, so short ones don't look like tooltips.
pub const MENU_MIN_WIDTH: f32 = 200.0;

/// The right-click menu for `response`, styled like the menu bar's menus.
pub fn context_menu(response: &egui::Response, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Popup::context_menu(response)
        .style(theme::menu_style)
        .show(|ui| {
            ui.set_min_width(MENU_MIN_WIDTH);
            contents(ui);
        });
}

/// A button that opens a menu, styled like the menu bar's menus.
pub fn menu_button(
    ui: &mut egui::Ui,
    button: egui::Button<'_>,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let config = egui::containers::menu::MenuConfig::new().style(theme::menu_style);
    egui::containers::menu::MenuButton::from_button(button)
        .config(config)
        .ui(ui, |ui| {
            ui.set_min_width(MENU_MIN_WIDTH);
            contents(ui);
        })
        .0
}

/// A menu item that queues `action`, shows its shortcut from the active
/// keymap, and is greyed out when it cannot run.
pub fn item(ui: &mut egui::Ui, shell: &mut Shell, action: Action) {
    item_checked(ui, shell, action, false);
}

/// An [`item`] shown as chosen, for options such as the interpolation of
/// the current key.
pub fn item_checked(ui: &mut egui::Ui, shell: &mut Shell, action: Action, checked: bool) {
    let label = match action {
        Action::Undo => shell
            .editor
            .history
            .undo_label()
            .map_or("Undo".to_owned(), |l| format!("Undo {l}")),
        Action::Redo => shell
            .editor
            .history
            .redo_label()
            .map_or("Redo".to_owned(), |l| format!("Redo {l}")),
        // the submenu already says "Open Sample"
        Action::OpenSample(sample) => sample.name().to_owned(),
        // the submenu already says "New from Preset"
        Action::NewPreset(preset) => preset.name().to_owned(),
        // the submenu already says "Workspace"
        Action::Workspace(w) => w.name().to_owned(),
        // a switch menu lists the layers to show by name
        Action::SetSwitch(i) => switch_children(shell)
            .and_then(|c| c.into_iter().nth(i))
            .unwrap_or_else(|| action.label().to_owned()),
        a => a.label().to_owned(),
    };
    // the gap applies on both sides of the stretch before the shortcut, so
    // a long label keeps a clear 24 px from its keys
    let button = egui::Button::new(label)
        .shortcut_text(shell.keymap.shortcut_text(action))
        .selected(checked)
        .gap(12.0);
    // with Home in front only what needs no document can run
    let enabled =
        shell.editor.can_run(action) && (!shell.docs.home_front() || crate::docs::works_on_home(action));
    if ui.add_enabled(enabled, button).clicked() {
        shell.pending.push(action);
        ui.close();
    }
}

fn items(ui: &mut egui::Ui, shell: &mut Shell, actions: &[Action]) {
    for a in actions {
        item(ui, shell, *a);
    }
}

pub fn file(ui: &mut egui::Ui, shell: &mut Shell) {
    item(ui, shell, Action::New);
    ui.menu_button("New from Preset", |ui| {
        ui.set_min_width(MENU_MIN_WIDTH);
        for preset in Preset::ALL {
            item(ui, shell, Action::NewPreset(preset));
        }
    });
    items(ui, shell, &[Action::Open, Action::OpenFolder, Action::OpenOther]);
    ui.menu_button("Open Sample", |ui| {
        ui.set_min_width(MENU_MIN_WIDTH);
        for sample in Sample::ALL {
            item(ui, shell, Action::OpenSample(sample));
        }
    });
    ui.separator();
    items(ui, shell, &[Action::Save, Action::SaveAs]);
    items(ui, shell, &[Action::CloseDocument, Action::ShowHome]);
    ui.separator();
    item(ui, shell, Action::Import);
    items(ui, shell, &[Action::ExportVideo, Action::RenderQueue]);
    ui.separator();
    item(ui, shell, Action::Quit);
}

pub fn edit(ui: &mut egui::Ui, shell: &mut Shell) {
    items(ui, shell, &[Action::Undo, Action::Redo, Action::UndoLayer]);
    ui.separator();
    items(
        ui,
        shell,
        &[
            Action::Cut,
            Action::Copy,
            Action::Paste,
            Action::Duplicate,
            Action::Delete,
        ],
    );
    ui.separator();
    items(ui, shell, &[Action::SelectAll, Action::SelectNone]);
    ui.separator();
    ui.menu_button("Combine Shapes", |ui| {
        ui.set_min_width(MENU_MIN_WIDTH);
        shape_items(ui, shell);
    });
    ui.separator();
    keymap_menu(ui, shell);
    item(ui, shell, Action::Preferences);
}

/// Commands on the shapes with selected points.
pub fn shape_items(ui: &mut egui::Ui, shell: &mut Shell) {
    items(
        ui,
        shell,
        &[
            Action::Union,
            Action::Subtract,
            Action::Intersect,
            Action::ExpandShape,
        ],
    );
}

pub fn keymap_menu(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.menu_button("Keyboard Shortcuts", |ui| {
        for kind in KeymapKind::ALL {
            if ui
                .radio(shell.keymap.kind == kind, kind.label())
                .on_hover_text(kind.description())
                .clicked()
            {
                shell.set_keymap(kind);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Show All Shortcuts…").clicked() {
            shell.ui.show_shortcuts = true;
            ui.close();
        }
    });
}

pub fn layer(ui: &mut egui::Ui, shell: &mut Shell) {
    items(
        ui,
        shell,
        &[
            Action::NewVectorLayer,
            Action::NewFramesLayer,
            Action::NewGroupLayer,
            Action::NewSwitchLayer,
            Action::NewBoneLayer,
        ],
    );
    ui.separator();
    items(
        ui,
        shell,
        &[
            Action::Rename,
            Action::ToggleVisibility,
            Action::GroupLayers,
            Action::Ungroup,
            Action::MoveLayerUp,
            Action::MoveLayerDown,
        ],
    );
    label_menu(ui, shell);
    switch_items(ui, shell);
    ui.separator();
    items(
        ui,
        shell,
        &[Action::BindToBone, Action::BindSmoothly, Action::Unbind],
    );
    ui.separator();
    item(ui, shell, Action::LayerProperties);
}

pub fn animation(ui: &mut egui::Ui, shell: &mut Shell) {
    items(
        ui,
        shell,
        &[
            Action::PlayPause,
            Action::FirstFrame,
            Action::PrevFrame,
            Action::NextFrame,
            Action::LastFrame,
            Action::PrevKey,
            Action::NextKey,
        ],
    );
    ui.separator();
    items(
        ui,
        shell,
        &[Action::AddKey, Action::DeleteKey, Action::NewDrawing],
    );
    interpolation_items(ui, shell);
    ui.separator();
    items(
        ui,
        shell,
        &[
            Action::NewAction,
            Action::NewSmartBoneAction,
            Action::FinishAction,
        ],
    );
}

/// The interpolation of the keys on the current frame, one item each.
/// The names of the selected switch layer's children, bottom first.
fn switch_children(shell: &Shell) -> Option<Vec<String>> {
    let id = shell.editor.switch_layer()?;
    let children = shell.editor.doc.find(id)?.kind.children()?;
    Some(children.iter().map(|c| c.name.clone()).collect())
}

/// "Show" items for each layer of the selected switch layer, top of the
/// stack first, the one shown on the current frame checked.
pub fn switch_items(ui: &mut egui::Ui, shell: &mut Shell) {
    let Some(id) = shell.editor.switch_layer() else {
        return;
    };
    let Some(fanimate_core::LayerKind::Switch { children, switch }) =
        shell.editor.doc.find(id).map(|l| &l.kind)
    else {
        return;
    };
    let count = children.len();
    let shown = fanimate_core::scene::switch_choice(children, switch, shell.editor.current_frame() as f32);
    ui.separator();
    ui.menu_button("Show Layer", |ui| {
        ui.set_min_width(MENU_MIN_WIDTH);
        for i in (0..count).rev() {
            item_checked(ui, shell, Action::SetSwitch(i), shown == Some(i));
        }
    });
    item(ui, shell, Action::LipSync);
}

pub fn interpolation_items(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.separator();
    let current = shell.editor.key_interpolation().flatten();
    for interp in fanimate_core::Interpolation::ALL {
        item_checked(
            ui,
            shell,
            Action::SetInterpolation(interp),
            current == Some(interp),
        );
    }
}

pub fn view(ui: &mut egui::Ui, shell: &mut Shell) {
    items(
        ui,
        shell,
        &[
            Action::ZoomIn,
            Action::ZoomOut,
            Action::FitView,
            Action::ActualSize,
        ],
    );
    ui.separator();
    let onion = shell.onion.on;
    item_checked(ui, shell, Action::ToggleOnionSkin, onion);
    let light = shell.light_table;
    item_checked(ui, shell, Action::ToggleLightTable, light);
    ui.separator();
    ui.menu_button("Theme", |ui| {
        for theme in Theme::ALL {
            if ui.radio(shell.theme == theme, theme.label()).clicked() {
                shell.set_theme(ui.ctx(), theme);
                ui.close();
            }
        }
    });
    ui.separator();
    item(ui, shell, Action::CommandPalette);
}

pub fn window(ui: &mut egui::Ui, shell: &mut Shell) {
    for tab in Tab::ALL {
        if ui.button(tab.title()).clicked() {
            shell.ui.open_tab = Some(tab);
            ui.close();
        }
    }
    ui.separator();
    ui.menu_button("Workspace", |ui| {
        ui.set_min_width(MENU_MIN_WIDTH);
        for w in fanimate_editor::Workspace::ALL {
            item(ui, shell, Action::Workspace(w));
        }
    });
    if ui.button("Reset Layout").clicked() {
        shell.ui.reset_layout = true;
        ui.close();
    }
}

pub fn help(ui: &mut egui::Ui, shell: &mut Shell) {
    if ui.button("Keyboard Shortcuts…").clicked() {
        shell.ui.show_shortcuts = true;
        ui.close();
    }
    if ui.button("Product Vision").clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(
            "https://github.com/toonittmedia/fanimate/blob/main/docs/product-vision.md",
        ));
        ui.close();
    }
    item(ui, shell, Action::CheckForUpdates);
    if ui.button("Report a Bug").clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(
            "https://github.com/toonittmedia/fanimate/issues/new/choose",
        ));
        ui.close();
    }
    ui.separator();
    if ui.button("About Fanimate").clicked() {
        shell.ui.show_about = true;
        ui.close();
    }
}

/// Label colours for the selected layers, each with its swatch.
pub fn label_menu(ui: &mut egui::Ui, shell: &mut Shell) {
    let enabled = shell.editor.can_run(Action::SetLabel(fanimate_core::Label::Red));
    ui.add_enabled_ui(enabled, |ui| {
        ui.menu_button("Label Colour", |ui| {
            ui.set_min_width(MENU_MIN_WIDTH);
            let current = shell
                .editor
                .primary()
                .and_then(|id| shell.editor.doc.find(id))
                .map(|l| l.shown_label());
            for label in fanimate_core::Label::ALL {
                let button = egui::Button::new(label.name()).selected(current == Some(label));
                let response = ui.add(button);
                let r = response.rect;
                let square = egui::Rect::from_center_size(
                    egui::pos2(r.right() - 14.0, r.center().y),
                    egui::vec2(10.0, 10.0),
                );
                ui.painter()
                    .rect_filled(square, 2.0, crate::theme::label_color(label));
                if response.clicked() {
                    shell.pending.push(Action::SetLabel(label));
                    ui.close();
                }
            }
        });
    });
}

/// Right-click menu contents for layers (Layers panel rows and the canvas).
pub fn layer_context(ui: &mut egui::Ui, shell: &mut Shell) {
    ui.menu_button("New", |ui| {
        items(
            ui,
            shell,
            &[
                Action::NewVectorLayer,
                Action::NewGroupLayer,
                Action::NewBoneLayer,
                Action::NewFramesLayer,
                Action::NewSwitchLayer,
            ],
        );
    });
    ui.separator();
    items(
        ui,
        shell,
        &[
            Action::Cut,
            Action::Copy,
            Action::Paste,
            Action::Duplicate,
            Action::Delete,
        ],
    );
    ui.separator();
    items(
        ui,
        shell,
        &[
            Action::Rename,
            Action::ToggleVisibility,
            Action::GroupLayers,
            Action::Ungroup,
            Action::MoveLayerUp,
            Action::MoveLayerDown,
        ],
    );
    label_menu(ui, shell);
    switch_items(ui, shell);
    ui.menu_button("Bones", |ui| {
        ui.set_min_width(MENU_MIN_WIDTH);
        items(
            ui,
            shell,
            &[Action::BindToBone, Action::BindSmoothly, Action::Unbind],
        );
    });
    ui.separator();
    items(ui, shell, &[Action::AddKey, Action::DeleteKey]);
    ui.separator();
    item(ui, shell, Action::LayerProperties);
}

/// Tool shortcut for tooltips, e.g. " (V)".
pub fn tool_shortcut(shell: &Shell, tool: Tool) -> String {
    let s = shell.keymap.shortcut_text(Action::Tool(tool));
    if s.is_empty() { s } else { format!(" ({s})") }
}
