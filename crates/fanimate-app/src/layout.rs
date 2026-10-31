//! The panel layout: the canvas always fills the middle, with the panels in
//! three regions around it.
//!
//! Tools sit left of the canvas, the inspector tabs (Properties, Style,
//! Actions, Layers, Bones, History) in a column on the right, and the
//! Timeline and X-Sheet along the bottom. Each region is resizable and its tabs
//! can be reordered and moved between the leaves inside it; the canvas has no
//! tab bar and no border, so nothing around it wastes room.

use eframe::egui::{self, Id};
use egui_dock::{DockArea, DockState, NodeIndex};
use fanimate_editor::Workspace;
use serde::{Deserialize, Serialize};

use crate::app::Shell;
use crate::panels::{Tab, TabViewer};

/// Fewest and most columns of tool buttons.
pub const MIN_TOOL_COLUMNS: usize = 3;
pub const MAX_TOOL_COLUMNS: usize = 8;
/// A tool button, the gap between two, and the margin round the grid.
const TOOL_BUTTON: f32 = 32.0;
const TOOL_GAP: f32 = 3.0;
const TOOL_MARGIN: f32 = 6.0;

/// Where a docked panel lives (the tools column is not docked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Right,
    Bottom,
}

impl Tab {
    /// The region a panel opens in when it is not on screen.
    pub fn region(self) -> Region {
        match self {
            Self::Timeline | Self::XSheet | Self::Graph => Region::Bottom,
            Self::Layers | Self::Properties | Self::Style | Self::Actions | Self::History | Self::Bones => {
                Region::Right
            }
        }
    }
}

/// The three docks, which of them show, and how big they start.
#[derive(Clone, Serialize, Deserialize)]
pub struct Layout {
    pub right: DockState<Tab>,
    pub bottom: DockState<Tab>,
    pub show_tools: bool,
    pub show_right: bool,
    pub show_bottom: bool,
    /// Columns of tool buttons.
    pub tool_columns: usize,
    /// Starting size of the right column and the bottom area.
    pub right_width: f32,
    pub bottom_height: f32,
    /// Counts layout resets. Panel sizes are remembered under an id that
    /// includes it, so a reset starts from the sizes above again.
    pub generation: u32,
    /// Simple mode: the Tools panel shows only the basic tool groups.
    #[serde(default)]
    pub simple: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self::for_workspace(Workspace::Animate)
    }
}

impl Layout {
    /// The saved layouts of the first version of this layout, which had the
    /// canvas as a tab, are not read: they are saved under another key.
    pub const KEY: &'static str = "fanimate.layout.v2";

    /// The arrangement for a kind of work. `generation` carries on from the
    /// layout it replaces, so its sizes apply.
    pub fn for_workspace(w: Workspace) -> Self {
        let mut layout = Self {
            right: DockState::new(vec![Tab::Properties, Tab::Style, Tab::Actions]),
            bottom: DockState::new(vec![Tab::Timeline, Tab::XSheet, Tab::Graph]),
            show_tools: true,
            show_right: true,
            show_bottom: true,
            tool_columns: MIN_TOOL_COLUMNS,
            right_width: 300.0,
            bottom_height: 230.0,
            generation: 0,
            simple: w == Workspace::Simple,
        };
        // History is not shown until it is asked for (Window menu); it then
        // joins Layers. Bones is a tab beside Layers, where the room is.
        match w {
            Workspace::Animate => {
                layout.right.main_surface_mut().split_below(
                    NodeIndex::root(),
                    0.42,
                    vec![Tab::Layers, Tab::Bones],
                );
            }
            Workspace::Draw => {
                layout.right = DockState::new(vec![Tab::Style, Tab::Properties, Tab::Actions]);
                layout
                    .right
                    .main_surface_mut()
                    .split_below(NodeIndex::root(), 0.55, vec![Tab::Layers]);
                layout.bottom_height = 170.0;
            }
            Workspace::Rig => {
                layout.right = DockState::new(vec![Tab::Properties, Tab::Actions, Tab::Style]);
                let s = layout.right.main_surface_mut();
                s.split_below(NodeIndex::root(), 0.4, vec![Tab::Layers, Tab::Bones]);
                layout.right_width = 340.0;
                layout.bottom_height = 250.0;
            }
            Workspace::FrameByFrame => {
                layout.right = DockState::new(vec![Tab::XSheet]);
                layout.right.main_surface_mut().split_below(
                    NodeIndex::root(),
                    0.4,
                    vec![Tab::Layers, Tab::Style, Tab::Properties, Tab::Actions],
                );
                layout.bottom = DockState::new(vec![Tab::Timeline]);
                layout.right_width = 340.0;
            }
            Workspace::Simple => {
                layout.right = DockState::new(vec![Tab::Properties, Tab::Style]);
                layout
                    .right
                    .main_surface_mut()
                    .split_below(NodeIndex::root(), 0.5, vec![Tab::Layers]);
                layout.bottom = DockState::new(vec![Tab::Timeline]);
                layout.bottom_height = 200.0;
            }
            // only the canvas and the timeline: the next start opens in Animate
            Workspace::Review => {
                layout.bottom = DockState::new(vec![Tab::Timeline]);
                layout.show_tools = false;
                layout.show_right = false;
                layout.bottom_height = 200.0;
            }
        }
        layout
    }

