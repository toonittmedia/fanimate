//! The browser player: a canvas, playback, the layer list and opening files.

use std::cell::RefCell;
use std::rc::Rc;

use eframe::egui::{self, Color32, RichText};
use eframe::egui_wgpu::{RenderState, WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use fanimate_core::eval::Evaluator;
use fanimate_core::{Document, Layer, Vec2, samples};
use fanimate_render::{Mesh, RenderTarget, Renderer, Tessellator, View};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::theme::{self, Palette};

const DOWNLOAD: &str = "https://github.com/toonittmedia/fanimate/releases";
const KINDS: [&str; 6] = ["fnm", "zip", "moho", "mohoproj", "fla", "animeproj"];

/// Start Fanimate in the page's `<canvas id="fanimate">`.
#[wasm_bindgen(start)]
pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Warn).ok();
    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("fanimate"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
        let Some(canvas) = canvas else {
            log::error!("the page has no <canvas id=\"fanimate\">");
            return;
        };
        // WebGPU only: the renderer draws through an sRGB view of its
        // canvas texture, which WebGL cannot do
        let mut setup = WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
        let options = eframe::WebOptions {
            wgpu_options: WgpuConfiguration {
                wgpu_setup: WgpuSetup::CreateNew(setup),
                ..Default::default()
            },
            ..Default::default()
        };
        let started = eframe::WebRunner::new()
            .start(canvas, options, Box::new(|cc| Ok(Box::new(Player::new(cc)))))
            .await;
        if let Err(e) = started {
            log::error!("Fanimate could not start: {e:?}");
            show_message(
                "Fanimate needs WebGPU, which this browser does not offer. Try a recent Chrome or Edge, \
                 or download the desktop app.",
            );
        } else {
            show_message("");
        }
    });
}

/// Set the page's message (shown while loading and when WebGPU is missing).
fn show_message(text: &str) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("message"))
    {
        el.set_text_content(Some(text));
    }
}

/// A file picked or dropped, waiting to be opened.
struct Picked {
    name: String,
    bytes: Vec<u8>,
}

struct Canvas {
    renderer: Renderer,
    evaluator: Evaluator,
    tess: Tessellator,
    mesh: Mesh,
    target: Option<(wgpu::Texture, wgpu::TextureView, egui::TextureId, [u32; 2])>,
}

pub struct Player {
    palette: Palette,
    doc: Document,
    frame: f32,
    playing: bool,
    /// What the last open simplified or left out, or why it failed.
    notes: Vec<String>,
    error: Option<String>,
    picked: Rc<RefCell<Option<Picked>>>,
    canvas: Option<Canvas>,
    /// Pan and zoom; `None` fits the canvas to the window.
    camera: Option<(Vec2, f32)>,
}

