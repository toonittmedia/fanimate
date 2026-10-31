//! Interface icons: [Lucide](https://lucide.dev) SVGs (ISC licence) in
//! `icons/lucide/`, plus bone-tool icons drawn in the same style in
//! `icons/fanimate/`. Never font glyphs or emoji: egui's fonts lack many
//! symbols and they render as empty boxes.
//!
//! Each icon is rasterised once per pixel size with resvg, in white, and
//! tinted when painted, so it is sharp at every display scale and follows
//! the theme. Painting snaps the icon to the pixel grid so 2 px strokes land
//! on whole pixels instead of blurring across three.

use std::collections::HashMap;

use eframe::egui::{self, Color32, ColorImage, Painter, Rect, Sense, TextureHandle, TextureId, pos2, vec2};
use fanimate_editor::Tool;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Tool(Tool),
    VectorLayer,
    GroupLayer,
    BoneLayer,
    Add,
    Duplicate,
    Delete,
    Up,
    Down,
    Eye,
    EyeClosed,
    Expand,
    Collapse,
    FirstFrame,
    RestPose,
    PrevFrame,
    Play,
    Pause,
    NextFrame,
    LastFrame,
    AddKey,
    DeleteKey,
    Keyboard,
    Union,
    Subtract,
    Intersect,
    History,
    Action,
    ImageLayer,
    FramesLayer,
    OnionSkin,
    LightTable,
    XSheet,
    SwitchLayer,
    AudioLayer,
    TextLayer,
    Grip,
    Home,
    Close,
    Rotate,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ZoomFit,
    Lock,
    Unlock,
    Align,
    /// The pivot picker: a target to click where the pivot goes.
    Pivot,
    /// Save a style in the Style panel.
    Save,
}

macro_rules! lucide {
    ($name:literal) => {
        include_str!(concat!("../icons/lucide/", $name, ".svg"))
    };
}

macro_rules! own {
    ($name:literal) => {
        include_str!(concat!("../icons/fanimate/", $name, ".svg"))
    };
}