    /// A fresh arrangement, with panel sizes starting over.
    pub fn reset(&mut self, w: Workspace) {
        let generation = self.generation + 1;
        *self = Self::for_workspace(w);
        self.generation = generation;
    }

    fn dock(&mut self, region: Region) -> &mut DockState<Tab> {
        match region {
            Region::Right => &mut self.right,
            Region::Bottom => &mut self.bottom,
        }
    }

    /// Whether the panel is on screen, in either region.
    pub fn is_open(&self, tab: Tab) -> bool {
        self.show_right && self.right.find_tab(&tab).is_some()
            || self.show_bottom && self.bottom.find_tab(&tab).is_some()
    }

    /// Show a panel and bring it to the front of its tabs. A panel that was
    /// closed comes back in its own region: History and Bones beside Layers,
    /// the rest in the leaf that has focus.
    pub fn open(&mut self, tab: Tab) {
        for region in [Region::Right, Region::Bottom] {
            let dock = self.dock(region);
            if let Some(path) = dock.find_tab(&tab) {
                let _ = dock.set_active_tab(path);
                self.show(region);
                return;
            }
        }
        let region = tab.region();
        self.show(region);
        let dock = self.dock(region);
        if matches!(tab, Tab::History | Tab::Bones)
            && let Some(layers) = dock.find_tab(&Tab::Layers)
        {
            dock.set_focused_node_and_surface(layers.node_path());
        }
        dock.push_to_focused_leaf(tab);
        if let Some(path) = dock.find_tab(&tab) {
            let _ = dock.set_active_tab(path);
        }
    }

    fn show(&mut self, region: Region) {
        match region {
            Region::Right => self.show_right = true,
            Region::Bottom => self.show_bottom = true,
        }
    }

    /// Hide Tools, which has no tab to close.
    pub fn toggle_tools(&mut self) {
        self.show_tools = !self.show_tools;
    }

    /// The width of the tools column for this many columns of buttons: no
    /// room is left over.
    pub fn tools_width(columns: usize) -> f32 {
        let n = columns as f32;
        n * TOOL_BUTTON + (n - 1.0) * TOOL_GAP + 2.0 * TOOL_MARGIN
    }

    /// How many columns of buttons a width fits, within the limits.
    pub fn columns_for(width: f32) -> usize {
        let room = width - 2.0 * TOOL_MARGIN + TOOL_GAP;
        ((room / (TOOL_BUTTON + TOOL_GAP)).round() as usize).clamp(MIN_TOOL_COLUMNS, MAX_TOOL_COLUMNS)
    }
}

