//! Renders the document into a texture that egui shows as the canvas.
//!
//! The canvas uses the same `fanimate-render` renderer as export, on the
//! same wgpu device as the UI. egui samples native textures as
//! `Rgba8Unorm`; we render through an `Rgba8UnormSrgb` view of the same
//! texture so colours are encoded exactly as in an exported PNG.

use eframe::egui;
use eframe::egui_wgpu::RenderState;
use fanimate_core::eval::Evaluator;
use fanimate_core::{Affine2, Document, DrawItem, LayerId, Shape};
use fanimate_render::{Mesh, RenderTarget, Renderer, Tessellator, View};

/// How strongly the light table shows layers other than the selection.
const LIGHT_TABLE_FADE: f32 = 0.25;

const SAMPLE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const RENDER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

struct Target {
    texture: wgpu::Texture,
    render_view: wgpu::TextureView,
    id: egui::TextureId,
    size: [u32; 2],
}

/// Other frames shown faintly around the current one (onion skin), and
/// layers faded back (light table). On the canvas only, never in renders.
#[derive(Clone, Debug, Default)]
pub struct Overlays {
    /// (frame, tint colour, opacity) for each onion skin, drawn in order
    /// under the current frame.
    pub onion: Vec<(f32, fanimate_core::Color, f32)>,
    /// Layers the onion skins show; `None` for all.
    pub onion_layers: Option<Vec<LayerId>>,
    /// With the light table on, the layers kept at full strength; others
    /// are faded.
    pub light_table: Option<Vec<LayerId>>,
    /// Show the canvas as the document's camera sees it.
    pub camera: bool,
}

pub struct CanvasGpu {
    renderer: Renderer,
    target: Option<Target>,
    evaluator: Evaluator,
    tess: Tessellator,
    mesh: Mesh,
    /// Time spent evaluating and tessellating the last frame.
    pub cpu_time: std::time::Duration,
    pub shapes: usize,
}

impl CanvasGpu {
    pub fn new(rs: &RenderState) -> Self {
        Self {
            renderer: Renderer::new(&rs.device, &rs.queue, RENDER_FORMAT, 4),
            target: None,
            evaluator: Evaluator::default(),
            tess: Tessellator::new(),
            mesh: Mesh::new(),
            cpu_time: Default::default(),
            shapes: 0,
        }
    }

    /// Draw `doc` at `frame` into a `size`-pixel texture and return its id.
    /// `extra` (the brush stroke being drawn, in document space) goes on
    /// top, through the same renderer, so the preview is exactly the result.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        rs: &RenderState,
        doc: &Document,
        frame: f32,
        size: [u32; 2],
        view: &View,
        pasteboard: fanimate_core::Color,
        extra: Option<&Shape>,
        overlays: &Overlays,
    ) -> egui::TextureId {
        let size = [size[0].max(1), size[1].max(1)];
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            self.recreate(rs, size);
        }
        let target = self.target.as_ref().expect("created above");

        let started = std::time::Instant::now();
        let mut items = Vec::new();
        self.evaluator.camera = overlays.camera;
        self.evaluator.evaluate_into(doc, frame, &mut items);
        if let Some(shape) = extra {
            items.push(DrawItem {
                layer: LayerId(0),
                index: 0,
                shape: std::borrow::Cow::Borrowed(shape),
                part: None,
                image: None,
                transform: Affine2::IDENTITY,
                opacity: 1.0,
            });
        }
        if let Some(keep) = &overlays.light_table {
            for item in &mut items {
                if !keep.iter().any(|k| doc.is_within(item.layer, *k)) {
                    item.opacity *= LIGHT_TABLE_FADE;
                }
            }
        }
        self.tess.clear(&mut self.mesh);
        self.tess.tolerance = 0.25 / view.zoom.max(0.01);
        let camera = if overlays.camera {
            fanimate_core::camera_at(doc, frame).unwrap_or(Affine2::IDENTITY)
        } else {
            Affine2::IDENTITY
        };
        self.tess.canvas_through(doc, &mut self.mesh, camera);
        // onion skins under the current frame, each in its tint
        let mut onion = Evaluator::default();
        for &(at, tint, opacity) in &overlays.onion {
            let mut skin = Vec::new();
            onion.evaluate_into(doc, at, &mut skin);
            skin.retain(|i| {
                overlays
                    .onion_layers
                    .as_ref()
                    .is_none_or(|ls| ls.iter().any(|l| doc.is_within(i.layer, *l)))
            });
            for item in &mut skin {
                item.opacity *= opacity;
            }
            self.tess.tint = Some(tint);
            self.tess.draw_items(&skin, &mut self.mesh);
        }
        self.tess.tint = None;
        self.tess.draw_items(&items, &mut self.mesh);
        self.cpu_time = started.elapsed();
        self.shapes = items.len();

        self.renderer.render(
            &rs.device,
            &rs.queue,
            RenderTarget {
                view: &target.render_view,
                size: (size[0], size[1]),
            },
            view,
            &self.mesh,
            &self.tess.runs,
            &doc.media,
            pasteboard,
        );
        target.id
    }

    fn recreate(&mut self, rs: &RenderState, size: [u32; 2]) {
        let mut egui_renderer = rs.renderer.write();
        if let Some(old) = self.target.take() {
            egui_renderer.free_texture(&old.id);
            old.texture.destroy();
        }
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fanimate canvas"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SAMPLE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[RENDER_FORMAT],
        });
        let render_view = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(RENDER_FORMAT),
            ..Default::default()
        });
        let sample_view = texture.create_view(&Default::default());
        let id = egui_renderer.register_native_texture(&rs.device, &sample_view, wgpu::FilterMode::Linear);
        self.target = Some(Target {
            texture,
            render_view,
            id,
            size,
        });
    }
}