impl Icon {
    fn svg(self) -> &'static str {
        match self {
            Self::Tool(tool) => match tool {
                Tool::SelectPoints => lucide!("square-dashed-mouse-pointer"),
                Tool::TranslatePoints => lucide!("move"),
                Tool::AddPoint => lucide!("spline"),
                Tool::Pen => lucide!("pen-tool"),
                Tool::Curvature => own!("curvature"),
                Tool::Freehand => lucide!("pencil"),
                Tool::Rectangle => lucide!("square"),
                Tool::Ellipse => lucide!("circle"),
                Tool::Line => own!("line"),
                Tool::Polygon => own!("polygon"),
                Tool::Star => own!("star"),
                Tool::Triangle => own!("triangle"),
                Tool::Eraser => lucide!("eraser"),
                Tool::DeleteEdge => own!("delete-edge"),
                Tool::HideEdge => own!("hide-edge"),
                Tool::Magnet => own!("magnet"),
                Tool::PaintBucket => lucide!("paint-bucket"),
                Tool::Eyedropper => lucide!("pipette"),
                Tool::Text => lucide!("type"),
                Tool::DrawShape => own!("draw-shape"),
                Tool::LineWidth => own!("line-width"),
                Tool::StrokeExposure => own!("stroke-exposure"),
                Tool::ShapeBuilder => own!("shape-builder"),
                Tool::TransformLayer => lucide!("scaling"),
                Tool::TrackCamera => own!("camera-track"),
                Tool::ZoomCamera => own!("camera-zoom"),
                Tool::RollCamera => own!("camera-roll"),
                Tool::SelectBone => own!("bone-select"),
                Tool::AddBone => own!("bone-plus"),
                Tool::ReparentBone => own!("bone-reparent"),
                Tool::ManipulateBones => own!("bone-rotate"),
                Tool::PaintWeights => lucide!("brush"),
                Tool::Pan => lucide!("hand"),
                Tool::Zoom => lucide!("zoom-in"),
            },
            Self::VectorLayer => lucide!("vector-square"),
            Self::GroupLayer => lucide!("folder"),
            Self::BoneLayer => lucide!("bone"),
            Self::Add => lucide!("plus"),
            Self::Duplicate => lucide!("copy"),
            Self::Delete => lucide!("trash-2"),
            Self::Up => lucide!("arrow-up"),
            Self::Down => lucide!("arrow-down"),
            Self::Eye => lucide!("eye"),
            Self::EyeClosed => lucide!("eye-off"),
            Self::Expand => lucide!("chevron-right"),
            Self::Collapse => lucide!("chevron-down"),
            Self::FirstFrame => lucide!("skip-back"),
            Self::RestPose => own!("rest-pose"),
            Self::PrevFrame => lucide!("step-back"),
            Self::Play => lucide!("play"),
            Self::Pause => lucide!("pause"),
            Self::NextFrame => lucide!("step-forward"),
            Self::LastFrame => lucide!("skip-forward"),
            Self::AddKey => lucide!("diamond-plus"),
            Self::DeleteKey => lucide!("diamond-minus"),
            Self::Keyboard => lucide!("keyboard"),
            Self::Union => lucide!("squares-unite"),
            Self::Subtract => lucide!("squares-subtract"),
            Self::Intersect => lucide!("squares-intersect"),
            Self::History => lucide!("history"),
            Self::Action => lucide!("clapperboard"),
            Self::ImageLayer => lucide!("image"),
            Self::FramesLayer => lucide!("film"),
            Self::OnionSkin => lucide!("layers-2"),
            Self::LightTable => lucide!("lamp"),
            Self::XSheet => lucide!("table-2"),
            Self::SwitchLayer => lucide!("toggle-left"),
            Self::AudioLayer => lucide!("audio-lines"),
            Self::TextLayer => lucide!("type"),
            Self::Grip => lucide!("grip-vertical"),
            Self::Home => lucide!("house"),
            Self::Close => lucide!("x"),
            Self::Rotate => lucide!("rotate-cw"),
            Self::ZoomIn => lucide!("zoom-in"),
            Self::ZoomOut => lucide!("zoom-out"),
            Self::ZoomReset => lucide!("rotate-ccw"),
            Self::ZoomFit => lucide!("maximize-2"),
            Self::Lock => lucide!("lock"),
            Self::Unlock => lucide!("lock-open"),
            Self::Align => lucide!("align-center-vertical"),
            Self::Pivot => lucide!("crosshair"),
            Self::Save => lucide!("save"),
        }
    }
}

/// Rasterise an icon in white at `px` by `px` pixels.
fn rasterize(svg: &str, px: u32) -> Option<ColorImage> {
    use resvg::{tiny_skia, usvg};
    let svg = svg.replace("currentColor", "#ffffff");
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    let scale = px as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(ColorImage::from_rgba_premultiplied(
        [px as usize; 2],
        pixmap.data(),
    ))
}

type Cache = HashMap<(Icon, u32), TextureHandle>;

/// The icon's texture at `px` pixels, rasterised on first use.
fn texture(ctx: &egui::Context, icon: Icon, px: u32) -> TextureId {
    let cache_id = egui::Id::new("fanimate-icon-cache");
    if let Some(id) = ctx.data(|d| {
        d.get_temp::<Cache>(cache_id)
            .and_then(|c| c.get(&(icon, px)).map(|t| t.id()))
    }) {
        return id;
    }
    // a broken icon file shows as a blank square, never a crash; the tests
    // check every icon renders
    let image = rasterize(icon.svg(), px).unwrap_or_else(|| ColorImage::filled([1, 1], Color32::TRANSPARENT));
    let handle = ctx.load_texture(format!("icon-{icon:?}-{px}"), image, egui::TextureOptions::LINEAR);
    let id = handle.id();
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(cache_id)
            .insert((icon, px), handle)
    });
    id
}