impl Player {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let palette = theme::apply(&cc.egui_ctx);
        let canvas = cc.wgpu_render_state.as_ref().map(|rs| Canvas {
            renderer: Renderer::new(&rs.device, &rs.queue, wgpu::TextureFormat::Rgba8UnormSrgb, 4),
            evaluator: Evaluator::default(),
            tess: Tessellator::new(),
            mesh: Mesh::new(),
            target: None,
        });
        let doc = samples::hello();
        Self {
            palette,
            frame: doc.start as f32,
            doc,
            playing: true,
            notes: Vec::new(),
            error: None,
            picked: Rc::default(),
            canvas,
            camera: None,
        }
    }

    fn show(&mut self, doc: Document, notes: Vec<String>) {
        self.frame = doc.start as f32;
        self.doc = doc;
        self.notes = notes;
        self.error = None;
        self.camera = None;
        self.playing = true;
    }

    fn open(&mut self, picked: Picked) {
        match open_bytes(&picked.name, &picked.bytes) {
            Ok((doc, notes)) => self.show(doc, notes),
            Err(e) => self.error = Some(format!("Could not open {}: {e}", picked.name)),
        }
    }

    fn pick_file(&self, ctx: &egui::Context) {
        let picked = self.picked.clone();
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Fanimate (.fnm), Moho and Animate projects", &KINDS)
                .pick_file()
                .await;
            if let Some(f) = file {
                let bytes = f.read().await;
                *picked.borrow_mut() = Some(Picked {
                    name: f.file_name(),
                    bytes,
                });
                ctx.request_repaint();
            }
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new("Fanimate").strong().color(p.accent_strong));
            ui.separator();
            let open = egui::Button::new(RichText::new("Open…").color(p.on_button)).fill(p.button);
            if ui
                .add(open)
                .on_hover_text("A Fanimate .fnm project, a Moho project or an Animate .fla")
                .clicked()
            {
                self.pick_file(ui.ctx());
            }
            ui.menu_button("Samples", |ui| {
                if ui.button("Hello Fanimate").clicked() {
                    self.show(samples::hello(), Vec::new());
                    ui.close();
                }
                if ui.button("Benchmark Rig").clicked() {
                    self.show(samples::benchmark(40), Vec::new());
                    ui.close();
                }
            });
            ui.separator();
            let play = if self.playing { "Pause" } else { "Play" };
            if ui.button(play).on_hover_text("Space").clicked() {
                self.playing = !self.playing;
            }
            let (start, end) = (self.doc.start as f32, self.doc.end.max(self.doc.start) as f32);
            let mut frame = self.frame.floor();
            let slider = egui::Slider::new(&mut frame, start..=end)
                .step_by(1.0)
                .show_value(false);
            if ui.add(slider).changed() {
                self.frame = frame;
                self.playing = false;
            }
            ui.label(
                RichText::new(format!(
                    "Frame {} of {}  ·  {} fps",
                    self.frame.floor(),
                    self.doc.end,
                    self.doc.fps
                ))
                .color(p.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.hyperlink_to("Get the desktop app", DOWNLOAD);
            });
        });
    }

    fn layers(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Layers").strong());
        ui.label(RichText::new(&self.doc.name).color(self.palette.text_muted));
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            // top of the stack first, as in the desktop app
            for layer in self.doc.layers.iter_mut().rev() {
                layer_row(ui, layer, 0);
            }
        });
    }

    fn status(&self, ui: &mut egui::Ui) {
        let p = self.palette;
        ui.horizontal_centered(|ui| {
            if let Some(e) = &self.error {
                ui.label(RichText::new(e).color(p.error));
            } else if !self.notes.is_empty() {
                ui.label(RichText::new(format!("Opened; {}", self.notes.join("; "))).color(p.keyframe));
            } else {
                ui.label(
                    RichText::new(
                        "Fanimate in the browser plays and checks projects. Drawing, rigging, saving \
                         and export are in the desktop app.",
                    )
                    .color(p.text_muted),
                );
            }
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui, rs: Option<&RenderState>) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let size = [
            ((rect.width() * ppp) as u32).max(1),
            ((rect.height() * ppp) as u32).max(1),
        ];
        let target = Vec2::new(size[0] as f32, size[1] as f32);
        let (mut center, mut zoom) = self.camera.unwrap_or_else(|| {
            let fit = View::fit(self.doc.size(), (size[0], size[1]), 24.0 * ppp);
            (self.doc.size() / 2.0, fit.zoom)
        });
        if response.dragged() {
            let d = response.drag_delta() * ppp;
            center -= Vec2::new(d.x, d.y) / zoom;
            self.camera = Some((center, zoom));
        }
        let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if response.hovered() && scroll != 0.0 {
            zoom = (zoom * (scroll / 200.0).exp()).clamp(0.02, 64.0);
            self.camera = Some((center, zoom));
        }
        if response.double_clicked() {
            self.camera = None;
        }
        let view = View::looking_at(center, target, zoom);
        let (Some(rs), Some(canvas)) = (rs, self.canvas.as_mut()) else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No graphics",
                egui::FontId::default(),
                self.palette.text,
            );
            return;
        };
        let id = canvas.draw(
            rs,
            &self.doc,
            self.frame.floor(),
            size,
            &view,
            self.palette.pasteboard,
        );
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(id, rect, uv, Color32::WHITE);
    }
}

fn layer_row(ui: &mut egui::Ui, layer: &mut Layer, depth: usize) {
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);
        ui.checkbox(&mut layer.visible, &layer.name);
    });
    if let Some(children) = layer.kind.children_mut() {
        for child in children.iter_mut().rev() {
            layer_row(ui, child, depth + 1);
        }
    }
}