/// Draw the regions round the canvas. What is left is the canvas's.
pub fn show(ui: &mut egui::Ui, shell: &mut Shell, layout: &mut Layout, style: &egui_dock::Style) {
    let p = shell.palette;
    let generation = layout.generation;
    let frame = || egui::Frame::NONE.fill(p.panel);

    if layout.show_right {
        egui::Panel::right(Id::new(("layout-right", generation)))
            .resizable(true)
            .default_size(layout.right_width)
            .size_range(220.0..=720.0)
            .frame(frame())
            .show(ui, |ui| show_dock(ui, shell, &mut layout.right, style, "right"));
    }
    if layout.show_bottom {
        egui::Panel::bottom(Id::new(("layout-bottom", generation)))
            .resizable(true)
            .default_size(layout.bottom_height)
            .size_range(110.0..=640.0)
            .frame(frame())
            .show(ui, |ui| show_dock(ui, shell, &mut layout.bottom, style, "bottom"));
    }
    if layout.show_tools {
        tools_panel(ui, shell, layout);
    }
}

fn show_dock(
    ui: &mut egui::Ui,
    shell: &mut Shell,
    state: &mut DockState<Tab>,
    style: &egui_dock::Style,
    name: &str,
) {
    DockArea::new(state)
        .id(Id::new(("layout-dock", name)))
        .style(style.clone())
        .show_close_buttons(true)
        .show_add_buttons(false)
        .show_leaf_collapse_buttons(false)
        .show_leaf_close_all_buttons(false)
        .show_inside(ui, &mut TabViewer { shell });
}