/// Draw `icon` centred in `rect`, as large as fits, in `color`.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let ppp = p.ctx().pixels_per_point();
    let px = (rect.width().min(rect.height()) * ppp).round().max(1.0) as u32;
    let side = px as f32 / ppp;
    // snap the top-left corner to a physical pixel
    let min = rect.center() - vec2(side, side) / 2.0;
    let min = pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    p.image(
        texture(p.ctx(), icon, px),
        Rect::from_min_size(min, vec2(side, side)),
        uv,
        color,
    );
}

/// The icon as an [`egui::Image`], for buttons that combine an icon and text.
pub fn image(ctx: &egui::Context, icon: Icon, size: f32, color: Color32) -> egui::Image<'static> {
    let px = (size * ctx.pixels_per_point()).round().max(1.0) as u32;
    egui::Image::from_texture((texture(ctx, icon, px), vec2(size, size))).tint(color)
}

/// Icon size inside a square button of side `size`: Lucide's 16 and 20 px
/// grids, where its 2 px strokes read best.
fn icon_size(size: f32) -> f32 {
    if size >= 30.0 { 20.0 } else { 16.0 }
}

/// An icon button. `dim` draws it faded (unavailable or not yet built) but
/// it still reacts, so tooltips and hints keep working.
pub fn button(ui: &mut egui::Ui, icon: Icon, size: f32, selected: bool, dim: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let visuals = ui.visuals();
    let bg = if selected {
        Some(visuals.widgets.active.bg_fill)
    } else if response.hovered() {
        Some(visuals.widgets.hovered.bg_fill)
    } else {
        None
    };
    if let Some(bg) = bg {
        ui.painter().rect_filled(rect, 4.0, bg);
    }
    let color = if selected {
        visuals.widgets.active.fg_stroke.color
    } else if dim {
        // with the theme's text colour override, egui's weak text colour is
        // barely dimmer; half strength reads as "not available" yet legible
        visuals.text_color().gamma_multiply(0.45)
    } else {
        visuals.text_color()
    };
    let inner = Rect::from_center_size(rect.center(), vec2(icon_size(size), icon_size(size)));
    paint(ui.painter(), inner, icon, color);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_editor::Tool;

    fn all() -> Vec<Icon> {
        let mut icons: Vec<Icon> = Tool::ALL.into_iter().map(Icon::Tool).collect();
        icons.extend([
            Icon::VectorLayer,
            Icon::GroupLayer,
            Icon::BoneLayer,
            Icon::Add,
            Icon::Duplicate,
            Icon::Delete,
            Icon::Up,
            Icon::Down,
            Icon::Eye,
            Icon::EyeClosed,
            Icon::Expand,
            Icon::Collapse,
            Icon::FirstFrame,
            Icon::RestPose,
            Icon::PrevFrame,
            Icon::Play,
            Icon::Pause,
            Icon::NextFrame,
            Icon::LastFrame,
            Icon::AddKey,
            Icon::DeleteKey,
            Icon::Keyboard,
            Icon::Union,
            Icon::Subtract,
            Icon::Intersect,
            Icon::History,
            Icon::Action,
            Icon::ImageLayer,
            Icon::FramesLayer,
            Icon::OnionSkin,
            Icon::LightTable,
            Icon::XSheet,
            Icon::SwitchLayer,
            Icon::AudioLayer,
            Icon::TextLayer,
            Icon::Grip,
            Icon::Home,
            Icon::Close,
            Icon::Rotate,
            Icon::ZoomIn,
            Icon::ZoomOut,
            Icon::ZoomReset,
            Icon::ZoomFit,
            Icon::Lock,
            Icon::Unlock,
            Icon::Align,
            Icon::Pivot,
            Icon::Save,
        ]);
        icons
    }

    #[test]
    fn every_icon_renders() {
        for icon in all() {
            let image = rasterize(icon.svg(), 16).unwrap_or_else(|| panic!("{icon:?} does not parse"));
            assert_eq!(image.size, [16, 16]);
            let opaque = image.pixels.iter().filter(|p| p.a() > 128).count();
            assert!(opaque >= 8, "{icon:?} renders almost nothing ({opaque} px)");
        }
    }
}