/// Open a picked file by its extension.
fn open_bytes(name: &str, bytes: &[u8]) -> Result<(Document, Vec<String>), String> {
    let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    match ext.as_str() {
        "fla" => fanimate_import::animate::import_fla(stem, bytes).map(|im| (im.doc, im.notes)),
        "moho" | "mohoproj" | "anime" | "animeproj" => {
            fanimate_import::moho::import(stem, bytes, &|_| None).map(|im| (im.doc, im.notes))
        }
        // a project file, or a project folder packed by hand
        "fnm" | "zip" => {
            let (doc, report) = fanimate_fnm::load_archive(stem, bytes).map_err(|e| e.to_string())?;
            let notes = match report.repaired_count() {
                0 => Vec::new(),
                n => vec![format!("{n} problems repaired")],
            };
            Ok((doc, notes))
        }
        _ => Err("Fanimate in the browser opens .fnm projects, Moho and Animate .fla files".into()),
    }
}

impl Canvas {
    fn draw(
        &mut self,
        rs: &RenderState,
        doc: &Document,
        frame: f32,
        size: [u32; 2],
        view: &View,
        pasteboard: Color32,
    ) -> egui::TextureId {
        if self.target.as_ref().is_none_or(|t| t.3 != size) {
            self.recreate(rs, size);
        }
        let mut items = Vec::new();
        self.evaluator.evaluate_into(doc, frame, &mut items);
        self.tess.clear(&mut self.mesh);
        self.tess.tolerance = 0.25 / view.zoom.max(0.01);
        self.tess.canvas(doc, &mut self.mesh);
        self.tess.draw_items(&items, &mut self.mesh);
        let [r, g, b, _] = pasteboard.to_array();
        let clear = fanimate_core::Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
        let (_, render_view, id, _) = self.target.as_ref().expect("created above");
        self.renderer.render(
            &rs.device,
            &rs.queue,
            RenderTarget {
                view: render_view,
                size: (size[0], size[1]),
            },
            view,
            &self.mesh,
            &self.tess.runs,
            &doc.media,
            clear,
        );
        *id
    }

    fn recreate(&mut self, rs: &RenderState, size: [u32; 2]) {
        let mut egui_renderer = rs.renderer.write();
        if let Some((texture, _, id, _)) = self.target.take() {
            egui_renderer.free_texture(&id);
            texture.destroy();
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
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
        });
        let render_view = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            ..Default::default()
        });
        let sample_view = texture.create_view(&Default::default());
        let id = egui_renderer.register_native_texture(&rs.device, &sample_view, wgpu::FilterMode::Linear);
        self.target = Some((texture, render_view, id, size));
    }
}

impl eframe::App for Player {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // files picked through the dialog or dropped on the page
        let picked = self.picked.borrow_mut().take();
        if let Some(picked) = picked {
            self.open(picked);
        }
        // the browser reads a dropped file in the background
        let dropped = ctx.input(|i| i.raw.dropped_files.first().cloned());
        if let Some(file) = dropped {
            let picked = self.picked.clone();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let name = file.path().to_string_lossy().into_owned();
                match file.bytes_async().await {
                    Ok(bytes) => *picked.borrow_mut() = Some(Picked { name, bytes }),
                    Err(e) => log::warn!("could not read {name}: {e}"),
                }
                ctx.request_repaint();
            });
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.playing = !self.playing;
        }
        if self.playing {
            let dt = ctx.input(|i| i.stable_dt).min(0.25);
            self.frame += dt * self.doc.fps;
            let (start, end) = (self.doc.start as f32, self.doc.end.max(self.doc.start) as f32);
            if self.frame >= end + 1.0 || self.frame < start {
                self.frame = start;
            }
            ctx.request_repaint();
        }

        let p = self.palette;
        egui::Panel::top("bar")
            .exact_size(36.0)
            .frame(
                egui::Frame::NONE
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(
                egui::Frame::NONE
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| self.status(ui));
        egui::Panel::right("layers")
            .default_size(220.0)
            .frame(
                egui::Frame::NONE
                    .fill(p.panel)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| self.layers(ui));
        let rs = frame.wgpu_render_state().cloned();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.pasteboard))
            .show(ui, |ui| self.canvas(ui, rs.as_ref()));
    }
}