/// The tools column. Its width is a whole number of button columns, which
/// the edge snaps between when dragged, so no room is wasted.
fn tools_panel(ui: &mut egui::Ui, shell: &mut Shell, layout: &mut Layout) {
    let p = shell.palette;
    let columns = layout.tool_columns.clamp(MIN_TOOL_COLUMNS, MAX_TOOL_COLUMNS);
    let width = Layout::tools_width(columns);
    let out = egui::Panel::left(Id::new(("layout-tools", layout.generation)))
        .resizable(false)
        .exact_size(width)
        .frame(
            egui::Frame::NONE
                .fill(p.panel)
                .inner_margin(egui::Margin::symmetric(TOOL_MARGIN as i8, 4)),
        )
        .show(ui, |ui| {
            crate::panels::tools_panel(ui, shell, columns, layout.simple)
        });
    let rect = out.response.rect;
    let edge = egui::Rect::from_x_y_ranges((rect.right() - 3.0)..=(rect.right() + 3.0), rect.y_range());
    let grab = ui.interact(
        edge,
        Id::new(("layout-tools-edge", layout.generation)),
        egui::Sense::drag(),
    );
    if grab.hovered() || grab.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        ui.painter().vline(
            rect.right() - 0.5,
            rect.y_range(),
            egui::Stroke::new(1.0, p.accent),
        );
    }
    if grab.dragged()
        && let Some(pos) = grab.interact_pointer_pos()
    {
        layout.tool_columns = Layout::columns_for(pos.x - rect.left());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::Storage as _;

    #[test]
    fn the_tools_width_fits_whole_columns() {
        for n in MIN_TOOL_COLUMNS..=MAX_TOOL_COLUMNS {
            assert_eq!(Layout::columns_for(Layout::tools_width(n)), n);
        }
        // dragging narrower than three columns, or wider than eight, stops there
        assert_eq!(Layout::columns_for(10.0), MIN_TOOL_COLUMNS);
        assert_eq!(Layout::columns_for(2000.0), MAX_TOOL_COLUMNS);
        // between two widths it takes the nearer
        let mid = (Layout::tools_width(4) + Layout::tools_width(5)) / 2.0;
        assert_eq!(Layout::columns_for(mid - 1.0), 4);
        assert_eq!(Layout::columns_for(mid + 1.0), 5);
    }

    #[test]
    fn history_is_hidden_until_asked_for_and_joins_layers() {
        let mut layout = Layout::for_workspace(Workspace::Animate);
        assert!(!layout.is_open(Tab::History));
        layout.open(Tab::History);
        assert!(layout.is_open(Tab::History));
        let history = layout.right.find_tab(&Tab::History).unwrap();
        let layers = layout.right.find_tab(&Tab::Layers).unwrap();
        assert_eq!(history.node, layers.node, "History opens in Layers' leaf");
        // asking again only brings it forward
        layout.open(Tab::History);
        assert_eq!(
            layout
                .right
                .iter_all_tabs()
                .filter(|(_, t)| **t == Tab::History)
                .count(),
            1
        );
    }

    #[test]
    fn a_closed_panel_comes_back_in_its_own_region() {
        let mut layout = Layout::for_workspace(Workspace::Draw);
        layout.open(Tab::XSheet);
        assert!(layout.bottom.find_tab(&Tab::XSheet).is_some());
        assert!(layout.right.find_tab(&Tab::XSheet).is_none());
        layout.open(Tab::Actions);
        assert!(layout.is_open(Tab::Actions));
    }

    #[test]
    fn bones_is_a_tab_beside_layers_and_comes_back_there() {
        for w in [Workspace::Animate, Workspace::Rig] {
            let layout = Layout::for_workspace(w);
            let bones = layout.right.find_tab(&Tab::Bones).expect("Bones is open");
            let layers = layout.right.find_tab(&Tab::Layers).unwrap();
            assert_eq!(bones.node, layers.node, "{w:?}: Bones shares Layers' leaf");
        }
        // a workspace without it opens it beside Layers too
        let mut layout = Layout::for_workspace(Workspace::Draw);
        assert!(!layout.is_open(Tab::Bones));
        layout.open(Tab::Bones);
        let bones = layout.right.find_tab(&Tab::Bones).unwrap();
        let layers = layout.right.find_tab(&Tab::Layers).unwrap();
        assert_eq!(bones.node, layers.node);
        assert!(layout.bottom.find_tab(&Tab::Bones).is_none());
    }

    #[test]
    fn every_workspace_has_every_panel_it_needs() {
        for w in Workspace::ALL {
            let layout = Layout::for_workspace(w);
            assert!(layout.show_bottom, "{w:?}");
            assert!(layout.is_open(Tab::Timeline), "{w:?}");
            assert!(!layout.is_open(Tab::History), "{w:?}");
            if layout.show_right {
                assert!(layout.is_open(Tab::Layers), "{w:?}");
            }
        }
    }

    #[test]
    fn simple_mode_leaves_out_the_rig_and_drawn_animation_panels() {
        let layout = Layout::for_workspace(Workspace::Simple);
        assert!(layout.simple);
        for tab in [Tab::Bones, Tab::Actions, Tab::XSheet, Tab::History] {
            assert!(!layout.is_open(tab), "{tab:?}");
        }
        for tab in [Tab::Layers, Tab::Properties, Tab::Style, Tab::Timeline] {
            assert!(layout.is_open(tab), "{tab:?}");
        }
        assert!(!Layout::for_workspace(Workspace::Animate).simple);
        // a layout saved before the switch existed reads as not simple
        let mut store = Store::default();
        eframe::set_value(&mut store, Layout::KEY, &Layout::for_workspace(Workspace::Simple));
        let text = store.get_string(Layout::KEY).unwrap();
        assert!(text.contains("simple:true"), "{text}");
        store.set_string(Layout::KEY, text.replace("simple:true", ""));
        let old: Layout = eframe::get_value(&store, Layout::KEY).expect("an old layout reads");
        assert!(!old.simple);
    }

    #[test]
    fn a_reset_starts_the_sizes_over() {
        let mut layout = Layout::default();
        layout.reset(Workspace::Draw);
        assert_eq!(layout.generation, 1);
        layout.reset(Workspace::Animate);
        assert_eq!(layout.generation, 2);
    }

    /// eframe keeps the layout in its own storage (RON, which unlike JSON
    /// can hold the infinite sizes a dock keeps).
    #[derive(Default)]
    struct Store(std::collections::HashMap<String, String>);

    impl eframe::Storage for Store {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn the_layout_round_trips_through_its_saved_form() {
        let mut layout = Layout::for_workspace(Workspace::Rig);
        layout.reset(Workspace::Rig);
        let mut store = Store::default();
        eframe::set_value(&mut store, Layout::KEY, &layout);
        let back: Layout = eframe::get_value(&store, Layout::KEY).expect("the layout reads back");
        assert_eq!(back.generation, 1);
        assert!(back.is_open(Tab::Bones));
        assert!(!back.is_open(Tab::History));
    }
}
