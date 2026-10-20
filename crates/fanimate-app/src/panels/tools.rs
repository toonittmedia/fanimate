//! The Tools panel, grouped as in Moho Pro.

use eframe::egui::{self, RichText};
use fanimate_editor::{Action, Tool, ToolGroup};

use crate::app::Shell;
use crate::icons::{self, Icon};
use crate::menus::tool_shortcut;

/// The tools in rows of `columns` buttons. The column is exactly that wide
/// (`Layout::tools_width`), so no room is left over.
pub fn show(ui: &mut egui::Ui, shell: &mut Shell, columns: usize) {
    let p = shell.palette;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            for group in ToolGroup::ALL {
                ui.label(RichText::new(group.label()).size(11.0).color(p.text_muted));
                let tools: Vec<Tool> = Tool::ALL.into_iter().filter(|t| t.group() == group).collect();
                ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
                for row in tools.chunks(columns.max(1)) {
                    ui.horizontal(|ui| {
                        for &tool in row {
                            let selected = shell.editor.tool == tool;
                            let tip =
                                format!("{}{}\n{}", tool.label(), tool_shortcut(shell, tool), tool.hint());
                            if icons::button(ui, Icon::Tool(tool), 32.0, selected, !tool.is_available())
                                .on_hover_text(tip)
                                .clicked()
                            {
                                shell.pending.push(Action::Tool(tool));
                            }
                        }
                    });
                }
                ui.add_space(6.0);
                if group == ToolGroup::Draw {
                    combine_buttons(ui, shell);
                }
            }
        });
}

/// Union, Subtract and Intersect, for the shapes with selected points.
fn combine_buttons(ui: &mut egui::Ui, shell: &mut Shell) {
    let p = shell.palette;
    ui.label(RichText::new("Combine").size(11.0).color(p.text_muted));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for (icon, action) in [
            (Icon::Union, Action::Union),
            (Icon::Subtract, Action::Subtract),
            (Icon::Intersect, Action::Intersect),
        ] {
            let enabled = shell.editor.can_run(action);
            let tip = format!(
                "{}\nSelect points on two or more shapes with a point tool, then combine them.",
                action.label()
            );
            if icons::button(ui, icon, 32.0, false, !enabled)
                .on_hover_text(tip)
                .clicked()
                && enabled
            {
                shell.pending.push(action);
            }
        }
    });
    ui.add_space(6.0);
}
